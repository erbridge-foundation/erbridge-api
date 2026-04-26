use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tracing::info;
use uuid::Uuid;

use crate::audit::{self, AuditEvent};
use crate::db::{account, character};

pub struct AttachCharacterInput<'a> {
    pub account_id: Uuid,
    pub eve_character_id: i64,
    pub name: &'a str,
    pub corporation_id: i64,
    pub alliance_id: Option<i64>,
    pub esi_client_id: &'a str,
    pub access_token: &'a str,
    pub refresh_token: &'a str,
    pub esi_token_expires_at: DateTime<Utc>,
}

/// Attaches a new EVE character to an existing account, or updates its tokens
/// if the character is already linked to that account. Returns an error if the
/// character is already linked to a *different* account.
///
/// If the character exists as a ghost row (account_id = NULL), it is claimed
/// by this account (ADR-031).
pub async fn attach_character_to_account(
    pool: &PgPool,
    aes_key: &[u8; 32],
    input: AttachCharacterInput<'_>,
) -> Result<()> {
    let existing =
        character::find_character_by_eve_id(pool, aes_key, input.eve_character_id).await?;

    if let Some(ch) = existing {
        match ch.account_id {
            None => {
                // Ghost row — claim it for this account.
                let mut tx = pool.begin().await?;
                character::claim_ghost_character(
                    &mut tx,
                    aes_key,
                    input.eve_character_id,
                    input.account_id,
                    false,
                    input.name,
                    input.corporation_id,
                    input.alliance_id,
                    input.esi_client_id,
                    input.access_token,
                    input.refresh_token,
                    input.esi_token_expires_at,
                )
                .await?;
                audit::record_in_tx(
                    &mut tx,
                    Some(input.account_id),
                    AuditEvent::GhostCharacterClaimed {
                        account_id: input.account_id,
                        eve_character_id: input.eve_character_id,
                    },
                )
                .await?;
                tx.commit().await?;
                info!(
                    eve_character_id = input.eve_character_id,
                    account_id = %input.account_id,
                    "claimed ghost character for account"
                );
            }
            Some(existing_account_id) => {
                anyhow::ensure!(
                    existing_account_id == input.account_id,
                    "character is already linked to a different account"
                );
                // Already on this account — refresh tokens; no audit event needed.
                character::update_character_tokens(
                    pool,
                    aes_key,
                    input.eve_character_id,
                    input.corporation_id,
                    input.alliance_id,
                    input.esi_client_id,
                    input.access_token,
                    input.refresh_token,
                    input.esi_token_expires_at,
                )
                .await?;
                info!(
                    eve_character_id = input.eve_character_id,
                    account_id = %input.account_id,
                    "re-linked existing character to same account"
                );
            }
        }
        return Ok(());
    }

    // New character — insert (is_main = false; account already has a main).
    let mut tx = pool.begin().await?;
    let ch = character::insert_character(
        &mut tx,
        aes_key,
        character::InsertCharacterData {
            account_id: input.account_id,
            eve_character_id: input.eve_character_id,
            name: input.name,
            corporation_id: input.corporation_id,
            alliance_id: input.alliance_id,
            is_main: false,
            esi_client_id: input.esi_client_id,
            access_token: input.access_token,
            refresh_token: input.refresh_token,
            esi_token_expires_at: input.esi_token_expires_at,
        },
    )
    .await?;
    audit::record_in_tx(
        &mut tx,
        Some(input.account_id),
        AuditEvent::CharacterAdded {
            account_id: input.account_id,
            eve_character_id: input.eve_character_id,
            character_name: input.name.to_string(),
        },
    )
    .await?;
    tx.commit().await?;

    info!(
        eve_character_id = input.eve_character_id,
        account_id = %input.account_id,
        character_id = %ch.id,
        "character attached to existing account"
    );
    Ok(())
}

pub struct LoginInput<'a> {
    pub eve_character_id: i64,
    pub name: &'a str,
    pub corporation_id: i64,
    pub alliance_id: Option<i64>,
    pub esi_client_id: &'a str,
    pub access_token: &'a str,
    pub refresh_token: &'a str,
    pub esi_token_expires_at: DateTime<Utc>,
}

/// Either creates a new account + character (first login), claims a ghost
/// character row (ADR-031), or updates ESI tokens on an existing character
/// (subsequent login). Returns the `account_id` in all cases.
pub async fn login_or_register(
    pool: &PgPool,
    aes_key: &[u8; 32],
    input: LoginInput<'_>,
) -> Result<Uuid> {
    let existing =
        character::find_character_by_eve_id(pool, aes_key, input.eve_character_id).await?;

    if let Some(ch) = existing {
        match ch.account_id {
            None => {
                // Ghost row — create a new account and claim the row.
                let mut tx = pool.begin().await?;
                let acc = account::insert_account(&mut tx).await?;
                character::claim_ghost_character(
                    &mut tx,
                    aes_key,
                    input.eve_character_id,
                    acc.id,
                    true,
                    input.name,
                    input.corporation_id,
                    input.alliance_id,
                    input.esi_client_id,
                    input.access_token,
                    input.refresh_token,
                    input.esi_token_expires_at,
                )
                .await?;
                audit::record_in_tx(
                    &mut tx,
                    None,
                    AuditEvent::GhostCharacterClaimed {
                        account_id: acc.id,
                        eve_character_id: input.eve_character_id,
                    },
                )
                .await?;
                tx.commit().await?;
                info!(
                    eve_character_id = input.eve_character_id,
                    account_id = %acc.id,
                    "ghost character claimed on first login"
                );
                return Ok(acc.id);
            }
            Some(account_id) => {
                // Reactivate the account if it was pending deletion (self-actor).
                let reactivated =
                    account::reactivate_account(pool, account_id, Some(account_id)).await?;
                if reactivated {
                    info!(
                        eve_character_id = input.eve_character_id,
                        account_id = %account_id,
                        "reactivated account pending deletion on login"
                    );
                }

                // Subsequent login — update ESI tokens and corp/alliance info.
                character::update_character_tokens(
                    pool,
                    aes_key,
                    input.eve_character_id,
                    input.corporation_id,
                    input.alliance_id,
                    input.esi_client_id,
                    input.access_token,
                    input.refresh_token,
                    input.esi_token_expires_at,
                )
                .await?;
                info!(
                    eve_character_id = input.eve_character_id,
                    account_id = %account_id,
                    "existing character logged in"
                );
                return Ok(account_id);
            }
        }
    }

    // First login — create account and character in a single transaction.
    let mut tx = pool.begin().await?;

    let acc = account::insert_account(&mut tx).await?;
    let ch = character::insert_character(
        &mut tx,
        aes_key,
        character::InsertCharacterData {
            account_id: acc.id,
            eve_character_id: input.eve_character_id,
            name: input.name,
            corporation_id: input.corporation_id,
            alliance_id: input.alliance_id,
            is_main: true,
            esi_client_id: input.esi_client_id,
            access_token: input.access_token,
            refresh_token: input.refresh_token,
            esi_token_expires_at: input.esi_token_expires_at,
        },
    )
    .await?;

    tx.commit().await?;

    info!(
        eve_character_id = input.eve_character_id,
        account_id = %acc.id,
        character_id = %ch.id,
        "new account and character created"
    );

    Ok(acc.id)
}
