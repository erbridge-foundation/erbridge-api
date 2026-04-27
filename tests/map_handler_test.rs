mod common;

use axum::http::StatusCode;
use axum_test::TestServer;
use chrono::Utc;
use cookie::Cookie;
use erbridge_api::{
    db::sde_solar_system::SdeSolarSystem,
    dto::auth::SessionClaims,
    extractors::SESSION_COOKIE,
    services::{
        auth::{LoginInput, login_or_register},
        map::{AddSignatureInput, CreateConnectionInput, add_signature, create_connection, create_map},
    },
};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde_json::json;
use uuid::Uuid;

const FAKE_ACCESS: &str = "fake.access";
const FAKE_REFRESH: &str = "fake.refresh";
const ESI_CLIENT: &str = "test_client_id";

/// Builds a valid session JWT cookie value for the given account_id.
fn session_jwt(account_id: Uuid, jwt_key: &[u8; 32]) -> String {
    let claims = SessionClaims {
        account_id,
        exp: (Utc::now() + chrono::Duration::hours(1))
            .timestamp()
            .unsigned_abs(),
    };
    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(jwt_key),
    )
    .unwrap()
}

async fn make_account(pool: &sqlx::PgPool, eve_id: i64, name: &'static str) -> Uuid {
    let aes_key = common::test_aes_key();
    login_or_register(
        pool,
        &aes_key,
        LoginInput {
            eve_character_id: eve_id,
            name,
            corporation_id: 1_000_001,
            alliance_id: None,
            esi_client_id: ESI_CLIENT,
            access_token: FAKE_ACCESS,
            refresh_token: FAKE_REFRESH,
            esi_token_expires_at: Utc::now() + chrono::Duration::hours(1),
        },
    )
    .await
    .unwrap()
}

