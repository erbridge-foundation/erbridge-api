mod common;

use axum::http::StatusCode;
use axum_test::TestServer;
use chrono::Utc;
use cookie::Cookie;
use erbridge_api::dto::auth::SessionClaims;
use erbridge_api::{
    db::character::find_characters_by_account,
    extractors::SESSION_COOKIE,
    services::auth::{
        AttachCharacterInput, LoginInput, attach_character_to_account, login_or_register,
    },
};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn audit_event_count(pool: &sqlx::PgPool, event_type: &str) -> i64 {
    sqlx::query_scalar!(
        "SELECT COUNT(*) FROM audit_log WHERE event_type = $1",
        event_type
    )
    .fetch_one(pool)
    .await
    .unwrap()
    .unwrap_or(0)
}

const FAKE_ACCESS: &str = "fake.access";
const FAKE_REFRESH: &str = "fake.refresh";
const ESI_CLIENT: &str = "test_client_id";
const CORP_ID: i64 = 1_000_001;

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

async fn make_account(pool: &sqlx::PgPool, eve_id: i64, name: &str) -> Uuid {
    let aes_key = common::test_aes_key();
    login_or_register(
        pool,
        &aes_key,
        LoginInput {
            eve_character_id: eve_id,
            name,
            corporation_id: CORP_ID,
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

/// Stub `POST /universe/names/` to return a corporation name for CORP_ID.
async fn stub_names(esi: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/universe/names/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            { "id": CORP_ID, "name": "Test Corp", "category": "corporation" }
        ])))
        .mount(esi)
        .await;
}

// ── GET /api/v1/characters ────────────────────────────────────────────────────

#[tokio::test]
async fn get_characters_returns_list() {
    let (_pg, pool) = common::setup_db().await;
    let esi = MockServer::start().await;
    stub_names(&esi).await;
    let state = common::test_state_with_esi(pool.clone(), esi.uri());
    let server = TestServer::new(erbridge_api::router_from_state(state.clone()));

    let account_id = make_account(&pool, 90001, "Alpha Pilot").await;
    let jwt = session_jwt(account_id, &state.config.jwt_key);

    let resp = server
        .get("/api/v1/characters")
        .add_cookie(Cookie::new(SESSION_COOKIE, jwt))
        .await;

    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    let chars = body["data"]["characters"].as_array().unwrap();
    assert_eq!(chars.len(), 1);
    assert_eq!(chars[0]["name"], "Alpha Pilot");
    assert_eq!(chars[0]["eve_character_id"], 90001_i64);
    assert_eq!(chars[0]["corporation_id"], CORP_ID);
    assert_eq!(chars[0]["is_main"], true);
}

#[tokio::test]
async fn get_characters_without_auth_returns_401() {
    let (_pg, pool) = common::setup_db().await;
    let state = common::test_state(pool.clone());
    let server = TestServer::new(erbridge_api::router_from_state(state));

    let resp = server.get("/api/v1/characters").await;
    resp.assert_status_unauthorized();
}

// ── PUT /api/v1/characters/{id}/main ─────────────────────────────────────────

#[tokio::test]
async fn put_character_main_switches_main() {
    let (_pg, pool) = common::setup_db().await;
    let esi = MockServer::start().await;
    stub_names(&esi).await;
    let state = common::test_state_with_esi(pool.clone(), esi.uri());
    let server = TestServer::new(erbridge_api::router_from_state(state.clone()));
    let aes_key = common::test_aes_key();

    // Create account with first character (becomes main).
    let account_id = make_account(&pool, 90010, "Main Pilot").await;

    // Add a second character to the same account.
    attach_character_to_account(
        &pool,
        &aes_key,
        AttachCharacterInput {
            account_id,
            eve_character_id: 90011,
            name: "Alt Pilot",
            corporation_id: CORP_ID,
            alliance_id: None,
            esi_client_id: ESI_CLIENT,
            access_token: FAKE_ACCESS,
            refresh_token: FAKE_REFRESH,
            esi_token_expires_at: Utc::now() + chrono::Duration::hours(1),
        },
    )
    .await
    .unwrap();

    let chars = find_characters_by_account(&pool, &aes_key, account_id)
        .await
        .unwrap();
    let alt = chars.iter().find(|c| c.eve_character_id == 90011).unwrap();
    let alt_uuid = alt.id;

    let jwt = session_jwt(account_id, &state.config.jwt_key);
    let resp = server
        .put(&format!("/api/v1/characters/{alt_uuid}/main"))
        .add_cookie(Cookie::new(SESSION_COOKIE, jwt))
        .await;

    resp.assert_status(StatusCode::NO_CONTENT);

    // Confirm alt is now main.
    let chars = find_characters_by_account(&pool, &aes_key, account_id)
        .await
        .unwrap();
    let alt = chars.iter().find(|c| c.id == alt_uuid).unwrap();
    assert!(alt.is_main);
}

