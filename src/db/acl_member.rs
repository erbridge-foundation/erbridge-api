use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use sqlx::{PgPool, Postgres, Transaction};
use strum::{Display, EnumString};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumString, Deserialize)]
#[strum(serialize_all = "snake_case")]
#[serde(try_from = "String")]
pub enum MemberType {
    Character,
    Corporation,
    Alliance,
}

impl TryFrom<String> for MemberType {
    type Error = strum::ParseError;
    fn try_from(s: String) -> std::result::Result<Self, Self::Error> {
        s.parse()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumString, Deserialize)]
#[strum(serialize_all = "snake_case")]
#[serde(try_from = "String")]
pub enum AclPermission {
    Read,
    ReadWrite,
    Manage,
    Admin,
    Deny,
}

impl TryFrom<String> for AclPermission {
    type Error = strum::ParseError;
    fn try_from(s: String) -> std::result::Result<Self, Self::Error> {
        s.parse()
    }
}

#[derive(Debug)]
pub struct AclMember {
    pub id: Uuid,
    pub acl_id: Uuid,
    pub member_type: MemberType,
    pub eve_entity_id: Option<i64>,
    pub character_id: Option<Uuid>,
    pub name: String,
    pub permission: AclPermission,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

struct AclMemberRow {
    id: Uuid,
    acl_id: Uuid,
    member_type: String,
    eve_entity_id: Option<i64>,
    character_id: Option<Uuid>,
    name: String,
    permission: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

// this is still required to avoid using native Postgres enums
impl TryFrom<AclMemberRow> for AclMember {
    type Error = anyhow::Error;

    fn try_from(row: AclMemberRow) -> Result<Self> {
        Ok(Self {
            id: row.id,
            acl_id: row.acl_id,
            member_type: row
                .member_type
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid member_type in db: {}", row.member_type))?,
            eve_entity_id: row.eve_entity_id,
            character_id: row.character_id,
            name: row.name,
            permission: row
                .permission
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid permission in db: {}", row.permission))?,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

pub async fn insert_acl_member(
    tx: &mut Transaction<'_, Postgres>,
    acl_id: Uuid,
    member_type: MemberType,
    eve_entity_id: Option<i64>,
    character_id: Option<Uuid>,
    name: &str,
    permission: AclPermission,
) -> Result<AclMember> {
    sqlx::query_as!(
        AclMemberRow,
        r#"
        INSERT INTO acl_member (acl_id, member_type, eve_entity_id, character_id, name, permission)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id, acl_id, member_type,
                  COALESCE(eve_entity_id, (SELECT eve_character_id FROM eve_character WHERE id = character_id)) AS "eve_entity_id",
                  character_id, name, permission, created_at, updated_at
        "#,
        acl_id,
        member_type.to_string(),
        eve_entity_id,
        character_id,
        name,
        permission.to_string(),
    )
    .fetch_one(&mut **tx)
    .await
    .context("failed to insert acl member")?
    .try_into()
}

pub async fn find_members_by_acl(pool: &PgPool, acl_id: Uuid) -> Result<Vec<AclMember>> {
    let rows = sqlx::query_as!(
        AclMemberRow,
        r#"
        SELECT id, acl_id, member_type,
               COALESCE(eve_entity_id, (SELECT eve_character_id FROM eve_character WHERE id = character_id)) AS "eve_entity_id",
               character_id, name, permission, created_at, updated_at
        FROM acl_member
        WHERE acl_id = $1
        ORDER BY created_at ASC
        "#,
        acl_id,
    )
    .fetch_all(pool)
    .await
    .context("failed to fetch acl members")?;

    rows.into_iter().map(AclMember::try_from).collect()
}

pub async fn find_member_by_id(pool: &PgPool, member_id: Uuid) -> Result<Option<AclMember>> {
    let row = sqlx::query_as!(
        AclMemberRow,
        r#"
        SELECT id, acl_id, member_type,
               COALESCE(eve_entity_id, (SELECT eve_character_id FROM eve_character WHERE id = character_id)) AS "eve_entity_id",
               character_id, name, permission, created_at, updated_at
        FROM acl_member
        WHERE id = $1
        "#,
        member_id,
    )
    .fetch_optional(pool)
    .await
    .context("failed to fetch acl member by id")?;

    row.map(AclMember::try_from).transpose()
}

pub async fn update_member_permission(
    pool: &PgPool,
    member_id: Uuid,
    permission: AclPermission,
) -> Result<AclMember> {
    sqlx::query_as!(
        AclMemberRow,
        r#"
        UPDATE acl_member
        SET permission = $2, updated_at = now()
        WHERE id = $1
        RETURNING id, acl_id, member_type,
                  COALESCE(eve_entity_id, (SELECT eve_character_id FROM eve_character WHERE id = character_id)) AS "eve_entity_id",
                  character_id, name, permission, created_at, updated_at
        "#,
        member_id,
        permission.to_string(),
    )
    .fetch_one(pool)
    .await
    .context("failed to update acl member permission")?
    .try_into()
}

pub async fn update_member_permission_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    member_id: Uuid,
    permission: AclPermission,
) -> Result<AclMember> {
    sqlx::query_as!(
        AclMemberRow,
        r#"
        UPDATE acl_member
        SET permission = $2, updated_at = now()
        WHERE id = $1
        RETURNING id, acl_id, member_type,
                  COALESCE(eve_entity_id, (SELECT eve_character_id FROM eve_character WHERE id = character_id)) AS "eve_entity_id",
                  character_id, name, permission, created_at, updated_at
        "#,
        member_id,
        permission.to_string(),
    )
    .fetch_one(&mut **tx)
    .await
    .context("failed to update acl member permission")?
    .try_into()
}

pub async fn delete_member(pool: &PgPool, member_id: Uuid) -> Result<()> {
    sqlx::query!("DELETE FROM acl_member WHERE id = $1", member_id)
        .execute(pool)
        .await
        .context("failed to delete acl member")?;
    Ok(())
}

pub async fn delete_member_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    member_id: Uuid,
) -> Result<()> {
    sqlx::query!("DELETE FROM acl_member WHERE id = $1", member_id)
        .execute(&mut **tx)
        .await
        .context("failed to delete acl member")?;
    Ok(())
}
