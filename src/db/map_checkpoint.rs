use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct MapCheckpoint {
    pub checkpoint_id: i64,
    pub map_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub last_included_seq: i64,
    pub checkpoint_version: i32,
    pub event_count: Option<i32>,
    pub checksum: Option<String>,
    pub state: Value,
}

pub async fn insert_checkpoint(
    tx: &mut Transaction<'_, Postgres>,
    map_id: Uuid,
    last_included_seq: i64,
    checkpoint_version: i32,
    event_count: Option<i32>,
    checksum: Option<&str>,
    state: &Value,
) -> Result<MapCheckpoint> {
    sqlx::query_as!(
        MapCheckpoint,
        r#"
        INSERT INTO map_checkpoints
            (map_id, last_included_seq, checkpoint_version, event_count, checksum, state)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING checkpoint_id, map_id, created_at, last_included_seq,
                  checkpoint_version, event_count, checksum, state
        "#,
        map_id,
        last_included_seq,
        checkpoint_version,
        event_count,
        checksum,
        state,
    )
    .fetch_one(&mut **tx)
    .await
    .context("failed to insert map checkpoint")
}

pub async fn find_latest_checkpoint(
    pool: &PgPool,
    map_id: Uuid,
) -> Result<Option<MapCheckpoint>> {
    sqlx::query_as!(
        MapCheckpoint,
        r#"
        SELECT checkpoint_id, map_id, created_at, last_included_seq,
               checkpoint_version, event_count, checksum, state
        FROM map_checkpoints
        WHERE map_id = $1
        ORDER BY last_included_seq DESC
        LIMIT 1
        "#,
        map_id,
    )
    .fetch_optional(pool)
    .await
    .context("failed to fetch latest map checkpoint")
}
