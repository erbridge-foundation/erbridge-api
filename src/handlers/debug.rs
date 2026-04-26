use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use tracing::{debug, info};

use crate::{state::AppState, tasks::character_location_poll::subscribe};

pub async fn location_subscribe(
    State(state): State<Arc<AppState>>,
    Path(character_id): Path<i64>,
) -> impl IntoResponse {
    let mut rx = subscribe(&state.location_subs, character_id);
    info!(eve_character_id = character_id, "debug: holding location subscription");

    // Wait for up to 60 seconds, logging each event received.
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(60);
    loop {
        match tokio::time::timeout_at(deadline, rx.recv()).await {
            Ok(Ok(event)) => {
                debug!(
                    eve_character_id = event.eve_character_id,
                    solar_system_id = event.solar_system_id,
                    station_id = ?event.station_id,
                    structure_id = ?event.structure_id,
                    "debug: location event received"
                );
            }
            Ok(Err(e)) => {
                debug!(error = %e, "debug: location subscription error");
                break;
            }
            Err(_) => {
                debug!(eve_character_id = character_id, "debug: location subscription timed out");
                break;
            }
        }
    }

    StatusCode::OK
}
