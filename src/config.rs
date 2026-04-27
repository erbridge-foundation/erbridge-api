use anyhow::{Context, Result, anyhow};
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
    /// ESI OAuth clients (one or more).
    pub esi_clients: Vec<EsiClient>,
    /// Full callback URL registered with CCP (e.g. `https://example.com/auth/callback`).
    pub esi_callback_url: String,
    /// AES-256 key derived from `ENCRYPTION_SECRET` via SHA-256.
    pub aes_key: [u8; 32],
    /// HS256 JWT signing key derived from `ENCRYPTION_SECRET` via SHA-256("erbridge:jwt:" + secret).
    pub jwt_key: [u8; 32],
    /// Base URL of the frontend; used for post-login redirects.
    pub frontend_url: String,
    /// Directory for the filesystem-backed EVE image proxy cache.
    pub image_cache_dir: std::path::PathBuf,
    /// Grace period in days before a pending-delete account is hard-deleted.
    pub account_deletion_grace_days: u32,
    /// Base URL for ESI API calls. Defaults to the live ESI endpoint.
    /// Overridable in tests via `ESI_BASE_URL` env var.
    pub esi_base: String,
    /// Maximum age of an ESI refresh token in days before it is considered
    /// expired and the character must re-authenticate (ADR-029).
    pub esi_refresh_token_max_days: u32,
    /// Maximum concurrent in-flight ESI requests per client for the location
    /// poller. Tune upward for large character counts. (ESI_POLL_CONCURRENCY)
    pub esi_poll_concurrency: usize,
    /// Characters per batch for the online poller, per client.
    /// (ESI_POLL_BATCH_SIZE)
    pub esi_poll_batch_size: usize,
    /// Minimum milliseconds to sleep between online poll batches per client.
    /// Clamped to at least 100ms. (ESI_POLL_BATCH_DELAY_MS)
    pub esi_poll_batch_delay_ms: u64,
    /// How often (in minutes) the map checkpoint task snapshots map state.
    /// (MAP_CHECKPOINT_INTERVAL_MINS, default 60)
    pub map_checkpoint_interval_mins: u64,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let encryption_secret =
            std::env::var("ENCRYPTION_SECRET").context("ENCRYPTION_SECRET must be set")?;

        let aes_key: [u8; 32] = Sha256::digest(encryption_secret.as_bytes()).into();

        let jwt_input = format!("erbridge:jwt:{}", encryption_secret);
        let jwt_key: [u8; 32] = Sha256::digest(jwt_input.as_bytes()).into();

        let esi_clients = parse_esi_clients()?;

        let app_url = std::env::var("APP_URL")
            .context("APP_URL must be set")?
            .trim_end_matches('/')
            .to_string();

        let esi_callback_url = std::env::var("ESI_CALLBACK_URL")
            .unwrap_or_else(|_| format!("{}/auth/callback", app_url));

        let frontend_url = std::env::var("FRONTEND_URL").unwrap_or_else(|_| app_url);

        let image_cache_dir = std::env::var("IMAGE_CACHE_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir().join("erbridge-images"));

        let account_deletion_grace_days = std::env::var("ACCOUNT_DELETION_GRACE_DAYS")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(30);

        let esi_base = std::env::var("ESI_BASE_URL")
            .unwrap_or_else(|_| "https://esi.evetech.net/latest".to_string());

        let esi_refresh_token_max_days = std::env::var("ESI_REFRESH_TOKEN_MAX_DAYS")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(7);

        let esi_poll_concurrency = std::env::var("ESI_POLL_CONCURRENCY")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(10);

        let esi_poll_batch_size = std::env::var("ESI_POLL_BATCH_SIZE")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(10);

        let esi_poll_batch_delay_ms = {
            let raw = std::env::var("ESI_POLL_BATCH_DELAY_MS")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(500);
            if raw < 100 {
                tracing::warn!(
                    configured = raw,
                    clamped = 100,
                    "ESI_POLL_BATCH_DELAY_MS is below minimum; clamping to 100ms"
                );
                100
            } else {
                raw
            }
        };

        let map_checkpoint_interval_mins = std::env::var("MAP_CHECKPOINT_INTERVAL_MINS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(60);

        Ok(Self {
            esi_clients,
            esi_callback_url,
            aes_key,
            jwt_key,
            frontend_url,
            image_cache_dir,
            account_deletion_grace_days,
            esi_base,
            esi_refresh_token_max_days,
            esi_poll_concurrency,
            esi_poll_batch_size,
            esi_poll_batch_delay_ms,
            map_checkpoint_interval_mins,
        })
    }
}

