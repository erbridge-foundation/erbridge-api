use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::db::map_types::{ConnectionStatus, LifeState, MassState, Side};

struct ConnectionRow {
    connection_id: Uuid,
    map_id: Uuid,
    status: String,
    life_state: Option<String>,
    mass_state: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    extra: Value,
}

#[derive(Debug, Clone)]
pub struct Connection {
    pub connection_id: Uuid,
    pub map_id: Uuid,
    pub status: ConnectionStatus,
    pub life_state: Option<LifeState>,
    pub mass_state: Option<MassState>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub extra: Value,
}

impl TryFrom<ConnectionRow> for Connection {
    type Error = anyhow::Error;

    fn try_from(row: ConnectionRow) -> Result<Self> {
        Ok(Self {
            connection_id: row.connection_id,
            map_id: row.map_id,
            status: row
                .status
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid connection status: {}", row.status))?,
            life_state: row
                .life_state
                .map(|s| {
                    s.parse()
                        .map_err(|_| anyhow::anyhow!("invalid life_state: {s}"))
                })
                .transpose()?,
            mass_state: row
                .mass_state
                .map(|s| {
                    s.parse()
                        .map_err(|_| anyhow::anyhow!("invalid mass_state: {s}"))
                })
                .transpose()?,
            created_at: row.created_at,
            updated_at: row.updated_at,
            extra: row.extra,
        })
    }
}

