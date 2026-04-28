mod common;

use chrono::Utc;
use erbridge_api::{
    db::{
        map_acl::find_acls_for_map,
        map_types::{ConnectionStatus, LifeState, MassState, Side},
        sde_solar_system::SdeSolarSystem,
    },
    services::{
        acl::create_acl,
        auth::{LoginInput, login_or_register},
        map::{
            AddSignatureInput, CreateConnectionInput, MapError, RouteQuery,
            UpdateConnectionMetadataInput, add_signature, attach_acl_to_map, create_connection,
            create_map, delete_map, detach_acl_from_map, find_routes, get_map, link_signature,
            list_maps, update_connection_metadata,
        },
    },
};
use uuid::Uuid;

const FAKE_ACCESS: &str = "fake.access";
const FAKE_REFRESH: &str = "fake.refresh";
const ESI_CLIENT: &str = "test_client_id";

async fn make_account(pool: &sqlx::PgPool, eve_id: i64, name: &'static str) -> Uuid {
    let aes_key = common::test_aes_key();
    login_or_register(
        pool,
        &aes_key,
        LoginInput {
            eve_character_id: eve_id,
            name,
            corporation_id: 1_000_001,
            alliance_id: None,
            esi_client_id: ESI_CLIENT,
            access_token: FAKE_ACCESS,
            refresh_token: FAKE_REFRESH,
            esi_token_expires_at: Utc::now() + chrono::Duration::hours(1),
        },
    )
    .await
    .unwrap()
}

async fn seed_solar(pool: &sqlx::PgPool, id: i64) {
    let mut tx = pool.begin().await.unwrap();
    erbridge_api::db::sde_solar_system::bulk_upsert_solar_systems(
        &mut tx,
        &[SdeSolarSystem {
            solar_system_id: id,
            name: format!("TestSystem{id}"),
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

// ── create_map ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn create_map_succeeds() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool, 10001, "Pilot One").await;

    let map = create_map(&pool, account_id, "My Map", "my-map", None, None)
        .await
        .unwrap();

    assert_eq!(map.owner_account_id, Some(account_id));
    assert_eq!(map.name, "My Map");
    assert_eq!(map.retention_days, 14);
}

#[tokio::test]
async fn create_map_records_audit_event() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool, 10002, "Pilot Two").await;

    let map = create_map(&pool, account_id, "Audit Map", "audit-map", None, None)
        .await
        .unwrap();

    let row =
        sqlx::query!("SELECT event_type, details FROM audit_log WHERE event_type = 'map_created'")
            .fetch_one(&pool)
            .await
            .unwrap();

    assert_eq!(row.event_type, "map_created");
    assert_eq!(row.details["map_id"], map.id.to_string());
    assert_eq!(row.details["name"], "Audit Map");
}

#[tokio::test]
async fn create_map_records_map_event() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool, 10003, "Pilot Three").await;

    let map = create_map(&pool, account_id, "Event Map", "event-map", None, None)
        .await
        .unwrap();

    let row = sqlx::query!(
        "SELECT event_type, entity_type FROM map_events WHERE map_id = $1",
        map.id,
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(row.event_type, "MapCreated");
    assert_eq!(row.entity_type, "map");
}

// ── list_maps / ownership isolation ──────────────────────────────────────────

#[tokio::test]
async fn list_maps_returns_only_owned() {
    let (_pg, pool) = common::setup_db().await;
    let account_a = make_account(&pool, 20001, "Pilot A").await;
    let account_b = make_account(&pool, 20002, "Pilot B").await;

    create_map(&pool, account_a, "A's Map", "a-map", None, None)
        .await
        .unwrap();
    create_map(&pool, account_b, "B's Map", "b-map", None, None)
        .await
        .unwrap();

    let maps_a = list_maps(&pool, account_a).await.unwrap();
    let maps_b = list_maps(&pool, account_b).await.unwrap();

    assert_eq!(maps_a.len(), 1);
    assert_eq!(maps_a[0].name, "A's Map");

    assert_eq!(maps_b.len(), 1);
    assert_eq!(maps_b[0].name, "B's Map");
}

