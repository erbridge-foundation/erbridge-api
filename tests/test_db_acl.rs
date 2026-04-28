mod common;

use chrono::Utc;
use erbridge_api::db::{
    account::insert_account,
    acl::{
        delete_acl, find_acl_by_id, find_acls_manageable_by_account, insert_acl,
        purge_expired_acls, set_acl_pending_delete, update_acl_name,
    },
    acl_member::{
        AclPermission, MemberType, delete_member, find_member_by_id, find_members_by_acl,
        insert_acl_member, update_member_permission,
    },
    character::{InsertCharacterData, insert_character},
    map::insert_map,
    map_acl::{attach_acl, detach_acl},
};
use uuid::Uuid;

async fn make_account(pool: &sqlx::PgPool) -> Uuid {
    let mut tx = pool.begin().await.unwrap();
    let acc = insert_account(&mut tx).await.unwrap();
    tx.commit().await.unwrap();
    acc.id
}

async fn make_character(pool: &sqlx::PgPool, account_id: Uuid, eve_id: i64) -> Uuid {
    let aes_key = common::test_aes_key();
    let mut tx = pool.begin().await.unwrap();
    let ch = insert_character(
        &mut tx,
        &aes_key,
        InsertCharacterData {
            account_id,
            eve_character_id: eve_id,
            name: "Test Char",
            corporation_id: 1000,
            alliance_id: None,
            is_main: true,
            esi_client_id: "test_client_id",
            access_token: "tok",
            refresh_token: "ref",
            esi_token_expires_at: Utc::now() + chrono::Duration::hours(1),
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    ch.id
}

// ---------------------------------------------------------------------------
// insert_acl / find_acl_by_id
// ---------------------------------------------------------------------------

#[tokio::test]
async fn insert_and_find_acl() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool).await;

    let mut tx = pool.begin().await.unwrap();
    let acl = insert_acl(&mut tx, account_id, "Test ACL").await.unwrap();
    tx.commit().await.unwrap();

    assert_eq!(acl.name, "Test ACL");
    assert_eq!(acl.owner_account_id, Some(account_id));
    // Newly created ACLs are immediately orphaned (ADR-028) until attached to a map.
    assert!(acl.pending_delete_at.is_some());

    let found = find_acl_by_id(&pool, acl.id).await.unwrap().unwrap();
    assert_eq!(found.id, acl.id);
    assert_eq!(found.name, "Test ACL");
}

#[tokio::test]
async fn find_acl_by_id_returns_none_for_missing() {
    let (_pg, pool) = common::setup_db().await;
    let result = find_acl_by_id(&pool, Uuid::new_v4()).await.unwrap();
    assert!(result.is_none());
}

// ---------------------------------------------------------------------------
// update_acl_name
// ---------------------------------------------------------------------------

#[tokio::test]
async fn update_acl_name_changes_name() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool).await;

    let mut tx = pool.begin().await.unwrap();
    let acl = insert_acl(&mut tx, account_id, "Old Name").await.unwrap();
    tx.commit().await.unwrap();

    let updated = update_acl_name(&pool, acl.id, "New Name").await.unwrap();
    assert_eq!(updated.name, "New Name");

    let found = find_acl_by_id(&pool, acl.id).await.unwrap().unwrap();
    assert_eq!(found.name, "New Name");
}

// ---------------------------------------------------------------------------
// delete_acl
// ---------------------------------------------------------------------------

#[tokio::test]
async fn delete_acl_removes_row() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool).await;

    let mut tx = pool.begin().await.unwrap();
    let acl = insert_acl(&mut tx, account_id, "To Delete").await.unwrap();
    tx.commit().await.unwrap();

    let mut tx = pool.begin().await.unwrap();
    delete_acl(&mut tx, acl.id).await.unwrap();
    tx.commit().await.unwrap();

    let found = find_acl_by_id(&pool, acl.id).await.unwrap();
    assert!(found.is_none());
}

// ---------------------------------------------------------------------------
// set_acl_pending_delete / purge_expired_acls
// ---------------------------------------------------------------------------

#[tokio::test]
async fn set_and_clear_pending_delete() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool).await;

    let mut tx = pool.begin().await.unwrap();
    let acl = insert_acl(&mut tx, account_id, "ACL").await.unwrap();
    tx.commit().await.unwrap();

    set_acl_pending_delete(&pool, acl.id, Some(Utc::now()))
        .await
        .unwrap();
    let found = find_acl_by_id(&pool, acl.id).await.unwrap().unwrap();
    assert!(found.pending_delete_at.is_some());

    set_acl_pending_delete(&pool, acl.id, None).await.unwrap();
    let found = find_acl_by_id(&pool, acl.id).await.unwrap().unwrap();
    assert!(found.pending_delete_at.is_none());
}

