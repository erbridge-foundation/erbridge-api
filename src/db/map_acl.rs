use anyhow::{Context, Result};
use sqlx::{PgPool, Postgres, Transaction};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use super::acl::Acl;

/// Attaches an ACL to a map and clears the ACL's `pending_delete_at` if set.
/// Must run inside a transaction.
pub async fn attach_acl(
    tx: &mut Transaction<'_, Postgres>,
    map_id: Uuid,
    acl_id: Uuid,
) -> Result<()> {
    sqlx::query!(
        r#"
        INSERT INTO map_acl (map_id, acl_id) VALUES ($1, $2)
        ON CONFLICT DO NOTHING
        "#,
        map_id,
        acl_id,
    )
    .execute(&mut **tx)
    .await
    .context("failed to attach acl to map")?;

    sqlx::query!(
        "UPDATE acl SET pending_delete_at = NULL, updated_at = now() WHERE id = $1",
        acl_id,
    )
    .execute(&mut **tx)
    .await
    .context("failed to clear acl pending_delete_at on attach")?;

    Ok(())
}

/// Detaches an ACL from a map. If the ACL has no remaining map_acl rows,
/// sets `pending_delete_at = now()` to start the orphan grace period.
/// Must run inside a transaction.
pub async fn detach_acl(
    tx: &mut Transaction<'_, Postgres>,
    map_id: Uuid,
    acl_id: Uuid,
) -> Result<()> {
    sqlx::query!(
        "DELETE FROM map_acl WHERE map_id = $1 AND acl_id = $2",
        map_id,
        acl_id,
    )
    .execute(&mut **tx)
    .await
    .context("failed to detach acl from map")?;

    let remaining: i64 =
        sqlx::query_scalar!("SELECT COUNT(*) FROM map_acl WHERE acl_id = $1", acl_id,)
            .fetch_one(&mut **tx)
            .await
            .context("failed to count remaining map_acl rows")?
            .unwrap_or(0);

    if remaining == 0 {
        sqlx::query!(
            "UPDATE acl SET pending_delete_at = now(), updated_at = now() WHERE id = $1",
            acl_id,
        )
        .execute(&mut **tx)
        .await
        .context("failed to set acl pending_delete_at on orphan")?;
    }

    Ok(())
}

pub async fn find_acls_for_map(pool: &PgPool, map_id: Uuid) -> Result<Vec<Acl>> {
    sqlx::query_as!(
        Acl,
        r#"
        SELECT a.id, a.name, a.owner_account_id, a.pending_delete_at, a.created_at, a.updated_at
        FROM acl a
        JOIN map_acl ma ON ma.acl_id = a.id
        WHERE ma.map_id = $1
        ORDER BY a.name
        "#,
        map_id,
    )
    .fetch_all(pool)
    .await
    .context("failed to fetch acls for map")
}

/// Returns a map from map_id → set of attached acl_ids for a batch of maps.
pub async fn find_acl_ids_for_maps(
    pool: &PgPool,
    map_ids: &[Uuid],
) -> Result<HashMap<Uuid, HashSet<Uuid>>> {
    struct Row {
        map_id: Uuid,
        acl_id: Uuid,
    }

    let rows = sqlx::query_as!(
        Row,
        "SELECT map_id, acl_id FROM map_acl WHERE map_id = ANY($1)",
        map_ids,
    )
    .fetch_all(pool)
    .await
    .context("failed to fetch acl ids for maps")?;

    let mut out: HashMap<Uuid, HashSet<Uuid>> = HashMap::new();
    for row in rows {
        out.entry(row.map_id).or_default().insert(row.acl_id);
    }
    Ok(out)
}
