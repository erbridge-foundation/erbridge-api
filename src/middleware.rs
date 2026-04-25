use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};

use crate::{
    db::account::{AccountStatus, get_account_status},
    extractors::AccountId,
    state::AppState,
};

/// Rejects requests from accounts that are not in `active` status.
/// Must run after the `AccountId` extractor has already validated the JWT.
pub async fn require_actclive_account(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let status = get_account_status(&state.db, account_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match status {
        Some(AccountStatus::Active) => Ok(next.run(request).await),
        Some(_) => Err(StatusCode::FORBIDDEN),
        None => Err(StatusCode::UNAUTHORIZED),
    }
}
