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
    db::account::request_account_deletion,
    db::character::{
        DeleteCharacterResult, delete_character, find_characters_by_account, set_main_character,
    },
    dto::character::{CharacterListResponse, CharacterResponse},
    dto::envelope::ApiResponse,
    esi::universe::resolve_names,
    extractors::{AccountId, SESSION_COOKIE},
    state::AppState,
};

// ---------------------------------------------------------------------------
// GET /api/v1/characters
// ---------------------------------------------------------------------------

pub async fn list_characters(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
) -> Result<Json<ApiResponse<CharacterListResponse>>, StatusCode> {
    let characters = find_characters_by_account(&state.db, &state.config.aes_key, account_id)
        .await
        .map_err(|e| {
            warn!(error = %e, %account_id, "failed to list characters");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Collect unique corp/alliance IDs to resolve in one ESI call.
    let mut ids: Vec<i64> = characters.iter().map(|c| c.corporation_id).collect();
    for c in &characters {
        if let Some(aid) = c.alliance_id {
            ids.push(aid);
        }
    }
    ids.sort_unstable();
    ids.dedup();

    let resolved = resolve_names(&state.http, &state.config.esi_base, ids)
        .await
        .map_err(|e| {
            warn!(error = %e, %account_id, "failed to resolve corp/alliance names from ESI");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let name_map: std::collections::HashMap<i64, String> =
        resolved.into_iter().map(|r| (r.id, r.name)).collect();
    let name_for = |id: i64| -> Option<String> { name_map.get(&id).cloned() };

    let items = characters
        .into_iter()
        .map(|c| CharacterResponse {
            id: c.id,
            eve_character_id: c.eve_character_id,
            name: c.name,
            corporation_id: c.corporation_id,
            corporation_name: name_for(c.corporation_id).unwrap_or_default(),
            alliance_id: c.alliance_id,
            alliance_name: c.alliance_id.and_then(name_for),
            is_main: c.is_main,
        })
        .collect();

    Ok(Json(ApiResponse::ok(CharacterListResponse {
        characters: items,
    })))
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/characters/:character_id
// ---------------------------------------------------------------------------

pub async fn remove_character(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    Path(character_id): Path<Uuid>,
) -> Result<StatusCode, StatusCode> {
    match delete_character(&state.db, account_id, character_id)
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
    set_main_character(&state.db, account_id, character_id)
        .await
        .map_err(|e| {
            warn!(error = %e, %account_id, %character_id, "failed to set main character");
            // The service returns an error if the character doesn't belong to
            // this account — surface that as 404.
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
    let updated = request_account_deletion(&state.db, account_id, Some(account_id))
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

// ---------------------------------------------------------------------------
// DELETE /api/v1/admin/accounts/:id/purge  (placeholder — requires admin role)
// ---------------------------------------------------------------------------

pub async fn admin_purge_account(
    State(state): State<Arc<AppState>>,
    AccountId(_account_id): AccountId,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, StatusCode> {
    // TODO(US-admin-roles): gate on admin role once that story lands.
    let _ = (&state, id);
    Err(StatusCode::FORBIDDEN)
}

// ---------------------------------------------------------------------------
// POST /api/v1/admin/accounts/:id/restore  (placeholder — requires admin role)
// ---------------------------------------------------------------------------

pub async fn admin_restore_account(
    State(state): State<Arc<AppState>>,
    AccountId(_account_id): AccountId,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, StatusCode> {
    // TODO(US-admin-roles): gate on admin role once that story lands.
    let _ = (&state, id);
    Err(StatusCode::FORBIDDEN)
}
