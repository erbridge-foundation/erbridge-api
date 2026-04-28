use std::sync::Arc;

use axum::{
    extract::FromRequestParts,
    http::{StatusCode, header::AUTHORIZATION, request::Parts},
};
use axum_extra::extract::CookieJar;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use uuid::Uuid;

use crate::{crypto::sha256_hex, dto::auth::SessionClaims, state::AppState};

/// Axum extractor that reads the session cookie, verifies the JWT, and
/// provides the authenticated `account_id`. Returns `401` if missing or invalid.
pub struct AccountId(pub Uuid);

/// Axum extractor that resolves to the authenticated account_id only if that
/// account has `is_server_admin = TRUE`. Returns `401` if unauthenticated,
/// `403` if authenticated but not a server admin.
pub struct ServerAdmin(pub Uuid);

pub const SESSION_COOKIE: &str = "erbridge_session";

impl FromRequestParts<Arc<AppState>> for AccountId {
    type Rejection = StatusCode;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        // Path 1: session cookie (existing behaviour — unchanged)
        let jar = CookieJar::from_request_parts(parts, state)
            .await
            .map_err(|_| StatusCode::UNAUTHORIZED)?;

        if let Some(cookie) = jar.get(SESSION_COOKIE) {
            let key = DecodingKey::from_secret(&state.config.jwt_key);
            let mut validation = Validation::new(Algorithm::HS256);
            validation.set_required_spec_claims(&["exp"]);
            let data = decode::<SessionClaims>(cookie.value(), &key, &validation)
                .map_err(|_| StatusCode::UNAUTHORIZED)?;
            return Ok(AccountId(data.claims.account_id));
        }

        // Path 2: Bearer API key
        if let Some(auth_header) = parts.headers.get(AUTHORIZATION) {
            let header_str = auth_header.to_str().map_err(|_| StatusCode::UNAUTHORIZED)?;
            let raw_key = header_str
                .strip_prefix("Bearer ")
                .ok_or(StatusCode::UNAUTHORIZED)?;
            // Fast reject: all erbridge API keys start with this prefix
            if !raw_key.starts_with("erbridge_") {
                return Err(StatusCode::UNAUTHORIZED);
            }
            let key_hash = sha256_hex(raw_key.as_bytes());
            let account_id = crate::db::api_key::find_account_id_by_key_hash(&state.db, &key_hash)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            return account_id.map(AccountId).ok_or(StatusCode::UNAUTHORIZED);
        }

        Err(StatusCode::UNAUTHORIZED)
    }
}

impl FromRequestParts<Arc<AppState>> for ServerAdmin {
    type Rejection = StatusCode;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let AccountId(account_id) = AccountId::from_request_parts(parts, state).await?;
        let is_admin = crate::db::account::is_server_admin(&state.db, account_id)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if is_admin {
            Ok(ServerAdmin(account_id))
        } else {
            Err(StatusCode::FORBIDDEN)
        }
    }
}
