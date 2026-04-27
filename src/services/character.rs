use anyhow::Context;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    audit::{self, AuditEvent},
    db::character::{DeleteCharacterResult, delete_character_in_tx, set_main_character_in_tx},
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