#[tokio::test]
async fn put_character_main_other_account_returns_404() {
    let (_pg, pool) = common::setup_db().await;
    let state = common::test_state(pool.clone());
    let server = TestServer::new(erbridge_api::router_from_state(state.clone()));
    let aes_key = common::test_aes_key();

    let account_a = make_account(&pool, 90020, "Pilot A").await;
    let account_b = make_account(&pool, 90021, "Pilot B").await;

    // Get account_b's character UUID.
    let chars_b = find_characters_by_account(&pool, &aes_key, account_b)
        .await
        .unwrap();
    let char_b_uuid = chars_b[0].id;

    // Account A tries to set account B's character as their main.
    let jwt = session_jwt(account_a, &state.config.jwt_key);
    let resp = server
        .put(&format!("/api/v1/characters/{char_b_uuid}/main"))
        .add_cookie(Cookie::new(SESSION_COOKIE, jwt))
        .await;

    resp.assert_status_not_found();
}

// ── DELETE /api/v1/characters/{id} ───────────────────────────────────────────

#[tokio::test]
async fn delete_character_removes_alt() {
    let (_pg, pool) = common::setup_db().await;
    let esi = MockServer::start().await;
    stub_names(&esi).await;
    let state = common::test_state_with_esi(pool.clone(), esi.uri());
    let server = TestServer::new(erbridge_api::router_from_state(state.clone()));
    let aes_key = common::test_aes_key();

    let account_id = make_account(&pool, 90030, "Delete Main").await;
    attach_character_to_account(
        &pool,
        &aes_key,
        AttachCharacterInput {
            account_id,
            eve_character_id: 90031,
            name: "Delete Alt",
            corporation_id: CORP_ID,
            alliance_id: None,
            esi_client_id: ESI_CLIENT,
            access_token: FAKE_ACCESS,
            refresh_token: FAKE_REFRESH,
            esi_token_expires_at: Utc::now() + chrono::Duration::hours(1),
        },
    )
    .await
    .unwrap();

    let chars = find_characters_by_account(&pool, &aes_key, account_id)
        .await
        .unwrap();
    let alt = chars.iter().find(|c| c.eve_character_id == 90031).unwrap();
    let alt_uuid = alt.id;

    let jwt = session_jwt(account_id, &state.config.jwt_key);
    let resp = server
        .delete(&format!("/api/v1/characters/{alt_uuid}"))
        .add_cookie(Cookie::new(SESSION_COOKIE, jwt))
        .await;

    resp.assert_status(StatusCode::NO_CONTENT);

    let chars = find_characters_by_account(&pool, &aes_key, account_id)
        .await
        .unwrap();
    assert!(chars.iter().all(|c| c.id != alt_uuid));
}

#[tokio::test]
async fn delete_main_character_returns_422() {
    let (_pg, pool) = common::setup_db().await;
    let state = common::test_state(pool.clone());
    let server = TestServer::new(erbridge_api::router_from_state(state.clone()));
    let aes_key = common::test_aes_key();

    let account_id = make_account(&pool, 90040, "Solo Main").await;
    let chars = find_characters_by_account(&pool, &aes_key, account_id)
        .await
        .unwrap();
    let main_uuid = chars[0].id;

    let jwt = session_jwt(account_id, &state.config.jwt_key);
    let resp = server
        .delete(&format!("/api/v1/characters/{main_uuid}"))
        .add_cookie(Cookie::new(SESSION_COOKIE, jwt))
        .await;

    resp.assert_status_unprocessable_entity();
}

