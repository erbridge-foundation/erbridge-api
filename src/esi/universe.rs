use anyhow::{Context, Result};
use serde::Deserialize;

use super::esi_request;

#[derive(Debug, Deserialize)]
pub struct ResolvedName {
    pub category: String,
    pub id: i64,
    pub name: String,
}

/// Resolves a list of EVE entity IDs to names via `POST /universe/names/`.
/// Retries automatically on ESI 429 responses.
pub async fn resolve_names(
    http: &reqwest::Client,
    esi_base: &str,
    ids: Vec<i64>,
) -> Result<Vec<ResolvedName>> {
    if ids.is_empty() {
        return Ok(vec![]);
    }

    let url = format!("{esi_base}/universe/names/");

    esi_request(|| async { http.post(&url).json(&ids).send().await })
        .await
        .map_err(|e| anyhow::anyhow!(e))
        .context("ESI /universe/names/ failed after retries")?
        .json::<Vec<ResolvedName>>()
        .await
        .context("failed to parse ESI /universe/names/ response")
}
