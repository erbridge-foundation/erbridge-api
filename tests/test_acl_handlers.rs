mod common;

use axum_test::TestServer;
use axum_test::http::StatusCode;
use chrono::Utc;
use cookie::Cookie;
use erbridge_api::{
    dto::acl::{AclListResponse, AclMemberListResponse, AclMemberResponse, AclResponse},
    router_from_state,
    services::auth::{login_or_register, LoginInput},
};
use serde::Deserialize;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_server(pool: sqlx::PgPool) -> TestServer {
    let state = common::test_state(pool);
    let app = router_from_state(state);
    TestServer::new(app)
}

async fn make_account(pool: &sqlx::PgPool, eve_id: i64, name: &str) -> uuid::Uuid {
    let aes_key = common::test_aes_key();
    login_or_register(
        pool,
        &aes_key,
        LoginInput {
            eve_character_id: eve_id,
            name,
            corporation_id: 98000001,
            alliance_id: None,
            esi_client_id: "test_client_id",
            access_token: "tok",
            refresh_token: "rtok",
            esi_token_expires_at: Utc::now() + chrono::Duration::hours(1),
        },
    )
    .await
    .unwrap()
}

#[derive(Deserialize)]
struct AclEnvelope {
    data: AclResponse,
}

#[derive(Deserialize)]
struct AclListEnvelope {
    data: AclListResponse,
}

#[derive(Deserialize)]
struct MemberEnvelope {
    data: AclMemberResponse,
}

#[derive(Deserialize)]
struct MemberListEnvelope {
    data: AclMemberListResponse,
}

// ---------------------------------------------------------------------------
// GET /api/v1/acls — unauthenticated
// ---------------------------------------------------------------------------

#[tokio::test]
async fn list_acls_unauthenticated_returns_401() {
    let (_pg, pool) = common::setup_db().await;
    let server = make_server(pool);

    let resp = server.get("/api/v1/acls").expect_failure().await;
    assert_eq!(resp.status_code(), StatusCode::UNAUTHORIZED);
}

// ---------------------------------------------------------------------------
// GET /api/v1/acls — empty list for new account
// ---------------------------------------------------------------------------

#[tokio::test]
async fn list_acls_empty_for_new_account() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();
    let account_id = make_account(&pool, 30000001, "Owner").await;

    let session = common::make_session_jwt(account_id, &jwt_key);
    let mut server = make_server(pool);
    server.add_cookie(Cookie::new("erbridge_session", session));

    let resp = server.get("/api/v1/acls").await;
    assert_eq!(resp.status_code(), StatusCode::OK);
    let body: AclListEnvelope = resp.json();
    assert!(body.data.acls.is_empty());
}

// ---------------------------------------------------------------------------
// POST /api/v1/acls — create
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_acl_returns_201() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();
    let account_id = make_account(&pool, 30000002, "Owner").await;

    let session = common::make_session_jwt(account_id, &jwt_key);
    let mut server = make_server(pool);
    server.add_cookie(Cookie::new("erbridge_session", session));

    let resp = server
        .post("/api/v1/acls")
        .json(&serde_json::json!({ "name": "Test ACL" }))
        .await;
    assert_eq!(resp.status_code(), StatusCode::CREATED);

    let body: AclEnvelope = resp.json();
    assert_eq!(body.data.name, "Test ACL");
    assert_eq!(body.data.owner_account_id, Some(account_id));
    assert!(body.data.pending_delete_at.is_some());
}

#[tokio::test]
async fn create_acl_appears_in_list() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();
    let account_id = make_account(&pool, 30000003, "Owner").await;

    let session = common::make_session_jwt(account_id, &jwt_key);
    let mut server = make_server(pool);
    server.add_cookie(Cookie::new("erbridge_session", session));

    server
        .post("/api/v1/acls")
        .json(&serde_json::json!({ "name": "My ACL" }))
        .await;

    let resp = server.get("/api/v1/acls").await;
    let body: AclListEnvelope = resp.json();
    assert_eq!(body.data.acls.len(), 1);
    assert_eq!(body.data.acls[0].name, "My ACL");
}

