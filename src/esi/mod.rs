pub mod cache;
pub mod character;
pub mod discovery;
pub mod jwks;
pub mod search;
pub mod token;
pub mod universe;

use std::future::Future;
use std::time::Duration;

use reqwest::Response;
use thiserror::Error;
use tokio::time::sleep;
use tracing::warn;

const MAX_RETRIES_429: u32 = 4;
const MAX_RETRIES_5XX: u32 = 3;
const BASE_BACKOFF_MS: u64 = 500;
const MAX_BACKOFF_MS: u64 = 30_000;

#[derive(Debug, Error)]
pub enum EsiError {
    #[error("ESI rate limited; backed off after {MAX_RETRIES_429} retries")]
    RateLimited,
    #[error("ESI server error {status} after {MAX_RETRIES_5XX} retries")]
    ServerError { status: u16 },
    #[error("ESI returned HTTP {status}")]
    Http { status: u16 },
    #[error("ESI network error: {0}")]
    Network(#[from] reqwest::Error),
}

/// Executes an ESI request with automatic 429 and 5xx retry handling.
///
/// `make_request` is called on each attempt — it must rebuild the request from
/// scratch, since `RequestBuilder` is not `Clone` (POST bodies are consumed).
/// On 429 the function waits for the `Retry-After` duration if present,
/// otherwise uses exponential backoff with jitter, then retries. On 5xx the
/// function retries up to MAX_RETRIES_5XX times with exponential backoff.
/// All other non-2xx responses are returned as `EsiError::Http` immediately.
pub async fn esi_request<F, Fut>(make_request: F) -> Result<Response, EsiError>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<Response, reqwest::Error>>,
{
    esi_request_with_backoff(make_request, BASE_BACKOFF_MS).await
}

pub async fn esi_request_with_backoff<F, Fut>(
    make_request: F,
    base_backoff_ms: u64,
) -> Result<Response, EsiError>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<Response, reqwest::Error>>,
{
    let mut attempts_429 = 0u32;
    let mut attempts_5xx = 0u32;
    let mut total_attempt = 0u32;

    loop {
        let response = make_request().await?;
        let status = response.status();

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            attempts_429 += 1;
            total_attempt += 1;
            if attempts_429 > MAX_RETRIES_429 {
                return Err(EsiError::RateLimited);
            }

            let wait_ms = retry_after_ms(&response)
                .unwrap_or_else(|| exponential_backoff_ms(total_attempt, base_backoff_ms));

            warn!(
                attempt = total_attempt,
                wait_ms, "ESI 429 received, backing off before retry"
            );

            sleep(Duration::from_millis(wait_ms)).await;
        } else if status.is_server_error() {
            attempts_5xx += 1;
            total_attempt += 1;
            if attempts_5xx > MAX_RETRIES_5XX {
                return Err(EsiError::ServerError {
                    status: status.as_u16(),
                });
            }

            let wait_ms = exponential_backoff_ms(total_attempt, base_backoff_ms);

            warn!(
                attempt = total_attempt,
                status = status.as_u16(),
                wait_ms,
                "ESI 5xx received, backing off before retry"
            );

            sleep(Duration::from_millis(wait_ms)).await;
        } else if status.is_success() {
            return Ok(response);
        } else {
            return Err(EsiError::Http {
                status: status.as_u16(),
            });
        }
    }
}

/// Reads `Retry-After` header (seconds) and converts to milliseconds.
fn retry_after_ms(response: &Response) -> Option<u64> {
    response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<f64>().ok())
        .map(|secs| (secs * 1000.0) as u64)
}

/// Exponential backoff with ±25% jitter, capped at MAX_BACKOFF_MS.
fn exponential_backoff_ms(attempt: u32, base_backoff_ms: u64) -> u64 {
    let base = base_backoff_ms.saturating_mul(1u64 << attempt.min(10));
    let capped = base.min(MAX_BACKOFF_MS);
    // Apply up to -25% jitter (never exceeds cap, never goes below base_backoff_ms).
    let jitter = capped / 4;
    let hash = (attempt as u64)
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1);
    capped - (hash % (jitter + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_stays_within_bounds() {
        for attempt in 1..=10 {
            let ms = exponential_backoff_ms(attempt, BASE_BACKOFF_MS);
            assert!(
                ms <= MAX_BACKOFF_MS,
                "attempt {attempt}: {ms}ms exceeds cap"
            );
            assert!(ms >= BASE_BACKOFF_MS, "attempt {attempt}: {ms}ms too low");
        }
    }

    #[test]
    fn backoff_grows_with_attempts() {
        let a1 = exponential_backoff_ms(1, BASE_BACKOFF_MS);
        let a3 = exponential_backoff_ms(3, BASE_BACKOFF_MS);
        let a6 = exponential_backoff_ms(6, BASE_BACKOFF_MS);
        assert!(a3 > a1, "backoff should grow: a1={a1} a3={a3}");
        assert!(a6 >= a3, "backoff should grow: a3={a3} a6={a6}");
    }
}
