use std::sync::Arc;
use std::time::Duration;

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

    let pool = PgPoolOptions::new()
        .max_connections(config.database_max_connections)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&config.database_url)
        .await?;
    info!(
        max_connections = config.database_max_connections,
        "Connected to database"
    );

    sqlx::migrate!("./migrations").run(&pool).await?;
    info!("migrations applied");

    let http = Client::builder().timeout(Duration::from_secs(10)).build()?;

    let esi_metadata = erbridge_api::esi::discovery::discover(&http).await?;
    info!(
        "ESI discovery complete: token_endpoint={}",
        esi_metadata.token_endpoint
    );

    let jwks = erbridge_api::esi::jwks::fetch_jwks(&http, &esi_metadata.jwks_uri).await?;
    info!("JWK set loaded ({} keys)", jwks.keys.len());
    let jwks = Arc::new(RwLock::new(jwks));

    // Root cancellation token — child tokens are handed to each background task.
    let cancel = CancellationToken::new();

    let app = erbridge_api::router::new_router(pool, http, config, esi_metadata, jwks);

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
