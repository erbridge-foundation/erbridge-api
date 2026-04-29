use anyhow::{Context, Result};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    audit::{self, AuditEvent},
    crypto::{generate_api_key, sha256_hex},
    db::{
        account::request_account_deletion,
        api_key::{ApiKey, delete_account_api_key, insert_account_api_key, list_account_api_keys},
    },
};

/// Returned by `create_api_key`: the persisted row plus the one-time plaintext token.
pub struct CreatedApiKey {
    pub key: ApiKey,
    pub plaintext: String,
}

/// Generates a new API key, hashes it, persists the row, and audits the creation —
/// all in a single transaction. The plaintext is returned to the caller and never
/// stored or logged.
pub async fn create_api_key(pool: &PgPool, account_id: Uuid, name: &str) -> Result<CreatedApiKey> {
    let plaintext = generate_api_key();
    let key_hash = sha256_hex(plaintext.as_bytes());

    let mut tx = pool.begin().await.context("begin tx")?;

    let key = insert_account_api_key(&mut tx, account_id, name, &key_hash).await?;

    audit::record_in_tx(
        &mut tx,
        Some(account_id),
        AuditEvent::ApiKeyCreated {
            account_id,
            key_id: key.id,
            name: key.name.clone(),
        },
    )
    .await?;

    tx.commit().await.context("commit tx")?;

    Ok(CreatedApiKey { key, plaintext })
}

/// Lists all API keys for the given account.
pub async fn list_api_keys(pool: &PgPool, account_id: Uuid) -> Result<Vec<ApiKey>> {
    list_account_api_keys(pool, account_id).await
}

/// Marks the account as `pending_delete` and writes an audit entry in the same
/// transaction as the status flip. Returns `Ok(false)` if the account was not
/// found or already pending deletion.
pub async fn request_deletion(pool: &PgPool, account_id: Uuid) -> Result<bool> {
    request_account_deletion(pool, account_id, Some(account_id)).await
}

/// Revokes an account-scoped API key by id, scoped to the owning account.
/// Returns `Ok(true)` if a row was deleted, `Ok(false)` if not found / wrong account.
pub async fn revoke_api_key(pool: &PgPool, account_id: Uuid, key_id: Uuid) -> Result<bool> {
    let mut tx = pool.begin().await.context("begin tx")?;

    let deleted = delete_account_api_key(&mut tx, account_id, key_id).await?;
    if !deleted {
        return Ok(false);
    }

    audit::record_in_tx(
        &mut tx,
        Some(account_id),
        AuditEvent::ApiKeyRevoked { account_id, key_id },
    )
    .await?;

    tx.commit().await.context("commit tx")?;
    Ok(true)
}
