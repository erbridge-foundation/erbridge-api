use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use reqwest::header::CACHE_CONTROL;
use serde::Deserialize;
use tokio::{sync::mpsc, time::sleep};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::{
    db::character::{Character, find_all_pollable_characters, update_character_online_status},
    esi::{
        self,
        cache::parse_max_age,
        token::{TokenStatus, ensure_token_fresh},
    },
    state::AppState,
};

const DEFAULT_CACHE_SECS: u64 = 60;
const MIN_BATCH_DELAY_MS: u64 = 100;

#[derive(Debug, Deserialize)]
struct OnlineResponse {
    online: bool,
}

/// Spawns the online poll background task.
///
/// Returns the sender half of the channel. The login/add-character handler
/// should send the account's eve_character_ids down this channel so they are
/// registered for polling immediately.
pub fn spawn_online_poller(
    state: Arc<AppState>,
    cancel: CancellationToken,
) -> mpsc::Sender<Vec<i64>> {
    let (tx, rx) = mpsc::channel::<Vec<i64>>(256);
    tokio::spawn(run_online_poller(state, rx, cancel));
    tx
}

async fn run_online_poller(
    state: Arc<AppState>,
    mut rx: mpsc::Receiver<Vec<i64>>,
    cancel: CancellationToken,
) {
    // next_poll_at tracks when each character is next due. Characters are
    // added here when the login handler sends their IDs, and refreshed every
    // cycle from the DB so newly-added characters are picked up automatically.
    let mut next_poll_at: HashMap<i64, Instant> = HashMap::new();

    loop {
        if cancel.is_cancelled() {
            info!("online poller: shutting down");
            return;
        }

        // Drain any incoming character registrations without blocking.
        while let Ok(ids) = rx.try_recv() {
            for id in ids {
                next_poll_at.entry(id).or_insert_with(Instant::now);
            }
        }

        // Re-fetch the full pollable set from the DB each cycle so characters
        // added between login events are picked up without an explicit message.
        let characters = match find_all_pollable_characters(&state.db, &state.config.aes_key).await
        {
            Ok(c) => c,
            Err(e) => {
                error!(error = %e, "online poller: failed to fetch characters");
                tokio::select! {
                    _ = sleep(Duration::from_secs(DEFAULT_CACHE_SECS)) => {}
                    _ = cancel.cancelled() => {
                        info!("online poller: shutting down");
                        return;
                    }
                }
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
                        next_poll_at.insert(eve_id, Instant::now() + Duration::from_secs(ttl));
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
            tokio::select! {
                _ = sleep(sleep_until - now) => {}
                _ = cancel.cancelled() => {
                    info!("online poller: shutting down");
                    return;
                }
            }
        }
    }
}

/// Polls ESI for one character's online status. Returns the Cache-Control
/// max-age from the response, or None on error (caller uses the default TTL).
async fn poll_one_online(state: &AppState, character: &Character, _client_id: &str) -> Option<u64> {
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

    debug!(
        eve_character_id = character.eve_character_id,
        "online poller: polling"
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

    debug!(
        eve_character_id = character.eve_character_id,
        is_online = body.online,
        cache_secs = ?cache_secs,
        "online poller: got result"
    );

    // Only write to the DB if the status changed.
    if character.is_online != Some(body.online) {
        if let Err(e) =
            update_character_online_status(&state.db, character.eve_character_id, body.online).await
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
    use crate::{
        config::{Config, EsiClient},
        esi::discovery::EsiMetadata,
        state::AppState,
        tasks::character_location_poll::LocationEvent,
    };
    use dashmap::DashMap;
    use jsonwebtoken::jwk::JwkSet;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    fn make_state() -> Arc<AppState> {
        let config = Config {
            esi_clients: vec![EsiClient {
                client_id: "test".into(),
                client_secret: "test".into(),
            }],
            esi_callback_url: "http://localhost/callback".into(),
            aes_key: [0u8; 32],
            jwt_key: [0u8; 32],
            frontend_url: "http://localhost".into(),
            account_deletion_grace_days: 30,
            esi_base: "http://localhost".into(),
            esi_refresh_token_max_days: 7,
            esi_poll_concurrency: 10,
            esi_poll_batch_size: 10,
            esi_poll_batch_delay_ms: 500,
            map_checkpoint_interval_mins: 60,
            database_max_connections: 5,
        };
        Arc::new(AppState {
            db: sqlx::PgPool::connect_lazy("postgres://localhost/dummy").unwrap(),
            http: reqwest::Client::new(),
            config,
            esi_metadata: EsiMetadata {
                authorization_endpoint: "http://localhost".into(),
                token_endpoint: "http://localhost".into(),
                jwks_uri: "http://localhost".into(),
            },
            jwks: Arc::new(RwLock::new(JwkSet { keys: vec![] })),
            online_poll_tx: None,
            location_subs: Arc::new(
                DashMap::<i64, tokio::sync::broadcast::Sender<LocationEvent>>::new(),
            ),
        })
    }

    #[tokio::test]
    async fn online_poller_exits_on_cancel() {
        let cancel = CancellationToken::new();
        cancel.cancel();

        let (tx, rx) = tokio::sync::mpsc::channel::<Vec<i64>>(1);
        drop(tx);

        let state = make_state();
        let handle = tokio::spawn(run_online_poller(state, rx, cancel));

        tokio::time::timeout(std::time::Duration::from_secs(1), handle)
            .await
            .expect("online poller did not exit within 1s")
            .expect("task panicked");
    }
}
