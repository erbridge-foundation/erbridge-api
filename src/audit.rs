use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub enum AuditEvent {
    AccountRegistered {
        account_id: Uuid,
        eve_character_id: i64,
        character_name: String,
    },
    AccountDeletionRequested {
        account_id: Uuid,
    },
    AccountReactivated {
        account_id: Uuid,
    },
    AccountPurged {
        account_id: Uuid,
    },
    CharacterAdded {
        account_id: Uuid,
        eve_character_id: i64,
        character_name: String,
    },
    CharacterRemoved {
        account_id: Uuid,
        eve_character_id: i64,
    },
    CharacterSetMain {
        account_id: Uuid,
        eve_character_id: i64,
    },
    GhostCharacterClaimed {
        account_id: Uuid,
        eve_character_id: i64,
        character_name: String,
    },
    MapCreated {
        account_id: Uuid,
        map_id: Uuid,
        name: String,
    },
    MapDeleted {
        account_id: Uuid,
        map_id: Uuid,
        name: String,
    },
    AclCreated {
        account_id: Uuid,
        acl_id: Uuid,
        name: String,
    },
    AclRenamed {
        account_id: Uuid,
        acl_id: Uuid,
        old_name: String,
        new_name: String,
    },
    AclDeleted {
        account_id: Uuid,
        acl_id: Uuid,
        name: String,
    },
    AclMemberAdded {
        account_id: Uuid,
        acl_id: Uuid,
        member_id: Uuid,
        member_type: String,
        permission: String,
    },
    AclMemberPermissionChanged {
        account_id: Uuid,
        acl_id: Uuid,
        member_id: Uuid,
        permission: String,
    },
    AclMemberRemoved {
        account_id: Uuid,
        acl_id: Uuid,
        member_id: Uuid,
    },
    AclAttachedToMap {
        account_id: Uuid,
        map_id: Uuid,
        acl_id: Uuid,
    },
    AclDetachedFromMap {
        account_id: Uuid,
        map_id: Uuid,
        acl_id: Uuid,
    },
    ApiKeyCreated {
        account_id: Uuid,
        key_id: Uuid,
        name: String,
    },
    ApiKeyRevoked {
        account_id: Uuid,
        key_id: Uuid,
    },
    ServerAdminGranted {
        account_id: Uuid,
        source: ServerAdminGrantSource,
    },
    ServerAdminRevoked {
        account_id: Uuid,
    },
    AdminMapOwnershipChanged {
        map_id: Uuid,
        old_owner: Uuid,
        new_owner: Uuid,
    },
    AdminMapHardDeleted {
        map_id: Uuid,
        name: String,
    },
    AdminAclOwnershipChanged {
        acl_id: Uuid,
        old_owner: Uuid,
        new_owner: Uuid,
    },
    AdminAclHardDeleted {
        acl_id: Uuid,
        name: String,
    },
    EveCharacterBlocked {
        eve_character_id: i64,
        reason: Option<String>,
    },
    EveCharacterUnblocked {
        eve_character_id: i64,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum ServerAdminGrantSource {
    FirstAccountBootstrap,
    AdminGrant,
}

impl ServerAdminGrantSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FirstAccountBootstrap => "first_account_bootstrap",
            Self::AdminGrant => "admin_grant",
        }
    }
}

