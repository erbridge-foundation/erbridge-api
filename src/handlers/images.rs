use axum::{
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use std::sync::Arc;

use crate::services::images::{ImageError, fetch_image};
use crate::state::AppState;

const CACHE_MAX_AGE: &str = "public, max-age=3600";

#[derive(Deserialize)]
pub struct ImageParams {
    size: Option<u32>,
    tenant: Option<String>,
}

/// GET /api/v1/images/{category}/{id}/{variation}
///
/// Valid combinations:
///   characters/{id}/portrait
///   corporations/{id}/logo
///   alliances/{id}/logo
///   types/{id}/render | icon | bp | bpc | relic
pub async fn image(
    State(state): State<Arc<AppState>>,
    Path((category, id, variation)): Path<(String, i64, String)>,
    Query(params): Query<ImageParams>,
) -> Response {
    if !is_valid_combination(&category, &variation) {
        return StatusCode::NOT_FOUND.into_response();
    }

    let mut upstream_url = format!(
        "https://images.evetech.net/{}/{}/{}",
        category, id, variation
    );

    let mut query_parts: Vec<String> = Vec::new();
    if let Some(size) = params.size {
        query_parts.push(format!("size={}", size));
    }
    if let Some(tenant) = &params.tenant {
        query_parts.push(format!("tenant={}", tenant));
    }
    if !query_parts.is_empty() {
        upstream_url.push('?');
        upstream_url.push_str(&query_parts.join("&"));
    }

    match fetch_image(
        &state,
        &category,
        id,
        &variation,
        params.size,
        &upstream_url,
    )
    .await
    {
        Ok((data, content_type)) => image_response(data, &content_type),
        Err(
            ImageError::UpstreamRequestFailed
            | ImageError::UpstreamErrorStatus
            | ImageError::UpstreamBodyFailed,
        ) => StatusCode::BAD_GATEWAY.into_response(),
    }
}

fn is_valid_combination(category: &str, variation: &str) -> bool {
    match (category, variation) {
        ("characters", "portrait") => true,
        ("corporations", "logo") | ("alliances", "logo") => true,
        ("types", "render")
        | ("types", "icon")
        | ("types", "bp")
        | ("types", "bpc")
        | ("types", "relic") => true,
        _ => false,
    }
}

fn image_response(data: Vec<u8>, content_type: &str) -> Response {
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, CACHE_MAX_AGE),
        ],
        data,
    )
        .into_response()
}
