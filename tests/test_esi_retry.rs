use erbridge_api::esi::esi_request_with_backoff;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Returns 503 twice then 200; asserts esi_request succeeds.
#[tokio::test]
async fn retries_5xx_and_succeeds() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/test"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(2)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/test"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server)
        .await;

    let url = format!("{}/test", server.uri());
    let client = reqwest::Client::new();

    let response = esi_request_with_backoff(
        || {
            let client = client.clone();
            let url = url.clone();
            async move { Ok(client.get(&url).send().await?) }
        },
        1, // 1ms backoff so the test runs fast
    )
    .await
    .expect("should succeed after retrying 5xx");

    assert_eq!(response.status(), reqwest::StatusCode::OK);
}

/// Returns 503 indefinitely; asserts esi_request fails after exhausting retries.
#[tokio::test]
async fn fails_after_max_5xx_retries() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/test"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;

    let url = format!("{}/test", server.uri());
    let client = reqwest::Client::new();

    let result = esi_request_with_backoff(
        || {
            let client = client.clone();
            let url = url.clone();
            async move { Ok(client.get(&url).send().await?) }
        },
        1,
    )
    .await;

    assert!(result.is_err(), "should fail after exhausting 5xx retries");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("server error") || msg.contains("retries"),
        "unexpected error message: {msg}"
    );
}
