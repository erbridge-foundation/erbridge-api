use anyhow::{Context, Result, anyhow};
use jsonwebtoken::{
    Algorithm, DecodingKey, TokenData, Validation, decode, decode_header,
    jwk::{AlgorithmParameters, JwkSet},
};
use serde::{Deserialize, Serialize};

/// Claims we extract from the EVE SSO access token JWT.
#[derive(Debug, Deserialize, Serialize)]
pub struct EveClaims {
    /// `CHARACTER:EVE:<character_id>`
    pub sub: String,
    /// Character name.
    pub name: String,
    pub iss: String,
    pub aud: serde_json::Value,
    pub exp: u64,
}

pub async fn fetch_jwks(http: &reqwest::Client, jwks_uri: &str) -> Result<JwkSet> {
    http.get(jwks_uri)
        .send()
        .await
        .context("failed to fetch EVE JWK set")?
        .error_for_status()
        .context("EVE JWK set endpoint returned non-2xx")?
        .json::<JwkSet>()
        .await
        .context("failed to parse EVE JWK set")
}

/// Verifies an EVE SSO access token JWT against the cached JWK set.
///
/// Validation rules (ADR-009):
/// - `iss` must be `https://login.eveonline.com/` or `login.eveonline.com`
/// - `aud` must contain both `client_id` and `"EVE Online"`
/// - `exp` must not be in the past
/// - Signature must verify against the JWK matching the token's `kid`
pub fn verify_eve_jwt(token: &str, jwks: &JwkSet, client_id: &str) -> Result<TokenData<EveClaims>> {
    let header = decode_header(token).context("failed to decode EVE JWT header")?;

    let kid = header
        .kid
        .as_deref()
        .ok_or_else(|| anyhow!("EVE JWT missing kid"))?;

    let jwk = jwks
        .find(kid)
        .ok_or_else(|| anyhow!("no JWK found for kid"))?;

    let decoding_key = match &jwk.algorithm {
        AlgorithmParameters::RSA(rsa) => DecodingKey::from_rsa_components(&rsa.n, &rsa.e)
            .context("failed to build RSA decoding key")?,
        other => return Err(anyhow!("unexpected JWK algorithm: {:?}", other)),
    };

    // Disable library-level iss/aud checks — we validate both manually below
    // for full control over the accepted values (ADR-009). Signature and exp
    // are still verified by the library.
    let mut validation = Validation::new(Algorithm::RS256);
    validation.validate_aud = false;
    // iss = None disables issuer validation in jsonwebtoken 10; we check manually.
    validation.iss = None;

    let token_data = decode::<EveClaims>(token, &decoding_key, &validation)
        .context("EVE JWT verification failed")?;

    // Issuer must be one of the two CCP-documented forms (ADR-009).
    // Bare HTTP or any other host is rejected.
    let iss = &token_data.claims.iss;
    if iss != "https://login.eveonline.com"
        && iss != "https://login.eveonline.com/"
        && iss != "login.eveonline.com"
    {
        return Err(anyhow!("EVE JWT issuer not accepted: {iss}"));
    }

    // Audience must contain both `client_id` and `"EVE Online"`.
    let aud_values: Vec<String> = match &token_data.claims.aud {
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        serde_json::Value::String(s) => vec![s.clone()],
        _ => return Err(anyhow!("EVE JWT aud claim has unexpected shape")),
    };

    if !aud_values.iter().any(|a| a == client_id) {
        return Err(anyhow!("EVE JWT aud does not contain client_id"));
    }
    if !aud_values.iter().any(|a| a == "EVE Online") {
        return Err(anyhow!("EVE JWT aud does not contain 'EVE Online'"));
    }

    Ok(token_data)
}

/// Parses `CHARACTER:EVE:<id>` from the `sub` claim and returns the numeric character ID.
pub fn parse_character_id(sub: &str) -> Result<i64> {
    sub.strip_prefix("CHARACTER:EVE:")
        .and_then(|s| s.parse::<i64>().ok())
        .ok_or_else(|| anyhow!("EVE JWT sub claim has unexpected format"))
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_character_id ---

    #[test]
    fn parse_character_id_valid() {
        assert_eq!(
            parse_character_id("CHARACTER:EVE:12345678").unwrap(),
            12345678
        );
    }

    #[test]
    fn parse_character_id_large_id() {
        assert_eq!(
            parse_character_id("CHARACTER:EVE:2112625428").unwrap(),
            2112625428
        );
    }

    #[test]
    fn parse_character_id_missing_prefix() {
        assert!(parse_character_id("12345678").is_err());
        assert!(parse_character_id("EVE:12345678").is_err());
    }

    #[test]
    fn parse_character_id_non_numeric() {
        assert!(parse_character_id("CHARACTER:EVE:abc").is_err());
    }

    #[test]
    fn parse_character_id_empty_id() {
        assert!(parse_character_id("CHARACTER:EVE:").is_err());
    }

    // --- issuer validation logic (tested via the accepted values) ---

    fn is_accepted_issuer(iss: &str) -> bool {
        iss == "https://login.eveonline.com"
            || iss == "https://login.eveonline.com/"
            || iss == "login.eveonline.com"
    }

    #[test]
    fn accepted_issuers() {
        // All three observed/documented CCP forms must be accepted.
        for iss in &[
            "https://login.eveonline.com",  // observed in practice
            "https://login.eveonline.com/", // CCP docs variant
            "login.eveonline.com",          // CCP docs variant
        ] {
            assert!(is_accepted_issuer(iss), "issuer '{iss}' should be accepted");
        }
    }

    #[test]
    fn rejected_issuers() {
        for iss in &[
            "http://login.eveonline.com/",
            "https://evil.example.com/",
            "",
        ] {
            assert!(
                !is_accepted_issuer(iss),
                "issuer '{iss}' should be rejected"
            );
        }
    }
}
