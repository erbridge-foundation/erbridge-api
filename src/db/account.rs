use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::{Postgres, Transaction};
use strum::{Display, EnumString};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumString)]
#[strum(serialize_all = "snake_case")]
pub enum AccountStatus {
    Active,
    PendingDelete,
}

struct AccountRow {
    id: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    status: String,
    delete_requested_at: Option<DateTime<Utc>>,
}

pub struct Account {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub status: AccountStatus,
    pub delete_requested_at: Option<DateTime<Utc>>,
}

impl TryFrom<AccountRow> for Account {
    type Error = anyhow::Error;

    fn try_from(row: AccountRow) -> Result<Self> {
        Ok(Self {
            id: row.id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            status: row
                .status
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid account status in db: {}", row.status))?,
            delete_requested_at: row.delete_requested_at,
        })
    }
}

pub async fn insert_account(tx: &mut Transaction<'_, Postgres>) -> Result<Account> {
    sqlx::query_as!(
        AccountRow,
        r#"
        INSERT INTO account DEFAULT VALUES
        RETURNING id, created_at, updated_at, status, delete_requested_at
        "#
    )
    .fetch_one(&mut **tx)
    .await
    .context("failed to insert account")?
    .try_into()
}

/// Reactivates an account that is in `pending_delete` status.
/// Returns `false` if the account was not found or was not pending deletion.
pub async fn reactivate_account(pool: &sqlx::PgPool, id: Uuid) -> Result<bool> {
    let result = sqlx::query!(
        r#"
        UPDATE account
        SET status = 'active',
            delete_requested_at = NULL,
            updated_at = now()
        WHERE id = $1
          AND status = 'pending_delete'
        "#,
        id,
    )
    .execute(pool)
    .await
    .context("failed to reactivate account")?;

    Ok(result.rows_affected() > 0)
}

/// Hard-deletes all accounts in `pending_delete` status whose grace period has expired.
/// Returns the number of accounts deleted.
pub async fn purge_expired_accounts(pool: &sqlx::PgPool, grace_days: u32) -> Result<u64> {
    let result = sqlx::query!(
        r#"
        DELETE FROM account
        WHERE status = 'pending_delete'
          AND delete_requested_at < now() - ($1 * interval '1 day')
        "#,
        grace_days as i32,
    )
    .execute(pool)
    .await
    .context("failed to purge expired pending-delete accounts")?;

    Ok(result.rows_affected())
}

/// Returns the status for the given account, or `None` if not found.
pub async fn get_account_status(pool: &sqlx::PgPool, id: Uuid) -> Result<Option<AccountStatus>> {
    let row = sqlx::query_scalar!("SELECT status FROM account WHERE id = $1", id)
        .fetch_optional(pool)
        .await
        .context("failed to fetch account status")?;
    row.map(|s| {
        s.parse()
            .map_err(|_| anyhow::anyhow!("invalid account status in db: {s}"))
    })
    .transpose()
}

/// Marks the account as `pending_delete` and records the request timestamp.
/// Returns `false` if the account was not found or already pending deletion.
pub async fn request_account_deletion(pool: &sqlx::PgPool, id: Uuid) -> Result<bool> {
    let result = sqlx::query!(
        r#"
        UPDATE account
        SET status = 'pending_delete',
            delete_requested_at = now(),
            updated_at = now()
        WHERE id = $1
          AND status = 'active'
        "#,
        id,
    )
    .execute(pool)
    .await
    .context("failed to request account deletion")?;

    Ok(result.rows_affected() > 0)
}
