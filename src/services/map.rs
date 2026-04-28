use anyhow::Context;
use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use sqlx::PgPool;
use thiserror::Error;
use tracing::{info, warn};
use uuid::Uuid;

use crate::audit::{self, AuditEvent};
use crate::db::connection::{Connection, ConnectionEnd};
use crate::db::map::{Map, MapWithAcls};
use crate::db::map_types::{LifeState, MassState, Side};
use crate::db::route::RouteRow;
use crate::db::signature::Signature;
use crate::db::{
    acl, connection as db_conn, map as db_map, map_acl, map_event, route as db_route,
    signature as db_sig,
};
use crate::dto::envelope::ApiResponse;
use crate::permissions::{Permission, effective_permission};

#[derive(Debug, Error)]
pub enum MapError {
    #[error("not found")]
    NotFound,
    #[error("no access")]
    Forbidden,
    #[error("acl owner mismatch")]
    AclOwnerMismatch,
    #[error("slug_conflict")]
    SlugConflict,
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
    #[error("{0}")]
    Validation(String),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl From<sqlx::Error> for MapError {
    fn from(e: sqlx::Error) -> Self {
        if let sqlx::Error::Database(ref dbe) = e
            && let Some(constraint) = dbe.constraint()
            && (constraint.contains("system_id") || constraint.contains("sde_solar_system"))
        {
            return MapError::SystemNotFound;
        }
        MapError::Internal(anyhow::Error::from(e))
    }
}

impl IntoResponse for MapError {
    fn into_response(self) -> Response {
        let status = match &self {
            MapError::NotFound => StatusCode::NOT_FOUND,
            MapError::Forbidden | MapError::AclOwnerMismatch => StatusCode::FORBIDDEN,
            MapError::SlugConflict
            | MapError::SelfLoop
            | MapError::SystemNotFound
            | MapError::SignatureAlreadyLinked
            | MapError::ConnectionMapMismatch
            | MapError::SignatureMapMismatch
            | MapError::Validation(_) => StatusCode::UNPROCESSABLE_ENTITY,
            MapError::Internal(_) => {
                warn!(error = %self, "internal map error");
                StatusCode::INTERNAL_SERVER_ERROR
            }
        };
        (status, Json(ApiResponse::<()>::error(self.to_string()))).into_response()
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

// ---------------------------------------------------------------------------
// Map listing
// ---------------------------------------------------------------------------

/// Returns all maps visible to the account, each annotated with only the ACLs
/// the account can manage (owner or manage/admin member).
pub async fn list_maps(pool: &PgPool, account_id: Uuid) -> Result<Vec<MapWithAcls>, MapError> {
    let maps = db_map::find_maps_for_account(pool, account_id).await?;
    if maps.is_empty() {
        return Ok(vec![]);
    }

    let manageable_acls = acl::find_acls_manageable_by_account(pool, account_id).await?;
    let map_ids: Vec<Uuid> = maps.iter().map(|m| m.id).collect();
    let attached = map_acl::find_acl_ids_for_maps(pool, &map_ids).await?;

    Ok(maps
        .into_iter()
        .map(|m| {
            let acls = manageable_acls
                .iter()
                .filter(|a| attached.get(&m.id).is_some_and(|ids| ids.contains(&a.id)))
                .map(|a| (a.id, a.name.clone()))
                .collect();
            MapWithAcls {
                id: m.id,
                name: m.name,
                slug: m.slug,
                owner_account_id: m.owner_account_id,
                description: m.description,
                acls,
                created_at: m.created_at,
                updated_at: m.updated_at,
            }
        })
        .collect())
}

pub async fn get_map(pool: &PgPool, account_id: Uuid, map_id: Uuid) -> Result<Map, MapError> {
    let map = db_map::find_map_by_id(pool, map_id)
        .await
        .context("failed to look up map")?
        .ok_or(MapError::NotFound)?;
    require_map_permission(pool, map_id, account_id, Permission::Read).await?;
    Ok(map)
}

// ---------------------------------------------------------------------------
// Map management
// ---------------------------------------------------------------------------

/// Creates a new map. Returns `Err(MapError::SlugConflict)` if slug is taken.
///
/// If `acl_id` is provided the map is immediately attached to that ACL and
/// the ACL's `pending_delete_at` is cleared. The caller must own the ACL.
pub async fn create_map(
    pool: &PgPool,
    owner_account_id: Uuid,
    name: &str,
    slug: &str,
    description: Option<&str>,
    acl_id: Option<Uuid>,
) -> Result<Map, MapError> {
    if let Some(acl_id) = acl_id {
        let acl = acl::find_acl_by_id(pool, acl_id)
            .await
            .context("failed to query acl")
            .map_err(MapError::Internal)?
            .ok_or(MapError::NotFound)?;
        require_acl_owner_or_admin(&acl, owner_account_id)?;
    }

    let mut tx = pool
        .begin()
        .await
        .context("begin tx")
        .map_err(MapError::Internal)?;

    let map = db_map::insert_map(&mut tx, owner_account_id, name, slug, description)
        .await
        .context("insert_map")
        .map_err(MapError::Internal)?
        .ok_or(MapError::SlugConflict)?;

    if let Some(acl_id) = acl_id {
        map_acl::attach_acl(&mut tx, map.id, acl_id)
            .await
            .context("attach_acl")
            .map_err(MapError::Internal)?;
    }

    audit::record_in_tx(
        &mut tx,
        Some(owner_account_id),
        AuditEvent::MapCreated {
            account_id: owner_account_id,
            map_id: map.id,
            name: name.to_owned(),
        },
    )
    .await
    .context("failed to record map created audit event")
    .map_err(MapError::Internal)?;

    map_event::append_event(
        &mut tx,
        map.id,
        "map",
        &map.id.to_string(),
        "MapCreated",
        Some(&owner_account_id.to_string()),
        &json!({ "name": name }),
    )
    .await
    .context("failed to append MapCreated event")
    .map_err(MapError::Internal)?;

    tx.commit()
        .await
        .context("commit tx")
        .map_err(MapError::Internal)?;
    info!(map_id = %map.id, owner = %owner_account_id, slug, "map created");
    Ok(map)
}

/// Updates a map's name, slug, and description.
/// Caller must hold `manage` or higher.
pub async fn update_map(
    pool: &PgPool,
    map_id: Uuid,
    requesting_account_id: Uuid,
    name: &str,
    slug: &str,
    description: Option<&str>,
) -> Result<Map, MapError> {
    require_map_permission(pool, map_id, requesting_account_id, Permission::Manage).await?;

    db_map::update_map(pool, map_id, name, slug, description)
        .await
        .context("update_map")
        .map_err(MapError::Internal)?
        .ok_or(MapError::SlugConflict)
}

/// Soft-deletes a map. Caller must hold `admin` or be the owner.
pub async fn delete_map(
    pool: &PgPool,
    map_id: Uuid,
    requesting_account_id: Uuid,
) -> Result<(), MapError> {
    require_map_permission(pool, map_id, requesting_account_id, Permission::Admin).await?;

    let map = db_map::find_map_by_id(pool, map_id)
        .await
        .context("find_map_by_id")
        .map_err(MapError::Internal)?
        .ok_or(MapError::NotFound)?;

    let mut tx = pool
        .begin()
        .await
        .context("begin tx")
        .map_err(MapError::Internal)?;

    audit::record_in_tx(
        &mut tx,
        Some(requesting_account_id),
        AuditEvent::MapDeleted {
            account_id: requesting_account_id,
            map_id,
            name: map.name,
        },
    )
    .await
    .context("failed to record map deleted audit event")
    .map_err(MapError::Internal)?;

    db_map::delete_map(&mut tx, map_id)
        .await
        .context("delete_map")
        .map_err(MapError::Internal)?;

    tx.commit()
        .await
        .context("commit tx")
        .map_err(MapError::Internal)?;
    info!(map_id = %map_id, "map deleted");
    Ok(())
}

// ---------------------------------------------------------------------------
// Map–ACL attachment
// ---------------------------------------------------------------------------

/// Attaches an ACL to a map. Caller must hold `admin` or be the map owner.
pub async fn attach_acl_to_map(
    pool: &PgPool,
    map_id: Uuid,
    acl_id: Uuid,
    requesting_account_id: Uuid,
) -> Result<(), MapError> {
    require_map_permission(pool, map_id, requesting_account_id, Permission::Admin).await?;

    let acl = acl::find_acl_by_id(pool, acl_id)
        .await
        .context("failed to query acl")
        .map_err(MapError::Internal)?
        .ok_or(MapError::NotFound)?;
    require_acl_owner_or_admin(&acl, requesting_account_id)?;

    let mut tx = pool
        .begin()
        .await
        .context("begin tx")
        .map_err(MapError::Internal)?;
    map_acl::attach_acl(&mut tx, map_id, acl_id)
        .await
        .context("attach_acl")
        .map_err(MapError::Internal)?;
    audit::record_in_tx(
        &mut tx,
        Some(requesting_account_id),
        AuditEvent::AclAttachedToMap {
            account_id: requesting_account_id,
            map_id,
            acl_id,
        },
    )
    .await
    .context("failed to record acl_attached_to_map audit event")
    .map_err(MapError::Internal)?;
    tx.commit()
        .await
        .context("commit tx")
        .map_err(MapError::Internal)?;

    info!(map_id = %map_id, acl_id = %acl_id, "acl attached to map");
    Ok(())
}

/// Detaches an ACL from a map. Caller must hold `admin` or be the map owner.
pub async fn detach_acl_from_map(
    pool: &PgPool,
    map_id: Uuid,
    acl_id: Uuid,
    requesting_account_id: Uuid,
) -> Result<(), MapError> {
    require_map_permission(pool, map_id, requesting_account_id, Permission::Admin).await?;

    let mut tx = pool
        .begin()
        .await
        .context("begin tx")
        .map_err(MapError::Internal)?;
    map_acl::detach_acl(&mut tx, map_id, acl_id)
        .await
        .context("detach_acl")
        .map_err(MapError::Internal)?;
    audit::record_in_tx(
        &mut tx,
        Some(requesting_account_id),
        AuditEvent::AclDetachedFromMap {
            account_id: requesting_account_id,
            map_id,
            acl_id,
        },
    )
    .await
    .context("failed to record acl_detached_from_map audit event")
    .map_err(MapError::Internal)?;
    tx.commit()
        .await
        .context("commit tx")
        .map_err(MapError::Internal)?;

    info!(map_id = %map_id, acl_id = %acl_id, "acl detached from map");
    Ok(())
}

// ---------------------------------------------------------------------------
// Connection / signature / route operations
// ---------------------------------------------------------------------------

pub async fn create_connection(
    pool: &PgPool,
    account_id: Uuid,
    input: CreateConnectionInput,
) -> Result<(Connection, ConnectionEnd, ConnectionEnd), MapError> {
    require_map_permission(pool, input.map_id, account_id, Permission::ReadWrite).await?;

    if input.system_a_id == input.system_b_id {
        return Err(MapError::SelfLoop);
    }

    let mut tx = pool.begin().await.context("failed to begin transaction")?;

    let (conn, end_a, end_b) =
        db_conn::insert_connection(&mut tx, input.map_id, input.system_a_id, input.system_b_id)
            .await
            .map_err(|e| {
                if let Some(sqlx::Error::Database(db_err)) = e.downcast_ref::<sqlx::Error>()
                    && let Some(constraint) = db_err.constraint()
                    && (constraint.contains("system_id") || constraint.contains("sde_solar_system"))
                {
                    return MapError::SystemNotFound;
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

    tx.commit()
        .await
        .context("failed to commit create_connection")?;

    Ok((conn, end_a, end_b))
}

pub async fn add_signature(
    pool: &PgPool,
    account_id: Uuid,
    input: AddSignatureInput,
) -> Result<Signature, MapError> {
    require_map_permission(pool, input.map_id, account_id, Permission::ReadWrite).await?;

    let mut tx = pool.begin().await.context("failed to begin transaction")?;

    let sig = db_sig::insert_signature(
        &mut tx,
        input.map_id,
        input.system_id,
        &input.sig_code,
        &input.sig_type,
    )
    .await
    .map_err(|e| {
        if let Some(sqlx::Error::Database(db_err)) = e.downcast_ref::<sqlx::Error>()
            && let Some(constraint) = db_err.constraint()
            && (constraint.contains("system_id") || constraint.contains("sde_solar_system"))
        {
            return MapError::SystemNotFound;
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

    tx.commit()
        .await
        .context("failed to commit add_signature")?;

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
    require_map_permission(pool, map_id, account_id, Permission::ReadWrite).await?;

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

    tx.commit()
        .await
        .context("failed to commit link_signature")?;

    Ok(())
}

pub async fn update_connection_metadata(
    pool: &PgPool,
    account_id: Uuid,
    map_id: Uuid,
    input: UpdateConnectionMetadataInput,
) -> Result<(), MapError> {
    require_map_permission(pool, map_id, account_id, Permission::ReadWrite).await?;

    let conn = db_conn::find_connection(pool, input.connection_id)
        .await
        .context("failed to look up connection")?
        .ok_or(MapError::NotFound)?;

    if conn.map_id != map_id {
        return Err(MapError::ConnectionMapMismatch);
    }

    let mut tx = pool.begin().await.context("failed to begin transaction")?;

    db_conn::update_connection_metadata(
        &mut tx,
        input.connection_id,
        input.life_state,
        input.mass_state,
    )
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

    tx.commit()
        .await
        .context("failed to commit update_connection_metadata")?;

    Ok(())
}

/// Soft-deletes a connection (sets status to `collapsed`). Caller must hold `ReadWrite` or higher.
pub async fn delete_connection(
    pool: &PgPool,
    account_id: Uuid,
    map_id: Uuid,
    connection_id: Uuid,
) -> Result<(), MapError> {
    require_map_permission(pool, map_id, account_id, Permission::ReadWrite).await?;

    let conn = db_conn::find_connection(pool, connection_id)
        .await
        .context("failed to look up connection")?
        .ok_or(MapError::NotFound)?;

    if conn.map_id != map_id {
        return Err(MapError::ConnectionMapMismatch);
    }

    let mut tx = pool.begin().await.context("failed to begin transaction")?;

    let found = db_conn::soft_delete_connection(&mut tx, map_id, connection_id)
        .await
        .context("failed to soft-delete connection")?;

    if !found {
        return Err(MapError::NotFound);
    }

    map_event::append_event(
        &mut tx,
        map_id,
        "connection",
        &connection_id.to_string(),
        "ConnectionDeleted",
        Some(&account_id.to_string()),
        &json!({}),
    )
    .await
    .context("failed to append ConnectionDeleted event")?;

    tx.commit()
        .await
        .context("failed to commit delete_connection")?;

    Ok(())
}

/// Soft-deletes a signature (sets status to `deleted`). Caller must hold `ReadWrite` or higher.
pub async fn delete_signature(
    pool: &PgPool,
    account_id: Uuid,
    map_id: Uuid,
    signature_id: Uuid,
) -> Result<(), MapError> {
    require_map_permission(pool, map_id, account_id, Permission::ReadWrite).await?;

    let sig = db_sig::find_signature(pool, signature_id)
        .await
        .context("failed to look up signature")?
        .ok_or(MapError::NotFound)?;

    if sig.map_id != map_id {
        return Err(MapError::SignatureMapMismatch);
    }

    let mut tx = pool.begin().await.context("failed to begin transaction")?;

    let found = db_sig::soft_delete_signature(&mut tx, map_id, signature_id)
        .await
        .context("failed to soft-delete signature")?;

    if !found {
        return Err(MapError::NotFound);
    }

    map_event::append_event(
        &mut tx,
        map_id,
        "signature",
        &signature_id.to_string(),
        "SignatureDeleted",
        Some(&account_id.to_string()),
        &json!({}),
    )
    .await
    .context("failed to append SignatureDeleted event")?;

    tx.commit()
        .await
        .context("failed to commit delete_signature")?;

    Ok(())
}

pub async fn find_routes(
    pool: &PgPool,
    account_id: Uuid,
    query: RouteQuery,
) -> Result<Vec<Route>, MapError> {
    require_map_permission(pool, query.map_id, account_id, Permission::Read).await?;

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

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

async fn require_map_permission(
    pool: &PgPool,
    map_id: Uuid,
    account_id: Uuid,
    required: Permission,
) -> Result<(), MapError> {
    let effective = effective_permission(pool, account_id, map_id)
        .await
        .context("failed to resolve map permission")
        .map_err(MapError::Internal)?;

    match effective {
        Some(p) if p >= required => Ok(()),
        Some(_) | None => Err(MapError::Forbidden),
    }
}

fn require_acl_owner_or_admin(acl: &acl::Acl, account_id: Uuid) -> Result<(), MapError> {
    if acl.owner_account_id == Some(account_id) {
        return Ok(());
    }
    Err(MapError::AclOwnerMismatch)
}
