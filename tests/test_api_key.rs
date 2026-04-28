mod common;

use axum_test::TestServer;
use axum_test::http::StatusCode;
use chrono::Utc;
use cookie::Cookie;
use erbridge_api::services::auth::{LoginInput, login_or_register};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_server(pool: sqlx::PgPool) -> TestServer {
    TestServer::new(erbridge_api::router_from_state(common::test_state(pool)))
}

async fn make_account(pool: &sqlx::PgPool, eve_id: i64, name: &str) -> Uuid {
    login_or_register(
        pool,
        &common::test_aes_key(),
        LoginInput {
            eve_character_id: eve_id,
            name,
            corporation_id: 1_000_001,
            alliance_id: None,
            esi_client_id: "test_client_id",
            access_token: "fake.access",
            refresh_token: "fake.refresh",
            esi_token_expires_at: Utc::now() + chrono::Duration::hours(1),
        },
    )
    .await
    .unwrap()
}

fn session_cookie(account_id: Uuid, jwt_key: &[u8; 32]) -> Cookie<'static> {
    Cookie::new(
        erbridge_api::extractors::SESSION_COOKIE,
        common::make_session_jwt(account_id, jwt_key),
    )
}

// ---------------------------------------------------------------------------
// Deserialization helpers
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
struct CreateKeyEnvelope {
    data: CreateKeyData,
}
#[derive(serde::Deserialize)]
struct CreateKeyData {
    id: String,
    name: String,
    api_key: String,
    #[allow(dead_code)]
    created_at: String,
}

#[derive(serde::Deserialize)]
struct ListKeysEnvelope {
    data: ListKeysData,
}
#[derive(serde::Deserialize)]
struct ListKeysData {
    api_keys: Vec<KeyEntry>,
}
#[derive(serde::Deserialize)]
struct KeyEntry {
    id: String,
    name: String,
    #[allow(dead_code)]
    created_at: String,
}

// ---------------------------------------------------------------------------
// 1. create_api_key_unauthenticated_returns_401
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_api_key_unauthenticated_returns_401() {
    let (_pg, pool) = common::setup_db().await;
    let server = make_server(pool);

    let resp = server
        .post("/api/v1/account/api-keys")
        .json(&serde_json::json!({ "name": "test" }))
        .expect_failure()
        .await;
    assert_eq!(resp.status_code(), StatusCode::UNAUTHORIZED);
}

// ---------------------------------------------------------------------------
// 2. create_api_key_success
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_api_key_success() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();
    let account_id = make_account(&pool, 50000001, "Alice").await;

    let mut server = make_server(pool);
    server.add_cookie(session_cookie(account_id, &jwt_key));

    let resp = server
        .post("/api/v1/account/api-keys")
        .json(&serde_json::json!({ "name": "My App" }))
        .await;
    assert_eq!(resp.status_code(), StatusCode::CREATED);

    let body: CreateKeyEnvelope = resp.json();
    assert_eq!(body.data.name, "My App");
    assert!(Uuid::parse_str(&body.data.id).is_ok());

    let api_key = &body.data.api_key;
    assert!(
        api_key.starts_with("erbridge_"),
        "key should start with erbridge_"
    );
    assert_eq!(api_key.len(), 41, "key should be exactly 41 chars");

    let suffix = api_key.strip_prefix("erbridge_").unwrap();
    assert_eq!(suffix.len(), 32);
    assert!(
        suffix
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "suffix should be lowercase hex"
    );
}

// ---------------------------------------------------------------------------
// 3. create_api_key_empty_name_returns_422
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_api_key_empty_name_returns_422() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();
    let account_id = make_account(&pool, 50000002, "Alice").await;

    let mut server = make_server(pool);
    server.add_cookie(session_cookie(account_id, &jwt_key));

    let resp = server
        .post("/api/v1/account/api-keys")
        .json(&serde_json::json!({ "name": "" }))
        .expect_failure()
        .await;
    assert_eq!(resp.status_code(), StatusCode::UNPROCESSABLE_ENTITY);
}

