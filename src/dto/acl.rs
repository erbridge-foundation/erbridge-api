use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::db::{acl::Acl, acl_member::AclMember};

// ---------------------------------------------------------------------------
// Responses
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
pub struct AclResponse {
    pub id: Uuid,
    pub name: String,
    pub owner_account_id: Option<Uuid>,
    pub pending_delete_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<Acl> for AclResponse {
    fn from(a: Acl) -> Self {
        Self {
            id: a.id,
            name: a.name,
            owner_account_id: a.owner_account_id,
            pending_delete_at: a.pending_delete_at,
            created_at: a.created_at,
            updated_at: a.updated_at,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct AclListResponse {
    pub acls: Vec<AclResponse>,
}

#[derive(Serialize, Deserialize)]
pub struct AclMemberResponse {
    pub id: Uuid,
    pub acl_id: Uuid,
    pub member_type: String,
    pub eve_entity_id: Option<i64>,
    pub character_id: Option<Uuid>,
    pub name: String,
    pub permission: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<AclMember> for AclMemberResponse {
    fn from(m: AclMember) -> Self {
        Self {
            id: m.id,
            acl_id: m.acl_id,
            member_type: m.member_type.to_string(),
            eve_entity_id: m.eve_entity_id,
            character_id: m.character_id,
            name: m.name,
            permission: m.permission.to_string(),
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct AclMemberListResponse {
    pub members: Vec<AclMemberResponse>,
}

// ---------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------

#[derive(Deserialize, Validate)]
pub struct AclNameRequest {
    #[validate(length(min = 1, max = 100))]
    pub name: String,
}

pub type CreateAclRequest = AclNameRequest;
pub type RenameAclRequest = AclNameRequest;

#[derive(Deserialize)]
pub struct AddMemberRequest {
    pub member_type: String,
    pub eve_entity_id: Option<i64>,
    pub permission: String,
}

#[derive(Deserialize)]
pub struct UpdateMemberRequest {
    pub permission: String,
}
