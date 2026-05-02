use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

/// A single EVE ESI application registration.
#[derive(Clone)]
pub struct EsiClient {
    pub client_id: String,
    pub client_secret: String,
}

/// Application configuration parsed from environment variables at startup.
#[derive(Clone)]
pub struct Config {
    /// Database settings
    /// URL for the database connection
    pub database_url: String,

    /// Maximum number of connections in the PostgreSQL connection pool.
    /// Validated to 1..=200. (DATABASE_MAX_CONNECTIONS, default 20)
    pub database_max_connections: u32,

    /// ESI OAuth client.
    pub esi_client: EsiClient,

    /// Full callback URL registered with CCP (e.g. `https://example.com/auth/callback`).
    pub esi_callback_url: String,

    /// AES-256 key derived from `ENCRYPTION_SECRET` via SHA-256.
    pub aes_key: [u8; 32],

    /// HS256 JWT signing key derived from `ENCRYPTION_SECRET` via SHA-256("erbridge:jwt:" + secret).
    pub jwt_key: [u8; 32],
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let database_url =
            std::env::var("DATABASE_URL").context("ENCRYPTION_SECRET must be set")?;

        let database_max_connections = {
            let raw = std::env::var("DATABASE_MAX_CONNECTIONS")
                .ok()
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(20);
            if !(1..=200).contains(&raw) {
                anyhow::bail!(
                    "DATABASE_MAX_CONNECTIONS must be between 1 and 200 (got {})",
                    raw
                );
            }
            raw
        };

        let client_id = std::env::var("ESI_CLIENT_ID").context("ESI_CLIENT_ID must be set")?;
        let client_secret =
            std::env::var("ESI_CLIENT_SECRET").context("ESI_CLIENT_SECRET must be set")?;

        let esi_client = EsiClient {
            client_id,
            client_secret,
        };

        let app_url = std::env::var("APP_URL")
            .context("APP_URL must be set")?
            .trim_end_matches('/')
            .to_string();

        let encryption_secret =
            std::env::var("ENCRYPTION_SECRET").context("ENCRYPTION_SECRET must be set")?;

        if encryption_secret.len() < 32 {
            anyhow::bail!(
                "ENCRYPTION_SECRET must be at least 32 bytes (got {})",
                encryption_secret.len()
            );
        }

        let aes_key: [u8; 32] = Sha256::digest(encryption_secret.as_bytes()).into();

        let jwt_input = format!("erbridge:jwt:{}", encryption_secret);
        let jwt_key: [u8; 32] = Sha256::digest(jwt_input.as_bytes()).into();

        let esi_callback_url = std::env::var("ESI_CALLBACK_URL")
            .unwrap_or_else(|_| format!("{}/auth/callback", app_url));

        Ok(Self {
            database_url,
            database_max_connections,
            esi_client,
            esi_callback_url,
            aes_key,
            jwt_key,
        })
    }
}