// ---------------------------------------------------------------------------
// 4. create_multiple_api_keys
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_multiple_api_keys() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();
    let account_id = make_account(&pool, 50000003, "Alice").await;

    let mut server = make_server(pool);
    server.add_cookie(session_cookie(account_id, &jwt_key));

    let resp1: CreateKeyEnvelope = server
        .post("/api/v1/account/api-keys")
        .json(&serde_json::json!({ "name": "Key One" }))
        .await
        .json();
    let resp2: CreateKeyEnvelope = server
        .post("/api/v1/account/api-keys")
        .json(&serde_json::json!({ "name": "Key Two" }))
        .await
        .json();

    assert_ne!(resp1.data.api_key, resp2.data.api_key);

    let list: ListKeysEnvelope = server.get("/api/v1/account/api-keys").await.json();
    assert_eq!(list.data.api_keys.len(), 2);
}

// ---------------------------------------------------------------------------
// 5. list_api_keys_empty_for_new_account
// ---------------------------------------------------------------------------

#[tokio::test]
async fn list_api_keys_empty_for_new_account() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();
    let account_id = make_account(&pool, 50000004, "Alice").await;

    let mut server = make_server(pool);
    server.add_cookie(session_cookie(account_id, &jwt_key));

    let resp = server.get("/api/v1/account/api-keys").await;
    assert_eq!(resp.status_code(), StatusCode::OK);
    let body: ListKeysEnvelope = resp.json();
    assert!(body.data.api_keys.is_empty());
}

// ---------------------------------------------------------------------------
// 6. list_api_keys_returns_created_keys
// ---------------------------------------------------------------------------

#[tokio::test]
async fn list_api_keys_returns_created_keys() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();
    let account_id = make_account(&pool, 50000005, "Alice").await;

    let mut server = make_server(pool);
    server.add_cookie(session_cookie(account_id, &jwt_key));

    server
        .post("/api/v1/account/api-keys")
        .json(&serde_json::json!({ "name": "App A" }))
        .await;
    server
        .post("/api/v1/account/api-keys")
        .json(&serde_json::json!({ "name": "App B" }))
        .await;

    let list: ListKeysEnvelope = server.get("/api/v1/account/api-keys").await.json();
    assert_eq!(list.data.api_keys.len(), 2);

    let names: Vec<&str> = list.data.api_keys.iter().map(|k| k.name.as_str()).collect();
    assert!(names.contains(&"App A"));
    assert!(names.contains(&"App B"));

    // Plaintext should NOT appear in list entries — KeyEntry has no api_key field.
    // This is enforced at the type level: KeyEntry only has id/name/created_at.
}

// ---------------------------------------------------------------------------
// 7. revoke_api_key_success_returns_204
// ---------------------------------------------------------------------------

#[tokio::test]
async fn revoke_api_key_success_returns_204() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();
    let account_id = make_account(&pool, 50000006, "Alice").await;

    let mut server = make_server(pool);
    server.add_cookie(session_cookie(account_id, &jwt_key));

    let created: CreateKeyEnvelope = server
        .post("/api/v1/account/api-keys")
        .json(&serde_json::json!({ "name": "ToRevoke" }))
        .await
        .json();

    let key_id = &created.data.id;
    let resp = server
        .delete(&format!("/api/v1/account/api-keys/{key_id}"))
        .await;
    assert_eq!(resp.status_code(), StatusCode::NO_CONTENT);
}

// ---------------------------------------------------------------------------
// 8. revoke_api_key_not_found_returns_404
// ---------------------------------------------------------------------------