async fn seed_solar(pool: &sqlx::PgPool, id: i64) {
    let mut tx = pool.begin().await.unwrap();
    erbridge_api::db::sde_solar_system::bulk_upsert_solar_systems(
        &mut tx,
        &[SdeSolarSystem {
            solar_system_id: id,
            name: format!("TestSystem{id}"),
            region_id: Some(10000001),
            constellation_id: Some(20000001),
            faction_id: None,
            star_id: None,
            security_status: Some(0.0),
            security_class: Some("H".into()),
            wh_class: Some("C1".into()),
            wormhole_class_id: None,
            luminosity: None,
            radius: None,
            border: Some(false),
            corridor: Some(false),
            fringe: Some(false),
            hub: Some(false),
            international: Some(false),
            regional: Some(false),
            visual_effect: None,
            name_i18n: None,
            planet_ids: None,
            stargate_ids: None,
            disallowed_anchor_categories: None,
            disallowed_anchor_groups: None,
            position: None,
            position_2d: None,
        }],
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

// ── POST /api/v1/maps ─────────────────────────────────────────────────────────

#[tokio::test]
async fn post_maps_creates_map() {
    let (_pg, pool) = common::setup_db().await;
    let state = common::test_state(pool.clone());
    let server = TestServer::new(erbridge_api::router_from_state(state.clone()));

    let account_id = make_account(&pool, 80001, "Handler Pilot").await;
    let jwt = session_jwt(account_id, &state.config.jwt_key);

    let resp = server
        .post("/api/v1/maps")
        .add_cookie(Cookie::new(SESSION_COOKIE, jwt))
        .json(&json!({ "name": "Handler Map", "slug": "handler-map" }))
        .await;

    resp.assert_status(StatusCode::CREATED);
    let body: serde_json::Value = resp.json();
    assert!(body["data"]["id"].is_string());
    assert_eq!(body["data"]["name"], "Handler Map");
}

#[tokio::test]
async fn post_maps_without_auth_returns_401() {
    let (_pg, pool) = common::setup_db().await;
    let state = common::test_state(pool.clone());
    let server = TestServer::new(erbridge_api::router_from_state(state));

    let resp = server
        .post("/api/v1/maps")
        .json(&json!({ "name": "Unauth Map", "slug": "unauth-map" }))
        .await;

    resp.assert_status_unauthorized();
}

#[tokio::test]
async fn post_maps_with_empty_name_returns_422() {
    let (_pg, pool) = common::setup_db().await;
    let state = common::test_state(pool.clone());
    let server = TestServer::new(erbridge_api::router_from_state(state.clone()));

    let account_id = make_account(&pool, 80002, "Name Pilot").await;
    let jwt = session_jwt(account_id, &state.config.jwt_key);

    let resp = server
        .post("/api/v1/maps")
        .add_cookie(Cookie::new(SESSION_COOKIE, jwt))
        .json(&json!({ "name": "", "slug": "noname" }))
        .await;

    resp.assert_status_unprocessable_entity();
}

// ── GET /api/v1/maps ──────────────────────────────────────────────────────────

#[tokio::test]
async fn get_maps_returns_list() {
    let (_pg, pool) = common::setup_db().await;
    let state = common::test_state(pool.clone());
    let server = TestServer::new(erbridge_api::router_from_state(state.clone()));

    let account_id = make_account(&pool, 80003, "List Pilot").await;
    create_map(&pool, account_id, "Listed Map", "listed-map", None, None)
        .await
        .unwrap();

    let jwt = session_jwt(account_id, &state.config.jwt_key);
    let resp = server
        .get("/api/v1/maps")
        .add_cookie(Cookie::new(SESSION_COOKIE, jwt))
        .await;

    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    let maps = body["data"]["maps"].as_array().unwrap();
    assert_eq!(maps.len(), 1);
    assert_eq!(maps[0]["name"], "Listed Map");
}

// ── DELETE /api/v1/maps/{map_id} ─────────────────────────────────────────────

#[tokio::test]
async fn delete_map_by_non_owner_returns_403() {
    let (_pg, pool) = common::setup_db().await;
    let state = common::test_state(pool.clone());
    let server = TestServer::new(erbridge_api::router_from_state(state.clone()));

    let owner = make_account(&pool, 80004, "Owner Pilot").await;
    let other = make_account(&pool, 80005, "Other Pilot").await;

    let map = create_map(&pool, owner, "Owned Map", "owned-map", None, None)
        .await
        .unwrap();
    let other_jwt = session_jwt(other, &state.config.jwt_key);

    let resp = server
        .delete(&format!("/api/v1/maps/{}", map.id))
        .add_cookie(Cookie::new(SESSION_COOKIE, other_jwt))
        .await;

    resp.assert_status_forbidden();
}

// ── POST /api/v1/maps/{id}/connections ───────────────────────────────────────

#[tokio::test]
async fn post_connections_self_loop_returns_422() {
    let (_pg, pool) = common::setup_db().await;
    let state = common::test_state(pool.clone());
    let server = TestServer::new(erbridge_api::router_from_state(state.clone()));

    let account_id = make_account(&pool, 80006, "Loop Pilot Handler").await;
    seed_solar(&pool, 41000001).await;

    let map = create_map(
        &pool,
        account_id,
        "Loop Map Handler",
        "loop-map-handler",
        None,
        None,
    )
    .await
    .unwrap();
    let jwt = session_jwt(account_id, &state.config.jwt_key);

    let resp = server
        .post(&format!("/api/v1/maps/{}/connections", map.id))
        .add_cookie(Cookie::new(SESSION_COOKIE, jwt))
        .json(&json!({ "system_a_id": 41000001, "system_b_id": 41000001 }))
        .await;

    resp.assert_status_unprocessable_entity();
}

// ── GET /api/v1/maps/{id}/routes ─────────────────────────────────────────────

#[tokio::test]
async fn get_routes_returns_list() {
    let (_pg, pool) = common::setup_db().await;
    let state = common::test_state(pool.clone());
    let server = TestServer::new(erbridge_api::router_from_state(state.clone()));

    let account_id = make_account(&pool, 80007, "Route Handler Pilot").await;
    seed_solar(&pool, 41000010).await;
    seed_solar(&pool, 41000011).await;

    let map = create_map(
        &pool,
        account_id,
        "Route Handler Map",
        "route-handler-map",
        None,
        None,
    )
    .await
    .unwrap();

    // Create a connection so there's at least one reachable system.
    erbridge_api::services::map::create_connection(
        &pool,
        account_id,
        erbridge_api::services::map::CreateConnectionInput {
            map_id: map.id,
            system_a_id: 41000010,
            system_b_id: 41000011,
        },
    )
    .await
    .unwrap();

    let jwt = session_jwt(account_id, &state.config.jwt_key);
    let resp = server
        .get(&format!(
            "/api/v1/maps/{}/routes?start_system_id=41000010",
            map.id
        ))
        .add_cookie(Cookie::new(SESSION_COOKIE, jwt))
        .await;

    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    let routes = body["data"]["routes"].as_array().unwrap();
    assert!(!routes.is_empty(), "should have at least one route");
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/maps/:map_id/connections/:conn_id
// ---------------------------------------------------------------------------

#[tokio::test]
async fn delete_connection_returns_204() {
    let (_pg, pool) = common::setup_db().await;
    let state = common::test_state(pool.clone());
    let server = TestServer::new(erbridge_api::router_from_state(state.clone()));

    let account_id = make_account(&pool, 60000001, "Pilot").await;
    seed_solar(&pool, 41000020).await;
    seed_solar(&pool, 41000021).await;

    let map = create_map(&pool, account_id, "Delete Conn Map", "delete-conn-map", None, None)
        .await
        .unwrap();

    let (conn, _, _) = create_connection(
        &pool,
        account_id,
        CreateConnectionInput {
            map_id: map.id,
            system_a_id: 41000020,
            system_b_id: 41000021,
        },
    )
    .await
    .unwrap();

    let jwt = session_jwt(account_id, &state.config.jwt_key);
    let resp = server
        .delete(&format!(
            "/api/v1/maps/{}/connections/{}",
            map.id, conn.connection_id
        ))
        .add_cookie(Cookie::new(SESSION_COOKIE, jwt))
        .await;

    assert_eq!(resp.status_code(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn delete_connection_wrong_map_returns_422() {
    let (_pg, pool) = common::setup_db().await;
    let state = common::test_state(pool.clone());
    let server = TestServer::new(erbridge_api::router_from_state(state.clone()));

    let account_id = make_account(&pool, 60000002, "Pilot").await;
    seed_solar(&pool, 41000022).await;
    seed_solar(&pool, 41000023).await;

    let map_a = create_map(&pool, account_id, "Map A", "del-map-a", None, None)
        .await
        .unwrap();
    let map_b = create_map(&pool, account_id, "Map B", "del-map-b", None, None)
        .await
        .unwrap();

    let (conn, _, _) = create_connection(
        &pool,
        account_id,
        CreateConnectionInput {
            map_id: map_a.id,
            system_a_id: 41000022,
            system_b_id: 41000023,
        },
    )
    .await
    .unwrap();

    // Try to delete connection from map_a using map_b's ID.
    let jwt = session_jwt(account_id, &state.config.jwt_key);
    let resp = server
        .delete(&format!(
            "/api/v1/maps/{}/connections/{}",
            map_b.id, conn.connection_id
        ))
        .add_cookie(Cookie::new(SESSION_COOKIE, jwt))
        .await;

    assert_eq!(resp.status_code(), StatusCode::UNPROCESSABLE_ENTITY);
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/maps/:map_id/signatures/:sig_id
// ---------------------------------------------------------------------------

#[tokio::test]
async fn delete_signature_returns_204() {
    let (_pg, pool) = common::setup_db().await;
    let state = common::test_state(pool.clone());
    let server = TestServer::new(erbridge_api::router_from_state(state.clone()));

    let account_id = make_account(&pool, 60000003, "Pilot").await;
    seed_solar(&pool, 41000030).await;

    let map = create_map(&pool, account_id, "Delete Sig Map", "delete-sig-map", None, None)
        .await
        .unwrap();

    let sig = add_signature(
        &pool,
        account_id,
        AddSignatureInput {
            map_id: map.id,
            system_id: 41000030,
            sig_code: "ABC-123".to_string(),
            sig_type: "wormhole".to_string(),
        },
    )
    .await
    .unwrap();

    let jwt = session_jwt(account_id, &state.config.jwt_key);
    let resp = server
        .delete(&format!(
            "/api/v1/maps/{}/signatures/{}",
            map.id, sig.signature_id
        ))
        .add_cookie(Cookie::new(SESSION_COOKIE, jwt))
        .await;

    assert_eq!(resp.status_code(), StatusCode::NO_CONTENT);
}

// ---------------------------------------------------------------------------
// GET /api/v1/maps/:map_id
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_map_returns_map() {
    let (_pg, pool) = common::setup_db().await;
    let state = common::test_state(pool.clone());
    let server = TestServer::new(erbridge_api::router_from_state(state.clone()));

    let account_id = make_account(&pool, 60000004, "Pilot").await;

    let map = create_map(&pool, account_id, "Single Map", "single-map", None, None)
        .await
        .unwrap();

    let jwt = session_jwt(account_id, &state.config.jwt_key);
    let resp = server
        .get(&format!("/api/v1/maps/{}", map.id))
        .add_cookie(Cookie::new(SESSION_COOKIE, jwt))
        .await;

    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body["data"]["name"].as_str().unwrap(), "Single Map");
}

#[tokio::test]
async fn get_map_non_member_returns_403() {
    let (_pg, pool) = common::setup_db().await;
    let state = common::test_state(pool.clone());
    let server = TestServer::new(erbridge_api::router_from_state(state.clone()));

    let owner_id = make_account(&pool, 60000005, "Owner").await;
    let other_id = make_account(&pool, 60000006, "Other").await;

    let map = create_map(&pool, owner_id, "Private Map", "private-map", None, None)
        .await
        .unwrap();

    let jwt = session_jwt(other_id, &state.config.jwt_key);
    let resp = server
        .get(&format!("/api/v1/maps/{}", map.id))
        .add_cookie(Cookie::new(SESSION_COOKIE, jwt))
        .await;

    assert_eq!(resp.status_code(), StatusCode::FORBIDDEN);
}
