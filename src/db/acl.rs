use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(Debug)]
pub struct Acl {
    pub id: Uuid,
    pub name: String,
    pub owner_account_id: Option<Uuid>,
    pub pending_delete_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Inserts a new ACL. `pending_delete_at` is set to `now()` immediately
/// because the ACL has no maps attached yet (ADR-028). If the caller
/// subsequently attaches it to a map within the same transaction,
/// `attach_acl` will clear `pending_delete_at`.
pub async fn insert_acl(
    tx: &mut Transaction<'_, Postgres>,
    owner_account_id: Uuid,
    name: &str,
) -> Result<Acl> {
    sqlx::query_as!(
        Acl,
        r#"
        INSERT INTO acl (name, owner_account_id, pending_delete_at)
        VALUES ($1, $2, now())
        RETURNING id, name, owner_account_id, pending_delete_at, created_at, updated_at
        "#,
        name,
        owner_account_id,
    )
    .fetch_one(&mut **tx)
    .await
    .context("failed to insert acl")
}

pub async fn find_acl_by_id(pool: &PgPool, id: Uuid) -> Result<Option<Acl>> {
    sqlx::query_as!(
        Acl,
        r#"
        SELECT id, name, owner_account_id, pending_delete_at, created_at, updated_at
        FROM acl
        WHERE id = $1
        "#,
        id,
    )
    .fetch_optional(pool)
    .await
    .context("failed to fetch acl by id")
}

/// Returns all ACLs where the given account is the owner or holds manage/admin
/// permission via a direct character member entry.
pub async fn find_acls_manageable_by_account(pool: &PgPool, account_id: Uuid) -> Result<Vec<Acl>> {
    sqlx::query_as!(
        Acl,
        r#"
        SELECT id, name, owner_account_id, pending_delete_at, created_at, updated_at
        FROM acl
        WHERE owner_account_id = $1
           OR EXISTS (
               SELECT 1
               FROM acl_member am
               JOIN eve_character ec ON ec.id = am.character_id
               WHERE am.acl_id = acl.id
                 AND am.member_type = 'character'
                 AND am.permission IN ('manage', 'admin')
                 AND ec.account_id = $1
           )
        ORDER BY name
        "#,
        account_id,
    )
    .fetch_all(pool)
    .await
    .context("failed to fetch manageable acls for account")
}

pub async fn update_acl_name(pool: &PgPool, id: Uuid, name: &str) -> Result<Acl> {
    sqlx::query_as!(
        Acl,
        r#"
        UPDATE acl
        SET name = $2, updated_at = now()
        WHERE id = $1
        RETURNING id, name, owner_account_id, pending_delete_at, created_at, updated_at
        "#,
        id,
        name,
    )
    .fetch_one(pool)
    .await
    .context("failed to update acl name")
}

pub async fn update_acl_name_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    name: &str,
) -> Result<Acl> {
    sqlx::query_as!(
        Acl,
        r#"
        UPDATE acl
        SET name = $2, updated_at = now()
        WHERE id = $1
        RETURNING id, name, owner_account_id, pending_delete_at, created_at, updated_at
        "#,
        id,
        name,
    )
    .fetch_one(&mut **tx)
    .await
    .context("failed to update acl name")
}

pub async fn delete_acl(tx: &mut Transaction<'_, Postgres>, id: Uuid) -> Result<()> {
    sqlx::query!("DELETE FROM acl WHERE id = $1", id)
        .execute(&mut **tx)
        .await
        .context("failed to delete acl")?;
    Ok(())
}

/// Sets or clears `pending_delete_at` on an ACL. Pass `None` to clear.
pub async fn set_acl_pending_delete(
    pool: &PgPool,
    id: Uuid,
    at: Option<DateTime<Utc>>,
) -> Result<()> {
    sqlx::query!(
        "UPDATE acl SET pending_delete_at = $2, updated_at = now() WHERE id = $1",
        id,
        at,
    )
    .execute(pool)
    .await
    .context("failed to set acl pending_delete_at")?;
    Ok(())
}

/// Hard-deletes all orphaned ACLs whose grace period has expired.
/// Returns the number of ACLs deleted.
pub async fn purge_expired_acls(pool: &PgPool, grace_days: u32) -> Result<u64> {
    let result = sqlx::query!(
        r#"
        DELETE FROM acl
        WHERE pending_delete_at IS NOT NULL
          AND pending_delete_at < now() - ($1 * interval '1 day')
        "#,
        grace_days as i32,
    )
    .execute(pool)
    .await
    .context("failed to purge expired orphaned acls")?;

    Ok(result.rows_affected())
}
