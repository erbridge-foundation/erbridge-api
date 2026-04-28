use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use uuid::Uuid;

use crate::{
    dto::{
        admin::{
            AdminAccountListResponse, AdminAccountResponse, AdminAclListResponse, AdminAclResponse,
            AdminMapListResponse, AdminMapResponse, AuditLogEntryResponse, AuditLogListResponse,
            AuditLogQueryParams, BlockEveCharacterRequest, BlockedEveCharacterListResponse,
            BlockedEveCharacterResponse, ChangeAclOwnerRequest, ChangeMapOwnerRequest,
        },
        envelope::ApiResponse,
    },
    extractors::ServerAdmin,
    services::admin::{
        AdminError, admin_block_eve_character, admin_change_acl_owner, admin_change_map_owner,
        admin_grant_admin, admin_hard_delete_acl, admin_hard_delete_map, admin_list_accounts,
        admin_list_acls, admin_list_audit_log, admin_list_blocked_eve_characters, admin_list_maps,
        admin_revoke_admin, admin_unblock_eve_character,
    },
    state::AppState,
};

// ---------------------------------------------------------------------------
// GET /api/v1/admin/maps
// ---------------------------------------------------------------------------

pub async fn list_maps(
    State(state): State<Arc<AppState>>,
    ServerAdmin(_admin_id): ServerAdmin,
) -> Result<Json<ApiResponse<AdminMapListResponse>>, AdminError> {
    let maps = admin_list_maps(&state.db).await?;
    Ok(Json(ApiResponse::ok(AdminMapListResponse {
        maps: maps.into_iter().map(AdminMapResponse::from).collect(),
    })))
}

// ---------------------------------------------------------------------------
// PATCH /api/v1/admin/maps/{map_id}/owner
// ---------------------------------------------------------------------------

