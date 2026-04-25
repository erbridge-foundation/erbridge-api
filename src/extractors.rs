use std::sync::Arc;

use axum::{
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
};
use axum_extra::extract::CookieJar;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use uuid::Uuid;

use crate::{dto::auth::SessionClaims, state::AppState};

/// Axum extractor that reads the session cookie, verifies the JWT, and
/// provides the authenticated `account_id`. Returns `401` if missing or invalid.
pub struct AccountId(pub Uuid);

pub const SESSION_COOKIE: &str = "erbridge_session";

impl FromRequestParts<Arc<AppState>> for AccountId {
    type Rejection = StatusCode;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_request_parts(parts, state)
            .await
            .map_err(|_| StatusCode::UNAUTHORIZED)?;

        let token = jar
            .get(SESSION_COOKIE)
            .map(|c| c.value().to_owned())
            .ok_or(StatusCode::UNAUTHORIZED)?;

        let key = DecodingKey::from_secret(&state.config.jwt_key);
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_required_spec_claims(&["exp"]);

        let data = decode::<SessionClaims>(&token, &key, &validation)
            .map_err(|_| StatusCode::UNAUTHORIZED)?;

        Ok(AccountId(data.claims.account_id))
    }
}
