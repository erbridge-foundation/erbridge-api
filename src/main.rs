use reqwest::Client;
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
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
        .max_connections(5)
        .connect(&database_url)
        .await?;
    info!("Connected to database");

    sqlx::migrate!("./migrations").run(&pool).await?;
    info!("migrations applied");

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

    let app = erbridge_api::router(pool, http, config, esi_metadata, jwks);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    info!("listening on {}", listener.local_addr()?);
    axum::serve(listener, app).await?;

    Ok(())
}
