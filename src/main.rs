use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use reqwest::Client;
use sqlx::postgres::PgPoolOptions;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "erbridge_api=info".into()),
        )
        .init();

    info!("E-R Bridge API starting up");

    let config = erbridge_api::config::Config::from_env()?;

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let pool = PgPoolOptions::new()
        .max_connections(config.database_max_connections)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&database_url)
        .await?;
    info!(
        max_connections = config.database_max_connections,
        "Connected to database"
    );

    sqlx::migrate!("./migrations").run(&pool).await?;
    info!("migrations applied");

    #[cfg(feature = "dev-seed")]
    erbridge_api::dev_seed::run_if_requested(&pool, &config.aes_key).await?;

    let http = Client::builder().timeout(Duration::from_secs(10)).build()?;

    erbridge_api::services::sde_solar_system::load_sde_if_needed(&pool, &http).await?;

    let esi_metadata = erbridge_api::esi::discovery::discover(&http).await?;
    info!(
        "ESI discovery complete: token_endpoint={}",
        esi_metadata.token_endpoint
    );

    let jwks = erbridge_api::esi::jwks::fetch_jwks(&http, &esi_metadata.jwks_uri).await?;
    info!("JWK set loaded ({} keys)", jwks.keys.len());
    let jwks = Arc::new(RwLock::new(jwks));

    let location_subs = Arc::new(DashMap::new());

    // Build a minimal AppState for the pollers before the router takes ownership.
    // The router gets its own Arc from the same fields below.
    let poller_state = Arc::new(erbridge_api::state::AppState {
        db: pool.clone(),
        http: http.clone(),
        config: config.clone(),
        esi_metadata: esi_metadata.clone(),
        jwks: Arc::clone(&jwks),
        // None here — the poller AppState never sends on this channel.
        online_poll_tx: None,
        location_subs: Arc::clone(&location_subs),
    });

    // Root cancellation token — child tokens are handed to each background task.
    let cancel = CancellationToken::new();

    // Spawn background tasks; each receives a child token so they all cancel together.
    erbridge_api::services::sde_solar_system::spawn_sde_update_check(
        pool.clone(),
        http.clone(),
        cancel.child_token(),
    );

    erbridge_api::tasks::purge::spawn_purge_task(Arc::clone(&poller_state), cancel.child_token());
    let online_poll_tx = erbridge_api::tasks::character_online_poll::spawn_online_poller(
        Arc::clone(&poller_state),
        cancel.child_token(),
    );
    erbridge_api::tasks::character_location_poll::spawn_location_poller(
        Arc::clone(&poller_state),
        cancel.child_token(),
    );
    erbridge_api::tasks::map_checkpoint::spawn_checkpoint_task(
        Arc::clone(&poller_state),
        cancel.child_token(),
    );

    let app = erbridge_api::router(
        pool,
        http,
        config,
        esi_metadata,
        jwks,
        online_poll_tx,
        Arc::clone(&location_subs),
    );

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    info!("listening on {}", listener.local_addr()?);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(cancel.clone()))
        .await?;

    info!("HTTP server stopped; waiting for background tasks (up to 10s)");
    cancel.cancel();

    // Give background tasks up to 10 seconds to finish their current iteration.
    // We don't hold JoinHandles (the tasks are fire-and-forget spawns), so we
    // simply wait a bounded amount of time for the runtime to drain.
    tokio::time::timeout(Duration::from_secs(10), async {
        // Yield repeatedly so spawned tasks get CPU time to observe cancellation.
        for _ in 0..100 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .ok();

    info!("shutdown complete");
    Ok(())
}

/// Resolves on the first SIGTERM or Ctrl-C (SIGINT), then cancels the token.
async fn shutdown_signal(cancel: CancellationToken) {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let sigterm = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let sigterm = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => { info!("received Ctrl-C, initiating shutdown"); }
        _ = sigterm => { info!("received SIGTERM, initiating shutdown"); }
        _ = cancel.cancelled() => {}
    }
}
