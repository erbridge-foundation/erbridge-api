use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use reqwest::header::CACHE_CONTROL;
use serde::Deserialize;
use tokio::{sync::mpsc, time::sleep};
use tracing::{debug, error, info, warn};

use crate::{
    db::character::{Character, find_all_pollable_characters, update_character_online_status},
    esi::{self, token::{TokenStatus, ensure_token_fresh}},
    state::AppState,
};

const DEFAULT_CACHE_SECS: u64 = 60;
const MIN_BATCH_DELAY_MS: u64 = 100;

#[derive(Debug, Deserialize)]
struct OnlineResponse {
    online: bool,
}

/// Parses `max-age=N` from a `Cache-Control` header value.
/// Returns `None` if the header is absent or unparseable.
pub fn parse_max_age(header: &str) -> Option<u64> {
    header.split(',').find_map(|part| {
        let part = part.trim();
        let rest = part.strip_prefix("max-age")?;
        let rest = rest.trim().strip_prefix('=')?;
        rest.trim().parse::<u64>().ok()
    })
}

/// Spawns the online poll background task.
///
/// Returns the sender half of the channel. The login/add-character handler
/// should send the account's eve_character_ids down this channel so they are
/// registered for polling immediately.
pub fn spawn_online_poller(state: Arc<AppState>) -> mpsc::Sender<Vec<i64>> {
    let (tx, rx) = mpsc::channel::<Vec<i64>>(256);
    tokio::spawn(run_online_poller(state, rx));
    tx
}

async fn run_online_poller(state: Arc<AppState>, mut rx: mpsc::Receiver<Vec<i64>>) {
    // next_poll_at tracks when each character is next due. Characters are
    // added here when the login handler sends their IDs, and refreshed every
    // cycle from the DB so newly-added characters are picked up automatically.
    let mut next_poll_at: HashMap<i64, Instant> = HashMap::new();

    loop {
        // Drain any incoming character registrations without blocking.
        loop {
            match rx.try_recv() {
                Ok(ids) => {
                    for id in ids {
                        next_poll_at.entry(id).or_insert_with(Instant::now);
                    }
                }
                Err(_) => break,
            }
        }

        // Re-fetch the full pollable set from the DB each cycle so characters
        // added between login events are picked up without an explicit message.
        let characters = match find_all_pollable_characters(&state.db, &state.config.aes_key).await
        {
            Ok(c) => c,
            Err(e) => {
                error!(error = %e, "online poller: failed to fetch characters");
                sleep(Duration::from_secs(DEFAULT_CACHE_SECS)).await;
                continue;
            }
        };

        // Ensure every character from the DB is in the schedule map.
        for ch in &characters {
            next_poll_at
                .entry(ch.eve_character_id)
                .or_insert_with(Instant::now);
        }

        // Group by esi_client_id; fall back to a sentinel for legacy/null rows.
        let mut by_client: HashMap<String, Vec<&Character>> = HashMap::new();
        for ch in &characters {
            let key = ch
                .esi_client_id
                .clone()
                .unwrap_or_else(|| "__unassigned__".to_string());
            by_client.entry(key).or_default().push(ch);
        }

        // Spawn one task per client; each processes its characters in batches.
        let mut handles = Vec::new();
        for (client_id, chars) in by_client {
            let state = Arc::clone(&state);
            let chars: Vec<Character> = chars
                .into_iter()
                .map(|c| {
                    // Shallow clone — we only need the fields; Character has no Clone
                    // derive, so we reconstruct from the referenced fields.
                    Character {
                        id: c.id,
                        account_id: c.account_id,
                        eve_character_id: c.eve_character_id,
                        name: c.name.clone(),
                        corporation_id: c.corporation_id,
                        alliance_id: c.alliance_id,
                        is_main: c.is_main,
                        is_online: c.is_online,
                        esi_client_id: c.esi_client_id.clone(),
                        access_token: c.access_token.clone(),
                        refresh_token: c.refresh_token.clone(),
                        esi_token_expires_at: c.esi_token_expires_at,
                        created_at: c.created_at,
                        updated_at: c.updated_at,
                    }
                })
                .collect();

            // Snapshot which are due now.
            let due: Vec<Character> = chars
                .into_iter()
                .filter(|c| {
                    next_poll_at
                        .get(&c.eve_character_id)
                        .map(|t| Instant::now() >= *t)
                        .unwrap_or(true)
                })
                .collect();

            if due.is_empty() {
                continue;
            }

            let batch_size = state.config.esi_poll_batch_size;
            let delay_ms = state.config.esi_poll_batch_delay_ms.max(MIN_BATCH_DELAY_MS);

            handles.push(tokio::spawn(async move {
                let mut results: Vec<(i64, Option<u64>)> = Vec::new();
                for batch in due.chunks(batch_size) {
                    for ch in batch {
                        let ttl = poll_one_online(&state, ch, &client_id).await;
                        results.push((ch.eve_character_id, ttl));
                    }
                    sleep(Duration::from_millis(delay_ms)).await;
                }
                results
            }));
        }

        // Collect TTLs from all client tasks and update the schedule.
        for handle in handles {
            match handle.await {
                Ok(results) => {
                    for (eve_id, ttl_secs) in results {
                        let ttl = ttl_secs.unwrap_or(DEFAULT_CACHE_SECS);
                        next_poll_at.insert(
                            eve_id,
                            Instant::now() + Duration::from_secs(ttl),
                        );
                    }
                }
                Err(e) => error!(error = %e, "online poller client task panicked"),
            }
        }

        // Sleep until the soonest character is due, or DEFAULT_CACHE_SECS.
        let sleep_until = next_poll_at
            .values()
            .min()
            .copied()
            .unwrap_or_else(|| Instant::now() + Duration::from_secs(DEFAULT_CACHE_SECS));
        let now = Instant::now();
        if sleep_until > now {
            sleep(sleep_until - now).await;
        }
    }
}