#[tokio::test]
async fn purge_expired_acls_deletes_old_orphans() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool).await;

    let mut tx = pool.begin().await.unwrap();
    let acl = insert_acl(&mut tx, account_id, "Orphan").await.unwrap();
    tx.commit().await.unwrap();

    // Set pending_delete_at to 31 days ago.
    sqlx::query!(
        "UPDATE acl SET pending_delete_at = now() - interval '31 days' WHERE id = $1",
        acl.id
    )
    .execute(&pool)
    .await
    .unwrap();

    let deleted = purge_expired_acls(&pool, 30).await.unwrap();
    assert_eq!(deleted, 1);
    assert!(find_acl_by_id(&pool, acl.id).await.unwrap().is_none());
}

#[tokio::test]
async fn purge_expired_acls_keeps_recent_orphans() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool).await;

    let mut tx = pool.begin().await.unwrap();
    let acl = insert_acl(&mut tx, account_id, "Recent Orphan")
        .await
        .unwrap();
    tx.commit().await.unwrap();

    set_acl_pending_delete(&pool, acl.id, Some(Utc::now()))
        .await
        .unwrap();

    let deleted = purge_expired_acls(&pool, 30).await.unwrap();
    assert_eq!(deleted, 0);
    assert!(find_acl_by_id(&pool, acl.id).await.unwrap().is_some());
}

// ---------------------------------------------------------------------------
// find_acls_manageable_by_account
// ---------------------------------------------------------------------------

#[tokio::test]
async fn owner_sees_their_acl() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool).await;

    let mut tx = pool.begin().await.unwrap();
    let acl = insert_acl(&mut tx, account_id, "Owned ACL").await.unwrap();
    tx.commit().await.unwrap();

    let acls = find_acls_manageable_by_account(&pool, account_id)
        .await
        .unwrap();
    assert!(acls.iter().any(|a| a.id == acl.id));
}

#[tokio::test]
async fn manage_member_sees_acl() {
    let (_pg, pool) = common::setup_db().await;
    let owner_id = make_account(&pool).await;
    let member_id = make_account(&pool).await;
    let char_id = make_character(&pool, member_id, 99991).await;

    let mut tx = pool.begin().await.unwrap();
    let acl = insert_acl(&mut tx, owner_id, "Member ACL").await.unwrap();
    insert_acl_member(
        &mut tx,
        acl.id,
        MemberType::Character,
        None,
        Some(char_id),
        "test",
        AclPermission::Manage,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let acls = find_acls_manageable_by_account(&pool, member_id)
        .await
        .unwrap();
    assert!(acls.iter().any(|a| a.id == acl.id));
}

#[tokio::test]
async fn read_member_does_not_see_acl_in_manage_list() {
    let (_pg, pool) = common::setup_db().await;
    let owner_id = make_account(&pool).await;
    let member_id = make_account(&pool).await;
    let char_id = make_character(&pool, member_id, 99992).await;

    let mut tx = pool.begin().await.unwrap();
    let acl = insert_acl(&mut tx, owner_id, "Read Only ACL")
        .await
        .unwrap();
    insert_acl_member(
        &mut tx,
        acl.id,
        MemberType::Character,
        None,
        Some(char_id),
        "test",
        AclPermission::Read,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let acls = find_acls_manageable_by_account(&pool, member_id)
        .await
        .unwrap();
    assert!(!acls.iter().any(|a| a.id == acl.id));
}

// ---------------------------------------------------------------------------
// acl_member CRUD
// ---------------------------------------------------------------------------

#[tokio::test]
async fn insert_and_find_character_member() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool).await;
    let char_id = make_character(&pool, account_id, 11111).await;

    let mut tx = pool.begin().await.unwrap();
    let acl = insert_acl(&mut tx, account_id, "ACL").await.unwrap();
    let member = insert_acl_member(
        &mut tx,
        acl.id,
        MemberType::Character,
        None,
        Some(char_id),
        "test",
        AclPermission::ReadWrite,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    assert_eq!(member.member_type, MemberType::Character);
    assert_eq!(member.character_id, Some(char_id));
    assert_eq!(member.eve_entity_id, Some(11111));
    assert_eq!(member.permission, AclPermission::ReadWrite);

    let found = find_member_by_id(&pool, member.id).await.unwrap().unwrap();
    assert_eq!(found.id, member.id);
}

#[tokio::test]
async fn insert_and_find_corporation_member() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool).await;

    let mut tx = pool.begin().await.unwrap();
    let acl = insert_acl(&mut tx, account_id, "ACL").await.unwrap();
    let member = insert_acl_member(
        &mut tx,
        acl.id,
        MemberType::Corporation,
        Some(98000001),
        None,
        "test",
        AclPermission::Read,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    assert_eq!(member.member_type, MemberType::Corporation);
    assert_eq!(member.eve_entity_id, Some(98000001));
    assert_eq!(member.character_id, None);
}