impl AuditEvent {
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::AccountRegistered { .. } => "account_registered",
            Self::AccountDeletionRequested { .. } => "account_deletion_requested",
            Self::AccountReactivated { .. } => "account_reactivated",
            Self::AccountPurged { .. } => "account_purged",
            Self::CharacterAdded { .. } => "character_added",
            Self::CharacterRemoved { .. } => "character_removed",
            Self::CharacterSetMain { .. } => "character_set_main",
            Self::GhostCharacterClaimed { .. } => "ghost_character_claimed",
            Self::MapCreated { .. } => "map_created",
            Self::MapDeleted { .. } => "map_deleted",
            Self::AclCreated { .. } => "acl_created",
            Self::AclRenamed { .. } => "acl_renamed",
            Self::AclDeleted { .. } => "acl_deleted",
            Self::AclMemberAdded { .. } => "acl_member_added",
            Self::AclMemberPermissionChanged { .. } => "acl_member_permission_changed",
            Self::AclMemberRemoved { .. } => "acl_member_removed",
            Self::AclAttachedToMap { .. } => "acl_attached_to_map",
            Self::AclDetachedFromMap { .. } => "acl_detached_from_map",
            Self::ApiKeyCreated { .. } => "api_key_created",
            Self::ApiKeyRevoked { .. } => "api_key_revoked",
            Self::ServerAdminGranted { .. } => "server_admin_granted",
            Self::ServerAdminRevoked { .. } => "server_admin_revoked",
            Self::AdminMapOwnershipChanged { .. } => "admin_map_ownership_changed",
            Self::AdminMapHardDeleted { .. } => "admin_map_hard_deleted",
            Self::AdminAclOwnershipChanged { .. } => "admin_acl_ownership_changed",
            Self::AdminAclHardDeleted { .. } => "admin_acl_hard_deleted",
            Self::EveCharacterBlocked { .. } => "eve_character_blocked",
            Self::EveCharacterUnblocked { .. } => "eve_character_unblocked",
        }
    }

    pub fn details(&self) -> Value {
        match self {
            // actor is NULL for registration — account_id not in actor column so include it here.
            Self::AccountRegistered {
                account_id,
                eve_character_id,
                character_name,
            } => json!({
                "account_id": account_id,
                "eve_character_id": eve_character_id,
                "character_name": character_name,
            }),
            // actor carries the account — no need to repeat it.
            Self::AccountDeletionRequested { .. } => json!({}),
            // actor is NULL for purge — include account_id so it's not lost.
            Self::AccountPurged { account_id } => json!({ "account_id": account_id }),
            // actor == account_id (self-reactivation) — include for clarity since actor is self.
            Self::AccountReactivated { account_id } => json!({ "account_id": account_id }),
            Self::CharacterAdded {
                eve_character_id,
                character_name,
                ..
            } => json!({
                "eve_character_id": eve_character_id,
                "character_name": character_name,
            }),
            Self::CharacterRemoved {
                eve_character_id, ..
            } => json!({
                "eve_character_id": eve_character_id,
            }),
            Self::CharacterSetMain {
                eve_character_id, ..
            } => json!({
                "eve_character_id": eve_character_id,
            }),
            // actor is NULL for login ghost-claim (no session yet) — include account_id.
            // actor is set for attach ghost-claim — but include for consistency.
            Self::GhostCharacterClaimed {
                account_id,
                eve_character_id,
                character_name,
            } => json!({
                "account_id": account_id,
                "eve_character_id": eve_character_id,
                "character_name": character_name,
            }),
            // actor carries account_id; include map_id and name for context.
            Self::MapCreated { map_id, name, .. } => json!({
                "map_id": map_id,
                "name": name,
            }),
            // actor carries account_id; include map_id and name so the event is queryable.
            Self::MapDeleted { map_id, name, .. } => json!({
                "map_id": map_id,
                "name": name,
            }),
            Self::AclCreated { acl_id, name, .. } => json!({
                "acl_id": acl_id,
                "name": name,
            }),
            Self::AclRenamed {
                acl_id,
                old_name,
                new_name,
                ..
            } => json!({
                "acl_id": acl_id,
                "old_name": old_name,
                "new_name": new_name,
            }),
            Self::AclDeleted { acl_id, name, .. } => json!({
                "acl_id": acl_id,
                "name": name,
            }),
            Self::AclMemberAdded {
                acl_id,
                member_id,
                member_type,
                permission,
                ..
            } => json!({
                "acl_id": acl_id,
                "member_id": member_id,
                "member_type": member_type,
                "permission": permission,
            }),
            Self::AclMemberPermissionChanged {
                acl_id,
                member_id,
                permission,
                ..
            } => json!({
                "acl_id": acl_id,
                "member_id": member_id,
                "permission": permission,
            }),
            Self::AclMemberRemoved {
                acl_id, member_id, ..
            } => json!({
                "acl_id": acl_id,
                "member_id": member_id,
            }),
            Self::AclAttachedToMap { map_id, acl_id, .. } => json!({
                "map_id": map_id,
                "acl_id": acl_id,
            }),
            Self::AclDetachedFromMap { map_id, acl_id, .. } => json!({
                "map_id": map_id,
                "acl_id": acl_id,
            }),
            // actor carries account_id; include key_id and name for context.
            Self::ApiKeyCreated { key_id, name, .. } => json!({
                "key_id": key_id,
                "name": name,
            }),
            // actor carries account_id; include key_id so the event is queryable.
            Self::ApiKeyRevoked { key_id, .. } => json!({
                "key_id": key_id,
            }),
            // actor is NULL for first-account bootstrap, set for admin_grant —
            // include account_id so it's not lost in the bootstrap case.
            Self::ServerAdminGranted { account_id, source } => json!({
                "account_id": account_id,
                "source": source.as_str(),
            }),
            // actor is the admin performing the action; include target account_id.
            Self::ServerAdminRevoked { account_id } => json!({ "account_id": account_id }),
            Self::AdminMapOwnershipChanged {
                map_id,
                old_owner,
                new_owner,
            } => json!({
                "map_id": map_id,
                "old_owner": old_owner,
                "new_owner": new_owner,
            }),
            Self::AdminMapHardDeleted { map_id, name } => json!({
                "map_id": map_id,
                "name": name,
            }),
            Self::AdminAclOwnershipChanged {
                acl_id,
                old_owner,
                new_owner,
            } => json!({
                "acl_id": acl_id,
                "old_owner": old_owner,
                "new_owner": new_owner,
            }),
            Self::AdminAclHardDeleted { acl_id, name } => json!({
                "acl_id": acl_id,
                "name": name,
            }),
            Self::EveCharacterBlocked {
                eve_character_id,
                reason,
            } => json!({
                "eve_character_id": eve_character_id,
                "reason": reason,
            }),
            Self::EveCharacterUnblocked { eve_character_id } => json!({
                "eve_character_id": eve_character_id,
            }),
        }
    }
}