/// Polls ESI for one character's online status. Returns the Cache-Control
/// max-age from the response, or None on error (caller uses the default TTL).
async fn poll_one_online(
    state: &AppState,
    character: &Character,
    _client_id: &str,
) -> Option<u64> {
    let token = match ensure_token_fresh(
        &state.db,
        &state.http,
        &state.config,
        &state.esi_metadata.token_endpoint,
        character,
    )
    .await
    {
        Ok(TokenStatus::Fresh(t)) => t,
        Ok(TokenStatus::RefreshExpired) => {
            info!(
                eve_character_id = character.eve_character_id,
                "online poller: skipping character with expired refresh token"
            );
            return None;
        }
        Err(e) => {
            warn!(
                error = %e,
                eve_character_id = character.eve_character_id,
                "online poller: token refresh failed"
            );
            return None;
        }
    };

    let url = format!(
        "{}/characters/{}/online/",
        state.config.esi_base, character.eve_character_id
    );

    let response = match esi::esi_request(|| async {
        state
            .http
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(anyhow::Error::from)
    })
    .await
    {
        Ok(r) => r,
        Err(e) => {
            warn!(
                error = %e,
                eve_character_id = character.eve_character_id,
                "online poller: ESI request failed"
            );
            return None;
        }
    };

    let cache_secs = response
        .headers()
        .get(CACHE_CONTROL)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_max_age);

    let body: OnlineResponse = match response.json().await {
        Ok(b) => b,
        Err(e) => {
            warn!(
                error = %e,
                eve_character_id = character.eve_character_id,
                "online poller: failed to parse ESI response"
            );
            return cache_secs;
        }
    };

    // Only write to the DB if the status changed.
    if character.is_online != Some(body.online) {
        if let Err(e) =
            update_character_online_status(&state.db, character.eve_character_id, body.online)
                .await
        {
            warn!(
                error = %e,
                eve_character_id = character.eve_character_id,
                "online poller: failed to persist online status"
            );
        } else {
            debug!(
                eve_character_id = character.eve_character_id,
                is_online = body.online,
                "online status updated"
            );
        }
    }

    cache_secs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_max_age_standard() {
        assert_eq!(parse_max_age("public, max-age=60"), Some(60));
    }

    #[test]
    fn parse_max_age_only() {
        assert_eq!(parse_max_age("max-age=30"), Some(30));
    }

    #[test]
    fn parse_max_age_missing() {
        assert_eq!(parse_max_age("no-cache"), None);
    }

    #[test]
    fn parse_max_age_with_spaces() {
        assert_eq!(parse_max_age("public, max-age = 120"), Some(120));
    }

    #[test]
    fn parse_max_age_zero() {
        assert_eq!(parse_max_age("max-age=0"), Some(0));
    }
}
