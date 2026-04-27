mod common;

use chrono::Utc;
use erbridge_api::{
    db::acl_member::{AclPermission, MemberType, find_members_by_acl},
    services::acl::{
        add_member, create_acl, delete_acl, remove_member, rename_acl, update_member_permission,
    },
    services::auth::{LoginInput, login_or_register},
};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ---------------------------------------------------------------------------
// ESI stub helpers
// ---------------------------------------------------------------------------

/// Stubs `POST /universe/names/` to return a single character entry.
async fn stub_resolve_name(server: &MockServer, eve_id: i64, name: &str) {
    Mock::given(method("POST"))
        .and(path("/universe/names/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            { "id": eve_id, "name": name, "category": "character" }
        ])))
        .mount(server)
        .await;
}

/// Stubs `GET /characters/{id}/` to return corp/alliance info.
async fn stub_char_public_info(
    server: &MockServer,
    eve_id: i64,
    corp_id: i64,
    alliance_id: Option<i64>,
) {
    Mock::given(method("GET"))
        .and(path(format!("/characters/{eve_id}/")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "corporation_id": corp_id,
            "alliance_id": alliance_id,
        })))
        .mount(server)
        .await;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn make_account(pool: &sqlx::PgPool, eve_id: i64, name: &str) -> uuid::Uuid {
    let aes_key = common::test_aes_key();
    login_or_register(
        pool,
        &aes_key,
        LoginInput {
            eve_character_id: eve_id,
            name,
            corporation_id: 98000001,
            alliance_id: None,
            esi_client_id: "test_client_id",
            access_token: "tok",
            refresh_token: "rtok",
            esi_token_expires_at: Utc::now() + chrono::Duration::hours(1),
        },
    )
    .await
    .unwrap()
}

// ---------------------------------------------------------------------------
// create_acl
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_acl_succeeds_and_sets_pending_delete() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool, 10000001, "Creator").await;

    let acl = create_acl(&pool, account_id, "My ACL").await.unwrap();

    assert_eq!(acl.name, "My ACL");
    assert_eq!(acl.owner_account_id, Some(account_id));
    // Freshly created ACL has no maps → pending_delete_at is set.
    assert!(acl.pending_delete_at.is_some());
}

// ---------------------------------------------------------------------------
// rename_acl
// ---------------------------------------------------------------------------

#[tokio::test]
async fn rename_acl_by_owner_succeeds() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool, 10000002, "Owner").await;

    let acl = create_acl(&pool, account_id, "Original").await.unwrap();
    let updated = rename_acl(&pool, acl.id, account_id, "Renamed")
        .await
        .unwrap();

    assert_eq!(updated.name, "Renamed");
}

#[tokio::test]
async fn rename_acl_without_permission_fails() {
    let (_pg, pool) = common::setup_db().await;
    let owner_id = make_account(&pool, 10000003, "Owner").await;
    let other_id = make_account(&pool, 10000004, "Other").await;

    let acl = create_acl(&pool, owner_id, "Private").await.unwrap();
    let result = rename_acl(&pool, acl.id, other_id, "Hijacked").await;

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("insufficient permission")
    );
}

// ---------------------------------------------------------------------------
// delete_acl
// ---------------------------------------------------------------------------

#[tokio::test]
async fn delete_acl_by_owner_succeeds() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool, 10000005, "Owner").await;

    let acl = create_acl(&pool, account_id, "To Delete").await.unwrap();
    delete_acl(&pool, acl.id, account_id).await.unwrap();

    let gone = erbridge_api::db::acl::find_acl_by_id(&pool, acl.id)
        .await
        .unwrap();
    assert!(gone.is_none());
}

