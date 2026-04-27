use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio::time;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::db::{connection as db_conn, map as db_map, map_checkpoint, signature as db_sig};
use crate::state::AppState;

pub fn spawn_checkpoint_task(state: Arc<AppState>, cancel: CancellationToken) {
    tokio::spawn(run_checkpoint_task(state, cancel));
}

async fn run_checkpoint_task(state: Arc<AppState>, cancel: CancellationToken) {
    let interval_secs = state.config.map_checkpoint_interval_mins * 60;
    let mut interval = time::interval(Duration::from_secs(interval_secs));
    // Skip the first immediate tick so startup isn't hammered.
    interval.tick().await;

    loop {
        tokio::select! {
            _ = interval.tick() => {}
            _ = cancel.cancelled() => {
                info!("map checkpoint: shutting down");
                return;
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{Config, EsiClient},
        esi::discovery::EsiMetadata,
        state::AppState,
        tasks::character_location_poll::LocationEvent,
    };
    use dashmap::DashMap;
    use jsonwebtoken::jwk::JwkSet;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    fn make_state() -> Arc<AppState> {
        let config = Config {
            esi_clients: vec![EsiClient {
                client_id: "test".into(),
                client_secret: "test".into(),
            }],
            esi_callback_url: "http://localhost/callback".into(),
            aes_key: [0u8; 32],
            jwt_key: [0u8; 32],
            frontend_url: "http://localhost".into(),
            account_deletion_grace_days: 30,
            esi_base: "http://localhost".into(),
            esi_refresh_token_max_days: 7,
            esi_poll_concurrency: 10,
            esi_poll_batch_size: 10,
            esi_poll_batch_delay_ms: 500,
            map_checkpoint_interval_mins: 60,
            database_max_connections: 5,
        };
        Arc::new(AppState {
            db: sqlx::PgPool::connect_lazy("postgres://localhost/dummy").unwrap(),
            http: reqwest::Client::new(),
            config,
            esi_metadata: EsiMetadata {
                authorization_endpoint: "http://localhost".into(),
                token_endpoint: "http://localhost".into(),
                jwks_uri: "http://localhost".into(),
            },
            jwks: Arc::new(RwLock::new(JwkSet { keys: vec![] })),
            online_poll_tx: None,
            location_subs: Arc::new(
                DashMap::<i64, tokio::sync::broadcast::Sender<LocationEvent>>::new(),
            ),
        })
    }

    #[tokio::test]
    async fn checkpoint_task_exits_on_cancel() {
        let cancel = CancellationToken::new();
        cancel.cancel();

        let state = make_state();
        let handle = tokio::spawn(run_checkpoint_task(state, cancel));

        tokio::time::timeout(std::time::Duration::from_secs(1), handle)
            .await
            .expect("checkpoint task did not exit within 1s")
            .expect("task panicked");
    }
}