pub async fn change_map_owner(
    State(state): State<Arc<AppState>>,
    ServerAdmin(admin_id): ServerAdmin,
    Path(map_id): Path<Uuid>,
    Json(body): Json<ChangeMapOwnerRequest>,
) -> Result<StatusCode, AdminError> {
    admin_change_map_owner(&state.db, admin_id, map_id, body.new_owner_account_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/admin/maps/{map_id}
// ---------------------------------------------------------------------------

pub async fn hard_delete_map(
    State(state): State<Arc<AppState>>,
    ServerAdmin(admin_id): ServerAdmin,
    Path(map_id): Path<Uuid>,
) -> Result<StatusCode, AdminError> {
    admin_hard_delete_map(&state.db, admin_id, map_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// GET /api/v1/admin/acls
// ---------------------------------------------------------------------------

pub async fn list_acls(
    State(state): State<Arc<AppState>>,
    ServerAdmin(_admin_id): ServerAdmin,
) -> Result<Json<ApiResponse<AdminAclListResponse>>, AdminError> {
    let acls = admin_list_acls(&state.db).await?;
    Ok(Json(ApiResponse::ok(AdminAclListResponse {
        acls: acls.into_iter().map(AdminAclResponse::from).collect(),
    })))
}

// ---------------------------------------------------------------------------
// PATCH /api/v1/admin/acls/{acl_id}/owner
// ---------------------------------------------------------------------------

pub async fn change_acl_owner(
    State(state): State<Arc<AppState>>,
    ServerAdmin(admin_id): ServerAdmin,
    Path(acl_id): Path<Uuid>,
    Json(body): Json<ChangeAclOwnerRequest>,
) -> Result<StatusCode, AdminError> {
    admin_change_acl_owner(&state.db, admin_id, acl_id, body.new_owner_account_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/admin/acls/{acl_id}
// ---------------------------------------------------------------------------

pub async fn hard_delete_acl(
    State(state): State<Arc<AppState>>,
    ServerAdmin(admin_id): ServerAdmin,
    Path(acl_id): Path<Uuid>,
) -> Result<StatusCode, AdminError> {
    admin_hard_delete_acl(&state.db, admin_id, acl_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// GET /api/v1/admin/characters/blocked
// ---------------------------------------------------------------------------

pub async fn list_blocked_characters(
    State(state): State<Arc<AppState>>,
    ServerAdmin(_admin_id): ServerAdmin,
) -> Result<Json<ApiResponse<BlockedEveCharacterListResponse>>, AdminError> {
    let blocked = admin_list_blocked_eve_characters(&state.db).await?;
    Ok(Json(ApiResponse::ok(BlockedEveCharacterListResponse {
        blocked: blocked
            .into_iter()
            .map(BlockedEveCharacterResponse::from)
            .collect(),
    })))
}

// ---------------------------------------------------------------------------
// POST /api/v1/admin/characters/{eve_id}/block
// ---------------------------------------------------------------------------

pub async fn block_character(
    State(state): State<Arc<AppState>>,
    ServerAdmin(admin_id): ServerAdmin,
    Path(eve_character_id): Path<i64>,
    body: Option<Json<BlockEveCharacterRequest>>,
) -> Result<StatusCode, AdminError> {
    let reason = body.and_then(|Json(b)| b.reason);
    admin_block_eve_character(&state.db, admin_id, eve_character_id, reason).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// POST /api/v1/admin/characters/{eve_id}/unblock
// ---------------------------------------------------------------------------

pub async fn unblock_character(
    State(state): State<Arc<AppState>>,
    ServerAdmin(admin_id): ServerAdmin,
    Path(eve_character_id): Path<i64>,
) -> Result<StatusCode, AdminError> {
    admin_unblock_eve_character(&state.db, admin_id, eve_character_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// GET /api/v1/admin/accounts
// ---------------------------------------------------------------------------

pub async fn list_accounts(
    State(state): State<Arc<AppState>>,
    ServerAdmin(_admin_id): ServerAdmin,
) -> Result<Json<ApiResponse<AdminAccountListResponse>>, AdminError> {
    let accounts = admin_list_accounts(&state.db).await?;
    Ok(Json(ApiResponse::ok(AdminAccountListResponse {
        accounts: accounts
            .into_iter()
            .map(AdminAccountResponse::from)
            .collect(),
    })))
}

// ---------------------------------------------------------------------------
// POST /api/v1/admin/accounts/{account_id}/grant-admin
// ---------------------------------------------------------------------------

pub async fn grant_admin(
    State(state): State<Arc<AppState>>,
    ServerAdmin(admin_id): ServerAdmin,
    Path(target): Path<Uuid>,
) -> Result<StatusCode, AdminError> {
    admin_grant_admin(&state.db, admin_id, target).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// POST /api/v1/admin/accounts/{account_id}/revoke-admin
// ---------------------------------------------------------------------------

pub async fn revoke_admin(
    State(state): State<Arc<AppState>>,
    ServerAdmin(admin_id): ServerAdmin,
    Path(target): Path<Uuid>,
) -> Result<StatusCode, AdminError> {
    admin_revoke_admin(&state.db, admin_id, target).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// GET /api/v1/admin/audit-log
// ---------------------------------------------------------------------------

pub async fn list_audit_log(
    State(state): State<Arc<AppState>>,
    ServerAdmin(_admin_id): ServerAdmin,
    Query(params): Query<AuditLogQueryParams>,
) -> Result<Json<ApiResponse<AuditLogListResponse>>, AdminError> {
    let entries = admin_list_audit_log(
        &state.db,
        params.event_type,
        params.actor,
        params.before,
        params.limit,
    )
    .await?;

    let next_before = entries.last().map(|e| e.occurred_at);

    Ok(Json(ApiResponse::ok(AuditLogListResponse {
        entries: entries
            .into_iter()
            .map(AuditLogEntryResponse::from)
            .collect(),
        next_before,
    })))
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/admin/accounts/{account_id}/purge
// ---------------------------------------------------------------------------

pub async fn purge_account(
    State(_state): State<Arc<AppState>>,
    ServerAdmin(_admin_id): ServerAdmin,
    Path(_id): Path<Uuid>,
) -> StatusCode {
    // TODO: implement hard-purge of pending-delete accounts
    StatusCode::NOT_IMPLEMENTED
}

// ---------------------------------------------------------------------------
// POST /api/v1/admin/accounts/{account_id}/restore
// ---------------------------------------------------------------------------

pub async fn restore_account(
    State(_state): State<Arc<AppState>>,
    ServerAdmin(_admin_id): ServerAdmin,
    Path(_id): Path<Uuid>,
) -> StatusCode {
    // TODO: implement restore of pending-delete accounts
    StatusCode::NOT_IMPLEMENTED
}
