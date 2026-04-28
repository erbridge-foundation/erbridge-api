use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use uuid::Uuid;

use validator::Validate;

use crate::db::connection::{Connection, ConnectionEnd};
use crate::db::signature::Signature;
use crate::{
    dto::{
        envelope::ApiResponse,
        map::{
            AddSignatureRequest, AttachAclRequest, ConnectionEndResponse, ConnectionResponse,
            CreateConnectionRequest, CreateConnectionResponse, CreateMapRequest,
            LinkSignatureRequest, MapListResponse, MapResponse, RouteListResponse,
            RouteQueryParams, RouteResponse, SignatureResponse, UpdateConnectionMetadataRequest,
            UpdateMapRequest,
        },
    },
    extractors::AccountId,
    services::map::{
        AddSignatureInput, CreateConnectionInput, MapError, RouteQuery,
        UpdateConnectionMetadataInput, attach_acl_to_map, create_map, delete_connection,
        delete_map, delete_signature, detach_acl_from_map, get_map, list_maps, update_map,
    },
    state::AppState,
};

fn connection_to_response(c: Connection) -> ConnectionResponse {
    ConnectionResponse {
        connection_id: c.connection_id,
        map_id: c.map_id,
        status: c.status.to_string(),
        life_state: c.life_state.map(|s| s.to_string()),
        mass_state: c.mass_state.map(|s| s.to_string()),
        created_at: c.created_at,
        updated_at: c.updated_at,
        extra: c.extra,
    }
}

fn end_to_response(e: ConnectionEnd) -> ConnectionEndResponse {
    ConnectionEndResponse {
        connection_id: e.connection_id,
        side: e.side.to_string(),
        system_id: e.system_id,
        signature_id: e.signature_id,
        wormhole_code: e.wormhole_code,
    }
}

fn signature_to_response(s: Signature) -> SignatureResponse {
    SignatureResponse {
        signature_id: s.signature_id,
        map_id: s.map_id,
        system_id: s.system_id,
        sig_code: s.sig_code,
        sig_type: s.sig_type,
        status: s.status.to_string(),
        connection_id: s.connection_id,
        connection_side: s.connection_side.map(|s| s.to_string()),
        wormhole_code: s.wormhole_code,
        derived_life_state: s.derived_life_state.map(|s| s.to_string()),
        derived_mass_state: s.derived_mass_state.map(|s| s.to_string()),
        created_at: s.created_at,
        updated_at: s.updated_at,
        extra: s.extra,
    }
}

// ---------------------------------------------------------------------------
// GET /api/v1/maps
// ---------------------------------------------------------------------------

pub async fn list_maps_handler(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
) -> Result<Json<ApiResponse<MapListResponse>>, MapError> {
    let maps = list_maps(&state.db, account_id).await?;

    Ok(Json(ApiResponse::ok(MapListResponse {
        maps: maps.into_iter().map(MapResponse::from).collect(),
    })))
}

// ---------------------------------------------------------------------------
// POST /api/v1/maps
// ---------------------------------------------------------------------------

pub async fn create_map_handler(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    Json(body): Json<CreateMapRequest>,
) -> Result<(StatusCode, Json<ApiResponse<MapResponse>>), MapError> {
    body.validate()
        .map_err(|e| MapError::Validation(e.to_string()))?;

    let map = create_map(
        &state.db,
        account_id,
        &body.name,
        &body.slug,
        body.description.as_deref(),
        body.acl_id,
    )
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(ApiResponse::ok(MapResponse::from(map))),
    ))
}

// ---------------------------------------------------------------------------
// PATCH /api/v1/maps/:map_id
// ---------------------------------------------------------------------------

