use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use std::sync::Arc;
use uuid::Uuid;

use validator::Validate;

use crate::{
    db::acl::find_acls_manageable_by_account,
    db::acl_member::{AclPermission, MemberType, find_members_by_acl},
    dto::acl::{
        AclListResponse, AclMemberListResponse, AclMemberResponse, AclResponse, AddMemberRequest,
        CreateAclRequest, RenameAclRequest, UpdateMemberRequest,
    },
    dto::envelope::ApiResponse,
    extractors::AccountId,
    services::acl::{
        AclError, AddMemberInput, add_member, assert_acl_list_members_permission, create_acl,
        delete_acl, remove_member, rename_acl, update_member_permission,
    },
    state::AppState,
};

// ---------------------------------------------------------------------------
// GET /api/v1/acls
// ---------------------------------------------------------------------------

pub async fn list_acls(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
) -> Result<Json<ApiResponse<AclListResponse>>, AclError> {
    let acls = find_acls_manageable_by_account(&state.db, account_id)
        .await
        .map_err(AclError::Internal)?;

    Ok(Json(ApiResponse::ok(AclListResponse {
        acls: acls.into_iter().map(AclResponse::from).collect(),
    })))
}

// ---------------------------------------------------------------------------
// POST /api/v1/acls
// ---------------------------------------------------------------------------

pub async fn create(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    Json(body): Json<CreateAclRequest>,
) -> Result<(StatusCode, Json<ApiResponse<AclResponse>>), AclError> {
    body.validate()
        .map_err(|e| AclError::Validation(e.to_string()))?;

    let acl = create_acl(&state.db, account_id, &body.name).await?;

    Ok((
        StatusCode::CREATED,
        Json(ApiResponse::ok(AclResponse::from(acl))),
    ))
}

// ---------------------------------------------------------------------------
// PUT /api/v1/acls/:acl_id
// ---------------------------------------------------------------------------

pub async fn rename(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    Path(acl_id): Path<Uuid>,
    Json(body): Json<RenameAclRequest>,
) -> Result<Json<ApiResponse<AclResponse>>, AclError> {
    body.validate()
        .map_err(|e| AclError::Validation(e.to_string()))?;

    let acl = rename_acl(&state.db, acl_id, account_id, &body.name).await?;

    Ok(Json(ApiResponse::ok(AclResponse::from(acl))))
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/acls/:acl_id
// ---------------------------------------------------------------------------

pub async fn delete(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    Path(acl_id): Path<Uuid>,
) -> Result<StatusCode, AclError> {
    delete_acl(&state.db, acl_id, account_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// GET /api/v1/acls/:acl_id/members
// ---------------------------------------------------------------------------

pub async fn list_members(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    Path(acl_id): Path<Uuid>,
) -> Result<Json<ApiResponse<AclMemberListResponse>>, AclError> {
    assert_acl_list_members_permission(&state.db, acl_id, account_id).await?;

    let members = find_members_by_acl(&state.db, acl_id)
        .await
        .map_err(AclError::Internal)?;

    Ok(Json(ApiResponse::ok(AclMemberListResponse {
        members: members.into_iter().map(AclMemberResponse::from).collect(),
    })))
}

// ---------------------------------------------------------------------------
// POST /api/v1/acls/:acl_id/members
// ---------------------------------------------------------------------------

pub async fn add(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    Path(acl_id): Path<Uuid>,
    Json(body): Json<AddMemberRequest>,
) -> Result<(StatusCode, Json<ApiResponse<AclMemberResponse>>), AclError> {
    let member_type = body
        .member_type
        .parse::<MemberType>()
        .map_err(|_| AclError::Validation(format!("invalid member_type: {}", body.member_type)))?;
    let permission = body
        .permission
        .parse::<AclPermission>()
        .map_err(|_| AclError::Validation(format!("invalid permission: {}", body.permission)))?;

    let member = add_member(
        &state.db,
        &state.http,
        &state.config.esi_base,
        acl_id,
        account_id,
        AddMemberInput {
            member_type,
            eve_entity_id: body.eve_entity_id,
            permission,
        },
    )
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(ApiResponse::ok(AclMemberResponse::from(member))),
    ))
}

// ---------------------------------------------------------------------------
// PATCH /api/v1/acls/:acl_id/members/:member_id
// ---------------------------------------------------------------------------

pub async fn update_member(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    Path((acl_id, member_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateMemberRequest>,
) -> Result<Json<ApiResponse<AclMemberResponse>>, AclError> {
    let permission = body
        .permission
        .parse::<AclPermission>()
        .map_err(|_| AclError::Validation(format!("invalid permission: {}", body.permission)))?;

    let member =
        update_member_permission(&state.db, acl_id, member_id, account_id, permission).await?;

    Ok(Json(ApiResponse::ok(AclMemberResponse::from(member))))
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/acls/:acl_id/members/:member_id
// ---------------------------------------------------------------------------

pub async fn delete_member(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    Path((acl_id, member_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AclError> {
    remove_member(&state.db, acl_id, member_id, account_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
