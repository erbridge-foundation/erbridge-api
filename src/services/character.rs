use anyhow::Context;
use reqwest::Client;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    audit::{self, AuditEvent},
    db::character::{
        DeleteCharacterResult, delete_character_in_tx, find_characters_by_account,
        find_pollable_character_ids_for_account, set_main_character_in_tx,
    },
    dto::character::CharacterResponse,
    esi::universe::resolve_names,
};

pub async fn remove_character(
    pool: &PgPool,
    account_id: Uuid,
    character_id: Uuid,
) -> anyhow::Result<DeleteCharacterResult> {
    let mut tx = pool.begin().await.context("begin tx")?;

    let (result, eve_character_id) =
        delete_character_in_tx(&mut tx, account_id, character_id).await?;

    if let (DeleteCharacterResult::Deleted, Some(eve_character_id)) = (&result, eve_character_id) {
        audit::record_in_tx(
            &mut tx,
            Some(account_id),
            AuditEvent::CharacterRemoved {
                account_id,
                eve_character_id,
            },
        )
        .await?;
    }

    tx.commit().await.context("commit tx")?;
    Ok(result)
}

/// Fetches all characters for an account and resolves corp/alliance names via ESI.
/// Returns `Vec<CharacterResponse>` ready for the API envelope.
pub async fn list_for_account(
    pool: &PgPool,
    aes_key: &[u8; 32],
    http: &Client,
    esi_base: &str,
    account_id: Uuid,
) -> anyhow::Result<Vec<CharacterResponse>> {
    let characters = find_characters_by_account(pool, aes_key, account_id).await?;

    let mut ids: Vec<i64> = characters.iter().map(|c| c.corporation_id).collect();
    for c in &characters {
        if let Some(aid) = c.alliance_id {
            ids.push(aid);
        }
    }
    ids.sort_unstable();
    ids.dedup();

    let resolved = resolve_names(http, esi_base, ids)
        .await
        .context("failed to resolve corp/alliance names from ESI")?;

    let name_map: std::collections::HashMap<i64, String> =
        resolved.into_iter().map(|r| (r.id, r.name)).collect();
    let name_for = |id: i64| -> Option<String> { name_map.get(&id).cloned() };

    Ok(characters
        .into_iter()
        .map(|c| CharacterResponse {
            id: c.id,
            eve_character_id: c.eve_character_id,
            name: c.name,
            corporation_id: c.corporation_id,
            corporation_name: name_for(c.corporation_id).unwrap_or_default(),
            alliance_id: c.alliance_id,
            alliance_name: c.alliance_id.and_then(name_for),
            is_main: c.is_main,
        })
        .collect())
}

/// Returns EVE character IDs for all pollable (non-ghost) characters on an account.
pub async fn list_pollable_ids(pool: &PgPool, account_id: Uuid) -> anyhow::Result<Vec<i64>> {
    find_pollable_character_ids_for_account(pool, account_id).await
}

pub async fn set_main(pool: &PgPool, account_id: Uuid, character_id: Uuid) -> anyhow::Result<()> {
    let mut tx = pool.begin().await.context("begin tx")?;

    let eve_character_id = set_main_character_in_tx(&mut tx, account_id, character_id).await?;

    audit::record_in_tx(
        &mut tx,
        Some(account_id),
        AuditEvent::CharacterSetMain {
            account_id,
            eve_character_id,
        },
    )
    .await?;

    tx.commit().await.context("commit tx")?;
    Ok(())
}