#[tokio::test]
async fn delete_acl_without_permission_fails() {
    let (_pg, pool) = common::setup_db().await;
    let owner_id = make_account(&pool, 10000006, "Owner").await;
    let other_id = make_account(&pool, 10000007, "Other").await;

    let acl = create_acl(&pool, owner_id, "Private").await.unwrap();
    let result = delete_acl(&pool, acl.id, other_id).await;

    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// MemberType / AclPermission parse errors (boundary validation)
// ---------------------------------------------------------------------------

#[test]
fn invalid_permission_parse_fails() {
    assert!("superadmin".parse::<AclPermission>().is_err());
}

#[test]
fn invalid_member_type_parse_fails() {
    assert!("faction".parse::<MemberType>().is_err());
}

// ---------------------------------------------------------------------------
// add_member — validation errors
// ---------------------------------------------------------------------------

#[tokio::test]
async fn add_member_manage_on_corporation_fails() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool, 10000010, "Owner").await;
    let acl = create_acl(&pool, account_id, "ACL").await.unwrap();
    let http = reqwest::Client::new();

    let result = add_member(
        &pool,
        &http,
        "http://unused",
        acl.id,
        account_id,
        MemberType::Corporation,
        Some(98000001),
        AclPermission::Manage,
    )
    .await;

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("only valid for character members")
    );
}

#[tokio::test]
async fn add_member_requires_manage_permission() {
    let (_pg, pool) = common::setup_db().await;
    let owner_id = make_account(&pool, 10000011, "Owner").await;
    let outsider_id = make_account(&pool, 10000012, "Outsider").await;

    let acl = create_acl(&pool, owner_id, "ACL").await.unwrap();
    let http = reqwest::Client::new();

    let result = add_member(
        &pool,
        &http,
        "http://unused",
        acl.id,
        outsider_id,
        MemberType::Corporation,
        Some(98000001),
        AclPermission::Read,
    )
    .await;

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("insufficient permission")
    );
}

// ---------------------------------------------------------------------------
// add_member — corporation member (no ESI needed)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn add_corporation_member_succeeds() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool, 10000013, "Owner").await;
    let acl = create_acl(&pool, account_id, "ACL").await.unwrap();
    let http = reqwest::Client::new();

    let member = add_member(
        &pool,
        &http,
        "http://unused",
        acl.id,
        account_id,
        MemberType::Corporation,
        Some(98000001),
        AclPermission::Read,
    )
    .await
    .unwrap();

    assert_eq!(member.member_type, MemberType::Corporation);
    assert_eq!(member.eve_entity_id, Some(98000001));
    assert_eq!(member.permission, AclPermission::Read);
}

#[tokio::test]
async fn add_alliance_member_succeeds() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool, 10000014, "Owner").await;
    let acl = create_acl(&pool, account_id, "ACL").await.unwrap();
    let http = reqwest::Client::new();

    let member = add_member(
        &pool,
        &http,
        "http://unused",
        acl.id,
        account_id,
        MemberType::Alliance,
        Some(99000001),
        AclPermission::Deny,
    )
    .await
    .unwrap();

    assert_eq!(member.member_type, MemberType::Alliance);
    assert_eq!(member.permission, AclPermission::Deny);
}

// ---------------------------------------------------------------------------
// add_member — duplicate detection
// ---------------------------------------------------------------------------

#[tokio::test]
async fn duplicate_corporation_member_fails() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool, 10000015, "Owner").await;
    let acl = create_acl(&pool, account_id, "ACL").await.unwrap();
    let http = reqwest::Client::new();

    add_member(
        &pool,
        &http,
        "http://unused",
        acl.id,
        account_id,
        MemberType::Corporation,
        Some(98000001),
        AclPermission::Read,
    )
    .await
    .unwrap();

    let result = add_member(
        &pool,
        &http,
        "http://unused",
        acl.id,
        account_id,
        MemberType::Corporation,
        Some(98000001),
        AclPermission::ReadWrite,
    )
    .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("duplicate_member"));
}

// ---------------------------------------------------------------------------
// add_member — character ghost creation via ESI
// ---------------------------------------------------------------------------

