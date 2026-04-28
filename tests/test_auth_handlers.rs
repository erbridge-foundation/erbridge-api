mod common;

use axum_test::TestServer;
use erbridge_api::router_from_state;
use serde_json::Value;

/// Verify that callback error responses use the ApiResponse envelope
/// (`{"error":"..."}`) rather than a plain string body (ADR-021 / F3).
#[tokio::test]
async fn callback_bad_state_returns_envelope() {
    let (_pg, pool) = common::setup_db().await;
    let state = common::test_state(pool);
    let app = router_from_state(state);
    let server = TestServer::new(app);

    let resp = server
        .get("/auth/callback")
        .add_query_param("code", "any")
        .add_query_param("state", "not-a-valid-jwt")
        .await;

    assert_eq!(resp.status_code(), 400);
    let body: Value = resp.json();
    assert!(
        body.get("error").is_some(),
        "expected {{\"error\": ...}} envelope, got: {body}"
    );
    assert!(
        body.get("data").is_none(),
        "unexpected 'data' key in error response: {body}"
    );
}
