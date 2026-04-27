pub mod cache;
pub mod character;
pub mod discovery;
pub mod jwks;
pub mod search;
pub mod token;
pub mod universe;

use std::future::Future;
use std::time::Duration;

use anyhow::{Result, bail};
use reqwest::Response;
use tokio::time::sleep;
use tracing::warn;

const MAX_RETRIES: u32 = 4;
const BASE_BACKOFF_MS: u64 = 500;
const MAX_BACKOFF_MS: u64 = 30_000;

/// Executes an ESI request with automatic 429 retry handling.
///
/// `make_request` is called on each attempt — it must rebuild the request from
/// scratch, since `RequestBuilder` is not `Clone` (POST bodies are consumed).
/// On 429 the function waits for the `Retry-After` duration if present,
/// otherwise uses exponential backoff with jitter, then retries. All other
/// non-2xx responses are returned as errors immediately without retrying.
pub async fn esi_request<F, Fut>(make_request: F) -> Result<Response>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<Response>>,
{
    let mut attempt = 0u32;

    loop {
        let response = make_request().await?;

        if response.status() != reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Ok(response.error_for_status()?);
        }

        attempt += 1;
        if attempt > MAX_RETRIES {
            bail!("ESI rate limit exceeded after {MAX_RETRIES} retries");
        }

        let wait_ms = retry_after_ms(&response).unwrap_or_else(|| exponential_backoff_ms(attempt));

        warn!(
            attempt,
            wait_ms, "ESI 429 received, backing off before retry"
        );

        sleep(Duration::from_millis(wait_ms)).await;
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
fn exponential_backoff_ms(attempt: u32) -> u64 {
    let base = BASE_BACKOFF_MS.saturating_mul(1u64 << attempt.min(10));
    let capped = base.min(MAX_BACKOFF_MS);
    // Apply up to -25% jitter (never exceeds cap, never goes below BASE_BACKOFF_MS).
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
            let ms = exponential_backoff_ms(attempt);
            assert!(
                ms <= MAX_BACKOFF_MS,
                "attempt {attempt}: {ms}ms exceeds cap"
            );
            assert!(ms >= BASE_BACKOFF_MS, "attempt {attempt}: {ms}ms too low");
        }
    }

    #[test]
    fn backoff_grows_with_attempts() {
        let a1 = exponential_backoff_ms(1);
        let a3 = exponential_backoff_ms(3);
        let a6 = exponential_backoff_ms(6);
        assert!(a3 > a1, "backoff should grow: a1={a1} a3={a3}");
        assert!(a6 >= a3, "backoff should grow: a3={a3} a6={a6}");
    }
}