#[tokio::test]
async fn add_character_member_creates_ghost_and_returns_member() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool, 10000016, "Owner").await;
    let acl = create_acl(&pool, account_id, "ACL").await.unwrap();

    let esi = MockServer::start().await;
    stub_resolve_name(&esi, 20000001, "Ghost Pilot").await;
    stub_char_public_info(&esi, 20000001, 98000099, None).await;
    let http = reqwest::Client::new();

    let member = add_member(
        &pool,
        &http,
        &esi.uri(),
        acl.id,
        account_id,
        MemberType::Character,
        Some(20000001),
        AclPermission::Read,
    )
    .await
    .unwrap();

    assert_eq!(member.member_type, MemberType::Character);
    assert_eq!(member.permission, AclPermission::Read);
    assert!(member.character_id.is_some());

    // Ghost row exists in eve_character with no account.
    let ghost = sqlx::query!(
        "SELECT account_id FROM eve_character WHERE id = $1",
        member.character_id
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(ghost.account_id.is_none());
}

#[tokio::test]
async fn add_character_member_reuses_existing_character_row() {
    let (_pg, pool) = common::setup_db().await;
    let owner_id = make_account(&pool, 10000017, "Owner").await;
    // 10000017 already has a character row with account_id = owner_id.
    let char_eve_id = 10000017i64;

    let acl = create_acl(&pool, owner_id, "ACL").await.unwrap();
    // No ESI stub needed — character already exists.
    let http = reqwest::Client::new();

    let member = add_member(
        &pool,
        &http,
        "http://unused",
        acl.id,
        owner_id,
        MemberType::Character,
        Some(char_eve_id),
        AclPermission::Read,
    )
    .await
    .unwrap();

    assert_eq!(member.member_type, MemberType::Character);

    // Should be exactly one ghost created (none, since char already existed).
    let count: i64 = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM eve_character WHERE eve_character_id = $1",
        char_eve_id
    )
    .fetch_one(&pool)
    .await
    .unwrap()
    .unwrap_or(0);
    assert_eq!(count, 1);
}

#[tokio::test]
async fn duplicate_character_member_fails() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool, 10000018, "Owner").await;
    let acl = create_acl(&pool, account_id, "ACL").await.unwrap();

    let esi = MockServer::start().await;
    stub_resolve_name(&esi, 20000002, "Ghost Pilot 2").await;
    stub_char_public_info(&esi, 20000002, 98000099, None).await;
    // Second call should not need ESI (row already exists), but stub it anyway.
    stub_resolve_name(&esi, 20000002, "Ghost Pilot 2").await;
    stub_char_public_info(&esi, 20000002, 98000099, None).await;
    let http = reqwest::Client::new();

    add_member(
        &pool,
        &http,
        &esi.uri(),
        acl.id,
        account_id,
        MemberType::Character,
        Some(20000002),
        AclPermission::Read,
    )
    .await
    .unwrap();

    let result = add_member(
        &pool,
        &http,
        &esi.uri(),
        acl.id,
        account_id,
        MemberType::Character,
        Some(20000002),
        AclPermission::ReadWrite,
    )
    .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("duplicate_member"));
}

// ---------------------------------------------------------------------------
// update_member_permission
// ---------------------------------------------------------------------------

#[tokio::test]
async fn update_member_permission_succeeds() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool, 10000019, "Owner").await;
    let acl = create_acl(&pool, account_id, "ACL").await.unwrap();
    let http = reqwest::Client::new();

    let member = add_member(
        &pool,
        &http,
        "http://unused",
        acl.id,
        account_id,
        MemberType::Corporation,
        Some(98000001),
        AclPermission::Read,
    )
    .await
    .unwrap();

    let updated = update_member_permission(
        &pool,
        acl.id,
        member.id,
        account_id,
        AclPermission::ReadWrite,
    )
    .await
    .unwrap();

    assert_eq!(updated.permission, AclPermission::ReadWrite);
}

