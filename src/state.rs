use std::sync::Arc;

use jsonwebtoken::jwk::JwkSet;
use reqwest::Client;
use sqlx::PgPool;
use tokio::sync::RwLock;

use crate::{config::Config, esi::discovery::EsiMetadata};

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub http: Client,
    pub config: Config,
    pub esi_metadata: EsiMetadata,
    pub jwks: Arc<RwLock<JwkSet>>,
}