// ── get_map / delete_map ─────────────────────────────────────────────────────

#[tokio::test]
async fn delete_map_removes_map() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool, 30001, "Pilot D").await;
    let map = create_map(&pool, account_id, "Temp Map", "temp-map", None, None)
        .await
        .unwrap();

    delete_map(&pool, map.id, account_id).await.unwrap();

    let result = get_map(&pool, account_id, map.id).await;
    assert!(matches!(result, Err(MapError::NotFound)));
}

#[tokio::test]
async fn delete_map_by_non_owner_is_forbidden() {
    let (_pg, pool) = common::setup_db().await;
    let owner = make_account(&pool, 30002, "Owner").await;
    let other = make_account(&pool, 30003, "Other").await;
    let map = create_map(&pool, owner, "Protected Map", "protected-map", None, None)
        .await
        .unwrap();

    let result = delete_map(&pool, map.id, other).await;
    assert!(matches!(result, Err(MapError::Forbidden)));
}

// ── create_connection ─────────────────────────────────────────────────────────

#[tokio::test]
async fn create_connection_succeeds() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool, 40001, "Conn Pilot").await;
    seed_solar(&pool, 31000001).await;
    seed_solar(&pool, 31000002).await;

    let map = create_map(&pool, account_id, "Conn Map", "conn-map", None, None)
        .await
        .unwrap();
    let (conn, end_a, end_b) = create_connection(
        &pool,
        account_id,
        CreateConnectionInput {
            map_id: map.id,
            system_a_id: 31000001,
            system_b_id: 31000002,
        },
    )
    .await
    .unwrap();

    assert_eq!(conn.map_id, map.id);
    assert_eq!(conn.status, ConnectionStatus::Partial);
    assert_eq!(end_a.system_id, 31000001);
    assert_eq!(end_b.system_id, 31000002);
}

#[tokio::test]
async fn create_connection_self_loop_rejected() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool, 40002, "Loop Pilot").await;
    seed_solar(&pool, 31000010).await;

    let map = create_map(&pool, account_id, "Loop Map", "loop-map", None, None)
        .await
        .unwrap();
    let result = create_connection(
        &pool,
        account_id,
        CreateConnectionInput {
            map_id: map.id,
            system_a_id: 31000010,
            system_b_id: 31000010,
        },
    )
    .await;

    assert!(matches!(result, Err(MapError::SelfLoop)));
}

#[tokio::test]
async fn create_connection_appends_event() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool, 40003, "Event Conn Pilot").await;
    seed_solar(&pool, 31000020).await;
    seed_solar(&pool, 31000021).await;

    let map = create_map(
        &pool,
        account_id,
        "Event Conn Map",
        "event-conn-map",
        None,
        None,
    )
    .await
    .unwrap();
    let (conn, _, _) = create_connection(
        &pool,
        account_id,
        CreateConnectionInput {
            map_id: map.id,
            system_a_id: 31000020,
            system_b_id: 31000021,
        },
    )
    .await
    .unwrap();

    let event = sqlx::query!(
        "SELECT event_type, entity_type FROM map_events WHERE map_id = $1 AND entity_type = 'connection'",
        map.id,
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(event.event_type, "ConnectionCreated");
    assert_eq!(event.entity_type, "connection");
    let _ = conn.connection_id;
}

// ── link_signature ────────────────────────────────────────────────────────────

#[tokio::test]
async fn link_signature_updates_status_to_linked() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool, 50001, "Link Pilot").await;
    seed_solar(&pool, 31000030).await;
    seed_solar(&pool, 31000031).await;

    let map = create_map(&pool, account_id, "Link Map", "link-map", None, None)
        .await
        .unwrap();
    let (conn, _, _) = create_connection(
        &pool,
        account_id,
        CreateConnectionInput {
            map_id: map.id,
            system_a_id: 31000030,
            system_b_id: 31000031,
        },
    )
    .await
    .unwrap();

    let sig = add_signature(
        &pool,
        account_id,
        AddSignatureInput {
            map_id: map.id,
            system_id: 31000030,
            sig_code: "ABC-123".into(),
            sig_type: "wormhole".into(),
        },
    )
    .await
    .unwrap();

    link_signature(
        &pool,
        account_id,
        map.id,
        conn.connection_id,
        sig.signature_id,
        Side::A,
    )
    .await
    .unwrap();

    let updated = erbridge_api::db::connection::find_connection(&pool, conn.connection_id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(updated.status, ConnectionStatus::Linked);
}

