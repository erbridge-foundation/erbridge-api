use anyhow::{Context, Result};
use serde_json::{json, Value};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub enum AuditEvent {
    AccountRegistered { account_id: Uuid, eve_character_id: i64, character_name: String },
    AccountDeletionRequested { account_id: Uuid },
    AccountReactivated { account_id: Uuid },
    AccountPurged { account_id: Uuid },
    CharacterAdded { account_id: Uuid, eve_character_id: i64, character_name: String },
    CharacterRemoved { account_id: Uuid, eve_character_id: i64 },
    CharacterSetMain { account_id: Uuid, eve_character_id: i64 },
    GhostCharacterClaimed { account_id: Uuid, eve_character_id: i64, character_name: String },
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
        }
    }

    pub fn details(&self) -> Value {
        match self {
            // actor is NULL for registration — account_id not in actor column so include it here.
            Self::AccountRegistered { account_id, eve_character_id, character_name } => json!({
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
            Self::CharacterAdded { eve_character_id, character_name, .. } => json!({
                "eve_character_id": eve_character_id,
                "character_name": character_name,
            }),
            Self::CharacterRemoved { eve_character_id, .. } => json!({
                "eve_character_id": eve_character_id,
            }),
            Self::CharacterSetMain { eve_character_id, .. } => json!({
                "eve_character_id": eve_character_id,
            }),
            // actor is NULL for login ghost-claim (no session yet) — include account_id.
            // actor is set for attach ghost-claim — but include for consistency.
            Self::GhostCharacterClaimed { account_id, eve_character_id, character_name } => json!({
                "account_id": account_id,
                "eve_character_id": eve_character_id,
                "character_name": character_name,
            }),
        }
    }
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
        let event = AuditEvent::CharacterRemoved { account_id: id, eve_character_id: 42 };
        assert_eq!(event.event_type(), "character_removed");
        assert_eq!(event.details()["eve_character_id"], 42i64);
        assert!(event.details().get("account_id").is_none());
    }

    #[test]
    fn character_set_main_serialises_correctly() {
        let id = test_uuid();
        let event = AuditEvent::CharacterSetMain { account_id: id, eve_character_id: 99 };
        assert_eq!(event.event_type(), "character_set_main");
        assert_eq!(event.details()["eve_character_id"], 99i64);
        assert!(event.details().get("account_id").is_none());
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
}