struct ConnectionEndRow {
    connection_id: Uuid,
    side: String,
    system_id: i64,
    signature_id: Option<Uuid>,
    wormhole_code: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ConnectionEnd {
    pub connection_id: Uuid,
    pub side: Side,
    pub system_id: i64,
    pub signature_id: Option<Uuid>,
    pub wormhole_code: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl TryFrom<ConnectionEndRow> for ConnectionEnd {
    type Error = anyhow::Error;

    fn try_from(row: ConnectionEndRow) -> Result<Self> {
        Ok(Self {
            connection_id: row.connection_id,
            side: row
                .side
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid side: {}", row.side))?,
            system_id: row.system_id,
            signature_id: row.signature_id,
            wormhole_code: row.wormhole_code,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

/// Inserts a connection and both its ends (side a and b) in a single transaction step.
/// Returns (connection, end_a, end_b).
pub async fn insert_connection(
    tx: &mut Transaction<'_, Postgres>,
    map_id: Uuid,
    system_a_id: i64,
    system_b_id: i64,
) -> Result<(Connection, ConnectionEnd, ConnectionEnd)> {
    let conn_row = sqlx::query_as!(
        ConnectionRow,
        r#"
        INSERT INTO map_connections (map_id)
        VALUES ($1)
        RETURNING connection_id, map_id, status, life_state, mass_state,
                  created_at, updated_at, extra
        "#,
        map_id,
    )
    .fetch_one(&mut **tx)
    .await
    .context("failed to insert connection")?;

    let connection: Connection = conn_row.try_into()?;
    let connection_id = connection.connection_id;

    let end_rows = sqlx::query_as!(
        ConnectionEndRow,
        r#"
        INSERT INTO map_connection_ends (connection_id, side, system_id)
        VALUES ($1, 'a', $2), ($1, 'b', $3)
        RETURNING connection_id, side, system_id, signature_id, wormhole_code,
                  created_at, updated_at
        "#,
        connection_id,
        system_a_id,
        system_b_id,
    )
    .fetch_all(&mut **tx)
    .await
    .context("failed to insert connection ends")?;

    let mut ends = end_rows.into_iter();
    let end_a: ConnectionEnd = ends.next().context("missing end a")?.try_into()?;
    let end_b: ConnectionEnd = ends.next().context("missing end b")?.try_into()?;

    Ok((connection, end_a, end_b))
}

pub async fn find_connection(pool: &PgPool, connection_id: Uuid) -> Result<Option<Connection>> {
    let row = sqlx::query_as!(
        ConnectionRow,
        r#"
        SELECT connection_id, map_id, status, life_state, mass_state,
               created_at, updated_at, extra
        FROM map_connections
        WHERE connection_id = $1
        "#,
        connection_id,
    )
    .fetch_optional(pool)
    .await
    .context("failed to fetch connection")?;

    row.map(TryInto::try_into).transpose()
}

pub async fn find_connection_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    connection_id: Uuid,
) -> Result<Option<Connection>> {
    let row = sqlx::query_as!(
        ConnectionRow,
        r#"
        SELECT connection_id, map_id, status, life_state, mass_state,
               created_at, updated_at, extra
        FROM map_connections
        WHERE connection_id = $1
        "#,
        connection_id,
    )
    .fetch_optional(&mut **tx)
    .await
    .context("failed to fetch connection in tx")?;

    row.map(TryInto::try_into).transpose()
}

pub async fn find_connections_for_map(pool: &PgPool, map_id: Uuid) -> Result<Vec<Connection>> {
    sqlx::query_as!(
        ConnectionRow,
        r#"
        SELECT connection_id, map_id, status, life_state, mass_state,
               created_at, updated_at, extra
        FROM map_connections
        WHERE map_id = $1
        ORDER BY created_at ASC
        "#,
        map_id,
    )
    .fetch_all(pool)
    .await
    .context("failed to fetch connections for map")?
    .into_iter()
    .map(TryInto::try_into)
    .collect()
}

pub async fn find_connection_ends_for_map(
    pool: &PgPool,
    map_id: Uuid,
) -> Result<Vec<ConnectionEnd>> {
    sqlx::query_as!(
        ConnectionEndRow,
        r#"
        SELECT ce.connection_id, ce.side, ce.system_id, ce.signature_id,
               ce.wormhole_code, ce.created_at, ce.updated_at
        FROM map_connection_ends ce
        JOIN map_connections c ON c.connection_id = ce.connection_id
        WHERE c.map_id = $1
        ORDER BY ce.connection_id, ce.side
        "#,
        map_id,
    )
    .fetch_all(pool)
    .await
    .context("failed to fetch connection ends for map")?
    .into_iter()
    .map(TryInto::try_into)
    .collect()
}

/// Links a signature to one end of a connection, then recomputes the connection status.
/// Must be called within a transaction; both updates happen before commit (deferred FK).
pub async fn link_signature_to_end(
    tx: &mut Transaction<'_, Postgres>,
    connection_id: Uuid,
    side: Side,
    signature_id: Uuid,
) -> Result<()> {
    let side_str = side.to_string();

    sqlx::query!(
        r#"
        UPDATE map_connection_ends
        SET signature_id = $3, updated_at = now()
        WHERE connection_id = $1 AND side = $2
        "#,
        connection_id,
        side_str,
        signature_id,
    )
    .execute(&mut **tx)
    .await
    .context("failed to update connection_ends.signature_id")?;

    sqlx::query!(
        r#"
        UPDATE map_signatures
        SET connection_id   = $2,
            connection_side = $3,
            updated_at      = now()
        WHERE signature_id = $1
        "#,
        signature_id,
        connection_id,
        side_str,
    )
    .execute(&mut **tx)
    .await
    .context("failed to update signature connection link")?;

    recompute_connection_status(tx, connection_id).await
}

async fn recompute_connection_status(
    tx: &mut Transaction<'_, Postgres>,
    connection_id: Uuid,
) -> Result<()> {
    sqlx::query!(
        r#"
        UPDATE map_connections
        SET status = CASE
            WHEN status IN ('collapsed', 'expired') THEN status
            WHEN (SELECT COUNT(*) FROM map_connection_ends WHERE connection_id = $1 AND signature_id IS NOT NULL) = 0
                THEN 'partial'
            WHEN (SELECT COUNT(*) FROM map_connection_ends WHERE connection_id = $1 AND signature_id IS NOT NULL) = 1
                THEN 'linked'
            ELSE 'fully_linked'
        END,
        updated_at = now()
        WHERE connection_id = $1
        "#,
        connection_id,
    )
    .execute(&mut **tx)
    .await
    .context("failed to recompute connection status")?;

    Ok(())
}

pub async fn update_connection_metadata(
    tx: &mut Transaction<'_, Postgres>,
    connection_id: Uuid,
    life_state: Option<LifeState>,
    mass_state: Option<MassState>,
) -> Result<()> {
    let life_str = life_state.map(|s| s.to_string());
    let mass_str = mass_state.map(|s| s.to_string());

    sqlx::query!(
        r#"
        UPDATE map_connections
        SET life_state = COALESCE($2, life_state),
            mass_state = COALESCE($3, mass_state),
            updated_at = now()
        WHERE connection_id = $1
        "#,
        connection_id,
        life_str,
        mass_str,
    )
    .execute(&mut **tx)
    .await
    .context("failed to update connection metadata")?;

    Ok(())
}

/// Soft-deletes a connection by setting its status to `collapsed`.
/// Returns `true` if a row was found and updated, `false` if not found.
pub async fn soft_delete_connection(
    tx: &mut Transaction<'_, Postgres>,
    map_id: Uuid,
    connection_id: Uuid,
) -> Result<bool> {
    let result = sqlx::query!(
        r#"
        UPDATE map_connections
        SET status = 'collapsed', updated_at = now()
        WHERE connection_id = $1
          AND map_id = $2
          AND status NOT IN ('collapsed', 'expired')
        "#,
        connection_id,
        map_id,
    )
    .execute(&mut **tx)
    .await
    .context("failed to soft-delete connection")?;

    Ok(result.rows_affected() > 0)
}

/// Propagates life_state and mass_state from a connection to all its linked signatures.
pub async fn propagate_metadata_to_signatures(
    tx: &mut Transaction<'_, Postgres>,
    connection_id: Uuid,
) -> Result<()> {
    sqlx::query!(
        r#"
        UPDATE map_signatures s
        SET derived_life_state = c.life_state,
            derived_mass_state = c.mass_state,
            updated_at         = now()
        FROM map_connections c
        WHERE c.connection_id = $1
          AND s.connection_id = $1
        "#,
        connection_id,
    )
    .execute(&mut **tx)
    .await
    .context("failed to propagate connection metadata to signatures")?;

    Ok(())
}
