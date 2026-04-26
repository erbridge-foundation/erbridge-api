use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

struct MapRow {
    map_id: Uuid,
    owner_account_id: Uuid,
    name: String,
    created_at: DateTime<Utc>,
    last_checkpoint_seq: i64,
    last_checkpoint_at: Option<DateTime<Utc>>,
    retention_days: i32,
}

#[derive(Debug, Clone)]
pub struct Map {
    pub map_id: Uuid,
    pub owner_account_id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub last_checkpoint_seq: i64,
    pub last_checkpoint_at: Option<DateTime<Utc>>,
    pub retention_days: i32,
}

impl From<MapRow> for Map {
    fn from(row: MapRow) -> Self {
        Self {
            map_id: row.map_id,
            owner_account_id: row.owner_account_id,
            name: row.name,
            created_at: row.created_at,
            last_checkpoint_seq: row.last_checkpoint_seq,
            last_checkpoint_at: row.last_checkpoint_at,
            retention_days: row.retention_days,
        }
    }
}

pub async fn insert_map(
    tx: &mut Transaction<'_, Postgres>,
    owner_account_id: Uuid,
    name: &str,
) -> Result<Map> {
    sqlx::query_as!(
        MapRow,
        r#"
        INSERT INTO maps (owner_account_id, name)
        VALUES ($1, $2)
        RETURNING map_id, owner_account_id, name, created_at,
                  last_checkpoint_seq, last_checkpoint_at, retention_days
        "#,
        owner_account_id,
        name,
    )
    .fetch_one(&mut **tx)
    .await
    .context("failed to insert map")
    .map(Into::into)
}

pub async fn find_map_by_id(pool: &PgPool, map_id: Uuid) -> Result<Option<Map>> {
    sqlx::query_as!(
        MapRow,
        r#"
        SELECT map_id, owner_account_id, name, created_at,
               last_checkpoint_seq, last_checkpoint_at, retention_days
        FROM maps
        WHERE map_id = $1
        "#,
        map_id,
    )
    .fetch_optional(pool)
    .await
    .context("failed to fetch map by id")
    .map(|r| r.map(Into::into))
}

pub async fn find_maps_for_account(pool: &PgPool, account_id: Uuid) -> Result<Vec<Map>> {
    sqlx::query_as!(
        MapRow,
        r#"
        SELECT map_id, owner_account_id, name, created_at,
               last_checkpoint_seq, last_checkpoint_at, retention_days
        FROM maps
        WHERE owner_account_id = $1
        ORDER BY created_at ASC
        "#,
        account_id,
    )
    .fetch_all(pool)
    .await
    .context("failed to fetch maps for account")
    .map(|rows| rows.into_iter().map(Into::into).collect())
}

/// Returns true if a row was deleted.
pub async fn delete_map(pool: &PgPool, map_id: Uuid) -> Result<bool> {
    let result = sqlx::query!(
        "DELETE FROM maps WHERE map_id = $1",
        map_id,
    )
    .execute(pool)
    .await
    .context("failed to delete map")?;

    Ok(result.rows_affected() > 0)
}

pub async fn update_last_checkpoint(
    tx: &mut Transaction<'_, Postgres>,
    map_id: Uuid,
    seq: i64,
) -> Result<()> {
    sqlx::query!(
        r#"
        UPDATE maps
        SET last_checkpoint_seq = $2,
            last_checkpoint_at  = now()
        WHERE map_id = $1
        "#,
        map_id,
        seq,
    )
    .execute(&mut **tx)
    .await
    .context("failed to update map last_checkpoint_seq")?;

    Ok(())
}
