use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::{db, state::AppState};

const PURGE_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

pub fn spawn_purge_task(state: Arc<AppState>, cancel: CancellationToken) {
    tokio::spawn(run_purge_task(state, cancel));
}

async fn run_purge_task(state: Arc<AppState>, cancel: CancellationToken) {
    let mut interval = tokio::time::interval(PURGE_INTERVAL);
    interval.tick().await;

    loop {
        tokio::select! {
            _ = interval.tick() => {}
            _ = cancel.cancelled() => {
                info!("purge task: shutting down");
                return;
            }
        }

        let grace_days = state.config.account_deletion_grace_days;

        match db::account::purge_expired_accounts(&state.db, grace_days).await {
            Ok(n) => info!(deleted = n, "purged expired accounts"),
            Err(e) => warn!(error = %e, "purge_expired_accounts failed"),
        }

        match db::acl::purge_expired_acls(&state.db, grace_days).await {
            Ok(n) => info!(deleted = n, "purged orphaned acls"),
            Err(e) => warn!(error = %e, "purge_expired_acls failed"),
        }
    }
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
    async fn purge_task_exits_on_cancel() {
        let cancel = CancellationToken::new();
        cancel.cancel();

        let state = make_state();
        let handle = tokio::spawn(run_purge_task(state, cancel));

        tokio::time::timeout(std::time::Duration::from_secs(1), handle)
            .await
            .expect("purge task did not exit within 1s")
            .expect("task panicked");
    }
}