#[tokio::test]
async fn link_signature_fully_linked() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool, 50002, "Full Link Pilot").await;
    seed_solar(&pool, 31000040).await;
    seed_solar(&pool, 31000041).await;

    let map = create_map(
        &pool,
        account_id,
        "Full Link Map",
        "full-link-map",
        None,
        None,
    )
    .await
    .unwrap();
    let (conn, _, _) = create_connection(
        &pool,
        account_id,
        CreateConnectionInput {
            map_id: map.id,
            system_a_id: 31000040,
            system_b_id: 31000041,
        },
    )
    .await
    .unwrap();

    let sig_a = add_signature(
        &pool,
        account_id,
        AddSignatureInput {
            map_id: map.id,
            system_id: 31000040,
            sig_code: "AAA-001".into(),
            sig_type: "wormhole".into(),
        },
    )
    .await
    .unwrap();

    let sig_b = add_signature(
        &pool,
        account_id,
        AddSignatureInput {
            map_id: map.id,
            system_id: 31000041,
            sig_code: "BBB-001".into(),
            sig_type: "wormhole".into(),
        },
    )
    .await
    .unwrap();

    link_signature(
        &pool,
        account_id,
        map.id,
        conn.connection_id,
        sig_a.signature_id,
        Side::A,
    )
    .await
    .unwrap();
    link_signature(
        &pool,
        account_id,
        map.id,
        conn.connection_id,
        sig_b.signature_id,
        Side::B,
    )
    .await
    .unwrap();

    let updated = erbridge_api::db::connection::find_connection(&pool, conn.connection_id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(updated.status, ConnectionStatus::FullyLinked);
}

#[tokio::test]
async fn link_already_linked_signature_rejected() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool, 50003, "Dup Link Pilot").await;
    seed_solar(&pool, 31000050).await;
    seed_solar(&pool, 31000051).await;

    let map = create_map(
        &pool,
        account_id,
        "Dup Link Map",
        "dup-link-map",
        None,
        None,
    )
    .await
    .unwrap();
    let (conn, _, _) = create_connection(
        &pool,
        account_id,
        CreateConnectionInput {
            map_id: map.id,
            system_a_id: 31000050,
            system_b_id: 31000051,
        },
    )
    .await
    .unwrap();

    let sig = add_signature(
        &pool,
        account_id,
        AddSignatureInput {
            map_id: map.id,
            system_id: 31000050,
            sig_code: "DUP-001".into(),
            sig_type: "wormhole".into(),
        },
    )
    .await
    .unwrap();

    link_signature(
        &pool,
        account_id,
        map.id,
        conn.connection_id,
        sig.signature_id,
        Side::A,
    )
    .await
    .unwrap();

    let result = link_signature(
        &pool,
        account_id,
        map.id,
        conn.connection_id,
        sig.signature_id,
        Side::A,
    )
    .await;

    assert!(matches!(result, Err(MapError::SignatureAlreadyLinked)));
}

// ── update_connection_metadata ────────────────────────────────────────────────

