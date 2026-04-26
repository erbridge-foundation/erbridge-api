use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Map {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub owner_account_id: Option<Uuid>,
    pub description: Option<String>,
    pub deleted: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_checkpoint_seq: i64,
    pub last_checkpoint_at: Option<DateTime<Utc>>,
    pub retention_days: i32,
}

#[derive(Debug, Clone)]
pub struct MapWithAcls {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub owner_account_id: Option<Uuid>,
    pub description: Option<String>,
    pub acls: Vec<(Uuid, String)>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Returns `None` if the slug is already taken (caller should return 422).
pub async fn insert_map(
    tx: &mut Transaction<'_, Postgres>,
    owner_account_id: Uuid,
    name: &str,
    slug: &str,
    description: Option<&str>,
) -> Result<Option<Map>> {
    let row = sqlx::query_as!(
        Map,
        r#"
        INSERT INTO map (name, slug, owner_account_id, description)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (slug) DO NOTHING
        RETURNING id, name, slug, owner_account_id, description, deleted, created_at, updated_at,
                  last_checkpoint_seq, last_checkpoint_at, retention_days
        "#,
        name,
        slug,
        owner_account_id,
        description,
    )
    .fetch_optional(&mut **tx)
    .await
    .context("failed to insert map")?;

    Ok(row)
}

pub async fn find_map_by_id(pool: &PgPool, id: Uuid) -> Result<Option<Map>> {
    sqlx::query_as!(
        Map,
        r#"
        SELECT id, name, slug, owner_account_id, description, deleted, created_at, updated_at,
               last_checkpoint_seq, last_checkpoint_at, retention_days
        FROM map
        WHERE id = $1 AND deleted = false
        "#,
        id,
    )
    .fetch_optional(pool)
    .await
    .context("failed to fetch map by id")
}

pub async fn find_map_by_slug(pool: &PgPool, slug: &str) -> Result<Option<Map>> {
    sqlx::query_as!(
        Map,
        r#"
        SELECT id, name, slug, owner_account_id, description, deleted, created_at, updated_at,
               last_checkpoint_seq, last_checkpoint_at, retention_days
        FROM map
        WHERE slug = $1 AND deleted = false
        "#,
        slug,
    )
    .fetch_optional(pool)
    .await
    .context("failed to fetch map by slug")
}

/// Returns all non-deleted maps the account owns or has any non-deny ACL
/// access to, via character, corporation, or alliance membership.
pub async fn find_maps_for_account(pool: &PgPool, account_id: Uuid) -> Result<Vec<Map>> {
    sqlx::query_as!(
        Map,
        r#"
        SELECT DISTINCT m.id, m.name, m.slug, m.owner_account_id, m.description,
                        m.deleted, m.created_at, m.updated_at,
                        m.last_checkpoint_seq, m.last_checkpoint_at, m.retention_days
        FROM map m
        WHERE m.deleted = false
          AND (
              m.owner_account_id = $1
              OR EXISTS (
                  SELECT 1
                  FROM map_acl ma
                  JOIN acl_member am ON am.acl_id = ma.acl_id
                  JOIN eve_character ec ON ec.account_id = $1
                  WHERE ma.map_id = m.id
                    AND am.permission != 'deny'
                    AND (
                        (am.member_type = 'character' AND am.character_id = ec.id)
                     OR (am.member_type = 'corporation' AND am.eve_entity_id = ec.corporation_id)
                     OR (am.member_type = 'alliance' AND am.eve_entity_id = ec.alliance_id
                         AND ec.alliance_id IS NOT NULL)
                    )
              )
          )
        ORDER BY m.name
        "#,
        account_id,
    )
    .fetch_all(pool)
    .await
    .context("failed to fetch maps for account")
}

/// Updates map name, slug, and description. Returns `Ok(None)` on slug conflict.
pub async fn update_map(
    pool: &PgPool,
    id: Uuid,
    name: &str,
    slug: &str,
    description: Option<&str>,
) -> Result<Option<Map>> {
    let result = sqlx::query_as!(
        Map,
        r#"
        UPDATE map
        SET name = $2, slug = $3, description = $4, updated_at = now()
        WHERE id = $1 AND deleted = false
        RETURNING id, name, slug, owner_account_id, description, deleted, created_at, updated_at,
                  last_checkpoint_seq, last_checkpoint_at, retention_days
        "#,
        id,
        name,
        slug,
        description,
    )
    .fetch_optional(pool)
    .await;

    match result {
        Ok(row) => Ok(row),
        Err(sqlx::Error::Database(db_err)) if db_err.constraint() == Some("map_slug_key") => {
            Ok(None)
        }
        Err(err) => Err(err).context("failed to update map"),
    }
}

/// Soft-deletes a map by setting `deleted = true`.
pub async fn delete_map(tx: &mut Transaction<'_, Postgres>, id: Uuid) -> Result<()> {
    sqlx::query!(
        "UPDATE map SET deleted = true, updated_at = now() WHERE id = $1",
        id,
    )
    .execute(&mut **tx)
    .await
    .context("failed to soft-delete map")?;
    Ok(())
}

pub async fn update_last_checkpoint(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    seq: i64,
) -> Result<()> {
    sqlx::query!(
        r#"
        UPDATE map
        SET last_checkpoint_seq = $2,
            last_checkpoint_at  = now()
        WHERE id = $1
        "#,
        id,
        seq,
    )
    .execute(&mut **tx)
    .await
    .context("failed to update map last_checkpoint_seq")?;

    Ok(())
}
