use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use strum::{Display, EnumString};
use uuid::Uuid;

use crate::audit::{self, AuditEvent};

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
    is_server_admin: bool,
}

pub struct Account {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub status: AccountStatus,
    pub delete_requested_at: Option<DateTime<Utc>>,
    pub is_server_admin: bool,
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
            is_server_admin: row.is_server_admin,
        })
    }
}

/// Inserts a new account, atomically promoting it to server admin if it is the
/// first account on the instance. See `DECISIONS_context.md` ("Server admin role:
/// scope and bootstrap"). Two registrations racing on a brand-new instance
/// could both come back as admin — that's acceptable; revoke via the admin API.
pub async fn insert_account(tx: &mut Transaction<'_, Postgres>) -> Result<Account> {
    sqlx::query_as!(
        AccountRow,
        r#"
        INSERT INTO account (is_server_admin)
        SELECT NOT EXISTS (SELECT 1 FROM account)
        RETURNING id, created_at, updated_at, status, delete_requested_at, is_server_admin
        "#
    )
    .fetch_one(&mut **tx)
    .await
    .context("failed to insert account")?
    .try_into()
}

/// Reactivates an account that is in `pending_delete` status.
/// Returns `false` if the account was not found or was not pending deletion.
/// `actor` should be the account's own id (self-reactivation via login).
pub async fn reactivate_account(pool: &PgPool, id: Uuid, actor: Option<Uuid>) -> Result<bool> {
    let mut tx = pool.begin().await?;

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
    .execute(&mut *tx)
    .await
    .context("failed to reactivate account")?;

    if result.rows_affected() > 0 {
        audit::record_in_tx(
            &mut tx,
            actor,
            AuditEvent::AccountReactivated { account_id: id },
        )
        .await?;
        tx.commit()
            .await
            .context("failed to commit reactivate_account")?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Hard-deletes all accounts in `pending_delete` status whose grace period has expired.
/// Returns the number of accounts deleted.
pub async fn purge_expired_accounts(pool: &PgPool, grace_days: u32) -> Result<u64> {
    let mut tx = pool.begin().await?;

    let deleted_ids = sqlx::query_scalar!(
        r#"
        DELETE FROM account
        WHERE status = 'pending_delete'
          AND delete_requested_at < now() - ($1 * interval '1 day')
        RETURNING id
        "#,
        grace_days as i32,
    )
    .fetch_all(&mut *tx)
    .await
    .context("failed to purge expired pending-delete accounts")?;

    let count = deleted_ids.len() as u64;
    for account_id in &deleted_ids {
        audit::record_in_tx(
            &mut tx,
            None,
            AuditEvent::AccountPurged {
                account_id: *account_id,
            },
        )
        .await?;
    }

    tx.commit()
        .await
        .context("failed to commit purge_expired_accounts")?;
    Ok(count)
}

/// Returns whether the given account has the server-admin flag set.
/// Returns `false` if the account does not exist.
pub async fn is_server_admin(pool: &PgPool, id: Uuid) -> Result<bool> {
    let row = sqlx::query_scalar!("SELECT is_server_admin FROM account WHERE id = $1", id)
        .fetch_optional(pool)
        .await
        .context("failed to fetch is_server_admin")?;
    Ok(row.unwrap_or(false))
}

/// Sets the `is_server_admin` flag on the given account. Returns `false` if
/// the account does not exist. Emits no audit event — callers are expected to
/// record `ServerAdminGranted`/`ServerAdminRevoked` themselves so they can
/// include the appropriate `source` and run inside the same transaction as
/// any guard checks (see step 12 last-admin guard).
pub async fn set_server_admin(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    is_server_admin: bool,
) -> Result<bool> {
    let result = sqlx::query!(
        r#"
        UPDATE account
        SET is_server_admin = $2,
            updated_at = now()
        WHERE id = $1
        "#,
        id,
        is_server_admin,
    )
    .execute(&mut **tx)
    .await
    .context("failed to set is_server_admin")?;
    Ok(result.rows_affected() > 0)
}

/// Returns true if an account row exists for the given id.
pub async fn account_exists(pool: &PgPool, id: Uuid) -> Result<bool> {
    let exists = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM account WHERE id = $1) AS "exists!""#,
        id,
    )
    .fetch_one(pool)
    .await
    .context("failed to check account existence")?;
    Ok(exists)
}

/// Counts the number of accounts with the server-admin flag set. Used by the
/// last-admin guard in `revoke-admin`.
pub async fn count_server_admins(tx: &mut Transaction<'_, Postgres>) -> Result<i64> {
    let count = sqlx::query_scalar!(
        r#"SELECT count(*) AS "count!" FROM account WHERE is_server_admin = TRUE"#
    )
    .fetch_one(&mut **tx)
    .await
    .context("failed to count server admins")?;
    Ok(count)
}

