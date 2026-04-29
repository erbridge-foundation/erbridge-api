use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use serde::Serialize;
use std::sync::Arc;
use tracing::warn;
use uuid::Uuid;

use crate::{
    db::character::DeleteCharacterResult,
    dto::character::CharacterListResponse,
    dto::envelope::ApiResponse,
    extractors::{AccountId, SESSION_COOKIE},
    services::account::request_deletion as svc_request_deletion,
    services::character::{
        list_for_account as svc_list_for_account, remove_character as svc_remove_character,
        set_main as svc_set_main,
    },
    state::AppState,
};

// ---------------------------------------------------------------------------
// GET /api/v1/characters
// ---------------------------------------------------------------------------

pub async fn list_characters(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
) -> Result<Json<ApiResponse<CharacterListResponse>>, StatusCode> {
    let characters = svc_list_for_account(
        &state.db,
        &state.config.aes_key,
        &state.http,
        &state.config.esi_base,
        account_id,
    )
    .await
    .map_err(|e| {
        warn!(error = %e, %account_id, "failed to list characters");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(ApiResponse::ok(CharacterListResponse { characters })))
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/characters/:character_id
// ---------------------------------------------------------------------------

pub async fn remove_character(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    Path(character_id): Path<Uuid>,
) -> Result<StatusCode, StatusCode> {
    match svc_remove_character(&state.db, account_id, character_id)
        .await
        .map_err(|e| {
            warn!(error = %e, %account_id, %character_id, "failed to delete character");
            StatusCode::INTERNAL_SERVER_ERROR
        })? {
        DeleteCharacterResult::Deleted => Ok(StatusCode::NO_CONTENT),
        DeleteCharacterResult::NotFound => Err(StatusCode::NOT_FOUND),
        DeleteCharacterResult::IsMain => Err(StatusCode::UNPROCESSABLE_ENTITY),
    }
}

// ---------------------------------------------------------------------------
// PUT /api/v1/characters/:character_id/main
// ---------------------------------------------------------------------------

pub async fn set_main(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    Path(character_id): Path<Uuid>,
) -> Result<StatusCode, StatusCode> {
    svc_set_main(&state.db, account_id, character_id)
        .await
        .map_err(|e| {
            warn!(error = %e, %account_id, %character_id, "failed to set main character");
            // Returns an error if the character doesn't belong to this account — surface as 404.
            StatusCode::NOT_FOUND
        })?;

    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/me
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct DeleteAccountResponse {
    message: String,
}

pub async fn delete_account(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    jar: CookieJar,
) -> Result<impl IntoResponse, StatusCode> {
    let updated = svc_request_deletion(&state.db, account_id)
        .await
        .map_err(|e| {
            warn!(error = %e, %account_id, "failed to request account deletion");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    if !updated {
        return Err(StatusCode::NOT_FOUND);
    }

    let jar = jar.remove(
        Cookie::build(SESSION_COOKIE)
            .path("/")
            .same_site(SameSite::Lax),
    );

    let body = Json(ApiResponse::ok(DeleteAccountResponse {
        message: format!(
            "Your account has been marked for deletion and will be permanently removed after {} days. \
             Log in again before then to cancel.",
            state.config.account_deletion_grace_days
        ),
    }));

    Ok((jar, body))
}
