use anyhow::Context;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::audit::{self, AuditEvent};
use crate::db::{
    connection as db_conn, map as db_map, map_event, route as db_route,
    signature as db_sig,
};
use crate::db::map::Map;
use crate::db::connection::{Connection, ConnectionEnd};
use crate::db::map_types::{LifeState, MassState, Side};
use crate::db::route::RouteRow;
use crate::db::signature::Signature;

#[derive(Debug, thiserror::Error)]
pub enum MapError {
    #[error("not found")]
    NotFound,
    #[error("forbidden")]
    Forbidden,
    #[error("a connection cannot link a system to itself")]
    SelfLoop,
    #[error("one or more systems were not found in the SDE")]
    SystemNotFound,
    #[error("signature is already linked to a connection end")]
    SignatureAlreadyLinked,
    #[error("connection does not belong to this map")]
    ConnectionMapMismatch,
    #[error("signature does not belong to this map")]
    SignatureMapMismatch,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl From<sqlx::Error> for MapError {
    fn from(e: sqlx::Error) -> Self {
        if let sqlx::Error::Database(ref dbe) = e {
            if let Some(constraint) = dbe.constraint() {
                if constraint.contains("system_id") || constraint.contains("sde_solar_system") {
                    return MapError::SystemNotFound;
                }
            }
        }
        MapError::Internal(anyhow::Error::from(e))
    }
}

pub struct CreateConnectionInput {
    pub map_id: Uuid,
    pub system_a_id: i64,
    pub system_b_id: i64,
}

pub struct AddSignatureInput {
    pub map_id: Uuid,
    pub system_id: i64,
    pub sig_code: String,
    pub sig_type: String,
}

pub struct UpdateConnectionMetadataInput {
    pub connection_id: Uuid,
    pub life_state: Option<LifeState>,
    pub mass_state: Option<MassState>,
}

pub struct RouteQuery {
    pub map_id: Uuid,
    pub start_system_id: i64,
    pub max_depth: i32,
    pub exclude_eol: bool,
    pub exclude_mass_critical: bool,
}

pub struct Route {
    pub current_system_id: i64,
    pub path_systems: Vec<i64>,
    pub path_connections: Vec<Uuid>,
    pub depth: i32,
}

impl From<RouteRow> for Route {
    fn from(r: RouteRow) -> Self {
        Self {
            current_system_id: r.current_system_id,
            path_systems: r.path_systems,
            path_connections: r.path_connections,
            depth: r.depth,
        }
    }
}

/// Fetches a map and verifies the requesting account owns it.
async fn get_owned_map(
    pool: &PgPool,
    account_id: Uuid,
    map_id: Uuid,
) -> Result<Map, MapError> {
    let map = db_map::find_map_by_id(pool, map_id)
        .await
        .context("failed to look up map")?
        .ok_or(MapError::NotFound)?;

    if map.owner_account_id != account_id {
        return Err(MapError::Forbidden);
    }

    Ok(map)
}

pub async fn create_map(
    pool: &PgPool,
    account_id: Uuid,
    name: &str,
) -> Result<Map, MapError> {
    let mut tx = pool.begin().await.context("failed to begin transaction")?;

    let map = db_map::insert_map(&mut tx, account_id, name)
        .await
        .context("failed to create map")?;

    audit::record_in_tx(
        &mut tx,
        Some(account_id),
        AuditEvent::MapCreated { account_id, map_id: map.map_id, name: name.to_owned() },
    )
    .await
    .context("failed to record map created audit event")?;

    map_event::append_event(
        &mut tx,
        map.map_id,
        "map",
        &map.map_id.to_string(),
        "MapCreated",
        Some(&account_id.to_string()),
        &json!({ "name": name }),
    )
    .await
    .context("failed to append MapCreated event")?;

    tx.commit().await.context("failed to commit create_map")?;

    Ok(map)
}

pub async fn list_maps_for_account(pool: &PgPool, account_id: Uuid) -> Result<Vec<Map>, MapError> {
    db_map::find_maps_for_account(pool, account_id)
        .await
        .map_err(|e| MapError::Internal(e))
}

pub async fn get_map(pool: &PgPool, account_id: Uuid, map_id: Uuid) -> Result<Map, MapError> {
    get_owned_map(pool, account_id, map_id).await
}

pub async fn delete_map(pool: &PgPool, account_id: Uuid, map_id: Uuid) -> Result<(), MapError> {
    let map = get_owned_map(pool, account_id, map_id).await?;

    let mut tx = pool.begin().await.context("failed to begin transaction")?;

    audit::record_in_tx(
        &mut tx,
        Some(account_id),
        AuditEvent::MapDeleted { account_id, map_id: map.map_id },
    )
    .await
    .context("failed to record map deleted audit event")?;

    db_map::delete_map(pool, map_id)
        .await
        .context("failed to delete map")?;

    tx.commit().await.context("failed to commit delete_map")?;

    Ok(())
}

pub async fn create_connection(
    pool: &PgPool,
    account_id: Uuid,
    input: CreateConnectionInput,
) -> Result<(Connection, ConnectionEnd, ConnectionEnd), MapError> {
    get_owned_map(pool, account_id, input.map_id).await?;

    if input.system_a_id == input.system_b_id {
        return Err(MapError::SelfLoop);
    }

    let mut tx = pool.begin().await.context("failed to begin transaction")?;

    let (conn, end_a, end_b) =
        db_conn::insert_connection(&mut tx, input.map_id, input.system_a_id, input.system_b_id)
            .await
            .map_err(|e| {
                if let Some(dbe) = e.downcast_ref::<sqlx::Error>() {
                    if let sqlx::Error::Database(db_err) = dbe {
                        if let Some(constraint) = db_err.constraint() {
                            if constraint.contains("system_id")
                                || constraint.contains("sde_solar_system")
                            {
                                return MapError::SystemNotFound;
                            }
                        }
                    }
                }
                MapError::Internal(e)
            })?;

    map_event::append_event(
        &mut tx,
        input.map_id,
        "connection",
        &conn.connection_id.to_string(),
        "ConnectionCreated",
        Some(&account_id.to_string()),
        &json!({
            "system_a_id": input.system_a_id,
            "system_b_id": input.system_b_id,
        }),
    )
    .await
    .context("failed to append ConnectionCreated event")?;

    tx.commit().await.context("failed to commit create_connection")?;

    Ok((conn, end_a, end_b))
}

pub async fn add_signature(
    pool: &PgPool,
    account_id: Uuid,
    input: AddSignatureInput,
) -> Result<Signature, MapError> {
    get_owned_map(pool, account_id, input.map_id).await?;

    let mut tx = pool.begin().await.context("failed to begin transaction")?;

    let sig =
        db_sig::insert_signature(&mut tx, input.map_id, input.system_id, &input.sig_code, &input.sig_type)
            .await
            .map_err(|e| {
                if let Some(dbe) = e.downcast_ref::<sqlx::Error>() {
                    if let sqlx::Error::Database(db_err) = dbe {
                        if let Some(constraint) = db_err.constraint() {
                            if constraint.contains("system_id")
                                || constraint.contains("sde_solar_system")
                            {
                                return MapError::SystemNotFound;
                            }
                        }
                    }
                }
                MapError::Internal(e)
            })?;

    map_event::append_event(
        &mut tx,
        input.map_id,
        "signature",
        &sig.signature_id.to_string(),
        "SignatureAdded",
        Some(&account_id.to_string()),
        &json!({
            "system_id": input.system_id,
            "sig_code":  input.sig_code,
            "sig_type":  input.sig_type,
        }),
    )
    .await
    .context("failed to append SignatureAdded event")?;

    tx.commit().await.context("failed to commit add_signature")?;

    Ok(sig)
}

pub async fn link_signature(
    pool: &PgPool,
    account_id: Uuid,
    map_id: Uuid,
    connection_id: Uuid,
    signature_id: Uuid,
    side: Side,
) -> Result<(), MapError> {
    get_owned_map(pool, account_id, map_id).await?;

    let conn = db_conn::find_connection(pool, connection_id)
        .await
        .context("failed to look up connection")?
        .ok_or(MapError::NotFound)?;

    if conn.map_id != map_id {
        return Err(MapError::ConnectionMapMismatch);
    }

    let sig = db_sig::find_signature(pool, signature_id)
        .await
        .context("failed to look up signature")?
        .ok_or(MapError::NotFound)?;

    if sig.map_id != map_id {
        return Err(MapError::SignatureMapMismatch);
    }

    if sig.connection_id.is_some() {
        return Err(MapError::SignatureAlreadyLinked);
    }

    let mut tx = pool.begin().await.context("failed to begin transaction")?;

    db_conn::link_signature_to_end(&mut tx, connection_id, side, signature_id)
        .await
        .context("failed to link signature to connection end")?;

    map_event::append_event(
        &mut tx,
        map_id,
        "connection",
        &connection_id.to_string(),
        "SignatureLinkedToConnectionEnd",
        Some(&account_id.to_string()),
        &json!({
            "signature_id": signature_id,
            "side": side.to_string(),
        }),
    )
    .await
    .context("failed to append SignatureLinkedToConnectionEnd event")?;

    tx.commit().await.context("failed to commit link_signature")?;

    Ok(())
}

pub async fn update_connection_metadata(
    pool: &PgPool,
    account_id: Uuid,
    map_id: Uuid,
    input: UpdateConnectionMetadataInput,
) -> Result<(), MapError> {
    get_owned_map(pool, account_id, map_id).await?;

    let conn = db_conn::find_connection(pool, input.connection_id)
        .await
        .context("failed to look up connection")?
        .ok_or(MapError::NotFound)?;

    if conn.map_id != map_id {
        return Err(MapError::ConnectionMapMismatch);
    }

    let mut tx = pool.begin().await.context("failed to begin transaction")?;

    db_conn::update_connection_metadata(&mut tx, input.connection_id, input.life_state, input.mass_state)
        .await
        .context("failed to update connection metadata")?;

    db_conn::propagate_metadata_to_signatures(&mut tx, input.connection_id)
        .await
        .context("failed to propagate connection metadata")?;

    map_event::append_event(
        &mut tx,
        map_id,
        "connection",
        &input.connection_id.to_string(),
        "ConnectionMetadataUpdated",
        Some(&account_id.to_string()),
        &json!({
            "life_state": input.life_state.map(|s| s.to_string()),
            "mass_state": input.mass_state.map(|s| s.to_string()),
        }),
    )
    .await
    .context("failed to append ConnectionMetadataUpdated event")?;

    tx.commit().await.context("failed to commit update_connection_metadata")?;

    Ok(())
}

pub async fn find_routes(
    pool: &PgPool,
    account_id: Uuid,
    query: RouteQuery,
) -> Result<Vec<Route>, MapError> {
    get_owned_map(pool, account_id, query.map_id).await?;

    let max_depth = query.max_depth.clamp(1, 20);

    db_route::find_routes(
        pool,
        query.map_id,
        query.start_system_id,
        max_depth,
        query.exclude_eol,
        query.exclude_mass_critical,
    )
    .await
    .context("failed to find routes")
    .map_err(MapError::Internal)
    .map(|rows| rows.into_iter().map(Into::into).collect())
}
