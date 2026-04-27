mod common;

use chrono::Utc;
use erbridge_api::{
    db::{
        account::insert_account,
        acl::insert_acl,
        acl_member::{AclPermission, MemberType, insert_acl_member},
        character::{InsertCharacterData, insert_character},
        map::insert_map,
        map_acl::attach_acl,
    },
    permissions::{Permission, effective_permission},
};
use uuid::Uuid;

async fn make_account(pool: &sqlx::PgPool) -> Uuid {
    let mut tx = pool.begin().await.unwrap();
    let acc = insert_account(&mut tx).await.unwrap();
    tx.commit().await.unwrap();
    acc.id
}

async fn make_character(
    pool: &sqlx::PgPool,
    account_id: Uuid,
    eve_id: i64,
    corporation_id: i64,
    alliance_id: Option<i64>,
) -> Uuid {
    let aes_key = common::test_aes_key();
    let mut tx = pool.begin().await.unwrap();
    let ch = insert_character(
        &mut tx,
        &aes_key,
        InsertCharacterData {
            account_id,
            eve_character_id: eve_id,
            name: "Pilot",
            corporation_id,
            alliance_id,
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
// Owner bypass
// ---------------------------------------------------------------------------

#[tokio::test]
async fn owner_always_gets_admin() {
    let (_pg, pool) = common::setup_db().await;
    let owner_id = make_account(&pool).await;

    let mut tx = pool.begin().await.unwrap();
    let map = insert_map(&mut tx, owner_id, "Map", "owner-map", None)
        .await
        .unwrap()
        .unwrap();
    tx.commit().await.unwrap();

    let perm = effective_permission(&pool, owner_id, map.id).await.unwrap();
    assert_eq!(perm, Some(Permission::Admin));
}

#[tokio::test]
async fn owner_is_not_affected_by_deny_entry() {
    let (_pg, pool) = common::setup_db().await;
    let owner_id = make_account(&pool).await;
    let char_id = make_character(&pool, owner_id, 1001, 1000, None).await;

    let mut tx = pool.begin().await.unwrap();
    let map = insert_map(&mut tx, owner_id, "Map", "owner-deny-map", None)
        .await
        .unwrap()
        .unwrap();
    let acl = insert_acl(&mut tx, owner_id, "ACL").await.unwrap();
    insert_acl_member(
        &mut tx,
        acl.id,
        MemberType::Character,
        None,
        Some(char_id),
        "test",
        AclPermission::Deny,
    )
    .await
    .unwrap();
    attach_acl(&mut tx, map.id, acl.id).await.unwrap();
    tx.commit().await.unwrap();

    let perm = effective_permission(&pool, owner_id, map.id).await.unwrap();
    assert_eq!(perm, Some(Permission::Admin));
}

// ---------------------------------------------------------------------------
// No access
// ---------------------------------------------------------------------------

#[tokio::test]
async fn unrelated_account_gets_none() {
    let (_pg, pool) = common::setup_db().await;
    let owner_id = make_account(&pool).await;
    let other_id = make_account(&pool).await;

    let mut tx = pool.begin().await.unwrap();
    let map = insert_map(&mut tx, owner_id, "Map", "no-access-map", None)
        .await
        .unwrap()
        .unwrap();
    tx.commit().await.unwrap();

    let perm = effective_permission(&pool, other_id, map.id).await.unwrap();
    assert_eq!(perm, None);
}

#[tokio::test]
async fn nonexistent_map_returns_none() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool).await;
    let perm = effective_permission(&pool, account_id, Uuid::new_v4())
        .await
        .unwrap();
    assert_eq!(perm, None);
}

// ---------------------------------------------------------------------------
// Direct character grant
// ---------------------------------------------------------------------------

#[tokio::test]
async fn character_member_read_grant() {
    let (_pg, pool) = common::setup_db().await;
    let owner_id = make_account(&pool).await;
    let member_id = make_account(&pool).await;
    let char_id = make_character(&pool, member_id, 2001, 1000, None).await;

    let mut tx = pool.begin().await.unwrap();
    let map = insert_map(&mut tx, owner_id, "Map", "char-read-map", None)
        .await
        .unwrap()
        .unwrap();
    let acl = insert_acl(&mut tx, owner_id, "ACL").await.unwrap();
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
    attach_acl(&mut tx, map.id, acl.id).await.unwrap();
    tx.commit().await.unwrap();

    let perm = effective_permission(&pool, member_id, map.id)
        .await
        .unwrap();
    assert_eq!(perm, Some(Permission::Read));
}

#[tokio::test]
async fn character_member_admin_grant() {
    let (_pg, pool) = common::setup_db().await;
    let owner_id = make_account(&pool).await;
    let member_id = make_account(&pool).await;
    let char_id = make_character(&pool, member_id, 2002, 1000, None).await;

    let mut tx = pool.begin().await.unwrap();
    let map = insert_map(&mut tx, owner_id, "Map", "char-admin-map", None)
        .await
        .unwrap()
        .unwrap();
    let acl = insert_acl(&mut tx, owner_id, "ACL").await.unwrap();
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
    attach_acl(&mut tx, map.id, acl.id).await.unwrap();
    tx.commit().await.unwrap();

    let perm = effective_permission(&pool, member_id, map.id)
        .await
        .unwrap();
    assert_eq!(perm, Some(Permission::Admin));
}

// ---------------------------------------------------------------------------
// Corporation and alliance grants
// ---------------------------------------------------------------------------

#[tokio::test]
async fn corporation_member_grant() {
    let (_pg, pool) = common::setup_db().await;
    let owner_id = make_account(&pool).await;
    let member_id = make_account(&pool).await;
    make_character(&pool, member_id, 3001, 98000001, None).await;

    let mut tx = pool.begin().await.unwrap();
    let map = insert_map(&mut tx, owner_id, "Map", "corp-map", None)
        .await
        .unwrap()
        .unwrap();
    let acl = insert_acl(&mut tx, owner_id, "ACL").await.unwrap();
    insert_acl_member(
        &mut tx,
        acl.id,
        MemberType::Corporation,
        Some(98000001),
        None,
        "test",
        AclPermission::ReadWrite,
    )
    .await
    .unwrap();
    attach_acl(&mut tx, map.id, acl.id).await.unwrap();
    tx.commit().await.unwrap();

    let perm = effective_permission(&pool, member_id, map.id)
        .await
        .unwrap();
    assert_eq!(perm, Some(Permission::ReadWrite));
}

#[tokio::test]
async fn alliance_member_grant() {
    let (_pg, pool) = common::setup_db().await;
    let owner_id = make_account(&pool).await;
    let member_id = make_account(&pool).await;
    make_character(&pool, member_id, 3002, 98000001, Some(99000001)).await;

    let mut tx = pool.begin().await.unwrap();
    let map = insert_map(&mut tx, owner_id, "Map", "alliance-map", None)
        .await
        .unwrap()
        .unwrap();
    let acl = insert_acl(&mut tx, owner_id, "ACL").await.unwrap();
    insert_acl_member(
        &mut tx,
        acl.id,
        MemberType::Alliance,
        Some(99000001),
        None,
        "test",
        AclPermission::Read,
    )
    .await
    .unwrap();
    attach_acl(&mut tx, map.id, acl.id).await.unwrap();
    tx.commit().await.unwrap();

    let perm = effective_permission(&pool, member_id, map.id)
        .await
        .unwrap();
    assert_eq!(perm, Some(Permission::Read));
}

#[tokio::test]
async fn character_without_alliance_does_not_match_alliance_entry() {
    let (_pg, pool) = common::setup_db().await;
    let owner_id = make_account(&pool).await;
    let member_id = make_account(&pool).await;
    make_character(&pool, member_id, 3003, 98000001, None).await; // no alliance

    let mut tx = pool.begin().await.unwrap();
    let map = insert_map(&mut tx, owner_id, "Map", "no-alliance-map", None)
        .await
        .unwrap()
        .unwrap();
    let acl = insert_acl(&mut tx, owner_id, "ACL").await.unwrap();
    insert_acl_member(
        &mut tx,
        acl.id,
        MemberType::Alliance,
        Some(99000001),
        None,
        "test",
        AclPermission::Read,
    )
    .await
    .unwrap();
    attach_acl(&mut tx, map.id, acl.id).await.unwrap();
    tx.commit().await.unwrap();

    let perm = effective_permission(&pool, member_id, map.id)
        .await
        .unwrap();
    assert_eq!(perm, None);
}

// ---------------------------------------------------------------------------
// Deny as hard stop
// ---------------------------------------------------------------------------

#[tokio::test]
async fn deny_overrides_grant_on_same_acl() {
    let (_pg, pool) = common::setup_db().await;
    let owner_id = make_account(&pool).await;
    let member_id = make_account(&pool).await;
    let char_id = make_character(&pool, member_id, 4001, 98000001, None).await;

    let mut tx = pool.begin().await.unwrap();
    let map = insert_map(&mut tx, owner_id, "Map", "deny-same-acl", None)
        .await
        .unwrap()
        .unwrap();
    let acl = insert_acl(&mut tx, owner_id, "ACL").await.unwrap();
    // Corp grant + character deny in same ACL.
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
    insert_acl_member(
        &mut tx,
        acl.id,
        MemberType::Character,
        None,
        Some(char_id),
        "test",
        AclPermission::Deny,
    )
    .await
    .unwrap();
    attach_acl(&mut tx, map.id, acl.id).await.unwrap();
    tx.commit().await.unwrap();

    let perm = effective_permission(&pool, member_id, map.id)
        .await
        .unwrap();
    assert_eq!(perm, None, "deny must override corp grant");
}

#[tokio::test]
async fn deny_on_one_acl_overrides_grant_on_another() {
    let (_pg, pool) = common::setup_db().await;
    let owner_id = make_account(&pool).await;
    let member_id = make_account(&pool).await;
    let char_id = make_character(&pool, member_id, 4002, 98000001, None).await;

    let mut tx = pool.begin().await.unwrap();
    let map = insert_map(&mut tx, owner_id, "Map", "deny-cross-acl", None)
        .await
        .unwrap()
        .unwrap();
    let acl1 = insert_acl(&mut tx, owner_id, "Grant ACL").await.unwrap();
    insert_acl_member(
        &mut tx,
        acl1.id,
        MemberType::Corporation,
        Some(98000001),
        None,
        "test",
        AclPermission::ReadWrite,
    )
    .await
    .unwrap();
    let acl2 = insert_acl(&mut tx, owner_id, "Deny ACL").await.unwrap();
    insert_acl_member(
        &mut tx,
        acl2.id,
        MemberType::Character,
        None,
        Some(char_id),
        "test",
        AclPermission::Deny,
    )
    .await
    .unwrap();
    attach_acl(&mut tx, map.id, acl1.id).await.unwrap();
    attach_acl(&mut tx, map.id, acl2.id).await.unwrap();
    tx.commit().await.unwrap();

    let perm = effective_permission(&pool, member_id, map.id)
        .await
        .unwrap();
    assert_eq!(perm, None, "deny on one acl must stop grants from another");
}

// ---------------------------------------------------------------------------
// Ghost character member matches no account
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ghost_character_member_does_not_grant_access_to_any_account() {
    let (_pg, pool) = common::setup_db().await;
    let owner_id = make_account(&pool).await;
    let other_id = make_account(&pool).await;
    make_character(&pool, other_id, 7001, 1000, None).await;

    // Insert a ghost character (account_id = NULL).
    let ghost_char_id: uuid::Uuid = sqlx::query_scalar!(
        r#"
        INSERT INTO eve_character (eve_character_id, name, corporation_id, is_main)
        VALUES (7999999, 'Ghost Pilot', 1000, false)
        RETURNING id
        "#
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let mut tx = pool.begin().await.unwrap();
    let map = insert_map(&mut tx, owner_id, "Map", "ghost-member-map", None)
        .await
        .unwrap()
        .unwrap();
    let acl = insert_acl(&mut tx, owner_id, "ACL").await.unwrap();
    insert_acl_member(
        &mut tx,
        acl.id,
        MemberType::Character,
        None,
        Some(ghost_char_id),
        "test",
        AclPermission::Admin,
    )
    .await
    .unwrap();
    attach_acl(&mut tx, map.id, acl.id).await.unwrap();
    tx.commit().await.unwrap();

    // No account should get access via the ghost member entry.
    let perm = effective_permission(&pool, other_id, map.id).await.unwrap();
    assert_eq!(
        perm, None,
        "ghost character member must not grant access to any account"
    );
}

// ---------------------------------------------------------------------------
// Most-permissive across multiple ACLs
// ---------------------------------------------------------------------------

#[tokio::test]
async fn most_permissive_grant_wins_across_acls() {
    let (_pg, pool) = common::setup_db().await;
    let owner_id = make_account(&pool).await;
    let member_id = make_account(&pool).await;
    let char_id = make_character(&pool, member_id, 5001, 98000001, Some(99000001)).await;

    let mut tx = pool.begin().await.unwrap();
    let map = insert_map(&mut tx, owner_id, "Map", "most-perm-map", None)
        .await
        .unwrap()
        .unwrap();
    // ACL 1: corp read
    let acl1 = insert_acl(&mut tx, owner_id, "Corp ACL").await.unwrap();
    insert_acl_member(
        &mut tx,
        acl1.id,
        MemberType::Corporation,
        Some(98000001),
        None,
        "test",
        AclPermission::Read,
    )
    .await
    .unwrap();
    // ACL 2: character manage
    let acl2 = insert_acl(&mut tx, owner_id, "Char ACL").await.unwrap();
    insert_acl_member(
        &mut tx,
        acl2.id,
        MemberType::Character,
        None,
        Some(char_id),
        "test",
        AclPermission::Manage,
    )
    .await
    .unwrap();
    attach_acl(&mut tx, map.id, acl1.id).await.unwrap();
    attach_acl(&mut tx, map.id, acl2.id).await.unwrap();
    tx.commit().await.unwrap();

    let perm = effective_permission(&pool, member_id, map.id)
        .await
        .unwrap();
    assert_eq!(
        perm,
        Some(Permission::Manage),
        "manage should win over read"
    );
}

#[tokio::test]
async fn multiple_characters_on_account_all_checked() {
    let (_pg, pool) = common::setup_db().await;
    let owner_id = make_account(&pool).await;
    let member_id = make_account(&pool).await;

    let aes_key = common::test_aes_key();
    // Insert main character (no matching ACL entry).
    let mut tx = pool.begin().await.unwrap();
    let _main = insert_character(
        &mut tx,
        &aes_key,
        InsertCharacterData {
            account_id: member_id,
            eve_character_id: 6001,
            name: "Main",
            corporation_id: 1000,
            alliance_id: None,
            is_main: true,
            esi_client_id: "test_client_id",
            access_token: "t",
            refresh_token: "r",
            esi_token_expires_at: Utc::now() + chrono::Duration::hours(1),
        },
    )
    .await
    .unwrap();
    // Insert alt character (matching ACL entry).
    let alt = insert_character(
        &mut tx,
        &aes_key,
        InsertCharacterData {
            account_id: member_id,
            eve_character_id: 6002,
            name: "Alt",
            corporation_id: 98000001,
            alliance_id: None,
            is_main: false,
            esi_client_id: "test_client_id",
            access_token: "t",
            refresh_token: "r",
            esi_token_expires_at: Utc::now() + chrono::Duration::hours(1),
        },
    )
    .await
    .unwrap();

    let map = insert_map(&mut tx, owner_id, "Map", "multi-char-map", None)
        .await
        .unwrap()
        .unwrap();
    let acl = insert_acl(&mut tx, owner_id, "ACL").await.unwrap();
    insert_acl_member(
        &mut tx,
        acl.id,
        MemberType::Character,
        None,
        Some(alt.id),
        "test",
        AclPermission::ReadWrite,
    )
    .await
    .unwrap();
    attach_acl(&mut tx, map.id, acl.id).await.unwrap();
    tx.commit().await.unwrap();

    let perm = effective_permission(&pool, member_id, map.id)
        .await
        .unwrap();
    assert_eq!(
        perm,
        Some(Permission::ReadWrite),
        "alt character grant should apply to account"
    );
}