#[tokio::test]
async fn delete_character_other_account_returns_404() {
    let (_pg, pool) = common::setup_db().await;
    let state = common::test_state(pool.clone());
    let server = TestServer::new(erbridge_api::router_from_state(state.clone()));
    let aes_key = common::test_aes_key();

    let account_a = make_account(&pool, 90050, "Owner A").await;
    let account_b = make_account(&pool, 90051, "Owner B").await;

    let chars_b = find_characters_by_account(&pool, &aes_key, account_b)
        .await
        .unwrap();
    let char_b_uuid = chars_b[0].id;

    let jwt = session_jwt(account_a, &state.config.jwt_key);
    let resp = server
        .delete(&format!("/api/v1/characters/{char_b_uuid}"))
        .add_cookie(Cookie::new(SESSION_COOKIE, jwt))
        .await;

    resp.assert_status_not_found();
}

// ── Audit log assertions ──────────────────────────────────────────────────────

#[tokio::test]
async fn delete_character_records_audit_row() {
    let (_pg, pool) = common::setup_db().await;
    let esi = MockServer::start().await;
    stub_names(&esi).await;
    let state = common::test_state_with_esi(pool.clone(), esi.uri());
    let server = TestServer::new(erbridge_api::router_from_state(state.clone()));
    let aes_key = common::test_aes_key();

    let account_id = make_account(&pool, 91001, "Audit Main").await;
    attach_character_to_account(
        &pool,
        &aes_key,
        AttachCharacterInput {
            account_id,
            eve_character_id: 91002,
            name: "Audit Alt",
            corporation_id: CORP_ID,
            alliance_id: None,
            esi_client_id: ESI_CLIENT,
            access_token: FAKE_ACCESS,
            refresh_token: FAKE_REFRESH,
            esi_token_expires_at: Utc::now() + chrono::Duration::hours(1),
        },
    )
    .await
    .unwrap();

    let chars = find_characters_by_account(&pool, &aes_key, account_id)
        .await
        .unwrap();
    let alt_uuid = chars
        .iter()
        .find(|c| c.eve_character_id == 91002)
        .unwrap()
        .id;

    let before = audit_event_count(&pool, "character_removed").await;

    let jwt = session_jwt(account_id, &state.config.jwt_key);
    let resp = server
        .delete(&format!("/api/v1/characters/{alt_uuid}"))
        .add_cookie(Cookie::new(SESSION_COOKIE, jwt))
        .await;
    resp.assert_status(StatusCode::NO_CONTENT);

    let after = audit_event_count(&pool, "character_removed").await;
    assert_eq!(after, before + 1);
}

#[tokio::test]
async fn set_main_character_records_audit_row() {
    let (_pg, pool) = common::setup_db().await;
    let esi = MockServer::start().await;
    stub_names(&esi).await;
    let state = common::test_state_with_esi(pool.clone(), esi.uri());
    let server = TestServer::new(erbridge_api::router_from_state(state.clone()));
    let aes_key = common::test_aes_key();

    let account_id = make_account(&pool, 92001, "Main Audit Pilot").await;
    attach_character_to_account(
        &pool,
        &aes_key,
        AttachCharacterInput {
            account_id,
            eve_character_id: 92002,
            name: "Alt Audit Pilot",
            corporation_id: CORP_ID,
            alliance_id: None,
            esi_client_id: ESI_CLIENT,
            access_token: FAKE_ACCESS,
            refresh_token: FAKE_REFRESH,
            esi_token_expires_at: Utc::now() + chrono::Duration::hours(1),
        },
    )
    .await
    .unwrap();

    let chars = find_characters_by_account(&pool, &aes_key, account_id)
        .await
        .unwrap();
    let alt_uuid = chars
        .iter()
        .find(|c| c.eve_character_id == 92002)
        .unwrap()
        .id;

    let before = audit_event_count(&pool, "character_set_main").await;

    let jwt = session_jwt(account_id, &state.config.jwt_key);
    let resp = server
        .put(&format!("/api/v1/characters/{alt_uuid}/main"))
        .add_cookie(Cookie::new(SESSION_COOKIE, jwt))
        .await;
    resp.assert_status(StatusCode::NO_CONTENT);

    let after = audit_event_count(&pool, "character_set_main").await;
    assert_eq!(after, before + 1);
}