pub struct BlockedEveCharacter {
    pub eve_character_id: i64,
    pub reason: Option<String>,
    pub blocked_at: DateTime<Utc>,
}

/// Inserts a row into `blocked_eve_character`. Returns `false` if the EVE
/// character is already blocked (ON CONFLICT DO NOTHING). Caller is
/// responsible for emitting `EveCharacterBlocked` in the same transaction.
pub async fn insert_blocked_eve_character(
    tx: &mut Transaction<'_, Postgres>,
    eve_character_id: i64,
    reason: Option<&str>,
) -> Result<bool> {
    let result = sqlx::query!(
        r#"
        INSERT INTO blocked_eve_character (eve_character_id, reason)
        VALUES ($1, $2)
        ON CONFLICT (eve_character_id) DO NOTHING
        "#,
        eve_character_id,
        reason,
    )
    .execute(&mut **tx)
    .await
    .context("failed to insert blocked_eve_character")?;
    Ok(result.rows_affected() > 0)
}

/// Removes a row from `blocked_eve_character`. Returns `false` if no row was
/// present. Caller is responsible for emitting `EveCharacterUnblocked` in
/// the same transaction.
pub async fn delete_blocked_eve_character(
    tx: &mut Transaction<'_, Postgres>,
    eve_character_id: i64,
) -> Result<bool> {
    let result = sqlx::query!(
        "DELETE FROM blocked_eve_character WHERE eve_character_id = $1",
        eve_character_id,
    )
    .execute(&mut **tx)
    .await
    .context("failed to delete blocked_eve_character")?;
    Ok(result.rows_affected() > 0)
}

/// Returns whether the given EVE character id is blocked. Used by the login
/// and add-character gates (step 10) and the usable-character middleware
/// extension (step 11).
pub async fn is_eve_character_blocked(pool: &PgPool, eve_character_id: i64) -> Result<bool> {
    let row = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM blocked_eve_character WHERE eve_character_id = $1) AS "exists!""#,
        eve_character_id,
    )
    .fetch_one(pool)
    .await
    .context("failed to check blocked_eve_character")?;
    Ok(row)
}

/// Lists blocked EVE characters, newest first.
pub async fn list_blocked_eve_characters(pool: &PgPool) -> Result<Vec<BlockedEveCharacter>> {
    let rows = sqlx::query_as!(
        BlockedEveCharacter,
        r#"
        SELECT eve_character_id, reason, blocked_at
        FROM blocked_eve_character
        ORDER BY blocked_at DESC
        "#
    )
    .fetch_all(pool)
    .await
    .context("failed to list blocked_eve_character")?;
    Ok(rows)
}

/// Lists every account on the instance for admin views, newest first.
pub async fn list_accounts_admin(pool: &PgPool) -> Result<Vec<Account>> {
    let rows = sqlx::query_as!(
        AccountRow,
        r#"
        SELECT id, created_at, updated_at, status, delete_requested_at, is_server_admin
        FROM account
        ORDER BY created_at DESC
        "#
    )
    .fetch_all(pool)
    .await
    .context("failed to list accounts for admin")?;
    rows.into_iter().map(Account::try_from).collect()
}

/// Returns whether any EVE character attached to the account is in
/// `blocked_eve_character`. One blocked character bans the whole account —
/// `require_active_account` rejects the session even if other characters on
/// the account remain unblocked. This prevents a banned actor from continuing
/// to operate via alts on the same account.
pub async fn account_has_blocked_character(pool: &PgPool, account_id: Uuid) -> Result<bool> {
    let exists = sqlx::query_scalar!(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM eve_character ec
            JOIN blocked_eve_character b ON b.eve_character_id = ec.eve_character_id
            WHERE ec.account_id = $1
        ) AS "exists!"
        "#,
        account_id,
    )
    .fetch_one(pool)
    .await
    .context("failed to check account_has_blocked_character")?;
    Ok(exists)
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
pub async fn request_account_deletion(
    pool: &PgPool,
    id: Uuid,
    actor: Option<Uuid>,
) -> Result<bool> {
    let mut tx = pool.begin().await?;

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
    .execute(&mut *tx)
    .await
    .context("failed to request account deletion")?;

    if result.rows_affected() > 0 {
        audit::record_in_tx(
            &mut tx,
            actor,
            AuditEvent::AccountDeletionRequested { account_id: id },
        )
        .await?;
        tx.commit()
            .await
            .context("failed to commit request_account_deletion")?;
        Ok(true)
    } else {
        Ok(false)
    }
}