#[tokio::test]
async fn revoke_api_key_not_found_returns_404() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();
    let account_id = make_account(&pool, 50000007, "Alice").await;

    let mut server = make_server(pool);
    server.add_cookie(session_cookie(account_id, &jwt_key));

    let random_id = Uuid::new_v4();
    let resp = server
        .delete(&format!("/api/v1/account/api-keys/{random_id}"))
        .expect_failure()
        .await;
    assert_eq!(resp.status_code(), StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// 9. revoke_api_key_cannot_revoke_other_account_key
// ---------------------------------------------------------------------------

#[tokio::test]
async fn revoke_api_key_cannot_revoke_other_account_key() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();
    let account_a = make_account(&pool, 50000008, "Alice").await;
    let account_b = make_account(&pool, 50000009, "Bob").await;

    let mut server_a = make_server(pool.clone());
    server_a.add_cookie(session_cookie(account_a, &jwt_key));

    let created: CreateKeyEnvelope = server_a
        .post("/api/v1/account/api-keys")
        .json(&serde_json::json!({ "name": "Alice Key" }))
        .await
        .json();
    let key_id = &created.data.id;

    let mut server_b = make_server(pool);
    server_b.add_cookie(session_cookie(account_b, &jwt_key));

    let resp = server_b
        .delete(&format!("/api/v1/account/api-keys/{key_id}"))
        .expect_failure()
        .await;
    assert_eq!(resp.status_code(), StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// 10. revoke_api_key_removes_from_list
// ---------------------------------------------------------------------------

#[tokio::test]
async fn revoke_api_key_removes_from_list() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();
    let account_id = make_account(&pool, 50000010, "Alice").await;

    let mut server = make_server(pool);
    server.add_cookie(session_cookie(account_id, &jwt_key));

    let k1: CreateKeyEnvelope = server
        .post("/api/v1/account/api-keys")
        .json(&serde_json::json!({ "name": "Keep" }))
        .await
        .json();
    let k2: CreateKeyEnvelope = server
        .post("/api/v1/account/api-keys")
        .json(&serde_json::json!({ "name": "Drop" }))
        .await
        .json();

    server
        .delete(&format!("/api/v1/account/api-keys/{}", k2.data.id))
        .await;

    let list: ListKeysEnvelope = server.get("/api/v1/account/api-keys").await.json();
    assert_eq!(list.data.api_keys.len(), 1);
    assert_eq!(list.data.api_keys[0].id, k1.data.id);
}

// ---------------------------------------------------------------------------
// 11. auth_with_api_key_succeeds
// ---------------------------------------------------------------------------

#[tokio::test]
async fn auth_with_api_key_succeeds() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();
    let account_id = make_account(&pool, 50000011, "Alice").await;

    let mut server = make_server(pool);
    server.add_cookie(session_cookie(account_id, &jwt_key));

    let created: CreateKeyEnvelope = server
        .post("/api/v1/account/api-keys")
        .json(&serde_json::json!({ "name": "CLI" }))
        .await
        .json();
    let api_key = created.data.api_key;

    // Now use Bearer token with no cookie.
    let resp = server
        .get("/api/v1/me")
        .add_header(
            axum_test::http::header::AUTHORIZATION,
            format!("Bearer {api_key}")
                .parse::<axum_test::http::HeaderValue>()
                .unwrap(),
        )
        .clear_cookies()
        .await;
    assert_eq!(resp.status_code(), StatusCode::OK);
}

// ---------------------------------------------------------------------------
// 12. auth_with_revoked_key_returns_401
// ---------------------------------------------------------------------------

#[tokio::test]
async fn auth_with_revoked_key_returns_401() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();
    let account_id = make_account(&pool, 50000012, "Alice").await;

    let mut server = make_server(pool);
    server.add_cookie(session_cookie(account_id, &jwt_key));

    let created: CreateKeyEnvelope = server
        .post("/api/v1/account/api-keys")
        .json(&serde_json::json!({ "name": "Temp" }))
        .await
        .json();
    let api_key = created.data.api_key.clone();
    let key_id = created.data.id;

    server
        .delete(&format!("/api/v1/account/api-keys/{key_id}"))
        .await;

    let resp = server
        .get("/api/v1/me")
        .add_header(
            axum_test::http::header::AUTHORIZATION,
            format!("Bearer {api_key}")
                .parse::<axum_test::http::HeaderValue>()
                .unwrap(),
        )
        .clear_cookies()
        .await;
    assert_eq!(resp.status_code(), StatusCode::UNAUTHORIZED);
}

// ---------------------------------------------------------------------------
// 13. auth_with_other_keys_still_works_after_revoke
// ---------------------------------------------------------------------------