/// A row read back from the `audit_log` table — used by the admin
/// audit-log read endpoint.
#[derive(Debug, Clone)]
pub struct AuditLogEntry {
    pub id: Uuid,
    pub occurred_at: DateTime<Utc>,
    pub actor_account_id: Option<Uuid>,
    pub event_type: String,
    pub details: Value,
}

/// Reads audit log entries newest-first, optionally filtered by
/// `event_type`, `actor`, and a keyset cursor `before` (`occurred_at < before`).
/// `limit` is the maximum number of rows to return; the caller is expected to
/// have already clamped it. All filters are bound parameters — no string
/// interpolation, so there is no SQL injection surface.
pub async fn list_audit_log(
    pool: &PgPool,
    event_type: Option<&str>,
    actor: Option<Uuid>,
    before: Option<DateTime<Utc>>,
    limit: i64,
) -> Result<Vec<AuditLogEntry>> {
    let rows = sqlx::query!(
        r#"
        SELECT id, occurred_at, actor_account_id, event_type, details
        FROM audit_log
        WHERE ($1::TEXT IS NULL        OR event_type       = $1)
          AND ($2::UUID IS NULL        OR actor_account_id = $2)
          AND ($3::TIMESTAMPTZ IS NULL OR occurred_at      < $3)
        ORDER BY occurred_at DESC
        LIMIT $4
        "#,
        event_type,
        actor,
        before,
        limit,
    )
    .fetch_all(pool)
    .await
    .context("failed to read audit_log")?;

    Ok(rows
        .into_iter()
        .map(|r| AuditLogEntry {
            id: r.id,
            occurred_at: r.occurred_at,
            actor_account_id: r.actor_account_id,
            event_type: r.event_type,
            details: r.details,
        })
        .collect())
}

