use anyhow::{Context, Result};
use serde::Deserialize;

const WELL_KNOWN_URL: &str = "https://login.eveonline.com/.well-known/oauth-authorization-server";

/// Metadata returned by the EVE SSO `/.well-known/oauth-authorization-server` endpoint.
#[derive(Clone, Debug, Deserialize)]
pub struct EsiMetadata {
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub jwks_uri: String,
}

pub async fn discover(http: &reqwest::Client) -> Result<EsiMetadata> {
    http.get(WELL_KNOWN_URL)
        .send()
        .await
        .context("failed to fetch EVE SSO discovery document")?
        .error_for_status()
        .context("EVE SSO discovery document returned non-2xx")?
        .json::<EsiMetadata>()
        .await
        .context("failed to parse EVE SSO discovery document")
}
