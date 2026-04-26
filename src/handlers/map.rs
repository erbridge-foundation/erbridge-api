use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use tracing::warn;
use uuid::Uuid;

use crate::{
    dto::{
        envelope::ApiResponse,
        map::{
            AddSignatureRequest, ConnectionEndResponse, ConnectionResponse, CreateConnectionRequest,
            CreateConnectionResponse, CreateMapRequest, LinkSignatureRequest, MapListResponse,
            MapResponse, RouteListResponse, RouteQueryParams, RouteResponse, SignatureResponse,
            UpdateConnectionMetadataRequest,
        },
    },
    extractors::AccountId,
    services::map::{
        AddSignatureInput, CreateConnectionInput, MapError, RouteQuery, UpdateConnectionMetadataInput,
    },
    state::AppState,
};
use crate::db::connection::{Connection, ConnectionEnd};
use crate::db::map::Map;
use crate::db::signature::Signature;

fn map_err(e: MapError) -> (StatusCode, Json<ApiResponse<()>>) {
    let status = match &e {
        MapError::NotFound => StatusCode::NOT_FOUND,
        MapError::Forbidden => StatusCode::FORBIDDEN,
        MapError::SelfLoop
        | MapError::SystemNotFound
        | MapError::SignatureAlreadyLinked
        | MapError::ConnectionMapMismatch
        | MapError::SignatureMapMismatch => StatusCode::UNPROCESSABLE_ENTITY,
        MapError::Internal(_) => {
            warn!(error = %e, "internal map error");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    };
    (status, Json(ApiResponse::error(e.to_string())))
}

fn map_to_response(m: Map) -> MapResponse {
    MapResponse {
        map_id: m.map_id,
        owner_account_id: m.owner_account_id,
        name: m.name,
        created_at: m.created_at,
        retention_days: m.retention_days,
    }
}

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

pub async fn create_map(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    Json(body): Json<CreateMapRequest>,
) -> Result<(StatusCode, Json<ApiResponse<MapResponse>>), (StatusCode, Json<ApiResponse<()>>)> {
    let name = body.name.trim().to_owned();
    if name.is_empty() || name.len() > 100 {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ApiResponse::error("name must be 1–100 characters")),
        ));
    }

    let map = crate::services::map::create_map(&state.db, account_id, &name)
        .await
        .map_err(map_err)?;

    Ok((StatusCode::CREATED, Json(ApiResponse::ok(map_to_response(map)))))
}

pub async fn list_maps(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
) -> Result<Json<ApiResponse<MapListResponse>>, (StatusCode, Json<ApiResponse<()>>)> {
    let maps = crate::services::map::list_maps_for_account(&state.db, account_id)
        .await
        .map_err(map_err)?;

    Ok(Json(ApiResponse::ok(MapListResponse {
        maps: maps.into_iter().map(map_to_response).collect(),
    })))
}

pub async fn get_map(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    Path(map_id): Path<Uuid>,
) -> Result<Json<ApiResponse<MapResponse>>, (StatusCode, Json<ApiResponse<()>>)> {
    let map = crate::services::map::get_map(&state.db, account_id, map_id)
        .await
        .map_err(map_err)?;

    Ok(Json(ApiResponse::ok(map_to_response(map))))
}

pub async fn delete_map(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    Path(map_id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<ApiResponse<()>>)> {
    crate::services::map::delete_map(&state.db, account_id, map_id)
        .await
        .map_err(map_err)?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn create_connection(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    Path(map_id): Path<Uuid>,
    Json(body): Json<CreateConnectionRequest>,
) -> Result<(StatusCode, Json<ApiResponse<CreateConnectionResponse>>), (StatusCode, Json<ApiResponse<()>>)> {
    let (conn, end_a, end_b) = crate::services::map::create_connection(
        &state.db,
        account_id,
        CreateConnectionInput {
            map_id,
            system_a_id: body.system_a_id,
            system_b_id: body.system_b_id,
        },
    )
    .await
    .map_err(map_err)?;

    Ok((
        StatusCode::CREATED,
        Json(ApiResponse::ok(CreateConnectionResponse {
            connection: connection_to_response(conn),
            end_a: end_to_response(end_a),
            end_b: end_to_response(end_b),
        })),
    ))
}

pub async fn add_signature(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    Path(map_id): Path<Uuid>,
    Json(body): Json<AddSignatureRequest>,
) -> Result<(StatusCode, Json<ApiResponse<SignatureResponse>>), (StatusCode, Json<ApiResponse<()>>)> {
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
    .await
    .map_err(map_err)?;

    Ok((StatusCode::CREATED, Json(ApiResponse::ok(signature_to_response(sig)))))
}

pub async fn link_signature(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    Path((map_id, conn_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<LinkSignatureRequest>,
) -> Result<StatusCode, (StatusCode, Json<ApiResponse<()>>)> {
    crate::services::map::link_signature(
        &state.db,
        account_id,
        map_id,
        conn_id,
        body.signature_id,
        body.side,
    )
    .await
    .map_err(map_err)?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn update_connection_metadata(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    Path((map_id, conn_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateConnectionMetadataRequest>,
) -> Result<StatusCode, (StatusCode, Json<ApiResponse<()>>)> {
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
    .await
    .map_err(map_err)?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn find_routes(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    Path(map_id): Path<Uuid>,
    Query(params): Query<RouteQueryParams>,
) -> Result<Json<ApiResponse<RouteListResponse>>, (StatusCode, Json<ApiResponse<()>>)> {
    let routes = crate::services::map::find_routes(
        &state.db,
        account_id,
        RouteQuery {
            map_id,
            start_system_id: params.start_system_id,
            max_depth: params.max_depth.unwrap_or(10),
            exclude_eol: params.exclude_eol.unwrap_or(false),
            exclude_mass_critical: params.exclude_mass_critical.unwrap_or(false),
        },
    )
    .await
    .map_err(map_err)?;

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
