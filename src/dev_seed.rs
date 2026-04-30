#![cfg(feature = "dev-seed")]

use anyhow::{Context, bail};
use chrono::Utc;
use regex::Regex;
use sqlx::PgPool;

use crate::{crypto, db};

pub async fn run_if_requested(pool: &PgPool, aes_key: &[u8; 32]) -> anyhow::Result<()> {
    let allow = std::env::var("ERBRIDGE_ALLOW_DEV_SEED").unwrap_or_default();
    let admin_key = std::env::var("DEV_SEED_ADMIN_API_KEY").ok();
    let user_key = std::env::var("DEV_SEED_USER_API_KEY").ok();

    if allow != "yes-i-know-this-is-insecure" || admin_key.is_none() || user_key.is_none() {
        tracing::info!("dev-seed compiled but not activated");
        return Ok(());
    }

    let admin_key = admin_key.unwrap();
    let user_key = user_key.unwrap();

    let key_re = Regex::new(r"^erbridge_[0-9a-f]{32}$").unwrap();
    if !key_re.is_match(&admin_key) {
        bail!("DEV_SEED_ADMIN_API_KEY does not match expected format erbridge_<32 hex>");
    }
    if !key_re.is_match(&user_key) {
        bail!("DEV_SEED_USER_API_KEY does not match expected format erbridge_<32 hex>");
    }
    if admin_key == user_key {
        bail!("DEV_SEED_ADMIN_API_KEY and DEV_SEED_USER_API_KEY must be distinct");
    }

    tracing::warn!(
        "\n\
        ╔══════════════════════════════════════════════════════════╗\n\
        ║  DEV-SEED MODE ACTIVE — DO NOT USE IN PRODUCTION         ║\n\
        ║  Seeding two accounts (admin + non-admin) with API keys. ║\n\
        ║  Disable by unsetting ERBRIDGE_ALLOW_DEV_SEED.           ║\n\
        ╚══════════════════════════════════════════════════════════╝"
    );

    let admin_hash = crypto::sha256_hex(admin_key.as_bytes());
    let user_hash = crypto::sha256_hex(user_key.as_bytes());

    let mut tx = pool.begin().await.context("failed to begin transaction")?;

    let existing = sqlx::query_scalar!("SELECT id FROM api_key WHERE key_hash = $1", admin_hash)
        .fetch_optional(&mut *tx)
        .await
        .context("failed to check for existing seed")?;

    if existing.is_some() {
        tracing::info!("already seeded — skipping");
        return Ok(());
    }

    let expires_at = Utc::now() + chrono::Duration::days(365);

    let admin_account = db::account::insert_account(&mut tx).await?;
    db::account::set_server_admin(&mut tx, admin_account.id, true).await?;

    db::character::insert_character(
        &mut tx,
        aes_key,
        db::character::InsertCharacterData {
            account_id: admin_account.id,
            eve_character_id: 90000001,
            name: "Seed Admin",
            corporation_id: 1000001,
            alliance_id: None,
            is_main: true,
            esi_client_id: "dev-seed",
            access_token: "placeholder-access",
            refresh_token: "placeholder-refresh",
            esi_token_expires_at: expires_at,
        },
    )
    .await?;

    db::api_key::insert_account_api_key(
        &mut tx,
        admin_account.id,
        "ci-seed-admin",
        &admin_hash,
        None,
    )
    .await?;

    let user_account = db::account::insert_account(&mut tx).await?;
    db::account::set_server_admin(&mut tx, user_account.id, false).await?;

    db::character::insert_character(
        &mut tx,
        aes_key,
        db::character::InsertCharacterData {
            account_id: user_account.id,
            eve_character_id: 90000002,
            name: "Seed User",
            corporation_id: 1000001,
            alliance_id: None,
            is_main: true,
            esi_client_id: "dev-seed",
            access_token: "placeholder-access",
            refresh_token: "placeholder-refresh",
            esi_token_expires_at: expires_at,
        },
    )
    .await?;

    db::api_key::insert_account_api_key(&mut tx, user_account.id, "ci-seed-user", &user_hash, None)
        .await?;

    tx.commit()
        .await
        .context("failed to commit seed transaction")?;

    tracing::info!(
        admin_account_id = %admin_account.id,
        user_account_id = %user_account.id,
        "dev-seed complete"
    );

    Ok(())
}
