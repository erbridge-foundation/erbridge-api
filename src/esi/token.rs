use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde::Deserialize;
use sqlx::PgPool;
use tracing::{info, warn};

use crate::{
    config::Config,
    db::character::{Character, update_character_tokens},
    esi::character::get_character_public_info,
};

const TOKEN_REFRESH_BUFFER_SECS: i64 = 60;

#[derive(Debug)]
pub enum TokenStatus {
    /// Token is valid; contains the current access token string.
    Fresh(String),
    /// Refresh token has expired — character must re-authenticate via OAuth.
    RefreshExpired,
}

#[derive(Deserialize)]
struct TokenRefreshResponse {
    access_token: String,
    refresh_token: String,
    expires_in: i64,
}

/// Ensures the character's ESI access token is valid, refreshing it if needed.
///
/// Returns `TokenStatus::Fresh(token)` with the current (possibly just-refreshed)
/// access token, or `TokenStatus::RefreshExpired` if the refresh token is too
/// old and the character must re-authenticate (ADR-029).
///
/// Returns `Err` only for hard failures (DB error, ESI token endpoint down).
pub async fn ensure_token_fresh(
    pool: &PgPool,
    http: &reqwest::Client,
    config: &Config,
    token_endpoint: &str,
    character: &Character,
) -> Result<TokenStatus> {
    let access_token = match &character.access_token {
        Some(t) => t,
        None => bail!(
            "character {} has no ESI access token (ghost?)",
            character.eve_character_id
        ),
    };

    let refresh_token = match &character.refresh_token {
        Some(t) => t,
        None => bail!(
            "character {} has no ESI refresh token (ghost?)",
            character.eve_character_id
        ),
    };

    // Check refresh token age — derived from updated_at per ADR-029.
    let max_age = chrono::Duration::days(config.esi_refresh_token_max_days as i64);
    if Utc::now() - character.updated_at > max_age {
        info!(
            eve_character_id = character.eve_character_id,
            max_days = config.esi_refresh_token_max_days,
            "ESI refresh token expired by age; character must re-authenticate"
        );
        return Ok(TokenStatus::RefreshExpired);
    }

    // Check if the access token still has enough life left.
    let needs_refresh = character
        .esi_token_expires_at
        .map(|exp| exp - Utc::now() < chrono::Duration::seconds(TOKEN_REFRESH_BUFFER_SECS))
        .unwrap_or(true); // no expiry recorded → assume stale

    if !needs_refresh {
        return Ok(TokenStatus::Fresh(access_token.clone()));
    }

    // Use the client that originally issued this character's grant.
    // Falling back to the first client only if the stored client_id is missing
    // (legacy rows) or no longer configured (client was rotated out).
    let esi_client = character
        .esi_client_id
        .as_deref()
        .and_then(|id| config.esi_clients.iter().find(|c| c.client_id == id))
        .or_else(|| {
            if character.esi_client_id.is_some() {
                warn!(
                    eve_character_id = character.eve_character_id,
                    "ESI client_id no longer configured; falling back to first client"
                );
            }
            config.esi_clients.first()
        })
        .context("no ESI clients configured")?;

    let resp = http
        .post(token_endpoint)
        .basic_auth(&esi_client.client_id, Some(&esi_client.client_secret))
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token.as_str()),
        ])
        .send()
        .await
        .context("ESI token refresh request failed")?
        .error_for_status()
        .context("ESI token endpoint returned error during refresh")?
        .json::<TokenRefreshResponse>()
        .await
        .context("failed to parse ESI token refresh response")?;

    let new_expires_at = Utc::now() + chrono::Duration::seconds(resp.expires_in);

    // Fetch updated corp/alliance to keep those fields current.
    let public_info = get_character_public_info(http, &config.esi_base, character.eve_character_id)
        .await
        .context("failed to fetch character public info after token refresh")?;

    update_character_tokens(
        pool,
        &config.aes_key,
        character.eve_character_id,
        public_info.corporation_id,
        public_info.alliance_id,
        &esi_client.client_id,
        &resp.access_token,
        &resp.refresh_token,
        new_expires_at,
    )
    .await
    .context("failed to persist refreshed ESI tokens")?;

    info!(
        eve_character_id = character.eve_character_id,
        "ESI access token refreshed"
    );

    Ok(TokenStatus::Fresh(resp.access_token))
}