#[tokio::test]
async fn update_member_permission_manage_on_corp_fails() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool, 10000021, "Owner").await;
    let acl = create_acl(&pool, account_id, "ACL").await.unwrap();
    let http = reqwest::Client::new();

    let member = add_member(
        &pool,
        &http,
        "http://unused",
        acl.id,
        account_id,
        MemberType::Corporation,
        Some(98000001),
        AclPermission::Read,
    )
    .await
    .unwrap();

    let result =
        update_member_permission(&pool, acl.id, member.id, account_id, AclPermission::Manage).await;

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("only valid for character members")
    );
}

#[tokio::test]
async fn update_member_permission_wrong_acl_fails() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool, 10000022, "Owner").await;
    let acl_a = create_acl(&pool, account_id, "ACL A").await.unwrap();
    let acl_b = create_acl(&pool, account_id, "ACL B").await.unwrap();
    let http = reqwest::Client::new();

    let member = add_member(
        &pool,
        &http,
        "http://unused",
        acl_a.id,
        account_id,
        MemberType::Corporation,
        Some(98000001),
        AclPermission::Read,
    )
    .await
    .unwrap();

    // Try to update acl_a's member using acl_b's ID.
    let result = update_member_permission(
        &pool,
        acl_b.id,
        member.id,
        account_id,
        AclPermission::ReadWrite,
    )
    .await;

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("does not belong to acl")
    );
}

// ---------------------------------------------------------------------------
// remove_member
// ---------------------------------------------------------------------------

#[tokio::test]
async fn remove_member_succeeds() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool, 10000023, "Owner").await;
    let acl = create_acl(&pool, account_id, "ACL").await.unwrap();
    let http = reqwest::Client::new();

    let member = add_member(
        &pool,
        &http,
        "http://unused",
        acl.id,
        account_id,
        MemberType::Corporation,
        Some(98000001),
        AclPermission::Read,
    )
    .await
    .unwrap();

    remove_member(&pool, acl.id, member.id, account_id)
        .await
        .unwrap();

    let members = find_members_by_acl(&pool, acl.id).await.unwrap();
    assert!(members.is_empty());
}

#[tokio::test]
async fn remove_member_wrong_acl_fails() {
    let (_pg, pool) = common::setup_db().await;
    let account_id = make_account(&pool, 10000024, "Owner").await;
    let acl_a = create_acl(&pool, account_id, "ACL A").await.unwrap();
    let acl_b = create_acl(&pool, account_id, "ACL B").await.unwrap();
    let http = reqwest::Client::new();

    let member = add_member(
        &pool,
        &http,
        "http://unused",
        acl_a.id,
        account_id,
        MemberType::Corporation,
        Some(98000001),
        AclPermission::Read,
    )
    .await
    .unwrap();

    let result = remove_member(&pool, acl_b.id, member.id, account_id).await;

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("does not belong to acl")
    );
}

// ---------------------------------------------------------------------------
// Permission escalation: character member holding manage can add/remove
// ---------------------------------------------------------------------------

#[tokio::test]
async fn character_with_manage_permission_can_add_members() {
    let (_pg, pool) = common::setup_db().await;
    let owner_id = make_account(&pool, 10000025, "Owner").await;
    let manager_id = make_account(&pool, 10000026, "Manager").await;
    let acl = create_acl(&pool, owner_id, "ACL").await.unwrap();
    let http = reqwest::Client::new();

    // Owner adds manager as a character member with manage.
    let manager_eve_id = 10000026i64;
    add_member(
        &pool,
        &http,
        "http://unused",
        acl.id,
        owner_id,
        MemberType::Character,
        Some(manager_eve_id),
        AclPermission::Manage,
    )
    .await
    .unwrap();

    // Manager should be able to add a corp member.
    let result = add_member(
        &pool,
        &http,
        "http://unused",
        acl.id,
        manager_id,
        MemberType::Corporation,
        Some(98000099),
        AclPermission::Read,
    )
    .await;

    assert!(
        result.is_ok(),
        "manager should be able to add members: {:?}",
        result.err()
    );
}
