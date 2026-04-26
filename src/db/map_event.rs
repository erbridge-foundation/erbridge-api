use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct MapEvent {
    pub seq: i64,
    pub map_id: Uuid,
    pub entity_type: String,
    pub entity_id: String,
    pub event_type: String,
    pub event_time: DateTime<Utc>,
    pub actor_id: Option<String>,
    pub payload: Value,
}

pub async fn append_event(
    tx: &mut Transaction<'_, Postgres>,
    map_id: Uuid,
    entity_type: &str,
    entity_id: &str,
    event_type: &str,
    actor_id: Option<&str>,
    payload: &Value,
) -> Result<i64> {
    let seq = sqlx::query_scalar!(
        r#"
        INSERT INTO map_events (map_id, entity_type, entity_id, event_type, actor_id, payload)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING seq
        "#,
        map_id,
        entity_type,
        entity_id,
        event_type,
        actor_id,
        payload,
    )
    .fetch_one(&mut **tx)
    .await
    .context("failed to append map event")?;

    Ok(seq)
}

pub async fn find_events_since(
    pool: &PgPool,
    map_id: Uuid,
    since_seq: i64,
) -> Result<Vec<MapEvent>> {
    sqlx::query_as!(
        MapEvent,
        r#"
        SELECT seq, map_id, entity_type, entity_id, event_type,
               event_time, actor_id, payload
        FROM map_events
        WHERE map_id = $1 AND seq > $2
        ORDER BY seq ASC
        "#,
        map_id,
        since_seq,
    )
    .fetch_all(pool)
    .await
    .context("failed to fetch map events")
}

pub async fn get_latest_seq(pool: &PgPool, map_id: Uuid) -> Result<i64> {
    let seq = sqlx::query_scalar!(
        r#"
        SELECT COALESCE(MAX(seq), 0) AS "seq!"
        FROM map_events
        WHERE map_id = $1
        "#,
        map_id,
    )
    .fetch_one(pool)
    .await
    .context("failed to get latest event seq")?;

    Ok(seq)
}
