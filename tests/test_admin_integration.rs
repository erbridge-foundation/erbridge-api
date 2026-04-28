mod common;

use axum_test::TestServer;
use axum_test::http::StatusCode;
use chrono::Utc;
use cookie::Cookie;
use erbridge_api::{
    db::account as db_account,
    extractors::SESSION_COOKIE,
    router_from_state,
    services::auth::{LoginInput, login_or_register},
};
use serde_json::Value;
use uuid::Uuid;

const ESI_CLIENT: &str = "test_client_id";
const FAKE_ACCESS: &str = "fake.access.token";
const FAKE_REFRESH: &str = "fake.refresh.token";
const CORP_ID: i64 = 98000001;

fn make_server(pool: sqlx::PgPool) -> TestServer {
    let state = common::test_state(pool);
    TestServer::new(router_from_state(state))
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

async fn force_admin(pool: &sqlx::PgPool, account_id: Uuid, value: bool) {
    sqlx::query!(
        "UPDATE account SET is_server_admin = $2 WHERE id = $1",
        account_id,
        value,
    )
    .execute(pool)
    .await
    .unwrap();
}

fn auth_cookie(account_id: Uuid, jwt_key: &[u8; 32]) -> Cookie<'static> {
    Cookie::new(
        SESSION_COOKIE,
        common::make_session_jwt(account_id, jwt_key),
    )
}

// ---------------------------------------------------------------------------
// Bootstrap: first account becomes server-admin; subsequent do not.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bootstrap_first_account_is_server_admin() {
    let (_pg, pool) = common::setup_db().await;

    let first = make_account(&pool, 30000001, "First").await;
    let second = make_account(&pool, 30000002, "Second").await;

    assert!(db_account::is_server_admin(&pool, first).await.unwrap());
    assert!(!db_account::is_server_admin(&pool, second).await.unwrap());

    // Bootstrap event was recorded with NULL actor and source = "first_account_bootstrap".
    let row = sqlx::query!(
        r#"SELECT actor_account_id, details FROM audit_log WHERE event_type = 'server_admin_granted'"#
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.actor_account_id, None);
    assert_eq!(row.details["account_id"], first.to_string());
    assert_eq!(row.details["source"], "first_account_bootstrap");
}

