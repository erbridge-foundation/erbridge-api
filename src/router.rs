use std::sync::Arc;

use axum::{Router, http::HeaderName};
use jsonwebtoken::jwk::JwkSet;
use reqwest::Client;
use sqlx::PgPool;
use tokio::sync::RwLock;
use tower_http::{
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer},
};
use utoipa::OpenApi;
use utoipa_axum::{router::OpenApiRouter, routes};
use utoipa_swagger_ui::SwaggerUi;

use crate::{config::Config, esi::discovery::EsiMetadata, handlers, openapi, state::AppState};
pub fn new_router(
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

    let public_open_api: OpenApiRouter<Arc<AppState>> =
        OpenApiRouter::with_openapi(openapi::ApiDoc::openapi())
            // Health
            .routes(routes!(handlers::health::health));

    // Merge OpenApi specs from all annotated branches into a single spec.
    let combined_open_api = public_open_api;

    let (public_and_auth, generated_api) = combined_open_api.split_for_parts();

    let public = public_and_auth
        .merge(SwaggerUi::new("/api/v1/swagger-ui").url("/api/v1/openapi.json", generated_api));

    let request_id = HeaderName::from_static("x-request-id");

    public
        .with_state(state)
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().include_headers(false))
                .on_response(DefaultOnResponse::new().level(tracing::Level::INFO)),
        )
        .layer(PropagateRequestIdLayer::new(request_id.clone()))
        .layer(SetRequestIdLayer::new(request_id, MakeRequestUuid))
}
