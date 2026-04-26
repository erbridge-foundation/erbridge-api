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
    routing::{delete, get, patch, post},
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

/// Build the router from an already-constructed `Arc<AppState>`.
/// Useful in tests where the state is built directly.
pub fn router_from_state(state: Arc<AppState>) -> Router {
    build_router(state)
}

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

    build_router(state)
}

fn build_router(state: Arc<AppState>) -> Router {
    // Routes that require an active (non-pending-delete) account.
    let authenticated = Router::new()
        // Account / character management
        .route("/api/v1/me", get(handlers::auth::me))
        .route("/api/v1/me", delete(handlers::character::delete_account))
        // Auth endpoints that required an authenticated account
        .route("/auth/characters/add", get(handlers::auth::add_character))
        .route("/auth/logout", post(handlers::auth::logout))
        // Map CRUD
        .route("/api/v1/maps", post(handlers::map::create_map))
        .route("/api/v1/maps", get(handlers::map::list_maps))
        .route("/api/v1/maps/{map_id}", get(handlers::map::get_map))
        .route("/api/v1/maps/{map_id}", delete(handlers::map::delete_map))
        // Connection and signature operations
        .route("/api/v1/maps/{map_id}/connections", post(handlers::map::create_connection))
        .route("/api/v1/maps/{map_id}/signatures", post(handlers::map::add_signature))
        .route(
            "/api/v1/maps/{map_id}/connections/{conn_id}/link",
            post(handlers::map::link_signature),
        )
        .route(
            "/api/v1/maps/{map_id}/connections/{conn_id}/metadata",
            patch(handlers::map::update_connection_metadata),
        )
        // Route finding
        .route("/api/v1/maps/{map_id}/routes", get(handlers::map::find_routes))
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