/// Writes a single audit event within an existing transaction.
pub async fn record_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    actor: Option<Uuid>,
    event: AuditEvent,
) -> Result<()> {
    let event_type = event.event_type();
    let details = event.details();

    sqlx::query!(
        r#"
        INSERT INTO audit_log (actor_account_id, event_type, details)
        VALUES ($1, $2, $3)
        "#,
        actor,
        event_type,
        details,
    )
    .execute(&mut **tx)
    .await
    .context("failed to insert audit log entry")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn test_uuid() -> Uuid {
        Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap()
    }

    #[test]
    fn account_registered_serialises_correctly() {
        let id = test_uuid();
        let event = AuditEvent::AccountRegistered {
            account_id: id,
            eve_character_id: 123456789,
            character_name: "Test Pilot".into(),
        };
        assert_eq!(event.event_type(), "account_registered");
        let d = event.details();
        assert_eq!(d["account_id"], id.to_string());
        assert_eq!(d["eve_character_id"], 123456789i64);
        assert_eq!(d["character_name"], "Test Pilot");
    }

    #[test]
    fn account_deletion_requested_serialises_correctly() {
        let id = test_uuid();
        let event = AuditEvent::AccountDeletionRequested { account_id: id };
        assert_eq!(event.event_type(), "account_deletion_requested");
        // account_id is carried by actor_account_id column, not repeated in details.
        assert!(event.details().as_object().unwrap().is_empty());
    }

    #[test]
    fn account_reactivated_serialises_correctly() {
        let id = test_uuid();
        let event = AuditEvent::AccountReactivated { account_id: id };
        assert_eq!(event.event_type(), "account_reactivated");
        assert_eq!(event.details()["account_id"], id.to_string());
    }

    #[test]
    fn account_purged_serialises_correctly() {
        let id = test_uuid();
        let event = AuditEvent::AccountPurged { account_id: id };
        assert_eq!(event.event_type(), "account_purged");
        assert_eq!(event.details()["account_id"], id.to_string());
    }

    #[test]
    fn character_added_serialises_correctly() {
        let id = test_uuid();
        let event = AuditEvent::CharacterAdded {
            account_id: id,
            eve_character_id: 123456789,
            character_name: "Test Character".into(),
        };
        assert!(event.details().get("account_id").is_none());
        assert_eq!(event.details()["eve_character_id"], 123456789i64);
    }

    #[test]
    fn character_removed_serialises_correctly() {
        let id = test_uuid();
        let event = AuditEvent::CharacterRemoved {
            account_id: id,
            eve_character_id: 42,
        };
        assert_eq!(event.event_type(), "character_removed");
        assert_eq!(event.details()["eve_character_id"], 42i64);
        assert!(event.details().get("account_id").is_none());
    }

    #[test]
    fn character_set_main_serialises_correctly() {
        let id = test_uuid();
        let event = AuditEvent::CharacterSetMain {
            account_id: id,
            eve_character_id: 99,
        };
        assert_eq!(event.event_type(), "character_set_main");
        assert_eq!(event.details()["eve_character_id"], 99i64);
        assert!(event.details().get("account_id").is_none());
    }

    #[test]
    fn map_created_serialises_correctly() {
        let account_id = test_uuid();
        let map_id = Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap();
        let event = AuditEvent::MapCreated {
            account_id,
            map_id,
            name: "Wormhole Chain Alpha".into(),
        };
        assert_eq!(event.event_type(), "map_created");
        let d = event.details();
        assert_eq!(d["map_id"], map_id.to_string());
        assert_eq!(d["name"], "Wormhole Chain Alpha");
        assert!(d.get("account_id").is_none());
    }

    #[test]
    fn map_deleted_serialises_correctly() {
        let account_id = test_uuid();
        let map_id = Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap();
        let event = AuditEvent::MapDeleted {
            account_id,
            map_id,
            name: "Test Map".into(),
        };
        assert_eq!(event.event_type(), "map_deleted");
        let d = event.details();
        assert_eq!(d["map_id"], map_id.to_string());
        assert_eq!(d["name"], "Test Map");
        assert!(d.get("account_id").is_none());
    }

    #[test]
    fn ghost_character_claimed_serialises_correctly() {
        let id = test_uuid();
        let event = AuditEvent::GhostCharacterClaimed {
            account_id: id,
            eve_character_id: 7,
            character_name: "Ghost Pilot".into(),
        };
        assert_eq!(event.event_type(), "ghost_character_claimed");
        let d = event.details();
        assert_eq!(d["account_id"], id.to_string());
        assert_eq!(d["eve_character_id"], 7i64);
        assert_eq!(d["character_name"], "Ghost Pilot");
    }

    #[test]
    fn api_key_created_serialises_correctly() {
        let account_id = test_uuid();
        let key_id = Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap();
        let event = AuditEvent::ApiKeyCreated {
            account_id,
            key_id,
            name: "My App".into(),
        };
        assert_eq!(event.event_type(), "api_key_created");
        let d = event.details();
        assert_eq!(d["key_id"], key_id.to_string());
        assert_eq!(d["name"], "My App");
        assert!(d.get("account_id").is_none());
    }

    #[test]
    fn server_admin_granted_serialises_correctly() {
        let id = test_uuid();
        let event = AuditEvent::ServerAdminGranted {
            account_id: id,
            source: ServerAdminGrantSource::FirstAccountBootstrap,
        };
        assert_eq!(event.event_type(), "server_admin_granted");
        let d = event.details();
        assert_eq!(d["account_id"], id.to_string());
        assert_eq!(d["source"], "first_account_bootstrap");

        let admin_grant = AuditEvent::ServerAdminGranted {
            account_id: id,
            source: ServerAdminGrantSource::AdminGrant,
        };
        assert_eq!(admin_grant.details()["source"], "admin_grant");
    }

    #[test]
    fn api_key_revoked_serialises_correctly() {
        let account_id = test_uuid();
        let key_id = Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap();
        let event = AuditEvent::ApiKeyRevoked { account_id, key_id };
        assert_eq!(event.event_type(), "api_key_revoked");
        assert_eq!(event.details()["key_id"], key_id.to_string());
        assert!(event.details().get("account_id").is_none());
    }

    #[test]
    fn server_admin_revoked_serialises_correctly() {
        let id = test_uuid();
        let event = AuditEvent::ServerAdminRevoked { account_id: id };
        assert_eq!(event.event_type(), "server_admin_revoked");
        assert_eq!(event.details()["account_id"], id.to_string());
    }

    #[test]
    fn admin_map_ownership_changed_serialises_correctly() {
        let map_id = test_uuid();
        let old = Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap();
        let new = Uuid::parse_str("00000000-0000-0000-0000-000000000003").unwrap();
        let event = AuditEvent::AdminMapOwnershipChanged {
            map_id,
            old_owner: old,
            new_owner: new,
        };
        assert_eq!(event.event_type(), "admin_map_ownership_changed");
        let d = event.details();
        assert_eq!(d["map_id"], map_id.to_string());
        assert_eq!(d["old_owner"], old.to_string());
        assert_eq!(d["new_owner"], new.to_string());
    }

    #[test]
    fn admin_map_hard_deleted_serialises_correctly() {
        let map_id = test_uuid();
        let event = AuditEvent::AdminMapHardDeleted {
            map_id,
            name: "Test Map".into(),
        };
        assert_eq!(event.event_type(), "admin_map_hard_deleted");
        let d = event.details();
        assert_eq!(d["map_id"], map_id.to_string());
        assert_eq!(d["name"], "Test Map");
    }

    #[test]
    fn admin_acl_ownership_changed_serialises_correctly() {
        let acl_id = test_uuid();
        let old = Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap();
        let new = Uuid::parse_str("00000000-0000-0000-0000-000000000003").unwrap();
        let event = AuditEvent::AdminAclOwnershipChanged {
            acl_id,
            old_owner: old,
            new_owner: new,
        };
        assert_eq!(event.event_type(), "admin_acl_ownership_changed");
        let d = event.details();
        assert_eq!(d["acl_id"], acl_id.to_string());
        assert_eq!(d["old_owner"], old.to_string());
        assert_eq!(d["new_owner"], new.to_string());
    }

    #[test]
    fn admin_acl_hard_deleted_serialises_correctly() {
        let acl_id = test_uuid();
        let event = AuditEvent::AdminAclHardDeleted {
            acl_id,
            name: "Test ACL".into(),
        };
        assert_eq!(event.event_type(), "admin_acl_hard_deleted");
        let d = event.details();
        assert_eq!(d["acl_id"], acl_id.to_string());
        assert_eq!(d["name"], "Test ACL");
    }

    #[test]
    fn eve_character_blocked_serialises_correctly() {
        let event = AuditEvent::EveCharacterBlocked {
            eve_character_id: 12345,
            reason: Some("botting".into()),
        };
        assert_eq!(event.event_type(), "eve_character_blocked");
        let d = event.details();
        assert_eq!(d["eve_character_id"], 12345i64);
        assert_eq!(d["reason"], "botting");

        let no_reason = AuditEvent::EveCharacterBlocked {
            eve_character_id: 12345,
            reason: None,
        };
        assert!(no_reason.details()["reason"].is_null());
    }

    #[test]
    fn eve_character_unblocked_serialises_correctly() {
        let event = AuditEvent::EveCharacterUnblocked {
            eve_character_id: 12345,
        };
        assert_eq!(event.event_type(), "eve_character_unblocked");
        assert_eq!(event.details()["eve_character_id"], 12345i64);
    }
}
