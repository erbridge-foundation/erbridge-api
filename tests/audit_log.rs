mod common;

use chrono::Utc;
use erbridge_api::{
    db::{account, sde_solar_system::SdeSolarSystem},
    services::{
        auth::{AttachCharacterInput, LoginInput, attach_character_to_account, login_or_register},
        map::{
            AddSignatureInput, CreateConnectionInput, add_signature, create_connection, create_map,
        },
    },
};
use uuid::Uuid;

/// Fake ESI token data used across tests — content doesn't matter since we're
/// not hitting ESI; only the DB writes are under test.
const FAKE_ACCESS_TOKEN: &str = "fake.access.token";
const FAKE_REFRESH_TOKEN: &str = "fake.refresh.token";
const ESI_CLIENT_ID: &str = "test_client_id";

fn login_input(eve_character_id: i64, name: &'static str) -> LoginInput<'static> {
    LoginInput {
        eve_character_id,
        name,
        corporation_id: 1_000_001,
        alliance_id: None,
        esi_client_id: ESI_CLIENT_ID,
        access_token: FAKE_ACCESS_TOKEN,
        refresh_token: FAKE_REFRESH_TOKEN,
        esi_token_expires_at: Utc::now() + chrono::Duration::hours(1),
    }
}

struct AuditRow {
    event_type: String,
    actor_account_id: Option<Uuid>,
    details: serde_json::Value,
}

async fn fetch_audit(pool: &sqlx::PgPool) -> Vec<AuditRow> {
    sqlx::query!("SELECT event_type, actor_account_id, details FROM audit_log ORDER BY occurred_at")
        .fetch_all(pool)
        .await
        .unwrap()
        .into_iter()
        .map(|r| AuditRow {
            event_type: r.event_type,
            actor_account_id: r.actor_account_id,
            details: r.details,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// account_registered
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_registration_writes_audit_entry() {
    let (_pg, pool) = common::setup_db().await;
    let aes_key = common::test_aes_key();

    let account_id = login_or_register(&pool, &aes_key, login_input(11111, "Tester Alpha"))
        .await
        .unwrap();

    let rows = fetch_audit(&pool).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].event_type, "account_registered");
    assert_eq!(rows[0].actor_account_id, None);
    assert_eq!(rows[0].details["account_id"], account_id.to_string());
    assert_eq!(rows[0].details["eve_character_id"], 11111i64);
    assert_eq!(rows[0].details["character_name"], "Tester Alpha");
}

// ---------------------------------------------------------------------------
// ghost_character_claimed (login path)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_ghost_claim_login_writes_audit_entry() {
    let (_pg, pool) = common::setup_db().await;
    let aes_key = common::test_aes_key();

    // Insert a ghost character directly.
    sqlx::query!(
        "INSERT INTO eve_character (eve_character_id, name, corporation_id) VALUES ($1, $2, $3)",
        22222i64,
        "Ghost Pilot",
        1_000_001i64,
    )
    .execute(&pool)
    .await
    .unwrap();

    let account_id = login_or_register(&pool, &aes_key, login_input(22222, "Ghost Pilot"))
        .await
        .unwrap();

    let rows = fetch_audit(&pool).await;
    // account_registered + ghost_character_claimed
    assert_eq!(rows.len(), 2);

    let registered = rows
        .iter()
        .find(|r| r.event_type == "account_registered")
        .unwrap();
    assert_eq!(registered.actor_account_id, None);
    assert_eq!(registered.details["account_id"], account_id.to_string());
    assert_eq!(registered.details["eve_character_id"], 22222i64);
    assert_eq!(registered.details["character_name"], "Ghost Pilot");

    let claimed = rows
        .iter()
        .find(|r| r.event_type == "ghost_character_claimed")
        .unwrap();
    assert_eq!(claimed.actor_account_id, None);
    assert_eq!(claimed.details["account_id"], account_id.to_string());
    assert_eq!(claimed.details["eve_character_id"], 22222i64);
    assert_eq!(claimed.details["character_name"], "Ghost Pilot");
}

// ---------------------------------------------------------------------------
// character_added (attach path — new character)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_character_added_writes_audit_entry() {
    let (_pg, pool) = common::setup_db().await;
    let aes_key = common::test_aes_key();

    let account_id = login_or_register(&pool, &aes_key, login_input(33333, "First Char"))
        .await
        .unwrap();

    // Clear the registration entry so we can focus on the add.
    sqlx::query!("DELETE FROM audit_log")
        .execute(&pool)
        .await
        .unwrap();

    attach_character_to_account(
        &pool,
        &aes_key,
        AttachCharacterInput {
            account_id,
            eve_character_id: 44444,
            name: "Second Char",
            corporation_id: 1_000_001,
            alliance_id: None,
            esi_client_id: ESI_CLIENT_ID,
            access_token: FAKE_ACCESS_TOKEN,
            refresh_token: FAKE_REFRESH_TOKEN,
            esi_token_expires_at: Utc::now() + chrono::Duration::hours(1),
        },
    )
    .await
    .unwrap();

    let rows = fetch_audit(&pool).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].event_type, "character_added");
    assert_eq!(rows[0].actor_account_id, Some(account_id));
    // account_id is in actor_account_id, not repeated in details
    assert!(rows[0].details.get("account_id").is_none());
    assert_eq!(rows[0].details["eve_character_id"], 44444i64);
    assert_eq!(rows[0].details["character_name"], "Second Char");
}

