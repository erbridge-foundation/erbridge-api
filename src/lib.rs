pub mod audit;
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
pub mod tasks;

use std::sync::Arc;

use axum::{
    Router,
    middleware::from_fn_with_state,
    routing::{delete, get, post},
};
use dashmap::DashMap;
use jsonwebtoken::jwk::JwkSet;
use reqwest::Client;
use sqlx::PgPool;
use tokio::sync::{RwLock, broadcast, mpsc};

use crate::{
    config::Config, esi::discovery::EsiMetadata, state::AppState,
    tasks::character_location_poll::LocationEvent,
};

pub fn router(
    pool: PgPool,
    http: Client,
    config: Config,
    esi_metadata: EsiMetadata,
    jwks: Arc<RwLock<JwkSet>>,
    online_poll_tx: mpsc::Sender<Vec<i64>>,
    location_subs: Arc<DashMap<i64, broadcast::Sender<LocationEvent>>>,
) -> Router {
    let state = Arc::new(AppState {
        db: pool,
        http,
        config,
        esi_metadata,
        jwks,
        online_poll_tx,
        location_subs,
    });

    // Routes that require an active (non-pending-delete) account.
    let authenticated = Router::new()
        // Account / character management
        .route("/api/v1/me", get(handlers::auth::me))
        .route("/api/v1/me", delete(handlers::character::delete_account))
        // Auth endpoints that required an authenticated account
        .route("/auth/characters/add", get(handlers::auth::add_character))
        .route("/auth/logout", post(handlers::auth::logout))
        .layer(from_fn_with_state(
            Arc::clone(&state),
            middleware::require_active_account,
        ));

    Router::new()
        // Health
        .route("/api/health", get(handlers::health::health))
        // Debug (temporary)
        .route(
            "/debug/location-subscribe/{character_id}",
            get(handlers::debug::location_subscribe),
        )
        // EVE SSO auth flow
        .route("/auth/login", get(handlers::auth::login))
        .route("/auth/callback", get(handlers::auth::callback))
        // EVE image proxy (no auth — images are public)
        .route(
            "/api/v1/images/{category}/{id}/{variation}",
            get(handlers::images::image),
        )
        .merge(authenticated)
        .with_state(state)
}
