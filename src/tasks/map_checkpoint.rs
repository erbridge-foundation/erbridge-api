use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio::time;
use tracing::error;

use crate::db::{connection as db_conn, map as db_map, map_checkpoint, signature as db_sig};
use crate::state::AppState;

pub fn spawn_checkpoint_task(state: Arc<AppState>) {
    tokio::spawn(run_checkpoint_task(state));
}

async fn run_checkpoint_task(state: Arc<AppState>) {
    let interval_secs = state.config.map_checkpoint_interval_mins * 60;
    let mut interval = time::interval(Duration::from_secs(interval_secs));
    // Skip the first immediate tick so startup isn't hammered.
    interval.tick().await;

    loop {
        interval.tick().await;
        if let Err(e) = checkpoint_all_maps(&state).await {
            error!(error = %e, "map checkpoint task failed");
        }
    }
}

async fn checkpoint_all_maps(state: &AppState) -> anyhow::Result<()> {
    let pool = &state.db;

    // Find maps where the latest event seq exceeds the last checkpoint seq.
    let maps_needing_checkpoint = sqlx::query!(
        r#"
        SELECT m.id AS map_id,
               m.last_checkpoint_seq,
               COALESCE(MAX(e.seq), 0) AS "latest_seq!"
        FROM map m
        LEFT JOIN map_events e ON e.map_id = m.id
        GROUP BY m.id, m.last_checkpoint_seq
        HAVING COALESCE(MAX(e.seq), 0) > m.last_checkpoint_seq
        "#,
    )
    .fetch_all(pool)
    .await?;

    for row in maps_needing_checkpoint {
        let map_id = row.map_id;
        let last_checkpoint_seq = row.last_checkpoint_seq;
        let latest_seq = row.latest_seq;

        if let Err(e) = checkpoint_map(state, map_id, last_checkpoint_seq, latest_seq).await {
            error!(error = %e, %map_id, "failed to checkpoint map");
        }
    }

    Ok(())
}

async fn checkpoint_map(
    state: &AppState,
    map_id: uuid::Uuid,
    last_checkpoint_seq: i64,
    latest_seq: i64,
) -> anyhow::Result<()> {
    let pool = &state.db;

    let connections = db_conn::find_connections_for_map(pool, map_id).await?;
    let connection_ends = db_conn::find_connection_ends_for_map(pool, map_id).await?;
    let signatures = db_sig::find_signatures_for_map(pool, map_id).await?;

    let conn_json: Vec<_> = connections
        .iter()
        .map(|c| {
            json!({
                "connection_id": c.connection_id,
                "status": c.status.to_string(),
                "life_state": c.life_state.map(|s| s.to_string()),
                "mass_state": c.mass_state.map(|s| s.to_string()),
                "created_at": c.created_at,
                "updated_at": c.updated_at,
            })
        })
        .collect();

    let ends_json: Vec<_> = connection_ends
        .iter()
        .map(|e| {
            json!({
                "connection_id": e.connection_id,
                "side": e.side.to_string(),
                "system_id": e.system_id,
                "signature_id": e.signature_id,
                "wormhole_code": e.wormhole_code,
            })
        })
        .collect();

    let sigs_json: Vec<_> = signatures
        .iter()
        .map(|s| {
            json!({
                "signature_id": s.signature_id,
                "system_id": s.system_id,
                "sig_code": s.sig_code,
                "sig_type": s.sig_type,
                "status": s.status.to_string(),
                "connection_id": s.connection_id,
                "connection_side": s.connection_side.map(|s| s.to_string()),
                "derived_life_state": s.derived_life_state.map(|s| s.to_string()),
                "derived_mass_state": s.derived_mass_state.map(|s| s.to_string()),
            })
        })
        .collect();

    let state_snapshot = json!({
        "connections":      conn_json,
        "connection_ends":  ends_json,
        "signatures":       sigs_json,
    });

    let event_count = (latest_seq - last_checkpoint_seq) as i32;

    let mut tx = pool.begin().await?;

    map_checkpoint::insert_checkpoint(
        &mut tx,
        map_id,
        latest_seq,
        1,
        Some(event_count),
        None,
        &state_snapshot,
    )
    .await?;

    db_map::update_last_checkpoint(&mut tx, map_id, latest_seq).await?;

    tx.commit().await?;

    Ok(())
}