// ---------------------------------------------------------------------------
// PUT /api/v1/acls/:acl_id — rename
// ---------------------------------------------------------------------------

#[tokio::test]
async fn rename_acl_by_owner_succeeds() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();
    let account_id = make_account(&pool, 30000004, "Owner").await;

    let session = common::make_session_jwt(account_id, &jwt_key);
    let mut server = make_server(pool);
    server.add_cookie(Cookie::new("erbridge_session", session));

    let create_resp: AclEnvelope = server
        .post("/api/v1/acls")
        .json(&serde_json::json!({ "name": "Original" }))
        .await
        .json();

    let acl_id = create_resp.data.id;

    let resp = server
        .put(&format!("/api/v1/acls/{acl_id}"))
        .json(&serde_json::json!({ "name": "Renamed" }))
        .await;
    assert_eq!(resp.status_code(), StatusCode::OK);

    let body: AclEnvelope = resp.json();
    assert_eq!(body.data.name, "Renamed");
}

#[tokio::test]
async fn rename_acl_by_non_owner_returns_403() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();
    let owner_id = make_account(&pool, 30000005, "Owner").await;
    let other_id = make_account(&pool, 30000006, "Other").await;

    let owner_session = common::make_session_jwt(owner_id, &jwt_key);
    let mut owner_server = make_server(pool.clone());
    owner_server.add_cookie(Cookie::new("erbridge_session", owner_session));

    let create_resp: AclEnvelope = owner_server
        .post("/api/v1/acls")
        .json(&serde_json::json!({ "name": "Private" }))
        .await
        .json();
    let acl_id = create_resp.data.id;

    let other_session = common::make_session_jwt(other_id, &jwt_key);
    let mut other_server = make_server(pool);
    other_server.add_cookie(Cookie::new("erbridge_session", other_session));

    let resp = other_server
        .put(&format!("/api/v1/acls/{acl_id}"))
        .json(&serde_json::json!({ "name": "Hijacked" }))
        .expect_failure()
        .await;
    assert_eq!(resp.status_code(), StatusCode::FORBIDDEN);
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/acls/:acl_id
// ---------------------------------------------------------------------------

#[tokio::test]
async fn delete_acl_by_owner_returns_204() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();
    let account_id = make_account(&pool, 30000007, "Owner").await;

    let session = common::make_session_jwt(account_id, &jwt_key);
    let mut server = make_server(pool);
    server.add_cookie(Cookie::new("erbridge_session", session));

    let create_resp: AclEnvelope = server
        .post("/api/v1/acls")
        .json(&serde_json::json!({ "name": "Bye" }))
        .await
        .json();
    let acl_id = create_resp.data.id;

    let resp = server.delete(&format!("/api/v1/acls/{acl_id}")).await;
    assert_eq!(resp.status_code(), StatusCode::NO_CONTENT);

    // Should no longer appear in list.
    let list: AclListEnvelope = server.get("/api/v1/acls").await.json();
    assert!(list.data.acls.is_empty());
}

#[tokio::test]
async fn delete_acl_by_non_owner_returns_403() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();
    let owner_id = make_account(&pool, 30000008, "Owner").await;
    let other_id = make_account(&pool, 30000009, "Other").await;

    let owner_session = common::make_session_jwt(owner_id, &jwt_key);
    let mut owner_server = make_server(pool.clone());
    owner_server.add_cookie(Cookie::new("erbridge_session", owner_session));

    let create_resp: AclEnvelope = owner_server
        .post("/api/v1/acls")
        .json(&serde_json::json!({ "name": "Private" }))
        .await
        .json();
    let acl_id = create_resp.data.id;

    let other_session = common::make_session_jwt(other_id, &jwt_key);
    let mut other_server = make_server(pool);
    other_server.add_cookie(Cookie::new("erbridge_session", other_session));

    let resp = other_server
        .delete(&format!("/api/v1/acls/{acl_id}"))
        .expect_failure()
        .await;
    assert_eq!(resp.status_code(), StatusCode::FORBIDDEN);
}