#[tokio::test]
async fn find_members_by_acl_returns_all() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool).await;
    let char_id = make_character(&pool, account_id, 22222).await;

    let mut tx = pool.begin().await.unwrap();
    let acl = insert_acl(&mut tx, account_id, "ACL").await.unwrap();
    insert_acl_member(
        &mut tx,
        acl.id,
        MemberType::Character,
        None,
        Some(char_id),
        "test",
        AclPermission::Admin,
    )
    .await
    .unwrap();
    insert_acl_member(
        &mut tx,
        acl.id,
        MemberType::Corporation,
        Some(98000001),
        None,
        "test",
        AclPermission::Read,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let members = find_members_by_acl(&pool, acl.id).await.unwrap();
    assert_eq!(members.len(), 2);
}

#[tokio::test]
async fn update_member_permission_changes_permission() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool).await;
    let char_id = make_character(&pool, account_id, 33333).await;

    let mut tx = pool.begin().await.unwrap();
    let acl = insert_acl(&mut tx, account_id, "ACL").await.unwrap();
    let member = insert_acl_member(
        &mut tx,
        acl.id,
        MemberType::Character,
        None,
        Some(char_id),
        "test",
        AclPermission::Read,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let updated = update_member_permission(&pool, member.id, AclPermission::Admin)
        .await
        .unwrap();
    assert_eq!(updated.permission, AclPermission::Admin);
}

#[tokio::test]
async fn delete_member_removes_row() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool).await;
    let char_id = make_character(&pool, account_id, 44444).await;

    let mut tx = pool.begin().await.unwrap();
    let acl = insert_acl(&mut tx, account_id, "ACL").await.unwrap();
    let member = insert_acl_member(
        &mut tx,
        acl.id,
        MemberType::Character,
        None,
        Some(char_id),
        "test",
        AclPermission::Read,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    delete_member(&pool, member.id).await.unwrap();
    let found = find_member_by_id(&pool, member.id).await.unwrap();
    assert!(found.is_none());
}

#[tokio::test]
async fn acl_member_cascade_deletes_with_acl() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool).await;
    let char_id = make_character(&pool, account_id, 55555).await;

    let mut tx = pool.begin().await.unwrap();
    let acl = insert_acl(&mut tx, account_id, "ACL").await.unwrap();
    let member = insert_acl_member(
        &mut tx,
        acl.id,
        MemberType::Character,
        None,
        Some(char_id),
        "test",
        AclPermission::Read,
    )
    .await
    .unwrap();
    delete_acl(&mut tx, acl.id).await.unwrap();
    tx.commit().await.unwrap();

    let found = find_member_by_id(&pool, member.id).await.unwrap();
    assert!(found.is_none());
}

// ---------------------------------------------------------------------------
// orphaned ACL is still visible in the manage list
// ---------------------------------------------------------------------------

#[tokio::test]
async fn orphaned_acl_still_visible_to_owner_in_manage_list() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool).await;

    let mut tx = pool.begin().await.unwrap();
    let acl = insert_acl(&mut tx, account_id, "Orphan ACL").await.unwrap();
    tx.commit().await.unwrap();

    // ACL is orphaned (pending_delete_at set by insert_acl).
    assert!(acl.pending_delete_at.is_some());

    let acls = find_acls_manageable_by_account(&pool, account_id)
        .await
        .unwrap();
    assert!(
        acls.iter().any(|a| a.id == acl.id),
        "orphaned ACL within grace period must still appear in manage list"
    );
}

// ---------------------------------------------------------------------------
// map_acl attach clears pending_delete_at
// ---------------------------------------------------------------------------

#[tokio::test]
async fn attaching_acl_to_map_clears_pending_delete() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool).await;

    let mut tx = pool.begin().await.unwrap();
    let acl = insert_acl(&mut tx, account_id, "Orphan ACL").await.unwrap();
    tx.commit().await.unwrap();

    set_acl_pending_delete(&pool, acl.id, Some(Utc::now()))
        .await
        .unwrap();

    // Create a map and attach.
    let mut tx = pool.begin().await.unwrap();
    let map = erbridge_api::db::map::insert_map(&mut tx, account_id, "Test Map", "test-map", None)
        .await
        .unwrap()
        .unwrap();
    attach_acl(&mut tx, map.id, acl.id).await.unwrap();
    tx.commit().await.unwrap();

    let found = find_acl_by_id(&pool, acl.id).await.unwrap().unwrap();
    assert!(found.pending_delete_at.is_none());
}

