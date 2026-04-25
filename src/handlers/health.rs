use axum::{Json, extract::State, http::StatusCode};
use std::sync::Arc;

use crate::{
    dto::health::{ComponentState, Components, HealthResponse},
    state::AppState,
};

pub async fn health(State(state): State<Arc<AppState>>) -> (StatusCode, Json<HealthResponse>) {
    let db_state = match sqlx::query("SELECT 1").execute(&state.db).await {
        Ok(_) => ComponentState::Ok,
        Err(_) => ComponentState::Degraded,
    };

    let overall = match db_state {
        ComponentState::Ok => ComponentState::Ok,
        ComponentState::Degraded => ComponentState::Degraded,
    };

    let http_status = match overall {
        ComponentState::Ok => StatusCode::OK,
        ComponentState::Degraded => StatusCode::SERVICE_UNAVAILABLE,
    };

    (
        http_status,
        Json(HealthResponse {
            status: overall,
            version: env!("CARGO_PKG_VERSION").to_string(),
            components: Components { database: db_state },
        }),
    )
}
