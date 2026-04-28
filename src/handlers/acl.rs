use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use std::sync::Arc;
use tracing::warn;
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
) -> Result<Json<ApiResponse<AclListResponse>>, StatusCode> {
    let acls = find_acls_manageable_by_account(&state.db, account_id)
        .await
        .map_err(|e| {
            warn!(error = %e, %account_id, "failed to list acls");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

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
) -> Result<(StatusCode, Json<ApiResponse<AclResponse>>), (StatusCode, Json<ApiResponse<()>>)> {
    body.validate().map_err(|e| {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ApiResponse::error(e.to_string())),
        )
    })?;

    let acl = create_acl(&state.db, account_id, &body.name)
        .await
        .map_err(|e| {
            warn!(error = %e, %account_id, "failed to create acl");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error(e.to_string())),
            )
        })?;

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
) -> Result<Json<ApiResponse<AclResponse>>, (StatusCode, Json<ApiResponse<()>>)> {
    body.validate().map_err(|e| {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ApiResponse::error(e.to_string())),
        )
    })?;

    let acl = rename_acl(&state.db, acl_id, account_id, &body.name)
        .await
        .map_err(|e| {
            warn!(error = %e, %acl_id, %account_id, "failed to rename acl");
            match e {
                AclError::NotFound => (
                    StatusCode::NOT_FOUND,
                    Json(ApiResponse::error(e.to_string())),
                ),
                AclError::Forbidden => (
                    StatusCode::FORBIDDEN,
                    Json(ApiResponse::error(e.to_string())),
                ),
                _ => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiResponse::error(e.to_string())),
                ),
            }
        })?;

    Ok(Json(ApiResponse::ok(AclResponse::from(acl))))
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/acls/:acl_id
// ---------------------------------------------------------------------------

pub async fn delete(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    Path(acl_id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<ApiResponse<()>>)> {
    delete_acl(&state.db, acl_id, account_id)
        .await
        .map_err(|e| {
            warn!(error = %e, %acl_id, %account_id, "failed to delete acl");
            match e {
                AclError::NotFound => (
                    StatusCode::NOT_FOUND,
                    Json(ApiResponse::error(e.to_string())),
                ),
                AclError::Forbidden => (
                    StatusCode::FORBIDDEN,
                    Json(ApiResponse::error(e.to_string())),
                ),
                _ => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiResponse::error(e.to_string())),
                ),
            }
        })?;

    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// GET /api/v1/acls/:acl_id/members
// ---------------------------------------------------------------------------

pub async fn list_members(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    Path(acl_id): Path<Uuid>,
) -> Result<Json<ApiResponse<AclMemberListResponse>>, StatusCode> {
    assert_acl_list_members_permission(&state.db, acl_id, account_id)
        .await
        .map_err(|e| match e {
            AclError::NotFound => StatusCode::NOT_FOUND,
            AclError::Forbidden => StatusCode::FORBIDDEN,
            e => {
                warn!(error = %e, %acl_id, "failed to check acl list_members permission");
                StatusCode::INTERNAL_SERVER_ERROR
            }
        })?;

    let members = find_members_by_acl(&state.db, acl_id).await.map_err(|e| {
        warn!(error = %e, %acl_id, "failed to list acl members");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

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
) -> Result<(StatusCode, Json<ApiResponse<AclMemberResponse>>), (StatusCode, Json<ApiResponse<()>>)>
{
    let member_type = body.member_type.parse::<MemberType>().map_err(|_| {
        let msg = format!("invalid member_type: {}", body.member_type);
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ApiResponse::error(msg)),
        )
    })?;
    let permission = body.permission.parse::<AclPermission>().map_err(|_| {
        let msg = format!("invalid permission: {}", body.permission);
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ApiResponse::error(msg)),
        )
    })?;

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
    .await
    .map_err(|e| {
        warn!(error = %e, %acl_id, %account_id, "failed to add acl member");
        match e {
            AclError::InvalidPermissionForType(_) | AclError::DuplicateMember => (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(ApiResponse::error(e.to_string())),
            ),
            AclError::Forbidden => (
                StatusCode::FORBIDDEN,
                Json(ApiResponse::error(e.to_string())),
            ),
            AclError::NotFound => (
                StatusCode::NOT_FOUND,
                Json(ApiResponse::error(e.to_string())),
            ),
            _ => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error(e.to_string())),
            ),
        }
    })?;

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
) -> Result<Json<ApiResponse<AclMemberResponse>>, (StatusCode, Json<ApiResponse<()>>)> {
    let permission = body.permission.parse::<AclPermission>().map_err(|_| {
        let msg = format!("invalid permission: {}", body.permission);
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ApiResponse::error(msg)),
        )
    })?;

    let member = update_member_permission(
        &state.db,
        acl_id,
        member_id,
        account_id,
        permission,
    )
    .await
    .map_err(|e| {
        warn!(error = %e, %acl_id, %member_id, %account_id, "failed to update acl member permission");
        match e {
            AclError::InvalidPermissionForType(_) => {
                (StatusCode::UNPROCESSABLE_ENTITY, Json(ApiResponse::error(e.to_string())))
            }
            AclError::Forbidden => (StatusCode::FORBIDDEN, Json(ApiResponse::error(e.to_string()))),
            AclError::NotFound | AclError::MemberAclMismatch => {
                (StatusCode::NOT_FOUND, Json(ApiResponse::error(e.to_string())))
            }
            _ => (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiResponse::error(e.to_string()))),
        }
    })?;

    Ok(Json(ApiResponse::ok(AclMemberResponse::from(member))))
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/acls/:acl_id/members/:member_id
// ---------------------------------------------------------------------------

pub async fn delete_member(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    Path((acl_id, member_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, (StatusCode, Json<ApiResponse<()>>)> {
    remove_member(&state.db, acl_id, member_id, account_id)
        .await
        .map_err(|e| {
            warn!(error = %e, %acl_id, %member_id, %account_id, "failed to remove acl member");
            match e {
                AclError::Forbidden => (
                    StatusCode::FORBIDDEN,
                    Json(ApiResponse::error(e.to_string())),
                ),
                AclError::NotFound | AclError::MemberAclMismatch => (
                    StatusCode::NOT_FOUND,
                    Json(ApiResponse::error(e.to_string())),
                ),
                _ => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiResponse::error(e.to_string())),
                ),
            }
        })?;

    Ok(StatusCode::NO_CONTENT)
}
