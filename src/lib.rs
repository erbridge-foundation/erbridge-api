pub mod config;
pub mod crypto;
pub mod db;
pub mod dto;
pub mod esi;
pub mod extractors;
pub mod handlers;
pub mod middleware;
pub mod services;
pub mod state;

use std::{collections::HashMap, sync::Arc};

use axum::{
    Router,
    middleware::from_fn_with_state,
    routing::{delete, get, patch, post, put},
};
use jsonwebtoken::jwk::JwkSet;
use reqwest::Client;
use sqlx::PgPool;
use tokio::sync::RwLock;

use crate::{config::Config, esi::discovery::EsiMetadata, state::AppState};

pub fn router(
    pool: PgPool,
    http: Client,
    config: Config,
    esi_metadata: EsiMetadata,
    jwks: Arc<RwLock<JwkSet>>,
) -> Router {
    let state = Arc::new(AppState {
        db: pool,
        http,
        config,
        esi_metadata,
        jwks,
    });

    Router::new()
        // Health
        .route("/api/health", get(handlers::health::health))
        // EVE image proxy (no auth — images are public)
        .with_state(state)
}
