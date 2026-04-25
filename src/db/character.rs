use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::crypto;

pub struct Character {
    pub id: Uuid,
    /// None for ghost characters (added to an ACL before ever logging in).
    pub account_id: Option<Uuid>,
    pub eve_character_id: i64,
    pub name: String,
    pub corporation_id: i64,
    pub alliance_id: Option<i64>,
    pub is_main: bool,
    pub is_online: Option<bool>,
    /// The ESI client_id used to obtain this character's token grant.
    /// None on ghost characters or rows created before this field was added.
    pub esi_client_id: Option<String>,
    /// None on ghost characters.
    pub access_token: Option<String>,
    /// None on ghost characters.
    pub refresh_token: Option<String>,
    /// None on ghost characters.
    pub esi_token_expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Raw DB row — holds encrypted blobs. Only used internally.
#[derive(sqlx::FromRow)]
struct CharacterRow {
    id: Uuid,
    account_id: Option<Uuid>,
    eve_character_id: i64,
    name: String,
    corporation_id: i64,
    alliance_id: Option<i64>,
    is_main: bool,
    pub is_online: Option<bool>,
    esi_client_id: Option<String>,
    encrypted_access_token: Option<Vec<u8>>,
    encrypted_refresh_token: Option<Vec<u8>>,
    esi_token_expires_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

fn decrypt_row(row: CharacterRow, aes_key: &[u8; 32]) -> Result<Character> {
    let access_token = row
        .encrypted_access_token
        .map(|b| {
            let bytes = crypto::decrypt(aes_key, &b).context("failed to decrypt access token")?;
            String::from_utf8(bytes).context("access token is not valid UTF-8")
        })
        .transpose()?;

    let refresh_token = row
        .encrypted_refresh_token
        .map(|b| {
            let bytes = crypto::decrypt(aes_key, &b).context("failed to decrypt refresh token")?;
            String::from_utf8(bytes).context("refresh token is not valid UTF-8")
        })
        .transpose()?;

    Ok(Character {
        id: row.id,
        account_id: row.account_id,
        eve_character_id: row.eve_character_id,
        name: row.name,
        corporation_id: row.corporation_id,
        alliance_id: row.alliance_id,
        is_main: row.is_main,
        is_online: row.is_online,
        esi_client_id: row.esi_client_id,
        access_token,
        refresh_token,
        esi_token_expires_at: row.esi_token_expires_at,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

pub struct InsertCharacterData<'a> {
    pub account_id: Uuid,
    pub eve_character_id: i64,
    pub name: &'a str,
    pub corporation_id: i64,
    pub alliance_id: Option<i64>,
    pub is_main: bool,
    pub esi_client_id: &'a str,
    pub access_token: &'a str,
    pub refresh_token: &'a str,
    pub esi_token_expires_at: DateTime<Utc>,
}

/// Inserts a new character within an existing transaction.
pub async fn insert_character(
    tx: &mut Transaction<'_, Postgres>,
    aes_key: &[u8; 32],
    data: InsertCharacterData<'_>,
) -> Result<Character> {
    let enc_access = crypto::encrypt(aes_key, data.access_token.as_bytes())
        .context("failed to encrypt access token")?;
    let enc_refresh = crypto::encrypt(aes_key, data.refresh_token.as_bytes())
        .context("failed to encrypt refresh token")?;

    let row = sqlx::query_as!(
        CharacterRow,
        r#"
        INSERT INTO eve_character (
            account_id, eve_character_id, name,
            corporation_id, alliance_id, is_main,
            esi_client_id,
            encrypted_access_token, encrypted_refresh_token,
            esi_token_expires_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        RETURNING
            id, account_id, eve_character_id, name,
            corporation_id, alliance_id, is_main, is_online,
            esi_client_id,
            encrypted_access_token, encrypted_refresh_token,
            esi_token_expires_at, created_at, updated_at
        "#,
        data.account_id,
        data.eve_character_id,
        data.name,
        data.corporation_id,
        data.alliance_id,
        data.is_main,
        data.esi_client_id,
        enc_access,
        enc_refresh,
        data.esi_token_expires_at,
    )
    .fetch_one(&mut **tx)
    .await
    .context("failed to insert character")?;

    decrypt_row(row, aes_key)
}

/// Claims a ghost character row: sets account_id, is_main, and ESI tokens in
/// one update. Must run inside a transaction.
pub async fn claim_ghost_character(
    tx: &mut Transaction<'_, Postgres>,
    aes_key: &[u8; 32],
    eve_character_id: i64,
    account_id: Uuid,
    is_main: bool,
    name: &str,
    corporation_id: i64,
    alliance_id: Option<i64>,
    esi_client_id: &str,
    access_token: &str,
    refresh_token: &str,
    esi_token_expires_at: DateTime<Utc>,
) -> Result<Character> {
    let enc_access = crypto::encrypt(aes_key, access_token.as_bytes())
        .context("failed to encrypt access token")?;
    let enc_refresh = crypto::encrypt(aes_key, refresh_token.as_bytes())
        .context("failed to encrypt refresh token")?;

    let row = sqlx::query_as!(
        CharacterRow,
        r#"
        UPDATE eve_character
        SET
            account_id              = $2,
            is_main                 = $3,
            name                    = $4,
            corporation_id          = $5,
            alliance_id             = $6,
            esi_client_id           = $7,
            encrypted_access_token  = $8,
            encrypted_refresh_token = $9,
            esi_token_expires_at    = $10,
            updated_at              = now()
        WHERE eve_character_id = $1
        RETURNING
            id, account_id, eve_character_id, name,
            corporation_id, alliance_id, is_main, is_online,
            esi_client_id,
            encrypted_access_token, encrypted_refresh_token,
            esi_token_expires_at, created_at, updated_at
        "#,
        eve_character_id,
        account_id,
        is_main,
        name,
        corporation_id,
        alliance_id,
        esi_client_id,
        enc_access,
        enc_refresh,
        esi_token_expires_at,
    )
    .fetch_one(&mut **tx)
    .await
    .context("failed to claim ghost character")?;

    decrypt_row(row, aes_key)
}

/// Updates ESI tokens on an existing claimed character row.
pub async fn update_character_tokens(
    pool: &PgPool,
    aes_key: &[u8; 32],
    eve_character_id: i64,
    corporation_id: i64,
    alliance_id: Option<i64>,
    esi_client_id: &str,
    access_token: &str,
    refresh_token: &str,
    esi_token_expires_at: DateTime<Utc>,
) -> Result<Character> {
    let enc_access = crypto::encrypt(aes_key, access_token.as_bytes())
        .context("failed to encrypt access token")?;
    let enc_refresh = crypto::encrypt(aes_key, refresh_token.as_bytes())
        .context("failed to encrypt refresh token")?;

    let row = sqlx::query_as!(
        CharacterRow,
        r#"
        UPDATE eve_character
        SET
            corporation_id          = $2,
            alliance_id             = $3,
            esi_client_id           = $4,
            encrypted_access_token  = $5,
            encrypted_refresh_token = $6,
            esi_token_expires_at    = $7,
            updated_at              = now()
        WHERE eve_character_id = $1
        RETURNING
            id, account_id, eve_character_id, name,
            corporation_id, alliance_id, is_main, is_online,
            esi_client_id,
            encrypted_access_token, encrypted_refresh_token,
            esi_token_expires_at, created_at, updated_at
        "#,
        eve_character_id,
        corporation_id,
        alliance_id,
        esi_client_id,
        enc_access,
        enc_refresh,
        esi_token_expires_at,
    )
    .fetch_one(pool)
    .await
    .context("failed to update character tokens")?;

    decrypt_row(row, aes_key)
}

pub async fn find_character_by_eve_id(
    pool: &PgPool,
    aes_key: &[u8; 32],
    eve_character_id: i64,
) -> Result<Option<Character>> {
    let row = sqlx::query_as!(
        CharacterRow,
        r#"
        SELECT
            id, account_id, eve_character_id, name,
            corporation_id, alliance_id, is_main, is_online,
            esi_client_id,
            encrypted_access_token, encrypted_refresh_token,
            esi_token_expires_at, created_at, updated_at
        FROM eve_character
        WHERE eve_character_id = $1
        "#,
        eve_character_id
    )
    .fetch_optional(pool)
    .await
    .context("failed to fetch character by eve_character_id")?;

    row.map(|r| decrypt_row(r, aes_key)).transpose()
}

pub async fn find_characters_by_account(
    pool: &PgPool,
    aes_key: &[u8; 32],
    account_id: Uuid,
) -> Result<Vec<Character>> {
    let rows = sqlx::query_as!(
        CharacterRow,
        r#"
        SELECT
            id, account_id, eve_character_id, name,
            corporation_id, alliance_id, is_main, is_online,
            esi_client_id,
            encrypted_access_token, encrypted_refresh_token,
            esi_token_expires_at, created_at, updated_at
        FROM eve_character
        WHERE account_id = $1
        ORDER BY is_main DESC, created_at ASC
        "#,
        account_id
    )
    .fetch_all(pool)
    .await
    .context("failed to fetch characters by account")?;

    rows.into_iter().map(|r| decrypt_row(r, aes_key)).collect()
}

/// Lightweight row used by the corp/alliance refresh background task.
pub struct CharacterForRefresh {
    pub id: Uuid,
    pub eve_character_id: i64,
}

/// Returns all characters (including ghosts) that need their corp/alliance refreshed.
pub async fn find_all_characters_for_refresh(pool: &PgPool) -> Result<Vec<CharacterForRefresh>> {
    sqlx::query_as!(
        CharacterForRefresh,
        "SELECT id, eve_character_id FROM eve_character ORDER BY updated_at ASC",
    )
    .fetch_all(pool)
    .await
    .context("failed to fetch characters for refresh")
}

/// Bulk-updates corporation_id and alliance_id for multiple characters in a
/// single query using UNNEST. Characters not present in the input are untouched.
pub async fn bulk_update_corp_alliance(
    pool: &PgPool,
    updates: &[(Uuid, i64, Option<i64>)],
) -> Result<()> {
    if updates.is_empty() {
        return Ok(());
    }

    let ids: Vec<Uuid> = updates.iter().map(|(id, _, _)| *id).collect();
    let corp_ids: Vec<i64> = updates.iter().map(|(_, corp, _)| *corp).collect();
    let alliance_ids: Vec<Option<i64>> = updates.iter().map(|(_, _, ally)| *ally).collect();

    sqlx::query!(
        r#"
        UPDATE eve_character AS ec
        SET
            corporation_id = u.corporation_id,
            alliance_id    = u.alliance_id,
            updated_at     = now()
        FROM UNNEST($1::uuid[], $2::bigint[], $3::bigint[])
            AS u(id, corporation_id, alliance_id)
        WHERE ec.id = u.id
        "#,
        &ids,
        &corp_ids,
        &alliance_ids as &[Option<i64>],
    )
    .execute(pool)
    .await
    .context("failed to bulk update corp/alliance")?;

    Ok(())
}

pub enum DeleteCharacterResult {
    Deleted,
    NotFound,
    IsMain,
}

pub async fn delete_character(
    pool: &PgPool,
    account_id: Uuid,
    character_id: Uuid,
) -> Result<DeleteCharacterResult> {
    let row = sqlx::query!(
        "SELECT is_main FROM eve_character WHERE id = $1 AND account_id = $2",
        character_id,
        account_id,
    )
    .fetch_optional(pool)
    .await
    .context("failed to fetch character for deletion")?;

    match row {
        None => return Ok(DeleteCharacterResult::NotFound),
        Some(r) if r.is_main => return Ok(DeleteCharacterResult::IsMain),
        _ => {}
    }

    sqlx::query!(
        "DELETE FROM eve_character WHERE id = $1 AND account_id = $2",
        character_id,
        account_id,
    )
    .execute(pool)
    .await
    .context("failed to delete character")?;

    Ok(DeleteCharacterResult::Deleted)
}

/// Atomically promotes `new_main_id` to main within a single transaction,
/// demoting any existing main. Returns an error if `new_main_id` does not
/// belong to `account_id`.
pub async fn set_main_character(pool: &PgPool, account_id: Uuid, new_main_id: Uuid) -> Result<()> {
    let mut tx = pool.begin().await?;

    let exists: bool = sqlx::query_scalar!(
        "SELECT EXISTS(SELECT 1 FROM eve_character WHERE id = $1 AND account_id = $2)",
        new_main_id,
        account_id,
    )
    .fetch_one(&mut *tx)
    .await
    .context("failed to check character ownership")?
    .unwrap_or(false);

    if !exists {
        anyhow::bail!("character not found for this account");
    }

    sqlx::query!(
        "UPDATE eve_character SET is_main = false, updated_at = now() WHERE account_id = $1 AND is_main = true",
        account_id,
    )
    .execute(&mut *tx)
    .await
    .context("failed to demote current main")?;

    sqlx::query!(
        "UPDATE eve_character SET is_main = true, updated_at = now() WHERE id = $1",
        new_main_id,
    )
    .execute(&mut *tx)
    .await
    .context("failed to promote new main")?;

    tx.commit()
        .await
        .context("failed to commit set_main transaction")
}
