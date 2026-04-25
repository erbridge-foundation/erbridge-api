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
