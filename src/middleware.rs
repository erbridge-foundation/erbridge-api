use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};

use crate::{
    db::account::{
        AccountStatus, account_has_blocked_character, get_account_status, is_server_admin,
    },
    extractors::AccountId,
    state::AppState,
};

/// Rejects requests from accounts that are not in `active` status, or that
/// have any blocked EVE character (one ban = account banned — see
/// `DECISIONS.md` "Banning model").
/// Must run after the `AccountId` extractor has already validated the JWT.
pub async fn require_active_account(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let status = get_account_status(&state.db, account_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match status {
        Some(AccountStatus::Active) => {}
        Some(_) => return Err(StatusCode::FORBIDDEN),
        None => return Err(StatusCode::UNAUTHORIZED),
    }

    let banned = account_has_blocked_character(&state.db, account_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if banned {
        return Err(StatusCode::FORBIDDEN);
    }

    Ok(next.run(request).await)
}

/// Rejects requests from accounts that do not have `is_server_admin = TRUE`.
/// Must run after the `AccountId` extractor has already validated the JWT.
/// Layer this AFTER `require_active_account` for admin routes.
pub async fn require_server_admin(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let is_admin = is_server_admin(&state.db, account_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if is_admin {
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}