// ---------------------------------------------------------------------------
// GET /api/v1/acls/:acl_id/members
// ---------------------------------------------------------------------------

#[tokio::test]
async fn list_members_empty_for_new_acl() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();
    let account_id = make_account(&pool, 30000010, "Owner").await;

    let session = common::make_session_jwt(account_id, &jwt_key);
    let mut server = make_server(pool);
    server.add_cookie(Cookie::new("erbridge_session", session));

    let create_resp: AclEnvelope = server
        .post("/api/v1/acls")
        .json(&serde_json::json!({ "name": "ACL" }))
        .await
        .json();
    let acl_id = create_resp.data.id;

    let resp = server.get(&format!("/api/v1/acls/{acl_id}/members")).await;
    assert_eq!(resp.status_code(), StatusCode::OK);
    let body: MemberListEnvelope = resp.json();
    assert!(body.data.members.is_empty());
}

// ---------------------------------------------------------------------------
// POST /api/v1/acls/:acl_id/members — add corporation member
// ---------------------------------------------------------------------------

#[tokio::test]
async fn add_corporation_member_returns_201() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();
    let account_id = make_account(&pool, 30000011, "Owner").await;

    let session = common::make_session_jwt(account_id, &jwt_key);
    let mut server = make_server(pool);
    server.add_cookie(Cookie::new("erbridge_session", session));

    let create_resp: AclEnvelope = server
        .post("/api/v1/acls")
        .json(&serde_json::json!({ "name": "ACL" }))
        .await
        .json();
    let acl_id = create_resp.data.id;

    let resp = server
        .post(&format!("/api/v1/acls/{acl_id}/members"))
        .json(&serde_json::json!({
            "member_type": "corporation",
            "eve_entity_id": 98000001,
            "permission": "read"
        }))
        .await;
    assert_eq!(resp.status_code(), StatusCode::CREATED);

    let body: MemberEnvelope = resp.json();
    assert_eq!(body.data.member_type, "corporation");
    assert_eq!(body.data.eve_entity_id, Some(98000001));
    assert_eq!(body.data.permission, "read");

    // Appears in member list.
    let list: MemberListEnvelope = server
        .get(&format!("/api/v1/acls/{acl_id}/members"))
        .await
        .json();
    assert_eq!(list.data.members.len(), 1);
}