// ---------------------------------------------------------------------------
// Concurrency: ACL orphan lifecycle (ADR-028)
//
// The COUNT/UPDATE in detach_acl is racy under concurrent operations. These
// tests confirm that the final state is always self-consistent even when two
// transactions race. The acceptable outcomes are documented per scenario below.
// ---------------------------------------------------------------------------

async fn make_map(pool: &sqlx::PgPool, account_id: Uuid, slug: &str) -> Uuid {
    let mut tx = pool.begin().await.unwrap();
    let map = insert_map(&mut tx, account_id, "Test Map", slug, None)
        .await
        .unwrap()
        .unwrap();
    tx.commit().await.unwrap();
    map.id
}

/// Concurrent detach from two maps: both see `remaining == 0` at the same time.
/// Both transactions race to set `pending_delete_at`. Final state: ACL is
/// orphaned (pending_delete_at IS NOT NULL). No deadlock or error must occur.
#[tokio::test]
async fn concurrent_detach_both_orphan_acl() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool).await;

    let mut tx = pool.begin().await.unwrap();
    let acl = insert_acl(&mut tx, account_id, "Shared ACL").await.unwrap();
    tx.commit().await.unwrap();

    let map_a = make_map(&pool, account_id, "map-concurrent-a").await;
    let map_b = make_map(&pool, account_id, "map-concurrent-b").await;

    // Attach ACL to both maps.
    let mut tx = pool.begin().await.unwrap();
    attach_acl(&mut tx, map_a, acl.id).await.unwrap();
    attach_acl(&mut tx, map_b, acl.id).await.unwrap();
    tx.commit().await.unwrap();

    // Detach from both maps concurrently.
    let pool_a = pool.clone();
    let pool_b = pool.clone();
    let acl_id = acl.id;

    let (r_a, r_b) = tokio::join!(
        async move {
            let mut tx = pool_a.begin().await.unwrap();
            let r = detach_acl(&mut tx, map_a, acl_id).await;
            tx.commit().await.unwrap();
            r
        },
        async move {
            let mut tx = pool_b.begin().await.unwrap();
            let r = detach_acl(&mut tx, map_b, acl_id).await;
            tx.commit().await.unwrap();
            r
        },
    );

    r_a.unwrap();
    r_b.unwrap();

    // ACL has no remaining map attachments — it must be marked for deletion.
    let found = find_acl_by_id(&pool, acl.id).await.unwrap().unwrap();
    assert!(
        found.pending_delete_at.is_some(),
        "ACL must be orphaned after both maps detach"
    );
}

/// Concurrent detach from one map + attach to a second map.
/// The attach always clears `pending_delete_at` unconditionally, so regardless
/// of commit order the ACL is non-orphaned after both transactions commit.
#[tokio::test]
async fn concurrent_detach_and_attach_leaves_acl_non_orphaned() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool).await;

    let mut tx = pool.begin().await.unwrap();
    let acl = insert_acl(&mut tx, account_id, "Race ACL").await.unwrap();
    tx.commit().await.unwrap();

    let map_a = make_map(&pool, account_id, "map-race-a").await;
    let map_b = make_map(&pool, account_id, "map-race-b").await;

    // Attach ACL to map_a only.
    let mut tx = pool.begin().await.unwrap();
    attach_acl(&mut tx, map_a, acl.id).await.unwrap();
    tx.commit().await.unwrap();

    let pool_detach = pool.clone();
    let pool_attach = pool.clone();
    let acl_id = acl.id;

    let (r_detach, r_attach) = tokio::join!(
        async move {
            let mut tx = pool_detach.begin().await.unwrap();
            let r = detach_acl(&mut tx, map_a, acl_id).await;
            tx.commit().await.unwrap();
            r
        },
        async move {
            let mut tx = pool_attach.begin().await.unwrap();
            let r = attach_acl(&mut tx, map_b, acl_id).await;
            tx.commit().await.unwrap();
            r
        },
    );

    r_detach.unwrap();
    r_attach.unwrap();

    // The attach unconditionally clears pending_delete_at, so regardless of
    // which transaction committed last, the ACL must end up non-orphaned.
    let found = find_acl_by_id(&pool, acl.id).await.unwrap().unwrap();
    assert!(
        found.pending_delete_at.is_none(),
        "ACL must not be orphaned when an attach races with the detach"
    );
}
