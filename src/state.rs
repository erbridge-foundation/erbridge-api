use std::sync::Arc;

use dashmap::DashMap;
use jsonwebtoken::jwk::JwkSet;
use reqwest::Client;
use sqlx::PgPool;
use tokio::sync::{RwLock, broadcast, mpsc};

use crate::{
    config::Config, esi::discovery::EsiMetadata, tasks::character_location_poll::LocationEvent,
};

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub http: Client,
    pub config: Config,
    pub esi_metadata: EsiMetadata,
    pub jwks: Arc<RwLock<JwkSet>>,
    /// Send a list of eve_character_ids into the online poller when an account
    /// logs in or adds a character. `None` in the poller `AppState` (which
    /// never sends); `Some` in the router `AppState`.
    pub online_poll_tx: Option<mpsc::Sender<Vec<i64>>>,
    /// Per-character broadcast channels for location events. Map sessions
    /// subscribe here; the location poller publishes here. Keyed by
    /// eve_character_id. The poller uses receiver_count() to skip idle characters.
    pub location_subs: Arc<DashMap<i64, broadcast::Sender<LocationEvent>>>,
}