fn parse_esi_clients() -> Result<Vec<EsiClient>> {
    // Numbered vars: ESI_CLIENT_ID_1/ESI_CLIENT_SECRET_1, _2, _3, ...
    // Stops at the first missing index.
    let mut numbered = Vec::new();
    let mut n = 1u32;
    loop {
        let id = std::env::var(format!("ESI_CLIENT_ID_{}", n)).ok();
        let secret = std::env::var(format!("ESI_CLIENT_SECRET_{}", n)).ok();
        match (id, secret) {
            (Some(client_id), Some(client_secret)) => {
                numbered.push(EsiClient {
                    client_id,
                    client_secret,
                });
                n += 1;
            }
            (None, None) => break,
            (Some(_), None) => return Err(anyhow!("ESI_CLIENT_SECRET_{} must be set", n)),
            (None, Some(_)) => return Err(anyhow!("ESI_CLIENT_ID_{} must be set", n)),
        }
    }
    if !numbered.is_empty() {
        return Ok(numbered);
    }

    // Fall back to single-client vars.
    let client_id =
        std::env::var("ESI_CLIENT_ID").context("ESI_CLIENT_ID or ESI_CLIENT_ID_1 must be set")?;
    let client_secret =
        std::env::var("ESI_CLIENT_SECRET").context("ESI_CLIENT_SECRET must be set")?;

    Ok(vec![EsiClient {
        client_id,
        client_secret,
    }])
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use sha2::{Digest, Sha256};

    /// Serializes tests that mutate env vars to prevent flakiness under parallel execution.
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    /// Keys derived from the same secret must be deterministic and distinct
    /// from each other.
    #[test]
    fn key_derivation_is_deterministic_and_distinct() {
        let secret = "test-secret";

        let aes_key: [u8; 32] = Sha256::digest(secret.as_bytes()).into();
        let jwt_input = format!("erbridge:jwt:{}", secret);
        let jwt_key: [u8; 32] = Sha256::digest(jwt_input.as_bytes()).into();

        // Same inputs → same outputs.
        let aes_key2: [u8; 32] = Sha256::digest(secret.as_bytes()).into();
        assert_eq!(aes_key, aes_key2);

        // AES key and JWT key must differ (domain separation).
        assert_ne!(aes_key, jwt_key);
    }

    #[test]
    fn different_secrets_produce_different_keys() {
        let key_a: [u8; 32] = Sha256::digest(b"secret-a").into();
        let key_b: [u8; 32] = Sha256::digest(b"secret-b").into();
        assert_ne!(key_a, key_b);
    }

    #[test]
    fn parse_esi_clients_single_vars() {
        let _guard = ENV_MUTEX.lock().unwrap();
        unsafe {
            std::env::remove_var("ESI_CLIENT_ID_1");
            std::env::set_var("ESI_CLIENT_ID", "client123");
            std::env::set_var("ESI_CLIENT_SECRET", "secret456");
        }

        let clients = parse_esi_clients().unwrap();
        assert_eq!(clients.len(), 1);
        assert_eq!(clients[0].client_id, "client123");
        assert_eq!(clients[0].client_secret, "secret456");

        unsafe {
            std::env::remove_var("ESI_CLIENT_ID");
            std::env::remove_var("ESI_CLIENT_SECRET");
        }
    }
}
