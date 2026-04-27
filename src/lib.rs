pub mod audit;
pub mod config;
pub mod crypto;
pub mod db;
pub mod dto;
pub mod esi;
pub mod extractors;
pub mod handlers;
pub mod middleware;
pub mod permissions;
pub mod services;
pub mod state;
pub mod tasks;

use std::sync::Arc;

use axum::{
    Router,
    middleware::from_fn_with_state,
    routing::{delete, get, patch, post, put},
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
        online_poll_tx: Some(online_poll_tx),
        location_subs,
    });

    build_router(state)
}

fn build_router(state: Arc<AppState>) -> Router {
    let authenticated = Router::new()
        // Account / character management
        .route("/api/v1/me", get(handlers::auth::me))
        .route("/api/v1/me", delete(handlers::character::delete_account))
        .route(
            "/api/v1/characters",
            get(handlers::character::list_characters),
        )
        .route(
            "/api/v1/characters/{character_id}",
            delete(handlers::character::remove_character),
        )
        .route(
            "/api/v1/characters/{character_id}/main",
            put(handlers::character::set_main),
        )
        // Auth endpoints that require an authenticated account
        .route("/auth/characters/add", get(handlers::auth::add_character))
        .route("/auth/logout", post(handlers::auth::logout))
        // Map CRUD
        .route("/api/v1/maps", get(handlers::map::list_maps_handler))
        .route("/api/v1/maps", post(handlers::map::create_map_handler))
        .route("/api/v1/maps/{map_id}", get(handlers::map::get_map_handler))
        .route(
            "/api/v1/maps/{map_id}",
            patch(handlers::map::update_map_handler),
        )
        .route(
            "/api/v1/maps/{map_id}",
            delete(handlers::map::delete_map_handler),
        )
        // Map–ACL attachment
        .route(
            "/api/v1/maps/{map_id}/acls",
            post(handlers::map::attach_acl),
        )
        .route(
            "/api/v1/maps/{map_id}/acls/{acl_id}",
            delete(handlers::map::detach_acl),
        )
        // ACL management
        .route("/api/v1/acls", get(handlers::acl::list_acls))
        .route("/api/v1/acls", post(handlers::acl::create))
        .route("/api/v1/acls/{acl_id}", put(handlers::acl::rename))
        .route("/api/v1/acls/{acl_id}", delete(handlers::acl::delete))
        .route(
            "/api/v1/acls/{acl_id}/members",
            get(handlers::acl::list_members),
        )
        .route("/api/v1/acls/{acl_id}/members", post(handlers::acl::add))
        .route(
            "/api/v1/acls/{acl_id}/members/{member_id}",
            patch(handlers::acl::update_member),
        )
        .route(
            "/api/v1/acls/{acl_id}/members/{member_id}",
            delete(handlers::acl::delete_member),
        )
        // Connection and signature operations
        .route(
            "/api/v1/maps/{map_id}/connections",
            post(handlers::map::create_connection),
        )
        .route(
            "/api/v1/maps/{map_id}/connections/{conn_id}",
            delete(handlers::map::delete_connection_handler),
        )
        .route(
            "/api/v1/maps/{map_id}/signatures",
            post(handlers::map::add_signature),
        )
        .route(
            "/api/v1/maps/{map_id}/signatures/{sig_id}",
            delete(handlers::map::delete_signature_handler),
        )
        .route(
            "/api/v1/maps/{map_id}/connections/{conn_id}/link",
            post(handlers::map::link_signature),
        )
        .route(
            "/api/v1/maps/{map_id}/connections/{conn_id}/metadata",
            patch(handlers::map::update_connection_metadata),
        )
        // Route finding
        .route(
            "/api/v1/maps/{map_id}/routes",
            get(handlers::map::find_routes),
        )
        .layer(from_fn_with_state(
            Arc::clone(&state),
            middleware::require_active_account,
        ));

    #[cfg_attr(not(debug_assertions), allow(unused_mut))]
    let mut public = Router::new()
        // Health
        .route("/api/health", get(handlers::health::health))
        .route("/auth/login", get(handlers::auth::login))
        .route("/auth/callback", get(handlers::auth::callback));

    #[cfg(debug_assertions)]
    {
        public = public.route(
            "/debug/location-subscribe/{character_id}",
            get(handlers::debug::location_subscribe),
        );
    }

    public.merge(authenticated).with_state(state)
}