#[tokio::test]
async fn add_member_invalid_permission_returns_422() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();
    let account_id = make_account(&pool, 30000012, "Owner").await;

    let session = common::make_session_jwt(account_id, &jwt_key);
    let mut server = make_server(pool);
    server.add_cookie(Cookie::new("erbridge_session", session));

    let create_resp: AclEnvelope = server
        .post("/api/v1/acls")
        .json(&serde_json::json!({ "name": "ACL" }))
        .await
        .json();
    let acl_id = create_resp.data.id;

    let resp = server
        .post(&format!("/api/v1/acls/{acl_id}/members"))
        .json(&serde_json::json!({
            "member_type": "corporation",
            "eve_entity_id": 98000001,
            "permission": "god"
        }))
        .expect_failure()
        .await;
    assert_eq!(resp.status_code(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn add_member_manage_on_corp_returns_422() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();
    let account_id = make_account(&pool, 30000013, "Owner").await;

    let session = common::make_session_jwt(account_id, &jwt_key);
    let mut server = make_server(pool);
    server.add_cookie(Cookie::new("erbridge_session", session));

    let create_resp: AclEnvelope = server
        .post("/api/v1/acls")
        .json(&serde_json::json!({ "name": "ACL" }))
        .await
        .json();
    let acl_id = create_resp.data.id;

    let resp = server
        .post(&format!("/api/v1/acls/{acl_id}/members"))
        .json(&serde_json::json!({
            "member_type": "corporation",
            "eve_entity_id": 98000001,
            "permission": "manage"
        }))
        .expect_failure()
        .await;
    assert_eq!(resp.status_code(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn add_duplicate_member_returns_422() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();
    let account_id = make_account(&pool, 30000014, "Owner").await;

    let session = common::make_session_jwt(account_id, &jwt_key);
    let mut server = make_server(pool);
    server.add_cookie(Cookie::new("erbridge_session", session));

    let create_resp: AclEnvelope = server
        .post("/api/v1/acls")
        .json(&serde_json::json!({ "name": "ACL" }))
        .await
        .json();
    let acl_id = create_resp.data.id;

    let payload = serde_json::json!({
        "member_type": "alliance",
        "eve_entity_id": 99000001i64,
        "permission": "read"
    });

    server
        .post(&format!("/api/v1/acls/{acl_id}/members"))
        .json(&payload)
        .await;

    let resp = server
        .post(&format!("/api/v1/acls/{acl_id}/members"))
        .json(&payload)
        .expect_failure()
        .await;
    assert_eq!(resp.status_code(), StatusCode::UNPROCESSABLE_ENTITY);
}

// ---------------------------------------------------------------------------
// PATCH /api/v1/acls/:acl_id/members/:member_id
// ---------------------------------------------------------------------------

#[tokio::test]
async fn update_member_permission_returns_200() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();
    let account_id = make_account(&pool, 30000015, "Owner").await;

    let session = common::make_session_jwt(account_id, &jwt_key);
    let mut server = make_server(pool);
    server.add_cookie(Cookie::new("erbridge_session", session));

    let create_resp: AclEnvelope = server
        .post("/api/v1/acls")
        .json(&serde_json::json!({ "name": "ACL" }))
        .await
        .json();
    let acl_id = create_resp.data.id;

    let member_resp: MemberEnvelope = server
        .post(&format!("/api/v1/acls/{acl_id}/members"))
        .json(&serde_json::json!({
            "member_type": "corporation",
            "eve_entity_id": 98000001,
            "permission": "read"
        }))
        .await
        .json();
    let member_id = member_resp.data.id;

    let resp = server
        .patch(&format!("/api/v1/acls/{acl_id}/members/{member_id}"))
        .json(&serde_json::json!({ "permission": "read_write" }))
        .await;
    assert_eq!(resp.status_code(), StatusCode::OK);

    let body: MemberEnvelope = resp.json();
    assert_eq!(body.data.permission, "read_write");
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/acls/:acl_id/members/:member_id
// ---------------------------------------------------------------------------

#[tokio::test]
async fn delete_member_returns_204() {
    let (_pg, pool) = common::setup_db().await;
    let jwt_key = common::test_jwt_key();
    let account_id = make_account(&pool, 30000016, "Owner").await;

    let session = common::make_session_jwt(account_id, &jwt_key);
    let mut server = make_server(pool);
    server.add_cookie(Cookie::new("erbridge_session", session));

    let create_resp: AclEnvelope = server
        .post("/api/v1/acls")
        .json(&serde_json::json!({ "name": "ACL" }))
        .await
        .json();
    let acl_id = create_resp.data.id;

    let member_resp: MemberEnvelope = server
        .post(&format!("/api/v1/acls/{acl_id}/members"))
        .json(&serde_json::json!({
            "member_type": "alliance",
            "eve_entity_id": 99000001i64,
            "permission": "deny"
        }))
        .await
        .json();
    let member_id = member_resp.data.id;

    let resp = server
        .delete(&format!("/api/v1/acls/{acl_id}/members/{member_id}"))
        .await;
    assert_eq!(resp.status_code(), StatusCode::NO_CONTENT);

    let list: MemberListEnvelope = server
        .get(&format!("/api/v1/acls/{acl_id}/members"))
        .await
        .json();
    assert!(list.data.members.is_empty());
}
