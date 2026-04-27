use anyhow::Context;
use reqwest::Client;
use sqlx::PgPool;
use thiserror::Error;
use tracing::info;
use uuid::Uuid;

use crate::{
    db::{
        acl::{self, Acl},
        acl_member::{self, AclMember, AclPermission, MemberType},
    },
    esi::{character::get_character_public_info, universe::resolve_names},
    permissions::Permission,
};

#[derive(Debug, Error)]
pub enum AclError {
    #[error("not found")]
    NotFound,
    #[error("insufficient permission")]
    Forbidden,
    #[error("duplicate_member")]
    DuplicateMember,
    #[error("member does not belong to acl")]
    MemberAclMismatch,
    #[error("permission '{0}' is only valid for character members")]
    InvalidPermissionForType(String),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

/// Returns `Err` if `manage` or `admin` is used on a non-character member type.
fn validate_permission_for_type(
    permission: AclPermission,
    member_type: MemberType,
) -> Result<(), AclError> {
    if matches!(permission, AclPermission::Manage | AclPermission::Admin)
        && member_type != MemberType::Character
    {
        return Err(AclError::InvalidPermissionForType(permission.to_string()));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// ACL management
// ---------------------------------------------------------------------------

/// Creates a new ACL owned by `owner_account_id`. The ACL is immediately
/// orphaned (pending_delete_at set) until attached to a map (ADR-028).
pub async fn create_acl(
    pool: &PgPool,
    owner_account_id: Uuid,
    name: &str,
) -> Result<Acl, AclError> {
    let mut tx = pool
        .begin()
        .await
        .context("begin tx")
        .map_err(AclError::Internal)?;
    let acl = acl::insert_acl(&mut tx, owner_account_id, name)
        .await
        .context("insert_acl")
        .map_err(AclError::Internal)?;
    tx.commit()
        .await
        .context("commit tx")
        .map_err(AclError::Internal)?;

    info!(acl_id = %acl.id, owner = %owner_account_id, "acl created");
    Ok(acl)
}

/// Renames an ACL. Caller must hold `admin` permission or be the owner.
pub async fn rename_acl(
    pool: &PgPool,
    acl_id: Uuid,
    requesting_account_id: Uuid,
    name: &str,
) -> Result<Acl, AclError> {
    let acl = require_acl(pool, acl_id).await?;
    require_acl_permission(&acl, requesting_account_id, pool, Permission::Admin).await?;

    let updated = acl::update_acl_name(pool, acl_id, name)
        .await
        .context("update_acl_name")
        .map_err(AclError::Internal)?;
    info!(acl_id = %acl_id, "acl renamed");
    Ok(updated)
}

/// Deletes an ACL. Caller must hold `admin` permission or be the owner.
pub async fn delete_acl(
    pool: &PgPool,
    acl_id: Uuid,
    requesting_account_id: Uuid,
) -> Result<(), AclError> {
    let acl = require_acl(pool, acl_id).await?;
    require_acl_permission(&acl, requesting_account_id, pool, Permission::Admin).await?;

    let mut tx = pool
        .begin()
        .await
        .context("begin tx")
        .map_err(AclError::Internal)?;
    acl::delete_acl(&mut tx, acl_id)
        .await
        .context("delete_acl")
        .map_err(AclError::Internal)?;
    tx.commit()
        .await
        .context("commit tx")
        .map_err(AclError::Internal)?;

    info!(acl_id = %acl_id, "acl deleted");
    Ok(())
}

// ---------------------------------------------------------------------------
// ACL member management
// ---------------------------------------------------------------------------

/// Adds a member to an ACL. Caller must hold `manage` or higher.
///
/// For `character` members, `eve_character_id` must be provided. If the
/// character has no erbridge account, a ghost row is created (ADR-031).
/// For `corporation` and `alliance` members, `eve_entity_id` must be provided.
///
/// Returns `Err` if:
/// - the caller lacks `manage` permission
/// - `member_type` is invalid
/// - `permission` is invalid
/// - `manage`/`admin` is used on a non-character member
/// - the entity is already a member of this ACL
#[allow(clippy::too_many_arguments)]
pub async fn add_member(
    pool: &PgPool,
    http: &Client,
    esi_base: &str,
    acl_id: Uuid,
    requesting_account_id: Uuid,
    member_type: MemberType,
    eve_entity_id: Option<i64>,
    permission: AclPermission,
) -> Result<AclMember, AclError> {
    validate_permission_for_type(permission, member_type)?;

    let acl = require_acl(pool, acl_id).await?;
    require_acl_permission(&acl, requesting_account_id, pool, Permission::Manage).await?;

    let (db_eve_entity_id, character_id, name) = match member_type {
        MemberType::Character => {
            let char_eve_id = eve_entity_id
                .ok_or_else(|| anyhow::anyhow!("eve_entity_id is required for character members"))
                .map_err(AclError::Internal)?;
            let (char_id, char_name) =
                find_or_create_ghost_character(pool, http, esi_base, char_eve_id).await?;
            (None, Some(char_id), char_name)
        }
        _ => {
            let entity_id = eve_entity_id
                .ok_or_else(|| {
                    anyhow::anyhow!("eve_entity_id is required for corporation/alliance members")
                })
                .map_err(AclError::Internal)?;
            let resolved_name = resolve_entity_name(http, esi_base, entity_id).await;
            (Some(entity_id), None, resolved_name)
        }
    };

    // Enforce no duplicate members.
    check_no_duplicate_member(pool, acl_id, member_type, db_eve_entity_id, character_id).await?;

    let mut tx = pool
        .begin()
        .await
        .context("begin tx")
        .map_err(AclError::Internal)?;
    let member = acl_member::insert_acl_member(
        &mut tx,
        acl_id,
        member_type,
        db_eve_entity_id,
        character_id,
        &name,
        permission,
    )
    .await
    .context("insert_acl_member")
    .map_err(AclError::Internal)?;
    tx.commit()
        .await
        .context("commit tx")
        .map_err(AclError::Internal)?;

    info!(
        acl_id = %acl_id,
        member_id = %member.id,
        member_type = member_type.to_string(),
        permission = permission.to_string(),
        "acl member added"
    );
    Ok(member)
}

/// Updates a member's permission. Caller must hold `manage` or higher.
///
/// Returns `Err` if the caller lacks permission, the member doesn't belong to
/// this ACL, or the new permission violates type constraints.
pub async fn update_member_permission(
    pool: &PgPool,
    acl_id: Uuid,
    member_id: Uuid,
    requesting_account_id: Uuid,
    permission: AclPermission,
) -> Result<AclMember, AclError> {
    let acl = require_acl(pool, acl_id).await?;
    require_acl_permission(&acl, requesting_account_id, pool, Permission::Manage).await?;

    let member = require_member(pool, acl_id, member_id).await?;
    validate_permission_for_type(permission, member.member_type)?;

    let updated = acl_member::update_member_permission(pool, member_id, permission)
        .await
        .context("update_member_permission")
        .map_err(AclError::Internal)?;
    info!(
        acl_id = %acl_id,
        member_id = %member_id,
        permission = permission.to_string(),
        "acl member permission updated"
    );
    Ok(updated)
}

/// Removes a member from an ACL. Caller must hold `manage` or higher.
pub async fn remove_member(
    pool: &PgPool,
    acl_id: Uuid,
    member_id: Uuid,
    requesting_account_id: Uuid,
) -> Result<(), AclError> {
    let acl = require_acl(pool, acl_id).await?;
    require_acl_permission(&acl, requesting_account_id, pool, Permission::Manage).await?;

    // Verify the member belongs to this ACL before deleting.
    require_member(pool, acl_id, member_id).await?;

    acl_member::delete_member(pool, member_id)
        .await
        .context("delete_member")
        .map_err(AclError::Internal)?;
    info!(acl_id = %acl_id, member_id = %member_id, "acl member removed");
    Ok(())
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Asserts that `account_id` holds `manage` or higher on the given ACL.
/// Used by the `list_members` handler.
pub async fn assert_acl_list_members_permission(
    pool: &PgPool,
    acl_id: Uuid,
    account_id: Uuid,
) -> Result<(), AclError> {
    let acl = require_acl(pool, acl_id).await?;
    require_acl_permission(&acl, account_id, pool, Permission::Manage).await
}

async fn require_acl(pool: &PgPool, acl_id: Uuid) -> Result<Acl, AclError> {
    acl::find_acl_by_id(pool, acl_id)
        .await
        .context("failed to query acl")
        .map_err(AclError::Internal)?
        .ok_or(AclError::NotFound)
}

/// Resolves the requesting account's effective permission on the ACL.
/// Owners always have admin. Character members with manage/admin qualify.
/// Returns `Err` if the required level is not met.
///
/// Only direct character membership is checked — corporation/alliance entries
/// cannot grant ACL management rights by design (manage/admin are
/// character-only permissions).
async fn require_acl_permission(
    acl: &Acl,
    account_id: Uuid,
    pool: &PgPool,
    required: Permission,
) -> Result<(), AclError> {
    if acl.owner_account_id == Some(account_id) {
        return Ok(());
    }

    let has = sqlx::query_scalar!(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM acl_member am
            JOIN eve_character ec ON ec.id = am.character_id
            WHERE am.acl_id = $1
              AND am.member_type = 'character'
              AND ec.account_id = $2
              AND am.permission = ANY($3)
        )
        "#,
        acl.id,
        account_id,
        &required_permission_strings(required),
    )
    .fetch_one(pool)
    .await
    .context("failed to check acl permission")
    .map_err(AclError::Internal)?
    .unwrap_or(false);

    if !has {
        return Err(AclError::Forbidden);
    }
    Ok(())
}

/// Returns the set of permission strings that satisfy the required level.
fn required_permission_strings(required: Permission) -> Vec<String> {
    match required {
        Permission::Read => vec![
            AclPermission::Read,
            AclPermission::ReadWrite,
            AclPermission::Manage,
            AclPermission::Admin,
        ],
        Permission::ReadWrite => vec![
            AclPermission::ReadWrite,
            AclPermission::Manage,
            AclPermission::Admin,
        ],
        Permission::Manage => vec![AclPermission::Manage, AclPermission::Admin],
        Permission::Admin => vec![AclPermission::Admin],
    }
    .into_iter()
    .map(|p| p.to_string())
    .collect()
}

async fn require_member(
    pool: &PgPool,
    acl_id: Uuid,
    member_id: Uuid,
) -> Result<AclMember, AclError> {
    let member = acl_member::find_member_by_id(pool, member_id)
        .await
        .context("failed to query acl member")
        .map_err(AclError::Internal)?
        .ok_or(AclError::NotFound)?;

    if member.acl_id != acl_id {
        return Err(AclError::MemberAclMismatch);
    }
    Ok(member)
}

async fn check_no_duplicate_member(
    pool: &PgPool,
    acl_id: Uuid,
    member_type: MemberType,
    eve_entity_id: Option<i64>,
    character_id: Option<Uuid>,
) -> Result<(), AclError> {
    let exists: bool = sqlx::query_scalar!(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM acl_member
            WHERE acl_id = $1
              AND member_type = $2
              AND (eve_entity_id IS NOT DISTINCT FROM $3)
              AND (character_id IS NOT DISTINCT FROM $4)
        )
        "#,
        acl_id,
        member_type.to_string(),
        eve_entity_id,
        character_id,
    )
    .fetch_one(pool)
    .await
    .context("failed to check duplicate acl member")
    .map_err(AclError::Internal)?
    .unwrap_or(false);

    if exists {
        return Err(AclError::DuplicateMember);
    }
    Ok(())
}

/// Resolves the display name for a single EVE entity ID via ESI.
/// Falls back to the numeric ID as a string if ESI is unavailable.
async fn resolve_entity_name(http: &Client, esi_base: &str, eve_entity_id: i64) -> String {
    let result = resolve_names(http, esi_base, vec![eve_entity_id]).await;
    match result {
        Ok(names) => names
            .into_iter()
            .find(|n| n.id == eve_entity_id)
            .map(|n| n.name)
            .unwrap_or_else(|| eve_entity_id.to_string()),
        Err(_) => eve_entity_id.to_string(),
    }
}

/// Finds an existing `eve_character` row by EVE character ID, or creates a
/// ghost row by fetching public info from ESI (ADR-031).
/// Returns the erbridge character UUID and the character's display name.
async fn find_or_create_ghost_character(
    pool: &PgPool,
    http: &Client,
    esi_base: &str,
    eve_character_id: i64,
) -> Result<(Uuid, String), AclError> {
    // Use a non-decrypting query since ghost rows have no tokens to decrypt.
    let existing = sqlx::query!(
        "SELECT id, name FROM eve_character WHERE eve_character_id = $1",
        eve_character_id,
    )
    .fetch_optional(pool)
    .await
    .context("failed to look up eve_character by eve_character_id")?;

    if let Some(row) = existing {
        return Ok((row.id, row.name));
    }

    // Fetch name + corp/alliance from ESI — two calls, both public endpoints.
    let names = resolve_names(http, esi_base, vec![eve_character_id])
        .await
        .context("failed to resolve character name from ESI")?;
    let name_entry = names
        .into_iter()
        .find(|n| n.id == eve_character_id && n.category == "character")
        .context("ESI did not return a character entry for the given ID")?;

    let public_info = get_character_public_info(http, esi_base, eve_character_id)
        .await
        .context("failed to fetch character public info from ESI")?;

    let id = sqlx::query_scalar!(
        r#"
        INSERT INTO eve_character (eve_character_id, name, corporation_id, alliance_id, is_main)
        VALUES ($1, $2, $3, $4, false)
        RETURNING id
        "#,
        eve_character_id,
        name_entry.name,
        public_info.corporation_id,
        public_info.alliance_id,
    )
    .fetch_one(pool)
    .await
    .context("failed to insert ghost character")?;

    info!(eve_character_id, "ghost character created");
    Ok((id, name_entry.name))
}
