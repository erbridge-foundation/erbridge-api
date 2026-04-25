use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use rand::seq::IndexedRandom;
use serde::Deserialize;
use tracing::{info, warn};
use url::Url;
use uuid::Uuid;

use crate::{
    db::character::{find_characters_by_account, find_pollable_character_ids_for_account},
    dto::auth::{AuthMode, MeCharacter, MeResponse, SessionClaims, StateClaims},
    dto::envelope::ApiResponse,
    esi::{character::get_character_public_info, jwks::parse_character_id, jwks::verify_eve_jwt},
    extractors::{AccountId, SESSION_COOKIE},
    services::auth::{
        AttachCharacterInput, LoginInput, attach_character_to_account, login_or_register,
    },
    state::AppState,
};

const STATE_JWT_TTL_SECS: u64 = 300; // 5 minutes
const SESSION_JWT_TTL_SECS: u64 = 60 * 60 * 24 * 7; // 7 days

const ESI_SCOPES: &str = "esi-location.read_location.v1 \
    esi-location.read_ship_type.v1 \
    esi-location.read_online.v1 \
    esi-search.search_structures.v1 \
    esi-ui.write_waypoint.v1";

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

// ---------------------------------------------------------------------------
// GET /auth/login
// ---------------------------------------------------------------------------

pub async fn login(State(state): State<Arc<AppState>>) -> Result<Redirect, StatusCode> {
    build_oauth_redirect(&state, AuthMode::Login, None)
}

// ---------------------------------------------------------------------------
// GET /auth/characters/add
// ---------------------------------------------------------------------------

pub async fn add_character(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
) -> Result<Redirect, StatusCode> {
    build_oauth_redirect(&state, AuthMode::Add, Some(account_id))
}

