use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use tracing::debug;
use uuid::Uuid;
use validator::Validate;

use crate::{
    dto::{
        account::{ApiKeyCreatedResponse, ApiKeyEntry, ApiKeyListResponse, CreateApiKeyRequest},
        envelope::ApiResponse,
    },
    extractors::AccountId,
    services::account::{
        create_api_key as svc_create_api_key, list_api_keys as svc_list_api_keys,
        revoke_api_key as svc_revoke_api_key,
    },
    state::AppState,
};

pub async fn create_api_key(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    Json(body): Json<CreateApiKeyRequest>,
) -> Result<(StatusCode, Json<ApiResponse<ApiKeyCreatedResponse>>), StatusCode> {
    body.validate()
        .map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;

    debug!(
        "received API create request for account {} with name {}",
        account_id, &body.name
    );

    let created = svc_create_api_key(&state.db, account_id, &body.name, body.expires_days)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok((
        StatusCode::CREATED,
        Json(ApiResponse::ok(ApiKeyCreatedResponse {
            id: created.key.id,
            name: created.key.name,
            api_key: created.plaintext,
            expires_at: created.key.expires_at,
            created_at: created.key.created_at,
        })),
    ))
}

pub async fn list_api_keys_handler(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
) -> Result<Json<ApiResponse<ApiKeyListResponse>>, StatusCode> {
    let keys = svc_list_api_keys(&state.db, account_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let entries = keys
        .into_iter()
        .map(|k| ApiKeyEntry {
            id: k.id,
            name: k.name,
            expires_at: k.expires_at,
            created_at: k.created_at,
        })
        .collect();

    Ok(Json(ApiResponse::ok(ApiKeyListResponse {
        api_keys: entries,
    })))
}

pub async fn revoke_api_key(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    Path(key_id): Path<Uuid>,
) -> Result<StatusCode, StatusCode> {
    let deleted = svc_revoke_api_key(&state.db, account_id, key_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}