pub async fn update_map_handler(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    Path(map_id): Path<Uuid>,
    Json(body): Json<UpdateMapRequest>,
) -> Result<Json<ApiResponse<MapResponse>>, MapError> {
    body.validate()
        .map_err(|e| MapError::Validation(e.to_string()))?;

    let map = update_map(
        &state.db,
        map_id,
        account_id,
        &body.name,
        &body.slug,
        body.description.as_deref(),
    )
    .await?;

    Ok(Json(ApiResponse::ok(MapResponse::from(map))))
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/maps/:map_id
// ---------------------------------------------------------------------------

pub async fn delete_map_handler(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    Path(map_id): Path<Uuid>,
) -> Result<StatusCode, MapError> {
    delete_map(&state.db, map_id, account_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// POST /api/v1/maps/:map_id/acls
// ---------------------------------------------------------------------------

pub async fn attach_acl(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    Path(map_id): Path<Uuid>,
    Json(body): Json<AttachAclRequest>,
) -> Result<StatusCode, MapError> {
    attach_acl_to_map(&state.db, map_id, body.acl_id, account_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/maps/:map_id/acls/:acl_id
// ---------------------------------------------------------------------------

pub async fn detach_acl(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    Path((map_id, acl_id)): Path<(Uuid, Uuid)>,
) -> impl IntoResponse {
    detach_acl_from_map(&state.db, map_id, acl_id, account_id)
        .await
        .map(|()| StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// POST /api/v1/maps/:map_id/connections
// ---------------------------------------------------------------------------

pub async fn create_connection(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    Path(map_id): Path<Uuid>,
    Json(body): Json<CreateConnectionRequest>,
) -> Result<(StatusCode, Json<ApiResponse<CreateConnectionResponse>>), MapError> {
    let (conn, end_a, end_b) = crate::services::map::create_connection(
        &state.db,
        account_id,
        CreateConnectionInput {
            map_id,
            system_a_id: body.system_a_id,
            system_b_id: body.system_b_id,
        },
    )
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(ApiResponse::ok(CreateConnectionResponse {
            connection: connection_to_response(conn),
            end_a: end_to_response(end_a),
            end_b: end_to_response(end_b),
        })),
    ))
}

// ---------------------------------------------------------------------------
// POST /api/v1/maps/:map_id/signatures
// ---------------------------------------------------------------------------

pub async fn add_signature(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    Path(map_id): Path<Uuid>,
    Json(body): Json<AddSignatureRequest>,
) -> Result<(StatusCode, Json<ApiResponse<SignatureResponse>>), MapError> {
    let sig = crate::services::map::add_signature(
        &state.db,
        account_id,
        AddSignatureInput {
            map_id,
            system_id: body.system_id,
            sig_code: body.sig_code,
            sig_type: body.sig_type,
        },
    )
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(ApiResponse::ok(signature_to_response(sig))),
    ))
}

// ---------------------------------------------------------------------------
// POST /api/v1/maps/:map_id/connections/:conn_id/link
// ---------------------------------------------------------------------------

pub async fn link_signature(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    Path((map_id, conn_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<LinkSignatureRequest>,
) -> Result<StatusCode, MapError> {
    crate::services::map::link_signature(
        &state.db,
        account_id,
        map_id,
        conn_id,
        body.signature_id,
        body.side,
    )
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// PATCH /api/v1/maps/:map_id/connections/:conn_id/metadata
// ---------------------------------------------------------------------------

pub async fn update_connection_metadata(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    Path((map_id, conn_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateConnectionMetadataRequest>,
) -> Result<StatusCode, MapError> {
    crate::services::map::update_connection_metadata(
        &state.db,
        account_id,
        map_id,
        UpdateConnectionMetadataInput {
            connection_id: conn_id,
            life_state: body.life_state,
            mass_state: body.mass_state,
        },
    )
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// GET /api/v1/maps/:map_id/routes
// ---------------------------------------------------------------------------

pub async fn find_routes(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    Path(map_id): Path<Uuid>,
    Query(params): Query<RouteQueryParams>,
) -> Result<Json<ApiResponse<RouteListResponse>>, MapError> {
    let routes = crate::services::map::find_routes(
        &state.db,
        account_id,
        RouteQuery {
            map_id,
            start_system_id: params.start_system_id,
            max_depth: params.max_depth,
            exclude_eol: params.exclude_eol,
            exclude_mass_critical: params.exclude_mass_critical,
        },
    )
    .await?;

    Ok(Json(ApiResponse::ok(RouteListResponse {
        routes: routes
            .into_iter()
            .map(|r| RouteResponse {
                current_system_id: r.current_system_id,
                path_systems: r.path_systems,
                path_connections: r.path_connections,
                depth: r.depth,
            })
            .collect(),
    })))
}

// ---------------------------------------------------------------------------
// GET /api/v1/maps/:map_id
// ---------------------------------------------------------------------------

pub async fn get_map_handler(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    Path(map_id): Path<Uuid>,
) -> Result<Json<ApiResponse<MapResponse>>, MapError> {
    let map = get_map(&state.db, account_id, map_id).await?;
    Ok(Json(ApiResponse::ok(MapResponse::from(map))))
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/maps/:map_id/connections/:conn_id
// ---------------------------------------------------------------------------

pub async fn delete_connection_handler(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    Path((map_id, conn_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, MapError> {
    delete_connection(&state.db, account_id, map_id, conn_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/maps/:map_id/signatures/:sig_id
// ---------------------------------------------------------------------------

pub async fn delete_signature_handler(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    Path((map_id, sig_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, MapError> {
    delete_signature(&state.db, account_id, map_id, sig_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
