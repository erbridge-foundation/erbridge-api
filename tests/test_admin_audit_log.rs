mod common;

use chrono::Utc;
use erbridge_api::{
    audit::{self, AuditEvent},
    services::admin::admin_list_audit_log,
};
use uuid::Uuid;

async fn write_event(pool: &sqlx::PgPool, actor: Option<Uuid>, event: AuditEvent) {
    let mut tx = pool.begin().await.unwrap();
    audit::record_in_tx(&mut tx, actor, event).await.unwrap();
    tx.commit().await.unwrap();
}

async fn make_account(pool: &sqlx::PgPool) -> Uuid {
    sqlx::query_scalar!("INSERT INTO account DEFAULT VALUES RETURNING id")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn lists_newest_first_with_default_limit() {
    let (_pg, pool) = common::setup_db().await;
    let actor = make_account(&pool).await;

    for i in 1..=3 {
        write_event(
            &pool,
            Some(actor),
            AuditEvent::EveCharacterBlocked {
                eve_character_id: i,
                reason: None,
            },
        )
        .await;
    }

    let entries = admin_list_audit_log(&pool, None, None, None, None)
        .await
        .unwrap();
    assert_eq!(entries.len(), 3);
    // Newest first.
    assert!(entries[0].occurred_at >= entries[1].occurred_at);
    assert!(entries[1].occurred_at >= entries[2].occurred_at);
}

#[tokio::test]
async fn filters_by_event_type() {
    let (_pg, pool) = common::setup_db().await;
    let actor = make_account(&pool).await;

    write_event(
        &pool,
        Some(actor),
        AuditEvent::EveCharacterBlocked {
            eve_character_id: 1,
            reason: None,
        },
    )
    .await;
    write_event(
        &pool,
        Some(actor),
        AuditEvent::EveCharacterUnblocked {
            eve_character_id: 1,
        },
    )
    .await;

    let entries = admin_list_audit_log(
        &pool,
        Some("eve_character_blocked".into()),
        None,
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].event_type, "eve_character_blocked");
}

#[tokio::test]
async fn filters_by_actor() {
    let (_pg, pool) = common::setup_db().await;
    let actor_a = make_account(&pool).await;
    let actor_b = make_account(&pool).await;

    write_event(
        &pool,
        Some(actor_a),
        AuditEvent::EveCharacterBlocked {
            eve_character_id: 1,
            reason: None,
        },
    )
    .await;
    write_event(
        &pool,
        Some(actor_b),
        AuditEvent::EveCharacterBlocked {
            eve_character_id: 2,
            reason: None,
        },
    )
    .await;

    let entries = admin_list_audit_log(&pool, None, Some(actor_a), None, None)
        .await
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].actor_account_id, Some(actor_a));
}

#[tokio::test]
async fn before_cursor_excludes_at_and_after() {
    let (_pg, pool) = common::setup_db().await;
    let actor = make_account(&pool).await;

    for i in 1..=3 {
        write_event(
            &pool,
            Some(actor),
            AuditEvent::EveCharacterBlocked {
                eve_character_id: i,
                reason: None,
            },
        )
        .await;
    }

    // First page (newest first), then page using `before = oldest_in_first_page`.
    let page1 = admin_list_audit_log(&pool, None, None, None, Some(2))
        .await
        .unwrap();
    assert_eq!(page1.len(), 2);
    let cursor = page1.last().unwrap().occurred_at;

    let page2 = admin_list_audit_log(&pool, None, None, Some(cursor), Some(2))
        .await
        .unwrap();
    assert_eq!(page2.len(), 1);
    assert!(page2[0].occurred_at < cursor);
}

#[tokio::test]
async fn limit_is_clamped_to_max() {
    let (_pg, pool) = common::setup_db().await;
    let actor = make_account(&pool).await;

    write_event(
        &pool,
        Some(actor),
        AuditEvent::EveCharacterBlocked {
            eve_character_id: 1,
            reason: None,
        },
    )
    .await;

    // Pass a huge limit; should not error and should return what's there.
    let entries = admin_list_audit_log(&pool, None, None, None, Some(1_000_000))
        .await
        .unwrap();
    assert_eq!(entries.len(), 1);

    // Pass zero/negative; should clamp to >=1, not panic.
    let entries = admin_list_audit_log(&pool, None, None, None, Some(0))
        .await
        .unwrap();
    assert_eq!(entries.len(), 1);

    // Suppress unused-import warning if Utc isn't otherwise used.
    let _ = Utc::now();
}