// ---------------------------------------------------------------------------
// ghost_character_claimed (attach path)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_ghost_claim_attach_writes_audit_entry() {
    let (_pg, pool) = common::setup_db().await;
    let aes_key = common::test_aes_key();

    // Create a real account first.
    let account_id = login_or_register(&pool, &aes_key, login_input(55555, "Owner"))
        .await
        .unwrap();

    // Insert a ghost character.
    sqlx::query!(
        "INSERT INTO eve_character (eve_character_id, name, corporation_id) VALUES ($1, $2, $3)",
        66666i64,
        "Ghost Alt",
        1_000_001i64,
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query!("DELETE FROM audit_log")
        .execute(&pool)
        .await
        .unwrap();

    attach_character_to_account(
        &pool,
        &aes_key,
        AttachCharacterInput {
            account_id,
            eve_character_id: 66666,
            name: "Ghost Alt",
            corporation_id: 1_000_001,
            alliance_id: None,
            esi_client_id: ESI_CLIENT_ID,
            access_token: FAKE_ACCESS_TOKEN,
            refresh_token: FAKE_REFRESH_TOKEN,
            esi_token_expires_at: Utc::now() + chrono::Duration::hours(1),
        },
    )
    .await
    .unwrap();

    let rows = fetch_audit(&pool).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].event_type, "ghost_character_claimed");
    assert_eq!(rows[0].actor_account_id, Some(account_id));
    assert_eq!(rows[0].details["account_id"], account_id.to_string());
    assert_eq!(rows[0].details["eve_character_id"], 66666i64);
    assert_eq!(rows[0].details["character_name"], "Ghost Alt");
}

// ---------------------------------------------------------------------------
// account_reactivated
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_reactivation_writes_audit_entry() {
    let (_pg, pool) = common::setup_db().await;
    let aes_key = common::test_aes_key();

    let account_id = login_or_register(&pool, &aes_key, login_input(77777, "Returning Pilot"))
        .await
        .unwrap();

    // Simulate deletion request directly.
    sqlx::query!(
        "UPDATE account SET status = 'pending_delete', delete_requested_at = now() WHERE id = $1",
        account_id,
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query!("DELETE FROM audit_log")
        .execute(&pool)
        .await
        .unwrap();

    let reactivated = account::reactivate_account(&pool, account_id, Some(account_id))
        .await
        .unwrap();
    assert!(reactivated);

    let rows = fetch_audit(&pool).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].event_type, "account_reactivated");
    assert_eq!(rows[0].actor_account_id, Some(account_id));
    assert_eq!(rows[0].details["account_id"], account_id.to_string());
}

