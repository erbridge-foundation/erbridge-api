use anyhow::{Context, Result};
use sqlx::PgPool;
use strum::{Display, EnumString, IntoStaticStr};
use uuid::Uuid;

use crate::db::acl_member::AclPermission;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Display, EnumString, IntoStaticStr,
)]
#[strum(serialize_all = "snake_case")]
pub enum Permission {
    Read,
    ReadWrite,
    Manage,
    Admin,
}

impl Permission {
    pub fn as_str(self) -> &'static str {
        self.into()
    }
}

/// Resolves the effective permission for `account_id` on `map_id`.
///
/// Returns `None` if the account has no access (no matching ACL entry, or a
/// `deny` entry overrides all grants).
///
/// Resolution rules (ADR-026):
/// - Map owner always gets effective Admin regardless of ACL entries.
/// - A `deny` entry anywhere across all attached ACLs is a hard stop.
/// - Otherwise, most-permissive grant across all matching entries wins.
pub async fn effective_permission(
    pool: &PgPool,
    account_id: Uuid,
    map_id: Uuid,
) -> Result<Option<Permission>> {
    // Owner bypass — check before touching ACLs.
    let is_owner: bool = sqlx::query_scalar!(
        "SELECT EXISTS(SELECT 1 FROM map WHERE id = $1 AND owner_account_id = $2 AND deleted = false)",
        map_id,
        account_id,
    )
    .fetch_one(pool)
    .await
    .context("failed to check map ownership")?
    .unwrap_or(false);

    if is_owner {
        return Ok(Some(Permission::Admin));
    }

    // Collect all permissions matching this account across all ACLs on the map.
    // Matches on: direct character membership, corporation membership, alliance membership.
    let rows = sqlx::query!(
        r#"
        SELECT am.permission
        FROM map_acl ma
        JOIN acl_member am ON am.acl_id = ma.acl_id
        JOIN eve_character ec ON ec.account_id = $2
        WHERE ma.map_id = $1
          AND (
              -- direct character match
              (am.member_type = 'character' AND am.character_id = ec.id)
              -- corporation match
          OR  (am.member_type = 'corporation' AND am.eve_entity_id = ec.corporation_id)
              -- alliance match
          OR  (am.member_type = 'alliance' AND am.eve_entity_id = ec.alliance_id
               AND ec.alliance_id IS NOT NULL)
          )
        "#,
        map_id,
        account_id,
    )
    .fetch_all(pool)
    .await
    .context("failed to resolve acl permissions")?;

    if rows.is_empty() {
        return Ok(None);
    }

    // Deny is a hard stop — overrides all grants.
    if rows
        .iter()
        .any(|r| r.permission == AclPermission::Deny.to_string())
    {
        return Ok(None);
    }

    // Most-permissive grant wins.
    let best = rows
        .iter()
        .filter_map(|r| r.permission.parse::<Permission>().ok())
        .max();

    Ok(best)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_ordering() {
        assert!(Permission::Admin > Permission::Manage);
        assert!(Permission::Manage > Permission::ReadWrite);
        assert!(Permission::ReadWrite > Permission::Read);
    }

    #[test]
    fn permission_round_trip() {
        for (s, p) in [
            ("read", Permission::Read),
            ("read_write", Permission::ReadWrite),
            ("manage", Permission::Manage),
            ("admin", Permission::Admin),
        ] {
            assert_eq!(s.parse::<Permission>().unwrap(), p);
            assert_eq!(p.as_str(), s);
        }
    }

    #[test]
    fn deny_and_unknown_parse_to_none() {
        assert!("deny".parse::<Permission>().is_err());
        assert!("bogus".parse::<Permission>().is_err());
    }
}
