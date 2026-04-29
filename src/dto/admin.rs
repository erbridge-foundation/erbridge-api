use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::account::Account;
use crate::db::acl::Acl;
use crate::db::map::Map;

#[derive(Debug, Serialize, Deserialize)]
pub struct AdminMapResponse {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub owner_account_id: Option<Uuid>,
    pub description: Option<String>,
    pub deleted: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<Map> for AdminMapResponse {
    fn from(m: Map) -> Self {
        Self {
            id: m.id,
            name: m.name,
            slug: m.slug,
            owner_account_id: m.owner_account_id,
            description: m.description,
            deleted: m.deleted,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct AdminMapListResponse {
    pub maps: Vec<AdminMapResponse>,
}

#[derive(Debug, Deserialize)]
pub struct ChangeMapOwnerRequest {
    pub new_owner_account_id: Uuid,
}

/// Admin ACL view. Deliberately omits members — DECISIONS_context.md "Capability
/// boundaries" forbids exposing ACL members to the admin role.
#[derive(Debug, Serialize)]
pub struct AdminAclResponse {
    pub id: Uuid,
    pub name: String,
    pub owner_account_id: Option<Uuid>,
    pub pending_delete_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<Acl> for AdminAclResponse {
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

#[derive(Debug, Serialize)]
pub struct AdminAclListResponse {
    pub acls: Vec<AdminAclResponse>,
}

#[derive(Debug, Deserialize)]
pub struct ChangeAclOwnerRequest {
    pub new_owner_account_id: Uuid,
}

#[derive(Debug, Default, Deserialize)]
pub struct BlockEveCharacterRequest {
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BlockedEveCharacterResponse {
    pub eve_character_id: i64,
    pub reason: Option<String>,
    pub blocked_at: DateTime<Utc>,
}

impl From<crate::db::account::BlockedEveCharacter> for BlockedEveCharacterResponse {
    fn from(b: crate::db::account::BlockedEveCharacter) -> Self {
        Self {
            eve_character_id: b.eve_character_id,
            reason: b.reason,
            blocked_at: b.blocked_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct BlockedEveCharacterListResponse {
    pub blocked: Vec<BlockedEveCharacterResponse>,
}

#[derive(Debug, Serialize)]
pub struct AdminAccountResponse {
    pub id: Uuid,
    pub status: String,
    pub is_server_admin: bool,
    pub delete_requested_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<Account> for AdminAccountResponse {
    fn from(a: Account) -> Self {
        Self {
            id: a.id,
            status: a.status.to_string(),
            is_server_admin: a.is_server_admin,
            delete_requested_at: a.delete_requested_at,
            created_at: a.created_at,
            updated_at: a.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct AdminAccountListResponse {
    pub accounts: Vec<AdminAccountResponse>,
}

#[derive(Debug, Deserialize)]
pub struct AuditLogQueryParams {
    pub event_type: Option<String>,
    pub actor: Option<Uuid>,
    pub before: Option<DateTime<Utc>>,
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct AuditLogEntryResponse {
    pub id: Uuid,
    pub occurred_at: DateTime<Utc>,
    pub actor_account_id: Option<Uuid>,
    pub event_type: String,
    pub details: serde_json::Value,
}

impl From<crate::audit::AuditLogEntry> for AuditLogEntryResponse {
    fn from(e: crate::audit::AuditLogEntry) -> Self {
        Self {
            id: e.id,
            occurred_at: e.occurred_at,
            actor_account_id: e.actor_account_id,
            event_type: e.event_type,
            details: e.details,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct AuditLogListResponse {
    pub entries: Vec<AuditLogEntryResponse>,
    pub next_before: Option<DateTime<Utc>>,
}