// ---------------------------------------------------------------------------
// account_purged
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_purge_writes_audit_entries() {
    let (_pg, pool) = common::setup_db().await;
    let aes_key = common::test_aes_key();

    let id1 = login_or_register(&pool, &aes_key, login_input(88881, "Pilot One"))
        .await
        .unwrap();
    let id2 = login_or_register(&pool, &aes_key, login_input(88882, "Pilot Two"))
        .await
        .unwrap();

    // Set both to pending_delete with an old timestamp (well past grace period).
    sqlx::query!(
        "UPDATE account SET status = 'pending_delete', delete_requested_at = now() - interval '60 days' WHERE id = ANY($1)",
        &[id1, id2],
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query!("DELETE FROM audit_log")
        .execute(&pool)
        .await
        .unwrap();

    let count = account::purge_expired_accounts(&pool, 30).await.unwrap();
    assert_eq!(count, 2);

    let rows = fetch_audit(&pool).await;
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| r.event_type == "account_purged"));
    assert!(rows.iter().all(|r| r.actor_account_id.is_none()));

    let purged_ids: Vec<String> = rows
        .iter()
        .map(|r| r.details["account_id"].as_str().unwrap().to_string())
        .collect();
    assert!(purged_ids.contains(&id1.to_string()));
    assert!(purged_ids.contains(&id2.to_string()));
}

// ---------------------------------------------------------------------------
// map_data_mutated (A6)
// ---------------------------------------------------------------------------

async fn seed_solar(pool: &sqlx::PgPool, id: i64) {
    let mut tx = pool.begin().await.unwrap();
    erbridge_api::db::sde_solar_system::bulk_upsert_solar_systems(
        &mut tx,
        &[SdeSolarSystem {
            solar_system_id: id,
            name: format!("AuditSystem{id}"),
            region_id: Some(10000001),
            constellation_id: Some(20000001),
            faction_id: None,
            star_id: None,
            security_status: Some(0.0),
            security_class: Some("H".into()),
            wh_class: Some("C1".into()),
            wormhole_class_id: None,
            luminosity: None,
            radius: None,
            border: Some(false),
            corridor: Some(false),
            fringe: Some(false),
            hub: Some(false),
            international: Some(false),
            regional: Some(false),
            visual_effect: None,
            name_i18n: None,
            planet_ids: None,
            stargate_ids: None,
            disallowed_anchor_categories: None,
            disallowed_anchor_groups: None,
            position: None,
            position_2d: None,
        }],
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn test_create_connection_writes_map_data_mutated_audit() {
    let (_pg, pool) = common::setup_db().await;
    let aes_key = common::test_aes_key();

    seed_solar(&pool, 31000901).await;
    seed_solar(&pool, 31000902).await;

    let account_id = login_or_register(&pool, &aes_key, login_input(99901, "Map Pilot"))
        .await
        .unwrap();

    let map = create_map(
        &pool,
        account_id,
        "Audit Conn Map",
        "audit-conn-map",
        None,
        None,
    )
    .await
    .unwrap();

    sqlx::query!("DELETE FROM audit_log")
        .execute(&pool)
        .await
        .unwrap();

    create_connection(
        &pool,
        account_id,
        CreateConnectionInput {
            map_id: map.id,
            system_a_id: 31000901,
            system_b_id: 31000902,
        },
    )
    .await
    .unwrap();

    let rows = fetch_audit(&pool).await;
    let mutated = rows
        .iter()
        .find(|r| r.event_type == "map_data_mutated")
        .unwrap();
    assert_eq!(mutated.details["kind"], "ConnectionCreated");
    assert_eq!(mutated.details["map_id"], map.id.to_string());
    assert_eq!(mutated.actor_account_id, Some(account_id));
}

#[tokio::test]
async fn test_add_signature_writes_map_data_mutated_audit() {
    let (_pg, pool) = common::setup_db().await;
    let aes_key = common::test_aes_key();

    seed_solar(&pool, 31000911).await;

    let account_id = login_or_register(&pool, &aes_key, login_input(99911, "Sig Pilot"))
        .await
        .unwrap();

    let map = create_map(
        &pool,
        account_id,
        "Audit Sig Map",
        "audit-sig-map",
        None,
        None,
    )
    .await
    .unwrap();

    sqlx::query!("DELETE FROM audit_log")
        .execute(&pool)
        .await
        .unwrap();

    add_signature(
        &pool,
        account_id,
        AddSignatureInput {
            map_id: map.id,
            system_id: 31000911,
            sig_code: "ABC-123".into(),
            sig_type: "wormhole".into(),
        },
    )
    .await
    .unwrap();

    let rows = fetch_audit(&pool).await;
    let mutated = rows
        .iter()
        .find(|r| r.event_type == "map_data_mutated")
        .unwrap();
    assert_eq!(mutated.details["kind"], "SignatureAdded");
}
