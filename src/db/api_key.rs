use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use strum::{Display, EnumString};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumString)]
#[strum(serialize_all = "snake_case")]
pub enum ApiKeyScope {
    Account,
}

// Future scopes (Map, Acl, Server) go here; extend the DB CHECK constraint too.

pub struct ApiKey {
    pub id: Uuid,
    pub scope: ApiKeyScope,
    pub account_id: Option<Uuid>,
    pub name: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

struct ApiKeyRow {
    id: Uuid,
    scope: String,
    account_id: Option<Uuid>,
    name: String,
    expires_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

impl TryFrom<ApiKeyRow> for ApiKey {
    type Error = anyhow::Error;

    fn try_from(row: ApiKeyRow) -> Result<Self> {
        Ok(Self {
            id: row.id,
            scope: row
                .scope
                .parse()
                .map_err(|_| anyhow!("invalid api key scope in db: {}", row.scope))?,
            account_id: row.account_id,
            name: row.name,
            expires_at: row.expires_at,
            created_at: row.created_at,
        })
    }
}

/// Inserts a new account-scoped API key.
pub async fn insert_account_api_key(
    tx: &mut Transaction<'_, Postgres>,
    account_id: Uuid,
    name: &str,
    key_hash: &str,
    expires_at: Option<DateTime<Utc>>,
) -> Result<ApiKey> {
    sqlx::query_as!(
        ApiKeyRow,
        r#"
        INSERT INTO api_key (scope, account_id, name, key_hash, expires_at)
        VALUES ('account', $1, $2, $3, $4)
        RETURNING id, scope, account_id, name, expires_at, created_at
        "#,
        account_id,
        name,
        key_hash,
        expires_at,
    )
    .fetch_one(&mut **tx)
    .await
    .context("failed to insert account api key")
    .and_then(ApiKey::try_from)
}

/// Returns all account-scoped API keys for an account, ordered by creation time.
pub async fn list_account_api_keys(pool: &PgPool, account_id: Uuid) -> Result<Vec<ApiKey>> {
    sqlx::query_as!(
        ApiKeyRow,
        r#"
        SELECT id, scope, account_id, name, expires_at, created_at
        FROM api_key
        WHERE scope = 'account'
          AND account_id = $1
        ORDER BY created_at ASC
        "#,
        account_id,
    )
    .fetch_all(pool)
    .await
    .context("failed to list account api keys")
    .and_then(|rows| rows.into_iter().map(ApiKey::try_from).collect())
}

/// Deletes a specific account-scoped API key by id, scoped to the owning account.
/// Returns true if a row was deleted, false if not found (or belongs to a different account).
pub async fn delete_account_api_key(
    tx: &mut Transaction<'_, Postgres>,
    account_id: Uuid,
    key_id: Uuid,
) -> Result<bool> {
    let result = sqlx::query!(
        r#"
        DELETE FROM api_key
        WHERE id = $1
          AND scope = 'account'
          AND account_id = $2
        "#,
        key_id,
        account_id,
    )
    .execute(&mut **tx)
    .await
    .context("failed to delete account api key")?;
    Ok(result.rows_affected() > 0)
}

/// Looks up the account_id for a given key hash on an account-scoped key,
/// but only if the account is active and the key has not expired.
/// Returns None if not found, inactive, or expired.
pub async fn find_account_id_by_key_hash(pool: &PgPool, key_hash: &str) -> Result<Option<Uuid>> {
    sqlx::query_scalar!(
        r#"
        SELECT ak.account_id
        FROM api_key ak
        JOIN account a ON a.id = ak.account_id
        WHERE ak.key_hash = $1
          AND ak.scope = 'account'
          AND a.status = 'active'
          AND (ak.expires_at IS NULL OR ak.expires_at > now())
        "#,
        key_hash,
    )
    .fetch_optional(pool)
    .await
    .context("failed to find account by api key hash")
    .map(|opt| opt.flatten())
}
