use anyhow::Context;
use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use thiserror::Error;
use tracing::{info, instrument, warn};
use uuid::Uuid;

use crate::audit::{self, AuditEvent, AuditLogEntry, ServerAdminGrantSource};
use crate::db::account::{Account, BlockedEveCharacter};
use crate::db::acl::Acl;
use crate::db::map::Map;
use crate::db::{account as db_account, acl as db_acl, map as db_map};
use crate::dto::envelope::ApiResponse;

#[derive(Debug, Error)]
pub enum AdminError {
    #[error("not found")]
    NotFound,
    #[error("target account not found")]
    TargetAccountNotFound,
    #[error("cannot revoke the last server admin")]
    LastAdmin,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl IntoResponse for AdminError {
    fn into_response(self) -> Response {
        let status = match &self {
            AdminError::NotFound => StatusCode::NOT_FOUND,
            AdminError::TargetAccountNotFound => StatusCode::UNPROCESSABLE_ENTITY,
            AdminError::LastAdmin => StatusCode::CONFLICT,
            AdminError::Internal(_) => {
                warn!(error = %self, "internal admin error");
                StatusCode::INTERNAL_SERVER_ERROR
            }
        };
        (status, Json(ApiResponse::<()>::error(self.to_string()))).into_response()
    }
}

/// Lists every map on the instance, including soft-deleted rows. The admin
/// list bypasses ACL/ownership filtering by design (see DECISIONS.md).
#[instrument(skip(pool), err)]
pub async fn admin_list_maps(pool: &PgPool) -> Result<Vec<Map>, AdminError> {
    db_map::find_all_maps_admin(pool)
        .await
        .context("failed to list maps for admin")
        .map_err(AdminError::Internal)
}

/// Reassigns map ownership to `new_owner`. Emits `AdminMapOwnershipChanged`
/// with the previous owner (which may be `NULL` if the map had been
/// orphaned by a prior account purge — in that case no audit event is
/// emitted because the variant requires a concrete `old_owner`).
///
/// Returns `Err(NotFound)` if the map id does not exist (including
/// soft-deleted maps — admins can transfer ownership of soft-deleted maps).
/// Returns `Err(TargetAccountNotFound)` if `new_owner` is not a real account.
#[instrument(skip(pool), err)]
pub async fn admin_change_map_owner(
    pool: &PgPool,
    actor: Uuid,
    map_id: Uuid,
    new_owner: Uuid,
) -> Result<(), AdminError> {
    let map = db_map::find_map_by_id_including_deleted(pool, map_id)
        .await
        .context("failed to look up map")?
        .ok_or(AdminError::NotFound)?;

    if !db_account::account_exists(pool, new_owner)
        .await
        .context("failed to look up target account")?
    {
        return Err(AdminError::TargetAccountNotFound);
    }

    let old_owner = map.owner_account_id;

    let mut tx = pool.begin().await.context("begin tx")?;

    let updated = db_map::change_map_owner(&mut tx, map_id, new_owner)
        .await
        .context("change_map_owner")?;
    if !updated {
        return Err(AdminError::NotFound);
    }

    if let Some(old) = old_owner {
        audit::record_in_tx(
            &mut tx,
            Some(actor),
            AuditEvent::AdminMapOwnershipChanged {
                map_id,
                old_owner: old,
                new_owner,
            },
        )
        .await
        .context("failed to record admin_map_ownership_changed audit event")?;
    }

    tx.commit().await.context("commit tx")?;
    info!(map_id = %map_id, ?old_owner, new_owner = %new_owner, actor = %actor, "admin changed map owner");
    Ok(())
}

/// Hard-deletes a map row. FK cascades remove all dependent rows
/// (connections, ends, signatures, events, checkpoints, map_acl).
#[instrument(skip(pool), err)]
pub async fn admin_hard_delete_map(
    pool: &PgPool,
    actor: Uuid,
    map_id: Uuid,
) -> Result<(), AdminError> {
    let map = db_map::find_map_by_id_including_deleted(pool, map_id)
        .await
        .context("failed to look up map")?
        .ok_or(AdminError::NotFound)?;

    let mut tx = pool.begin().await.context("begin tx")?;

    audit::record_in_tx(
        &mut tx,
        Some(actor),
        AuditEvent::AdminMapHardDeleted {
            map_id,
            name: map.name.clone(),
        },
    )
    .await
    .context("failed to record admin_map_hard_deleted audit event")?;

    let deleted = db_map::hard_delete_map(&mut tx, map_id)
        .await
        .context("hard_delete_map")?;
    if !deleted {
        return Err(AdminError::NotFound);
    }

    tx.commit().await.context("commit tx")?;
    info!(map_id = %map_id, name = %map.name, actor = %actor, "admin hard-deleted map");
    Ok(())
}

// ---------------------------------------------------------------------------
// ACL admin operations
// ---------------------------------------------------------------------------

/// Lists every ACL on the instance. Members are NOT returned — see
/// `DECISIONS.md` ("Capability boundaries").
#[instrument(skip(pool), err)]
pub async fn admin_list_acls(pool: &PgPool) -> Result<Vec<Acl>, AdminError> {
    db_acl::find_all_acls_admin(pool)
        .await
        .context("failed to list acls for admin")
        .map_err(AdminError::Internal)
}

/// Reassigns ACL ownership. Emits `AdminAclOwnershipChanged` only when the
/// previous owner was non-NULL (variant requires a concrete `old_owner`).
#[instrument(skip(pool), err)]
pub async fn admin_change_acl_owner(
    pool: &PgPool,
    actor: Uuid,
    acl_id: Uuid,
    new_owner: Uuid,
) -> Result<(), AdminError> {
    let acl = db_acl::find_acl_by_id(pool, acl_id)
        .await
        .context("failed to look up acl")?
        .ok_or(AdminError::NotFound)?;

    if !db_account::account_exists(pool, new_owner)
        .await
        .context("failed to look up target account")?
    {
        return Err(AdminError::TargetAccountNotFound);
    }

    let old_owner = acl.owner_account_id;

    let mut tx = pool.begin().await.context("begin tx")?;

    let updated = db_acl::change_acl_owner(&mut tx, acl_id, new_owner)
        .await
        .context("change_acl_owner")?;
    if !updated {
        return Err(AdminError::NotFound);
    }

    if let Some(old) = old_owner {
        audit::record_in_tx(
            &mut tx,
            Some(actor),
            AuditEvent::AdminAclOwnershipChanged {
                acl_id,
                old_owner: old,
                new_owner,
            },
        )
        .await
        .context("failed to record admin_acl_ownership_changed audit event")?;
    }

    tx.commit().await.context("commit tx")?;
    info!(acl_id = %acl_id, ?old_owner, new_owner = %new_owner, actor = %actor, "admin changed acl owner");
    Ok(())
}

/// Hard-deletes an ACL row. FK cascades remove `acl_member` and `map_acl`.
#[instrument(skip(pool), err)]
pub async fn admin_hard_delete_acl(
    pool: &PgPool,
    actor: Uuid,
    acl_id: Uuid,
) -> Result<(), AdminError> {
    let acl = db_acl::find_acl_by_id(pool, acl_id)
        .await
        .context("failed to look up acl")?
        .ok_or(AdminError::NotFound)?;

    let mut tx = pool.begin().await.context("begin tx")?;

    audit::record_in_tx(
        &mut tx,
        Some(actor),
        AuditEvent::AdminAclHardDeleted {
            acl_id,
            name: acl.name.clone(),
        },
    )
    .await
    .context("failed to record admin_acl_hard_deleted audit event")?;

    let deleted = db_acl::hard_delete_acl(&mut tx, acl_id)
        .await
        .context("hard_delete_acl")?;
    if !deleted {
        return Err(AdminError::NotFound);
    }

    tx.commit().await.context("commit tx")?;
    info!(acl_id = %acl_id, name = %acl.name, actor = %actor, "admin hard-deleted acl");
    Ok(())
}

// ---------------------------------------------------------------------------
// Blocked EVE character admin operations
// ---------------------------------------------------------------------------

/// Lists all currently blocked EVE characters, newest first.
#[instrument(skip(pool), err)]
pub async fn admin_list_blocked_eve_characters(
    pool: &PgPool,
) -> Result<Vec<BlockedEveCharacter>, AdminError> {
    db_account::list_blocked_eve_characters(pool)
        .await
        .context("failed to list blocked eve characters")
        .map_err(AdminError::Internal)
}

/// Blocks an EVE character id. Idempotent: if the character was already
/// blocked, no audit event is emitted but the call still succeeds.
#[instrument(skip(pool), err)]
pub async fn admin_block_eve_character(
    pool: &PgPool,
    actor: Uuid,
    eve_character_id: i64,
    reason: Option<String>,
) -> Result<(), AdminError> {
    let mut tx = pool.begin().await.context("begin tx")?;

    let inserted =
        db_account::insert_blocked_eve_character(&mut tx, eve_character_id, reason.as_deref())
            .await
            .context("insert_blocked_eve_character")?;

    if inserted {
        audit::record_in_tx(
            &mut tx,
            Some(actor),
            AuditEvent::EveCharacterBlocked {
                eve_character_id,
                reason: reason.clone(),
            },
        )
        .await
        .context("failed to record eve_character_blocked audit event")?;
    }

    tx.commit().await.context("commit tx")?;

    if inserted {
        info!(eve_character_id, actor = %actor, "admin blocked eve character");
    } else {
        info!(eve_character_id, actor = %actor, "admin block on already-blocked eve character (no-op)");
    }
    Ok(())
}

/// Unblocks an EVE character id. Returns `Err(NotFound)` if the character
/// was not in the blocked list.
#[instrument(skip(pool), err)]
pub async fn admin_unblock_eve_character(
    pool: &PgPool,
    actor: Uuid,
    eve_character_id: i64,
) -> Result<(), AdminError> {
    let mut tx = pool.begin().await.context("begin tx")?;

    let removed = db_account::delete_blocked_eve_character(&mut tx, eve_character_id)
        .await
        .context("delete_blocked_eve_character")?;
    if !removed {
        return Err(AdminError::NotFound);
    }

    audit::record_in_tx(
        &mut tx,
        Some(actor),
        AuditEvent::EveCharacterUnblocked { eve_character_id },
    )
    .await
    .context("failed to record eve_character_unblocked audit event")?;

    tx.commit().await.context("commit tx")?;
    info!(eve_character_id, actor = %actor, "admin unblocked eve character");
    Ok(())
}

// ---------------------------------------------------------------------------
// Account admin operations
// ---------------------------------------------------------------------------

/// Lists every account on the instance, newest first.
#[instrument(skip(pool), err)]
pub async fn admin_list_accounts(pool: &PgPool) -> Result<Vec<Account>, AdminError> {
    db_account::list_accounts_admin(pool)
        .await
        .context("failed to list accounts for admin")
        .map_err(AdminError::Internal)
}

/// Grants `is_server_admin = TRUE` on the target account. Idempotent: if the
/// account is already an admin no audit event is emitted but the call still
/// succeeds. Returns `Err(NotFound)` if the account does not exist.
#[instrument(skip(pool), err)]
pub async fn admin_grant_admin(pool: &PgPool, actor: Uuid, target: Uuid) -> Result<(), AdminError> {
    if !db_account::account_exists(pool, target)
        .await
        .context("failed to look up target account")?
    {
        return Err(AdminError::NotFound);
    }

    let already_admin = db_account::is_server_admin(pool, target)
        .await
        .context("failed to read is_server_admin")?;
    if already_admin {
        info!(actor = %actor, target = %target, "admin grant on already-admin account (no-op)");
        return Ok(());
    }

    let mut tx = pool.begin().await.context("begin tx")?;

    let updated = db_account::set_server_admin(&mut tx, target, true)
        .await
        .context("set_server_admin true")?;
    if !updated {
        return Err(AdminError::NotFound);
    }

    audit::record_in_tx(
        &mut tx,
        Some(actor),
        AuditEvent::ServerAdminGranted {
            account_id: target,
            source: ServerAdminGrantSource::AdminGrant,
        },
    )
    .await
    .context("failed to record server_admin_granted audit event")?;

    tx.commit().await.context("commit tx")?;
    info!(actor = %actor, target = %target, "admin granted server-admin");
    Ok(())
}

/// Revokes `is_server_admin` from the target account. The last-admin guard
/// runs inside the transaction: if revoking would drop the count of admins
/// to zero, returns `Err(LastAdmin)` and rolls back. Self-revoke is permitted
/// otherwise. Idempotent for a non-admin target (no-op).
#[instrument(skip(pool), err)]
pub async fn admin_revoke_admin(
    pool: &PgPool,
    actor: Uuid,
    target: Uuid,
) -> Result<(), AdminError> {
    if !db_account::account_exists(pool, target)
        .await
        .context("failed to look up target account")?
    {
        return Err(AdminError::NotFound);
    }

    let mut tx = pool.begin().await.context("begin tx")?;

    let count = db_account::count_server_admins(&mut tx)
        .await
        .context("count_server_admins")?;

    let is_admin = db_account::is_server_admin(pool, target)
        .await
        .context("failed to read is_server_admin")?;
    if !is_admin {
        info!(actor = %actor, target = %target, "admin revoke on non-admin account (no-op)");
        return Ok(());
    }

    if count <= 1 {
        return Err(AdminError::LastAdmin);
    }

    let updated = db_account::set_server_admin(&mut tx, target, false)
        .await
        .context("set_server_admin false")?;
    if !updated {
        return Err(AdminError::NotFound);
    }

    audit::record_in_tx(
        &mut tx,
        Some(actor),
        AuditEvent::ServerAdminRevoked { account_id: target },
    )
    .await
    .context("failed to record server_admin_revoked audit event")?;

    tx.commit().await.context("commit tx")?;
    info!(actor = %actor, target = %target, "admin revoked server-admin");
    Ok(())
}

// ---------------------------------------------------------------------------
// Audit log read
// ---------------------------------------------------------------------------

pub const AUDIT_LOG_DEFAULT_LIMIT: i64 = 50;
pub const AUDIT_LOG_MAX_LIMIT: i64 = 200;

/// Reads audit_log entries newest-first with optional filters. `limit` is
/// clamped to `[1, AUDIT_LOG_MAX_LIMIT]` (defaults to `AUDIT_LOG_DEFAULT_LIMIT`
/// when `None`). Filters are bound parameters — no SQL injection surface.
#[instrument(skip(pool), err)]
pub async fn admin_list_audit_log(
    pool: &PgPool,
    event_type: Option<String>,
    actor: Option<Uuid>,
    before: Option<DateTime<Utc>>,
    limit: Option<i64>,
) -> Result<Vec<AuditLogEntry>, AdminError> {
    let limit = limit
        .unwrap_or(AUDIT_LOG_DEFAULT_LIMIT)
        .clamp(1, AUDIT_LOG_MAX_LIMIT);
    audit::list_audit_log(pool, event_type.as_deref(), actor, before, limit)
        .await
        .context("failed to list audit_log")
        .map_err(AdminError::Internal)
}
