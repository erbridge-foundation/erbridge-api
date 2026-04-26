use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::db::map_types::{LifeState, MassState, Side};

// ── Map ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateMapRequest {
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct MapResponse {
    pub map_id: Uuid,
    pub owner_account_id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub retention_days: i32,
}

#[derive(Debug, Serialize)]
pub struct MapListResponse {
    pub maps: Vec<MapResponse>,
}

// ── Connection ────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateConnectionRequest {
    pub system_a_id: i64,
    pub system_b_id: i64,
}

#[derive(Debug, Serialize)]
pub struct ConnectionResponse {
    pub connection_id: Uuid,
    pub map_id: Uuid,
    pub status: String,
    pub life_state: Option<String>,
    pub mass_state: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub extra: Value,
}

#[derive(Debug, Serialize)]
pub struct ConnectionEndResponse {
    pub connection_id: Uuid,
    pub side: String,
    pub system_id: i64,
    pub signature_id: Option<Uuid>,
    pub wormhole_code: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CreateConnectionResponse {
    pub connection: ConnectionResponse,
    pub end_a: ConnectionEndResponse,
    pub end_b: ConnectionEndResponse,
}

// ── Signature ─────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct AddSignatureRequest {
    pub system_id: i64,
    pub sig_code: String,
    pub sig_type: String,
}

#[derive(Debug, Serialize)]
pub struct SignatureResponse {
    pub signature_id: Uuid,
    pub map_id: Uuid,
    pub system_id: i64,
    pub sig_code: String,
    pub sig_type: String,
    pub status: String,
    pub connection_id: Option<Uuid>,
    pub connection_side: Option<String>,
    pub wormhole_code: Option<String>,
    pub derived_life_state: Option<String>,
    pub derived_mass_state: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub extra: Value,
}

// ── Link signature ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct LinkSignatureRequest {
    pub signature_id: Uuid,
    pub side: Side,
}

// ── Connection metadata ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct UpdateConnectionMetadataRequest {
    pub life_state: Option<LifeState>,
    pub mass_state: Option<MassState>,
}

// ── Routes ────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct RouteQueryParams {
    pub start_system_id: i64,
    pub max_depth: Option<i32>,
    pub exclude_eol: Option<bool>,
    pub exclude_mass_critical: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct RouteResponse {
    pub current_system_id: i64,
    pub path_systems: Vec<i64>,
    pub path_connections: Vec<Uuid>,
    pub depth: i32,
}

#[derive(Debug, Serialize)]
pub struct RouteListResponse {
    pub routes: Vec<RouteResponse>,
}
