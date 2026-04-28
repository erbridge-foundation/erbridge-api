use anyhow::{Context, Result};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    audit::{self, AuditEvent},
    crypto::{generate_api_key, sha256_hex},
    db::api_key::{ApiKey, delete_account_api_key, insert_account_api_key},
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
