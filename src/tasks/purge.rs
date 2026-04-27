use std::sync::Arc;
use std::time::Duration;

use tracing::{info, warn};

use crate::{db, state::AppState};

const PURGE_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

pub fn spawn_purge_task(state: Arc<AppState>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(PURGE_INTERVAL);
        interval.tick().await;

        loop {
            interval.tick().await;

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
    });
}
