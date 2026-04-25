use anyhow::{Context, Result};
use serde::Deserialize;

use super::esi_request;

#[derive(Debug, Deserialize)]
pub struct CharacterPublicInfo {
    pub corporation_id: i64,
    pub alliance_id: Option<i64>,
}

/// Fetches public character info from `GET /characters/{character_id}/`.
/// Retries automatically on ESI 429 responses.
pub async fn get_character_public_info(
    http: &reqwest::Client,
    esi_base: &str,
    character_id: i64,
) -> Result<CharacterPublicInfo> {
    let url = format!("{esi_base}/characters/{character_id}/");

    esi_request(|| async {
        http.get(&url)
            .send()
            .await
            .context("failed to call ESI /characters/{id}/")
    })
    .await
    .context("ESI /characters/{id}/ failed after retries")?
    .json::<CharacterPublicInfo>()
    .await
    .context("failed to parse ESI character public info")
}
