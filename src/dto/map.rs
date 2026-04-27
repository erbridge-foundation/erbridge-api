use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::LazyLock;
use uuid::Uuid;
use validator::Validate;

use crate::db::map::{Map, MapWithAcls};
use crate::db::map_types::{LifeState, MassState, Side};

static SLUG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z0-9]+(?:-[a-z0-9]+)*$").unwrap());

// ── Map ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct AclSummary {
    pub id: Uuid,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MapResponse {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub owner_account_id: Option<Uuid>,
    pub description: Option<String>,
    pub acls: Vec<AclSummary>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<Map> for MapResponse {
    fn from(m: Map) -> Self {
        Self {
            id: m.id,
            name: m.name,
            slug: m.slug,
            owner_account_id: m.owner_account_id,
            description: m.description,
            acls: vec![],
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

impl From<MapWithAcls> for MapResponse {
    fn from(m: MapWithAcls) -> Self {
        Self {
            id: m.id,
            name: m.name,
            slug: m.slug,
            owner_account_id: m.owner_account_id,
            description: m.description,
            acls: m
                .acls
                .into_iter()
                .map(|(id, name)| AclSummary { id, name })
                .collect(),
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MapListResponse {
    pub maps: Vec<MapResponse>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateMapRequest {
    #[validate(length(min = 1, max = 100))]
    pub name: String,
    #[validate(length(min = 1, max = 100), regex(path = *SLUG_RE))]
    pub slug: String,
    #[validate(length(max = 500))]
    pub description: Option<String>,
    pub acl_id: Option<Uuid>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateMapRequest {
    #[validate(length(min = 1, max = 100))]
    pub name: String,
    #[validate(length(min = 1, max = 100), regex(path = *SLUG_RE))]
    pub slug: String,
    #[validate(length(max = 500))]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AttachAclRequest {
    pub acl_id: Uuid,
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