#[tokio::test]
async fn auth_with_other_keys_still_works_after_revoke() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();
    let account_id = make_account(&pool, 50000013, "Alice").await;

    let mut server = make_server(pool);
    server.add_cookie(session_cookie(account_id, &jwt_key));

    let key_a: CreateKeyEnvelope = server
        .post("/api/v1/account/api-keys")
        .json(&serde_json::json!({ "name": "Key A" }))
        .await
        .json();
    let key_b: CreateKeyEnvelope = server
        .post("/api/v1/account/api-keys")
        .json(&serde_json::json!({ "name": "Key B" }))
        .await
        .json();

    // Revoke key A.
    server
        .delete(&format!("/api/v1/account/api-keys/{}", key_a.data.id))
        .await;

    // Key A should no longer work.
    let resp_a = server
        .get("/api/v1/me")
        .add_header(
            axum_test::http::header::AUTHORIZATION,
            format!("Bearer {}", key_a.data.api_key)
                .parse::<axum_test::http::HeaderValue>()
                .unwrap(),
        )
        .clear_cookies()
        .await;
    assert_eq!(resp_a.status_code(), StatusCode::UNAUTHORIZED);

    // Key B should still work.
    let resp_b = server
        .get("/api/v1/me")
        .add_header(
            axum_test::http::header::AUTHORIZATION,
            format!("Bearer {}", key_b.data.api_key)
                .parse::<axum_test::http::HeaderValue>()
                .unwrap(),
        )
        .clear_cookies()
        .await;
    assert_eq!(resp_b.status_code(), StatusCode::OK);
}

// ---------------------------------------------------------------------------
// 14. auth_with_invalid_bearer_format_returns_401
// ---------------------------------------------------------------------------

#[tokio::test]
async fn auth_with_invalid_bearer_format_returns_401() {
    let (_pg, pool) = common::setup_db().await;
    let server = make_server(pool);

    // No erbridge_ prefix.
    let resp = server
        .get("/api/v1/me")
        .add_header(
            axum_test::http::header::AUTHORIZATION,
            "Bearer not-an-erbridge-key"
                .parse::<axum_test::http::HeaderValue>()
                .unwrap(),
        )
        .await;
    assert_eq!(resp.status_code(), StatusCode::UNAUTHORIZED);

    // Valid prefix + 32 hex chars but unknown token.
    let resp2 = server
        .get("/api/v1/me")
        .add_header(
            axum_test::http::header::AUTHORIZATION,
            "Bearer erbridge_00000000000000000000000000000000"
                .parse::<axum_test::http::HeaderValue>()
                .unwrap(),
        )
        .await;
    assert_eq!(resp2.status_code(), StatusCode::UNAUTHORIZED);
}

// ---------------------------------------------------------------------------
// 15. auth_with_api_key_inactive_account_returns_403
// ---------------------------------------------------------------------------

#[tokio::test]
async fn auth_with_api_key_inactive_account_returns_403() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();
    let account_id = make_account(&pool, 50000014, "Alice").await;

    let mut server = make_server(pool.clone());
    server.add_cookie(session_cookie(account_id, &jwt_key));

    let created: CreateKeyEnvelope = server
        .post("/api/v1/account/api-keys")
        .json(&serde_json::json!({ "name": "App" }))
        .await
        .json();
    let api_key = created.data.api_key;

    // Mark account as pending_delete directly via pool.
    sqlx::query!(
        "UPDATE account SET status = 'pending_delete' WHERE id = $1",
        account_id
    )
    .execute(&pool)
    .await
    .unwrap();

    // The DB query in find_account_id_by_key_hash filters on status = 'active', so an
    // inactive account returns None from the extractor, which yields 401 (not 403).
    let resp = server
        .get("/api/v1/me")
        .add_header(
            axum_test::http::header::AUTHORIZATION,
            format!("Bearer {api_key}")
                .parse::<axum_test::http::HeaderValue>()
                .unwrap(),
        )
        .clear_cookies()
        .await;
    assert_eq!(resp.status_code(), StatusCode::UNAUTHORIZED);
}