// ---------------------------------------------------------------------------
// Admin gating: non-admin authenticated user gets 403 on admin routes.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn admin_routes_reject_non_admin_with_403() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();

    // First account is the admin (bootstrap); we test the non-admin path.
    let _admin = make_account(&pool, 30000010, "Admin").await;
    let user = make_account(&pool, 30000011, "User").await;

    let mut server = make_server(pool);
    server.add_cookie(auth_cookie(user, &jwt_key));

    let resp = server.get("/api/v1/admin/accounts").expect_failure().await;
    assert_eq!(resp.status_code(), StatusCode::FORBIDDEN);

    let resp = server.get("/api/v1/admin/audit-log").expect_failure().await;
    assert_eq!(resp.status_code(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn admin_routes_reject_unauthenticated_with_401() {
    let (_pg, pool) = common::setup_db().await;
    // No admin exists yet — the gate should still reject unauthenticated callers.
    let server = make_server(pool);

    let resp = server.get("/api/v1/admin/accounts").expect_failure().await;
    assert_eq!(resp.status_code(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn admin_can_list_accounts() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();

    let admin = make_account(&pool, 30000020, "Admin").await;
    let _user = make_account(&pool, 30000021, "User").await;

    let mut server = make_server(pool);
    server.add_cookie(auth_cookie(admin, &jwt_key));

    let resp = server.get("/api/v1/admin/accounts").await;
    assert_eq!(resp.status_code(), StatusCode::OK);
    let body: Value = resp.json();
    let accounts = body["data"]["accounts"].as_array().unwrap();
    assert_eq!(accounts.len(), 2);
}

// ---------------------------------------------------------------------------
// Last-admin guard.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn revoke_last_admin_returns_409() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();

    // Only one admin (the bootstrap account).
    let admin = make_account(&pool, 30000030, "OnlyAdmin").await;

    let mut server = make_server(pool.clone());
    server.add_cookie(auth_cookie(admin, &jwt_key));

    // Self-revoke would drop admin count to zero — must be rejected.
    let resp = server
        .post(&format!("/api/v1/admin/accounts/{admin}/revoke-admin"))
        .expect_failure()
        .await;
    assert_eq!(resp.status_code(), StatusCode::CONFLICT);
    let body: Value = resp.json();
    assert_eq!(body["error"], "cannot revoke the last server admin");

    // Admin flag is still set.
    assert!(db_account::is_server_admin(&pool, admin).await.unwrap());
}

#[tokio::test]
async fn self_revoke_allowed_when_another_admin_exists() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();

    let admin_a = make_account(&pool, 30000040, "AdminA").await;
    let admin_b = make_account(&pool, 30000041, "AdminB").await;
    // Promote B so two admins exist.
    force_admin(&pool, admin_b, true).await;

    let mut server = make_server(pool.clone());
    server.add_cookie(auth_cookie(admin_a, &jwt_key));

    // A revokes themselves — allowed.
    let resp = server
        .post(&format!("/api/v1/admin/accounts/{admin_a}/revoke-admin"))
        .await;
    assert_eq!(resp.status_code(), StatusCode::NO_CONTENT);

    assert!(!db_account::is_server_admin(&pool, admin_a).await.unwrap());
    assert!(db_account::is_server_admin(&pool, admin_b).await.unwrap());

    // After self-revoke, A is no longer admin and admin routes reject.
    let resp = server.get("/api/v1/admin/accounts").expect_failure().await;
    assert_eq!(resp.status_code(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn grant_admin_promotes_target_account() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();

    let admin = make_account(&pool, 30000050, "Admin").await;
    let target = make_account(&pool, 30000051, "Target").await;

    let mut server = make_server(pool.clone());
    server.add_cookie(auth_cookie(admin, &jwt_key));

    let resp = server
        .post(&format!("/api/v1/admin/accounts/{target}/grant-admin"))
        .await;
    assert_eq!(resp.status_code(), StatusCode::NO_CONTENT);

    assert!(db_account::is_server_admin(&pool, target).await.unwrap());

    // Audit event recorded with admin source.
    let row = sqlx::query!(
        r#"
        SELECT actor_account_id, details
        FROM audit_log
        WHERE event_type = 'server_admin_granted'
          AND actor_account_id IS NOT NULL
        "#
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.actor_account_id, Some(admin));
    assert_eq!(row.details["account_id"], target.to_string());
    assert_eq!(row.details["source"], "admin_grant");
}

#[tokio::test]
async fn revoke_admin_on_non_existent_account_returns_404() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();

    let admin = make_account(&pool, 30000060, "Admin").await;
    let mut server = make_server(pool);
    server.add_cookie(auth_cookie(admin, &jwt_key));

    let stranger = Uuid::new_v4();
    let resp = server
        .post(&format!("/api/v1/admin/accounts/{stranger}/revoke-admin"))
        .expect_failure()
        .await;
    assert_eq!(resp.status_code(), StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// Blocked-character flow: admin blocks an EVE id; the owning account loses
// access through `require_active_account` (one ban = account banned).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn block_character_bans_owning_account_via_middleware() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();

    let admin = make_account(&pool, 30000070, "Admin").await;
    let user = make_account(&pool, 30000071, "User").await;

    let mut server = make_server(pool.clone());

    // Sanity: user can hit an authenticated endpoint before the block.
    server.add_cookie(auth_cookie(user, &jwt_key));
    let resp = server.get("/api/v1/me").await;
    assert_eq!(resp.status_code(), StatusCode::OK);

    // Admin blocks the user's EVE character.
    server.clear_cookies();
    server.add_cookie(auth_cookie(admin, &jwt_key));
    let resp = server
        .post("/api/v1/admin/characters/30000071/block")
        .json(&serde_json::json!({ "reason": "test" }))
        .await;
    assert_eq!(resp.status_code(), StatusCode::NO_CONTENT);

    // User's session is now rejected by `require_active_account`.
    server.clear_cookies();
    server.add_cookie(auth_cookie(user, &jwt_key));
    let resp = server.get("/api/v1/me").expect_failure().await;
    assert_eq!(resp.status_code(), StatusCode::FORBIDDEN);

    // Audit event recorded.
    let row = sqlx::query!(
        r#"
        SELECT actor_account_id, details
        FROM audit_log
        WHERE event_type = 'eve_character_blocked'
        "#
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.actor_account_id, Some(admin));
    assert_eq!(row.details["eve_character_id"], 30000071i64);
    assert_eq!(row.details["reason"], "test");
}

#[tokio::test]
async fn blocked_account_cannot_reach_admin_routes_even_if_admin() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();

    // Only admin — and we'll block their own character. Bans must apply even
    // to the server-admin (otherwise a self-block would still let them in).
    let admin = make_account(&pool, 30000080, "Admin").await;

    // Insert directly since blocking via the API would require another admin.
    sqlx::query!(
        "INSERT INTO blocked_eve_character (eve_character_id, reason) VALUES ($1, $2)",
        30000080i64,
        Some("self-test"),
    )
    .execute(&pool)
    .await
    .unwrap();

    let mut server = make_server(pool.clone());
    server.add_cookie(auth_cookie(admin, &jwt_key));

    // `require_active_account` runs before `require_server_admin` — banned takes precedence.
    let resp = server.get("/api/v1/admin/accounts").expect_failure().await;
    assert_eq!(resp.status_code(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn unblock_character_restores_account_access() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();

    let admin = make_account(&pool, 30000090, "Admin").await;
    let user = make_account(&pool, 30000091, "User").await;

    let mut server = make_server(pool);
    server.add_cookie(auth_cookie(admin, &jwt_key));

    // Block.
    let resp = server.post("/api/v1/admin/characters/30000091/block").await;
    assert_eq!(resp.status_code(), StatusCode::NO_CONTENT);

    // Confirm user is locked out.
    server.clear_cookies();
    server.add_cookie(auth_cookie(user, &jwt_key));
    let resp = server.get("/api/v1/me").expect_failure().await;
    assert_eq!(resp.status_code(), StatusCode::FORBIDDEN);

    // Admin unblocks.
    server.clear_cookies();
    server.add_cookie(auth_cookie(admin, &jwt_key));
    let resp = server
        .post("/api/v1/admin/characters/30000091/unblock")
        .await;
    assert_eq!(resp.status_code(), StatusCode::NO_CONTENT);

    // User can hit endpoints again.
    server.clear_cookies();
    server.add_cookie(auth_cookie(user, &jwt_key));
    let resp = server.get("/api/v1/me").await;
    assert_eq!(resp.status_code(), StatusCode::OK);
}

#[tokio::test]
async fn unblock_unknown_character_returns_404() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();

    let admin = make_account(&pool, 30000100, "Admin").await;
    let mut server = make_server(pool);
    server.add_cookie(auth_cookie(admin, &jwt_key));

    let resp = server
        .post("/api/v1/admin/characters/9999999/unblock")
        .expect_failure()
        .await;
    assert_eq!(resp.status_code(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn list_blocked_characters_returns_blocked_set() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();

    let admin = make_account(&pool, 30000110, "Admin").await;
    let _u1 = make_account(&pool, 30000111, "User1").await;
    let _u2 = make_account(&pool, 30000112, "User2").await;
    let mut server = make_server(pool);
    server.add_cookie(auth_cookie(admin, &jwt_key));

    server
        .post("/api/v1/admin/characters/30000111/block")
        .json(&serde_json::json!({ "reason": "spam" }))
        .await;
    server.post("/api/v1/admin/characters/30000112/block").await;

    let resp = server.get("/api/v1/admin/characters/blocked").await;
    assert_eq!(resp.status_code(), StatusCode::OK);
    let body: Value = resp.json();
    let blocked = body["data"]["blocked"].as_array().unwrap();
    assert_eq!(blocked.len(), 2);
}

// ---------------------------------------------------------------------------
// Audit-log read endpoint via HTTP.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn audit_log_endpoint_returns_recent_entries() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();

    let admin = make_account(&pool, 30000120, "Admin").await;
    let _user = make_account(&pool, 30000121, "User").await;
    let mut server = make_server(pool);
    server.add_cookie(auth_cookie(admin, &jwt_key));

    // Trigger an admin action so there's at least one filterable event.
    server
        .post("/api/v1/admin/characters/30000121/block")
        .json(&serde_json::json!({ "reason": "hello" }))
        .await;

    let resp = server
        .get("/api/v1/admin/audit-log")
        .add_query_param("event_type", "eve_character_blocked")
        .await;
    assert_eq!(resp.status_code(), StatusCode::OK);
    let body: Value = resp.json();
    let entries = body["data"]["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["event_type"], "eve_character_blocked");
    assert_eq!(entries[0]["actor_account_id"], admin.to_string());
}
