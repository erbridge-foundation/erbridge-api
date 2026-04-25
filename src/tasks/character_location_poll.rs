use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use dashmap::DashMap;
use reqwest::header::CACHE_CONTROL;
use serde::Deserialize;
use tokio::{sync::broadcast, time::sleep};
use tracing::{debug, error, warn};

use crate::{
    db::character::{Character, find_all_pollable_characters},
    esi::{self, token::{TokenStatus, ensure_token_fresh}},
    state::AppState,
};

const DEFAULT_CACHE_SECS: u64 = 5;
const BROADCAST_CAPACITY: usize = 32;

/// A location change event broadcast to all active map session subscribers.
#[derive(Debug, Clone)]
pub struct LocationEvent {
    pub eve_character_id: i64,
    pub solar_system_id: i64,
    pub station_id: Option<i64>,
    pub structure_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct LocationResponse {
    solar_system_id: i64,
    station_id: Option<i64>,
    structure_id: Option<i64>,
}

/// Parses `max-age=N` from a `Cache-Control` header value.
pub fn parse_max_age(header: &str) -> Option<u64> {
    header.split(',').find_map(|part| {
        let part = part.trim();
        let rest = part.strip_prefix("max-age")?;
        let rest = rest.trim().strip_prefix('=')?;
        rest.trim().parse::<u64>().ok()
    })
}

/// Returns the broadcast sender for a character, creating it if absent.
/// Map sessions call this to subscribe; the poller uses receiver_count()
/// to decide whether to poll.
pub fn subscribe(
    subs: &DashMap<i64, broadcast::Sender<LocationEvent>>,
    eve_character_id: i64,
) -> broadcast::Receiver<LocationEvent> {
    subs.entry(eve_character_id)
        .or_insert_with(|| broadcast::channel(BROADCAST_CAPACITY).0)
        .subscribe()
}

/// Spawns the location poll background task.
pub fn spawn_location_poller(state: Arc<AppState>) {
    tokio::spawn(run_location_poller(state));
}

async fn run_location_poller(state: Arc<AppState>) {
    let mut next_poll_at: HashMap<i64, Instant> = HashMap::new();

    loop {
        let characters =
            match find_all_pollable_characters(&state.db, &state.config.aes_key).await {
                Ok(c) => c,
                Err(e) => {
                    error!(error = %e, "location poller: failed to fetch characters");
                    sleep(Duration::from_secs(DEFAULT_CACHE_SECS)).await;
                    continue;
                }
            };

        // Group by esi_client_id.
        let mut by_client: HashMap<String, Vec<Character>> = HashMap::new();
        for ch in characters {
            // Skip characters nobody is listening to.
            if state
                .location_subs
                .get(&ch.eve_character_id)
                .map(|s| s.receiver_count() == 0)
                .unwrap_or(true)
            {
                continue;
            }

            let key = ch
                .esi_client_id
                .clone()
                .unwrap_or_else(|| "__unassigned__".to_string());
            by_client.entry(key).or_default().push(ch);
        }

        let mut handles = Vec::new();
        for (_client_id, chars) in by_client {
            let state = Arc::clone(&state);
            let concurrency = state.config.esi_poll_concurrency;

            // Collect only characters that are due.
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

            handles.push(tokio::spawn(async move {
                let semaphore = Arc::new(tokio::sync::Semaphore::new(concurrency));
                let mut inner_handles = Vec::new();

                for ch in due {
                    let state = Arc::clone(&state);
                    let sem = Arc::clone(&semaphore);
                    inner_handles.push(tokio::spawn(async move {
                        let _permit = sem.acquire().await;
                        let ttl = poll_one_location(&state, &ch).await;
                        (ch.eve_character_id, ttl)
                    }));
                }

                let mut results = Vec::new();
                for h in inner_handles {
                    match h.await {
                        Ok(r) => results.push(r),
                        Err(e) => error!(error = %e, "location poller: inner task panicked"),
                    }
                }
                results
            }));
        }

        for handle in handles {
            match handle.await {
                Ok(results) => {
                    for (eve_id, ttl_secs) in results {
                        let ttl = ttl_secs.unwrap_or(DEFAULT_CACHE_SECS);
                        next_poll_at
                            .insert(eve_id, Instant::now() + Duration::from_secs(ttl));
                    }
                }
                Err(e) => error!(error = %e, "location poller: client task panicked"),
            }
        }

        // Remove entries from location_subs where nobody is subscribed.
        state
            .location_subs
            .retain(|_, sender| sender.receiver_count() > 0);

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

async fn poll_one_location(state: &AppState, character: &Character) -> Option<u64> {
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
            debug!(
                eve_character_id = character.eve_character_id,
                "location poller: skipping character with expired refresh token"
            );
            return None;
        }
        Err(e) => {
            warn!(
                error = %e,
                eve_character_id = character.eve_character_id,
                "location poller: token refresh failed"
            );
            return None;
        }
    };

    let url = format!(
        "{}/characters/{}/location/",
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
                "location poller: ESI request failed"
            );
            return None;
        }
    };

    let cache_secs = response
        .headers()
        .get(CACHE_CONTROL)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_max_age);

    let body: LocationResponse = match response.json().await {
        Ok(b) => b,
        Err(e) => {
            warn!(
                error = %e,
                eve_character_id = character.eve_character_id,
                "location poller: failed to parse ESI response"
            );
            return cache_secs;
        }
    };

    let event = LocationEvent {
        eve_character_id: character.eve_character_id,
        solar_system_id: body.solar_system_id,
        station_id: body.station_id,
        structure_id: body.structure_id,
    };

    if let Some(sender) = state.location_subs.get(&character.eve_character_id) {
        // A lagged receiver error just means a slow consumer missed an event — not fatal.
        let _ = sender.send(event);
    }

    cache_secs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_max_age_standard() {
        assert_eq!(parse_max_age("public, max-age=5"), Some(5));
    }

    #[test]
    fn parse_max_age_only() {
        assert_eq!(parse_max_age("max-age=10"), Some(10));
    }

    #[test]
    fn parse_max_age_missing() {
        assert_eq!(parse_max_age("no-cache"), None);
    }

    #[test]
    fn parse_max_age_with_spaces() {
        assert_eq!(parse_max_age("public, max-age = 5"), Some(5));
    }

    #[test]
    fn broadcast_subscribe_creates_channel() {
        let subs: DashMap<i64, broadcast::Sender<LocationEvent>> = DashMap::new();
        let _rx = subscribe(&subs, 123456789);
        assert!(subs.contains_key(&123456789));
        assert_eq!(subs.get(&123456789).unwrap().receiver_count(), 1);
    }

    #[test]
    fn broadcast_subscribe_reuses_channel() {
        let subs: DashMap<i64, broadcast::Sender<LocationEvent>> = DashMap::new();
        let _rx1 = subscribe(&subs, 123456789);
        let _rx2 = subscribe(&subs, 123456789);
        assert_eq!(subs.get(&123456789).unwrap().receiver_count(), 2);
    }

    #[tokio::test]
    async fn broadcast_delivers_event() {
        let subs: DashMap<i64, broadcast::Sender<LocationEvent>> = DashMap::new();
        let mut rx = subscribe(&subs, 999);

        let event = LocationEvent {
            eve_character_id: 999,
            solar_system_id: 30000142,
            station_id: None,
            structure_id: None,
        };

        subs.get(&999).unwrap().send(event.clone()).unwrap();
        let received = rx.recv().await.unwrap();
        assert_eq!(received.solar_system_id, 30000142);
    }
}
