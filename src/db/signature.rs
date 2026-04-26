use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::db::map_types::{LifeState, MassState, Side, SignatureStatus};

struct SignatureRow {
    signature_id: Uuid,
    map_id: Uuid,
    system_id: i64,
    sig_code: String,
    sig_type: String,
    status: String,
    connection_id: Option<Uuid>,
    connection_side: Option<String>,
    wormhole_code: Option<String>,
    derived_life_state: Option<String>,
    derived_mass_state: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    extra: Value,
}

#[derive(Debug, Clone)]
pub struct Signature {
    pub signature_id: Uuid,
    pub map_id: Uuid,
    pub system_id: i64,
    pub sig_code: String,
    pub sig_type: String,
    pub status: SignatureStatus,
    pub connection_id: Option<Uuid>,
    pub connection_side: Option<Side>,
    pub wormhole_code: Option<String>,
    pub derived_life_state: Option<LifeState>,
    pub derived_mass_state: Option<MassState>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub extra: Value,
}

impl TryFrom<SignatureRow> for Signature {
    type Error = anyhow::Error;

    fn try_from(row: SignatureRow) -> Result<Self> {
        Ok(Self {
            signature_id: row.signature_id,
            map_id: row.map_id,
            system_id: row.system_id,
            sig_code: row.sig_code,
            sig_type: row.sig_type,
            status: row
                .status
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid signature status: {}", row.status))?,
            connection_id: row.connection_id,
            connection_side: row
                .connection_side
                .map(|s| s.parse().map_err(|_| anyhow::anyhow!("invalid side: {s}")))
                .transpose()?,
            wormhole_code: row.wormhole_code,
            derived_life_state: row
                .derived_life_state
                .map(|s| s.parse().map_err(|_| anyhow::anyhow!("invalid life_state: {s}")))
                .transpose()?,
            derived_mass_state: row
                .derived_mass_state
                .map(|s| s.parse().map_err(|_| anyhow::anyhow!("invalid mass_state: {s}")))
                .transpose()?,
            created_at: row.created_at,
            updated_at: row.updated_at,
            extra: row.extra,
        })
    }
}

pub async fn insert_signature(
    tx: &mut Transaction<'_, Postgres>,
    map_id: Uuid,
    system_id: i64,
    sig_code: &str,
    sig_type: &str,
) -> Result<Signature> {
    sqlx::query_as!(
        SignatureRow,
        r#"
        INSERT INTO map_signatures (map_id, system_id, sig_code, sig_type)
        VALUES ($1, $2, $3, $4)
        RETURNING signature_id, map_id, system_id, sig_code, sig_type, status,
                  connection_id, connection_side, wormhole_code,
                  derived_life_state, derived_mass_state,
                  created_at, updated_at, extra
        "#,
        map_id,
        system_id,
        sig_code,
        sig_type,
    )
    .fetch_one(&mut **tx)
    .await
    .context("failed to insert signature")?
    .try_into()
}

pub async fn find_signature(pool: &PgPool, signature_id: Uuid) -> Result<Option<Signature>> {
    let row = sqlx::query_as!(
        SignatureRow,
        r#"
        SELECT signature_id, map_id, system_id, sig_code, sig_type, status,
               connection_id, connection_side, wormhole_code,
               derived_life_state, derived_mass_state,
               created_at, updated_at, extra
        FROM map_signatures
        WHERE signature_id = $1
        "#,
        signature_id,
    )
    .fetch_optional(pool)
    .await
    .context("failed to fetch signature")?;

    row.map(TryInto::try_into).transpose()
}

pub async fn find_signature_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    signature_id: Uuid,
) -> Result<Option<Signature>> {
    let row = sqlx::query_as!(
        SignatureRow,
        r#"
        SELECT signature_id, map_id, system_id, sig_code, sig_type, status,
               connection_id, connection_side, wormhole_code,
               derived_life_state, derived_mass_state,
               created_at, updated_at, extra
        FROM map_signatures
        WHERE signature_id = $1
        "#,
        signature_id,
    )
    .fetch_optional(&mut **tx)
    .await
    .context("failed to fetch signature in tx")?;

    row.map(TryInto::try_into).transpose()
}

pub async fn find_signatures_for_map(pool: &PgPool, map_id: Uuid) -> Result<Vec<Signature>> {
    sqlx::query_as!(
        SignatureRow,
        r#"
        SELECT signature_id, map_id, system_id, sig_code, sig_type, status,
               connection_id, connection_side, wormhole_code,
               derived_life_state, derived_mass_state,
               created_at, updated_at, extra
        FROM map_signatures
        WHERE map_id = $1
        ORDER BY created_at ASC
        "#,
        map_id,
    )
    .fetch_all(pool)
    .await
    .context("failed to fetch signatures for map")?
    .into_iter()
    .map(TryInto::try_into)
    .collect()
}

pub async fn find_signatures_for_system(
    pool: &PgPool,
    map_id: Uuid,
    system_id: i64,
) -> Result<Vec<Signature>> {
    sqlx::query_as!(
        SignatureRow,
        r#"
        SELECT signature_id, map_id, system_id, sig_code, sig_type, status,
               connection_id, connection_side, wormhole_code,
               derived_life_state, derived_mass_state,
               created_at, updated_at, extra
        FROM map_signatures
        WHERE map_id = $1 AND system_id = $2
        ORDER BY created_at ASC
        "#,
        map_id,
        system_id,
    )
    .fetch_all(pool)
    .await
    .context("failed to fetch signatures for system")?
    .into_iter()
    .map(TryInto::try_into)
    .collect()
}