fn build_oauth_redirect(
    state: &AppState,
    mode: AuthMode,
    account_id: Option<Uuid>,
) -> Result<Redirect, StatusCode> {
    let client = state
        .config
        .esi_clients
        .choose(&mut rand::rng())
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let state_claims = StateClaims {
        client_id: client.client_id.clone(),
        mode,
        account_id,
        exp: now_secs() + STATE_JWT_TTL_SECS,
    };

    let state_jwt = encode(
        &Header::default(),
        &state_claims,
        &EncodingKey::from_secret(&state.config.jwt_key),
    )
    .map_err(|e| {
        warn!(error = %e, "failed to encode state JWT");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let mut auth_url = Url::parse(&state.esi_metadata.authorization_endpoint).map_err(|e| {
        warn!(error = %e, "invalid authorization_endpoint URL");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    auth_url
        .query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &client.client_id)
        .append_pair("redirect_uri", &state.config.esi_callback_url)
        .append_pair("scope", ESI_SCOPES)
        .append_pair("state", &state_jwt);

    Ok(Redirect::to(auth_url.as_str()))
}

// ---------------------------------------------------------------------------
// GET /auth/callback
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct CallbackQuery {
    pub code: String,
    pub state: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: i64,
}

pub async fn callback(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(query): axum::extract::Query<CallbackQuery>,
    jar: CookieJar,
) -> Result<(CookieJar, Redirect), Response> {
    let err = |msg: &'static str| -> Response { (StatusCode::BAD_REQUEST, msg).into_response() };

    // --- Verify state JWT ---
    let mut state_validation = Validation::new(Algorithm::HS256);
    state_validation.set_required_spec_claims(&["exp"]);

    let state_data = decode::<StateClaims>(
        &query.state,
        &DecodingKey::from_secret(&state.config.jwt_key),
        &state_validation,
    )
    .map_err(|_| err("invalid state"))?;

    let state_claims = state_data.claims;
    let client_id = &state_claims.client_id;

    let esi_client = state
        .config
        .esi_clients
        .iter()
        .find(|c| &c.client_id == client_id)
        .ok_or_else(|| err("invalid state"))?;

    // --- Exchange code for tokens ---
    let token_resp = state
        .http
        .post(&state.esi_metadata.token_endpoint)
        .basic_auth(&esi_client.client_id, Some(&esi_client.client_secret))
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", &query.code),
            ("redirect_uri", &state.config.esi_callback_url),
        ])
        .send()
        .await
        .map_err(|e| {
            warn!(error = %e, "token exchange request failed");
            (StatusCode::BAD_GATEWAY, "token exchange failed").into_response()
        })?
        .error_for_status()
        .map_err(|e| {
            warn!(error = %e, "token endpoint returned error");
            (StatusCode::BAD_GATEWAY, "token exchange failed").into_response()
        })?
        .json::<TokenResponse>()
        .await
        .map_err(|e| {
            warn!(error = %e, "failed to parse token response");
            (StatusCode::BAD_GATEWAY, "token exchange failed").into_response()
        })?;

    // --- Verify EVE access token JWT (with JWKS rotation retry — ADR-030) ---
    let eve_token_data = {
        let jwks_read = state.jwks.read().await;
        match verify_eve_jwt(&token_resp.access_token, &jwks_read, client_id) {
            Ok(data) => data,
            Err(e) => {
                let kid = jsonwebtoken::decode_header(&token_resp.access_token)
                    .ok()
                    .and_then(|h| h.kid);
                warn!(error = %e, ?kid, "EVE JWT verification failed; re-fetching JWKS");
                drop(jwks_read);

                let fresh = crate::esi::jwks::fetch_jwks(&state.http, &state.esi_metadata.jwks_uri)
                    .await
                    .map_err(|e| {
                        warn!(error = %e, "JWKS re-fetch failed");
                        err("invalid EVE token")
                    })?;
                let result =
                    verify_eve_jwt(&token_resp.access_token, &fresh, client_id).map_err(|e| {
                        warn!(error = %e, "EVE JWT verification failed after JWKS re-fetch");
                        err("invalid EVE token")
                    })?;
                *state.jwks.write().await = fresh;
                result
            }
        }
    };

    let eve_claims = eve_token_data.claims;
    let eve_character_id = parse_character_id(&eve_claims.sub).map_err(|e| {
        warn!(error = %e, "failed to parse EVE character ID from sub claim");
        err("invalid EVE token")
    })?;

    // --- Fetch corp/alliance from ESI ---
    let public_info =
        get_character_public_info(&state.http, &state.config.esi_base, eve_character_id)
            .await
            .map_err(|e| {
                warn!(error = %e, eve_character_id, "ESI character info fetch failed");
                (StatusCode::BAD_GATEWAY, "ESI request failed").into_response()
            })?;

    let expires_at = chrono::Utc::now() + chrono::Duration::seconds(token_resp.expires_in);

    match state_claims.mode {
        AuthMode::Add => {
            // --- Attach character to existing account ---
            let account_id = state_claims.account_id.ok_or_else(|| {
                warn!("add mode state JWT missing account_id");
                err("invalid state")
            })?;

            attach_character_to_account(
                &state.db,
                &state.config.aes_key,
                AttachCharacterInput {
                    account_id,
                    eve_character_id,
                    name: &eve_claims.name,
                    corporation_id: public_info.corporation_id,
                    alliance_id: public_info.alliance_id,
                    esi_client_id: client_id,
                    access_token: &token_resp.access_token,
                    refresh_token: &token_resp.refresh_token,
                    esi_token_expires_at: expires_at,
                },
            )
            .await
            .map_err(|e| {
                warn!(error = %e, "attach_character_to_account failed");
                (StatusCode::BAD_REQUEST, "could not attach character").into_response()
            })?;

            register_account_with_online_poller(&state, account_id).await;

            let redirect_url = format!("{}/characters", state.config.frontend_url);
            info!(account_id = %account_id, eve_character_id, "character added to account");
            let cookie = make_session_cookie(account_id, &state.config.jwt_key).map_err(|e| {
                warn!(error = %e, "failed to encode session JWT");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
            })?;
            Ok((jar.add(cookie), Redirect::to(&redirect_url)))
        }

        AuthMode::Login => {
            // --- login mode: create or update account ---
            let account_id = login_or_register(
                &state.db,
                &state.config.aes_key,
                LoginInput {
                    eve_character_id,
                    name: &eve_claims.name,
                    corporation_id: public_info.corporation_id,
                    alliance_id: public_info.alliance_id,
                    esi_client_id: client_id,
                    access_token: &token_resp.access_token,
                    refresh_token: &token_resp.refresh_token,
                    esi_token_expires_at: expires_at,
                },
            )
            .await
            .map_err(|e| {
                warn!(error = %e, "login_or_register failed");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
            })?;

            register_account_with_online_poller(&state, account_id).await;

            let redirect_url = format!("{}/", state.config.frontend_url);
            info!(account_id = %account_id, "session cookie issued");
            let cookie = make_session_cookie(account_id, &state.config.jwt_key).map_err(|e| {
                warn!(error = %e, "failed to encode session JWT");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
            })?;
            Ok((jar.add(cookie), Redirect::to(&redirect_url)))
        }
    }
}

/// Fetches all pollable character IDs for the account and sends them to the
/// online poller. Failures are non-fatal — the poller will pick them up on its
/// next DB scan anyway.
async fn register_account_with_online_poller(state: &AppState, account_id: Uuid) {
    match find_pollable_character_ids_for_account(&state.db, account_id).await {
        Ok(ids) if !ids.is_empty() => {
            if let Err(e) = state.online_poll_tx.send(ids).await {
                warn!(error = %e, %account_id, "failed to register characters with online poller");
            }
        }
        Ok(_) => {}
        Err(e) => {
            warn!(error = %e, %account_id, "failed to fetch character ids for online poller registration");
        }
    }
}

fn make_session_cookie(
    account_id: Uuid,
    jwt_key: &[u8; 32],
) -> Result<Cookie<'static>, jsonwebtoken::errors::Error> {
    let claims = SessionClaims {
        account_id,
        exp: now_secs() + SESSION_JWT_TTL_SECS,
    };
    let jwt = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(jwt_key),
    )?;
    Ok(Cookie::build((SESSION_COOKIE, jwt))
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Lax)
        .path("/")
        .build())
}

// ---------------------------------------------------------------------------
// POST /auth/logout
// ---------------------------------------------------------------------------

pub async fn logout(jar: CookieJar) -> (CookieJar, StatusCode) {
    let jar = jar.remove(Cookie::build(SESSION_COOKIE).path("/").build());
    (jar, StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// GET /api/v1/auth/me
// ---------------------------------------------------------------------------

pub async fn me(
    State(state): State<Arc<AppState>>,
    AccountId(account_id): AccountId,
) -> Result<Json<ApiResponse<MeResponse>>, StatusCode> {
    let characters = find_characters_by_account(&state.db, &state.config.aes_key, account_id)
        .await
        .map_err(|e| {
            warn!(error = %e, %account_id, "failed to fetch characters");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let me_characters: Vec<MeCharacter> = characters
        .iter()
        .map(|c| MeCharacter {
            id: c.id,
            eve_character_id: c.eve_character_id,
            name: c.name.clone(),
            corporation_id: c.corporation_id,
            alliance_id: c.alliance_id,
            is_main: c.is_main,
        })
        .collect();

    let main = me_characters
        .iter()
        .find(|c| c.is_main)
        .cloned()
        .ok_or_else(|| {
            warn!(%account_id, "account has no main character — data integrity issue");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(ApiResponse::ok(MeResponse {
        account_id,
        character: main,
        characters: me_characters,
    })))
}