#[tokio::test]
async fn update_metadata_propagates_to_signatures() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool, 60001, "Meta Pilot").await;
    seed_solar(&pool, 31000060).await;
    seed_solar(&pool, 31000061).await;

    let map = create_map(&pool, account_id, "Meta Map", "meta-map", None, None)
        .await
        .unwrap();
    let (conn, _, _) = create_connection(
        &pool,
        account_id,
        CreateConnectionInput {
            map_id: map.id,
            system_a_id: 31000060,
            system_b_id: 31000061,
        },
    )
    .await
    .unwrap();

    let sig = add_signature(
        &pool,
        account_id,
        AddSignatureInput {
            map_id: map.id,
            system_id: 31000060,
            sig_code: "META-001".into(),
            sig_type: "wormhole".into(),
        },
    )
    .await
    .unwrap();

    link_signature(
        &pool,
        account_id,
        map.id,
        conn.connection_id,
        sig.signature_id,
        Side::A,
    )
    .await
    .unwrap();

    update_connection_metadata(
        &pool,
        account_id,
        map.id,
        UpdateConnectionMetadataInput {
            connection_id: conn.connection_id,
            life_state: Some(LifeState::Eol),
            mass_state: Some(MassState::Critical),
        },
    )
    .await
    .unwrap();

    let updated_sig = erbridge_api::db::signature::find_signature(&pool, sig.signature_id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(updated_sig.derived_life_state, Some(LifeState::Eol));
    assert_eq!(updated_sig.derived_mass_state, Some(MassState::Critical));
}

#[tokio::test]
async fn update_metadata_appends_event() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool, 60002, "Meta Event Pilot").await;
    seed_solar(&pool, 31000070).await;
    seed_solar(&pool, 31000071).await;

    let map = create_map(
        &pool,
        account_id,
        "Meta Event Map",
        "meta-event-map",
        None,
        None,
    )
    .await
    .unwrap();
    let (conn, _, _) = create_connection(
        &pool,
        account_id,
        CreateConnectionInput {
            map_id: map.id,
            system_a_id: 31000070,
            system_b_id: 31000071,
        },
    )
    .await
    .unwrap();

    update_connection_metadata(
        &pool,
        account_id,
        map.id,
        UpdateConnectionMetadataInput {
            connection_id: conn.connection_id,
            life_state: Some(LifeState::Eol),
            mass_state: None,
        },
    )
    .await
    .unwrap();

    let event = sqlx::query!(
        "SELECT event_type FROM map_events WHERE map_id = $1 AND event_type = 'ConnectionMetadataUpdated'",
        map.id,
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(event.event_type, "ConnectionMetadataUpdated");
}

// ── find_routes ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn find_routes_returns_paths() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool, 70001, "Route Pilot").await;
    seed_solar(&pool, 31000100).await;
    seed_solar(&pool, 31000101).await;
    seed_solar(&pool, 31000102).await;

    let map = create_map(&pool, account_id, "Route Map", "route-map", None, None)
        .await
        .unwrap();

    // Chain: 100 → 101 → 102
    create_connection(
        &pool,
        account_id,
        CreateConnectionInput {
            map_id: map.id,
            system_a_id: 31000100,
            system_b_id: 31000101,
        },
    )
    .await
    .unwrap();

    create_connection(
        &pool,
        account_id,
        CreateConnectionInput {
            map_id: map.id,
            system_a_id: 31000101,
            system_b_id: 31000102,
        },
    )
    .await
    .unwrap();

    let routes = find_routes(
        &pool,
        account_id,
        RouteQuery {
            map_id: map.id,
            start_system_id: 31000100,
            max_depth: 5,
            exclude_eol: false,
            exclude_mass_critical: false,
        },
    )
    .await
    .unwrap();

    let reached: Vec<i64> = routes.iter().map(|r| r.current_system_id).collect();
    assert!(reached.contains(&31000101), "should reach 31000101");
    assert!(reached.contains(&31000102), "should reach 31000102");
}

#[tokio::test]
async fn find_routes_excludes_eol() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool, 70002, "EOL Pilot").await;
    seed_solar(&pool, 31000110).await;
    seed_solar(&pool, 31000111).await;

    let map = create_map(&pool, account_id, "EOL Map", "eol-map", None, None)
        .await
        .unwrap();
    let (conn, _, _) = create_connection(
        &pool,
        account_id,
        CreateConnectionInput {
            map_id: map.id,
            system_a_id: 31000110,
            system_b_id: 31000111,
        },
    )
    .await
    .unwrap();

    // Mark connection as EOL
    update_connection_metadata(
        &pool,
        account_id,
        map.id,
        UpdateConnectionMetadataInput {
            connection_id: conn.connection_id,
            life_state: Some(LifeState::Eol),
            mass_state: None,
        },
    )
    .await
    .unwrap();

    let routes = find_routes(
        &pool,
        account_id,
        RouteQuery {
            map_id: map.id,
            start_system_id: 31000110,
            max_depth: 5,
            exclude_eol: true,
            exclude_mass_critical: false,
        },
    )
    .await
    .unwrap();

    let reached: Vec<i64> = routes.iter().map(|r| r.current_system_id).collect();
    assert!(
        !reached.contains(&31000111),
        "EOL system should be excluded"
    );
}

