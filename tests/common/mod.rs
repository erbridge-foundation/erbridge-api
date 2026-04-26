use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use erbridge_api::{
    config::{Config, EsiClient},
    esi::discovery::EsiMetadata,
    state::AppState,
    tasks::character_location_poll::LocationEvent,
};
use jsonwebtoken::jwk::JwkSet;
use pg_embed::pg_enums::PgAuthMethod;
use pg_embed::pg_fetch::{PgFetchSettings, PG_V17};
use pg_embed::postgres::{PgEmbed, PgSettings};
use reqwest::Client;
use sha2::{Digest, Sha256};
use tokio::sync::{RwLock, broadcast, mpsc};

const TEST_SECRET: &str = "test-encryption-secret";

pub fn test_aes_key() -> [u8; 32] {
    Sha256::digest(TEST_SECRET.as_bytes()).into()
}

pub fn test_jwt_key() -> [u8; 32] {
    Sha256::digest(format!("erbridge:jwt:{TEST_SECRET}").as_bytes()).into()
}

/// Spins up a temporary PostgreSQL instance, applies migrations, and returns
/// the embed handle and pool. Keep `PgEmbed` alive for the test duration.
pub async fn setup_db() -> (PgEmbed, sqlx::PgPool) {
    let port = portpicker::pick_unused_port().expect("no free port");

    let pg_settings = PgSettings {
        database_dir: std::path::PathBuf::from(format!("/tmp/erbridge-test-pg-{port}")),
        port,
        user: "erbridge".into(),
        password: "test".into(),
        auth_method: PgAuthMethod::Plain,
        persistent: false,
        timeout: Some(Duration::from_secs(15)),
        migration_dir: None,
    };

    let fetch_settings = PgFetchSettings {
        version: PG_V17,
        ..Default::default()
    };

    let mut pg = PgEmbed::new(pg_settings, fetch_settings).await.unwrap();
    pg.setup().await.unwrap();
    pg.start_db().await.unwrap();
    pg.create_database("erbridge").await.unwrap();

    let db_url = pg.full_db_uri("erbridge");
    let pool = sqlx::PgPool::connect(&db_url).await.unwrap();

    sqlx::migrate!("./migrations").run(&pool).await.unwrap();

    (pg, pool)
}

#[allow(dead_code)]
pub fn test_state(pool: sqlx::PgPool) -> Arc<AppState> {
    let config = Config {
        esi_clients: vec![EsiClient {
            client_id: "test_client_id".into(),
            client_secret: "test_client_secret".into(),
        }],
        esi_callback_url: "http://localhost:8080/auth/callback".into(),
        aes_key: test_aes_key(),
        jwt_key: test_jwt_key(),
        frontend_url: "http://localhost:3000".into(),
        image_cache_dir: std::env::temp_dir().join("erbridge-test-images"),
        account_deletion_grace_days: 30,
        esi_base: "http://127.0.0.1:9999".into(),
        esi_refresh_token_max_days: 7,
        esi_poll_concurrency: 10,
        esi_poll_batch_size: 10,
        esi_poll_batch_delay_ms: 500,
    };

    let esi_metadata = EsiMetadata {
        authorization_endpoint: "https://login.eveonline.com/v2/oauth/authorize".into(),
        token_endpoint: "https://login.eveonline.com/v2/oauth/token".into(),
        jwks_uri: "https://login.eveonline.com/oauth/jwks".into(),
    };

    let http = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    let (online_poll_tx, _rx) = mpsc::channel::<Vec<i64>>(1);
    let location_subs: Arc<DashMap<i64, broadcast::Sender<LocationEvent>>> =
        Arc::new(DashMap::new());

    Arc::new(AppState {
        db: pool,
        http,
        config,
        esi_metadata,
        jwks: Arc::new(RwLock::new(JwkSet { keys: vec![] })),
        online_poll_tx,
        location_subs,
    })
}
