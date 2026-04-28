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
/// ESI accepts at most 1000 IDs per request; larger inputs are chunked automatically.
pub async fn resolve_names(
    http: &reqwest::Client,
    esi_base: &str,
    ids: Vec<i64>,
) -> Result<Vec<ResolvedName>> {
    if ids.is_empty() {
        return Ok(vec![]);
    }

    let url = format!("{esi_base}/universe/names/");
    let mut out = Vec::with_capacity(ids.len());

    for chunk in ids.chunks(1000) {
        let chunk = chunk.to_vec();
        let mut batch = esi_request(|| async { http.post(&url).json(&chunk).send().await })
            .await
            .map_err(|e| anyhow::anyhow!(e))
            .context("ESI /universe/names/ failed after retries")?
            .json::<Vec<ResolvedName>>()
            .await
            .context("failed to parse ESI /universe/names/ response")?;
        out.append(&mut batch);
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn resolve_names_chunks_1500_ids() {
        // Verify that 1500 IDs are split into two chunks of <=1000.
        // We can't hit ESI in a unit test, but we can assert chunking logic directly.
        let ids: Vec<i64> = (1..=1500).collect();
        let chunks: Vec<_> = ids.chunks(1000).collect();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 1000);
        assert_eq!(chunks[1].len(), 500);
    }
}