#[tokio::test]
async fn find_routes_clamps_depth() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool, 70003, "Depth Pilot").await;
    seed_solar(&pool, 31000120).await;

    let map = create_map(&pool, account_id, "Depth Map", "depth-map", None, None)
        .await
        .unwrap();

    // max_depth=100 should be clamped to 20 without error
    let routes = find_routes(
        &pool,
        account_id,
        RouteQuery {
            map_id: map.id,
            start_system_id: 31000120,
            max_depth: 100,
            exclude_eol: false,
            exclude_mass_critical: false,
        },
    )
    .await;

    assert!(routes.is_ok(), "clamped depth should not error");
}

// ---------------------------------------------------------------------------
// Audit log assertions for map–ACL attach/detach (A5)
// ---------------------------------------------------------------------------

async fn audit_event_count(pool: &sqlx::PgPool, event_type: &str) -> i64 {
    sqlx::query_scalar!(
        "SELECT COUNT(*) FROM audit_log WHERE event_type = $1",
        event_type
    )
    .fetch_one(pool)
    .await
    .unwrap()
    .unwrap_or(0)
}

#[tokio::test]
async fn attach_acl_to_map_records_audit_row() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool, 80001, "Attach Owner").await;

    let map = create_map(&pool, account_id, "Audit Map", "audit-map", None, None)
        .await
        .unwrap();
    let acl = create_acl(&pool, account_id, "Audit ACL").await.unwrap();

    let before = audit_event_count(&pool, "acl_attached_to_map").await;
    attach_acl_to_map(&pool, map.id, acl.id, account_id)
        .await
        .unwrap();
    let after = audit_event_count(&pool, "acl_attached_to_map").await;
    assert_eq!(after, before + 1);
}

#[tokio::test]
async fn detach_acl_from_map_records_audit_row() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool, 80002, "Detach Owner").await;

    let map = create_map(&pool, account_id, "Detach Map", "detach-map", None, None)
        .await
        .unwrap();
    let acl = create_acl(&pool, account_id, "Detach ACL").await.unwrap();

    attach_acl_to_map(&pool, map.id, acl.id, account_id)
        .await
        .unwrap();

    let before = audit_event_count(&pool, "acl_detached_from_map").await;
    detach_acl_from_map(&pool, map.id, acl.id, account_id)
        .await
        .unwrap();
    let after = audit_event_count(&pool, "acl_detached_from_map").await;
    assert_eq!(after, before + 1);
}

#[tokio::test]
async fn find_acls_for_map_excludes_soft_deleted_map() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool, 80003, "Soft Delete Owner").await;

    let map = create_map(&pool, account_id, "Deleted Map", "deleted-map", None, None)
        .await
        .unwrap();
    let acl = create_acl(&pool, account_id, "Test ACL").await.unwrap();

    attach_acl_to_map(&pool, map.id, acl.id, account_id)
        .await
        .unwrap();

    // Confirm ACL is visible before soft-delete.
    let acls_before = find_acls_for_map(&pool, map.id).await.unwrap();
    assert_eq!(acls_before.len(), 1);

    // Soft-delete the map directly.
    sqlx::query!("UPDATE map SET deleted = true WHERE id = $1", map.id)
        .execute(&pool)
        .await
        .unwrap();

    // ACL list must now be empty.
    let acls_after = find_acls_for_map(&pool, map.id).await.unwrap();
    assert!(acls_after.is_empty());
}
