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
    /// Maximum number of connections in the PostgreSQL connection pool.
    /// Validated to 1..=200. (DATABASE_MAX_CONNECTIONS, default 20)
    pub database_max_connections: u32,
}

impl Config {
    pub fn from_env() -> Result<Self> {
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

        let esi_clients = parse_esi_clients()?;

        let app_url = std::env::var("APP_URL")
            .context("APP_URL must be set")?
            .trim_end_matches('/')
            .to_string();

        let esi_callback_url = std::env::var("ESI_CALLBACK_URL")
            .unwrap_or_else(|_| format!("{}/auth/callback", app_url));

        let frontend_url = std::env::var("FRONTEND_URL").unwrap_or(app_url);

        let account_deletion_grace_days = {
            let raw = std::env::var("ACCOUNT_DELETION_GRACE_DAYS")
                .ok()
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(30);
            if !(1..=365).contains(&raw) {
                anyhow::bail!(
                    "ACCOUNT_DELETION_GRACE_DAYS must be between 1 and 365 (got {})",
                    raw
                );
            }
            raw
        };

        let esi_base = std::env::var("ESI_BASE_URL")
            .unwrap_or_else(|_| "https://esi.evetech.net/latest".to_string());

        let esi_refresh_token_max_days = {
            let raw = std::env::var("ESI_REFRESH_TOKEN_MAX_DAYS")
                .ok()
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(7);
            if !(1..=30).contains(&raw) {
                anyhow::bail!(
                    "ESI_REFRESH_TOKEN_MAX_DAYS must be between 1 and 30 (got {})",
                    raw
                );
            }
            raw
        };

        let esi_poll_concurrency = {
            let raw = std::env::var("ESI_POLL_CONCURRENCY")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(10);
            if !(1..=100).contains(&raw) {
                anyhow::bail!(
                    "ESI_POLL_CONCURRENCY must be between 1 and 100 (got {})",
                    raw
                );
            }
            raw
        };

        let esi_poll_batch_size = {
            let raw = std::env::var("ESI_POLL_BATCH_SIZE")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(10);
            if !(1..=100).contains(&raw) {
                anyhow::bail!(
                    "ESI_POLL_BATCH_SIZE must be between 1 and 100 (got {})",
                    raw
                );
            }
            raw
        };

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

        let map_checkpoint_interval_mins = {
            let raw = std::env::var("MAP_CHECKPOINT_INTERVAL_MINS")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(60);
            if !(1..=1440).contains(&raw) {
                anyhow::bail!(
                    "MAP_CHECKPOINT_INTERVAL_MINS must be between 1 and 1440 (got {})",
                    raw
                );
            }
            raw
        };

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

        Ok(Self {
            esi_clients,
            esi_callback_url,
            aes_key,
            jwt_key,
            frontend_url,
            account_deletion_grace_days,
            esi_base,
            esi_refresh_token_max_days,
            esi_poll_concurrency,
            esi_poll_batch_size,
            esi_poll_batch_delay_ms,
            map_checkpoint_interval_mins,
            database_max_connections,
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

    fn set_required_env_vars() {
        unsafe {
            std::env::set_var("ENCRYPTION_SECRET", "a-sufficiently-long-test-secret-value");
            std::env::set_var("APP_URL", "http://localhost:8080");
            std::env::remove_var("ESI_CLIENT_ID_1");
            std::env::set_var("ESI_CLIENT_ID", "cid");
            std::env::set_var("ESI_CLIENT_SECRET", "csecret");
        }
    }

    fn clear_required_env_vars() {
        unsafe {
            std::env::remove_var("ENCRYPTION_SECRET");
            std::env::remove_var("APP_URL");
            std::env::remove_var("ESI_CLIENT_ID");
            std::env::remove_var("ESI_CLIENT_SECRET");
        }
    }

    #[test]
    fn database_max_connections_default_is_20() {
        let _guard = ENV_MUTEX.lock().unwrap();
        set_required_env_vars();
        unsafe {
            std::env::remove_var("DATABASE_MAX_CONNECTIONS");
        }

        let config = Config::from_env().unwrap();
        assert_eq!(config.database_max_connections, 20);

        clear_required_env_vars();
    }

    #[test]
    fn database_max_connections_accepts_valid_value() {
        let _guard = ENV_MUTEX.lock().unwrap();
        set_required_env_vars();
        unsafe {
            std::env::set_var("DATABASE_MAX_CONNECTIONS", "50");
        }

        let config = Config::from_env().unwrap();
        assert_eq!(config.database_max_connections, 50);

        clear_required_env_vars();
        unsafe {
            std::env::remove_var("DATABASE_MAX_CONNECTIONS");
        }
    }

    #[test]
    fn database_max_connections_rejects_zero() {
        let _guard = ENV_MUTEX.lock().unwrap();
        set_required_env_vars();
        unsafe {
            std::env::set_var("DATABASE_MAX_CONNECTIONS", "0");
        }

        match Config::from_env() {
            Err(e) => assert!(
                e.to_string().contains("DATABASE_MAX_CONNECTIONS"),
                "error message should mention DATABASE_MAX_CONNECTIONS, got: {e}"
            ),
            Ok(_) => panic!("expected error for DATABASE_MAX_CONNECTIONS=0"),
        }

        clear_required_env_vars();
        unsafe {
            std::env::remove_var("DATABASE_MAX_CONNECTIONS");
        }
    }

    #[test]
    fn database_max_connections_rejects_over_200() {
        let _guard = ENV_MUTEX.lock().unwrap();
        set_required_env_vars();
        unsafe {
            std::env::set_var("DATABASE_MAX_CONNECTIONS", "201");
        }

        match Config::from_env() {
            Err(e) => assert!(
                e.to_string().contains("DATABASE_MAX_CONNECTIONS"),
                "error message should mention DATABASE_MAX_CONNECTIONS, got: {e}"
            ),
            Ok(_) => panic!("expected error for DATABASE_MAX_CONNECTIONS=201"),
        }

        clear_required_env_vars();
        unsafe {
            std::env::remove_var("DATABASE_MAX_CONNECTIONS");
        }
    }

    fn check_rejects(var: &str, value: &str) {
        set_required_env_vars();
        unsafe { std::env::set_var(var, value) }
        match Config::from_env() {
            Err(e) => assert!(
                e.to_string().contains(var),
                "error message should mention {var}, got: {e}"
            ),
            Ok(_) => panic!("expected error for {var}={value}"),
        }
        clear_required_env_vars();
        unsafe { std::env::remove_var(var) }
    }

    fn check_accepts(var: &str, value: &str, expected: impl Fn(&Config) -> bool) {
        set_required_env_vars();
        unsafe { std::env::set_var(var, value) }
        let config = Config::from_env().expect("should parse");
        assert!(expected(&config), "unexpected value for {var}={value}");
        clear_required_env_vars();
        unsafe { std::env::remove_var(var) }
    }

    #[test]
    fn esi_poll_concurrency_rejects_zero() {
        let _guard = ENV_MUTEX.lock().unwrap();
        check_rejects("ESI_POLL_CONCURRENCY", "0");
    }

    #[test]
    fn esi_poll_concurrency_rejects_over_100() {
        let _guard = ENV_MUTEX.lock().unwrap();
        check_rejects("ESI_POLL_CONCURRENCY", "101");
    }

    #[test]
    fn esi_poll_concurrency_accepts_valid() {
        let _guard = ENV_MUTEX.lock().unwrap();
        check_accepts("ESI_POLL_CONCURRENCY", "50", |c| {
            c.esi_poll_concurrency == 50
        });
    }

    #[test]
    fn esi_poll_batch_size_rejects_zero() {
        let _guard = ENV_MUTEX.lock().unwrap();
        check_rejects("ESI_POLL_BATCH_SIZE", "0");
    }

    #[test]
    fn esi_poll_batch_size_rejects_over_100() {
        let _guard = ENV_MUTEX.lock().unwrap();
        check_rejects("ESI_POLL_BATCH_SIZE", "101");
    }

    #[test]
    fn esi_poll_batch_size_accepts_valid() {
        let _guard = ENV_MUTEX.lock().unwrap();
        check_accepts("ESI_POLL_BATCH_SIZE", "25", |c| c.esi_poll_batch_size == 25);
    }

    #[test]
    fn map_checkpoint_interval_mins_rejects_zero() {
        let _guard = ENV_MUTEX.lock().unwrap();
        check_rejects("MAP_CHECKPOINT_INTERVAL_MINS", "0");
    }

    #[test]
    fn map_checkpoint_interval_mins_rejects_over_1440() {
        let _guard = ENV_MUTEX.lock().unwrap();
        check_rejects("MAP_CHECKPOINT_INTERVAL_MINS", "1441");
    }

    #[test]
    fn map_checkpoint_interval_mins_accepts_valid() {
        let _guard = ENV_MUTEX.lock().unwrap();
        check_accepts("MAP_CHECKPOINT_INTERVAL_MINS", "30", |c| {
            c.map_checkpoint_interval_mins == 30
        });
    }

    #[test]
    fn account_deletion_grace_days_rejects_zero() {
        let _guard = ENV_MUTEX.lock().unwrap();
        check_rejects("ACCOUNT_DELETION_GRACE_DAYS", "0");
    }

    #[test]
    fn account_deletion_grace_days_rejects_over_365() {
        let _guard = ENV_MUTEX.lock().unwrap();
        check_rejects("ACCOUNT_DELETION_GRACE_DAYS", "366");
    }

    #[test]
    fn account_deletion_grace_days_accepts_valid() {
        let _guard = ENV_MUTEX.lock().unwrap();
        check_accepts("ACCOUNT_DELETION_GRACE_DAYS", "90", |c| {
            c.account_deletion_grace_days == 90
        });
    }

    #[test]
    fn esi_refresh_token_max_days_rejects_zero() {
        let _guard = ENV_MUTEX.lock().unwrap();
        check_rejects("ESI_REFRESH_TOKEN_MAX_DAYS", "0");
    }

    #[test]
    fn esi_refresh_token_max_days_rejects_over_30() {
        let _guard = ENV_MUTEX.lock().unwrap();
        check_rejects("ESI_REFRESH_TOKEN_MAX_DAYS", "31");
    }

    #[test]
    fn esi_refresh_token_max_days_accepts_valid() {
        let _guard = ENV_MUTEX.lock().unwrap();
        check_accepts("ESI_REFRESH_TOKEN_MAX_DAYS", "14", |c| {
            c.esi_refresh_token_max_days == 14
        });
    }

    #[test]
    fn encryption_secret_rejects_empty() {
        let _guard = ENV_MUTEX.lock().unwrap();
        set_required_env_vars();
        unsafe { std::env::set_var("ENCRYPTION_SECRET", "") }

        match Config::from_env() {
            Err(e) => assert!(
                e.to_string().contains("ENCRYPTION_SECRET"),
                "error message should mention ENCRYPTION_SECRET, got: {e}"
            ),
            Ok(_) => panic!("expected error for empty ENCRYPTION_SECRET"),
        }

        clear_required_env_vars();
    }

    #[test]
    fn encryption_secret_rejects_short() {
        let _guard = ENV_MUTEX.lock().unwrap();
        set_required_env_vars();
        unsafe { std::env::set_var("ENCRYPTION_SECRET", "tooshort") }

        match Config::from_env() {
            Err(e) => assert!(
                e.to_string().contains("ENCRYPTION_SECRET"),
                "error message should mention ENCRYPTION_SECRET, got: {e}"
            ),
            Ok(_) => panic!("expected error for short ENCRYPTION_SECRET"),
        }

        clear_required_env_vars();
    }

    #[test]
    fn encryption_secret_accepts_exactly_32_bytes() {
        let _guard = ENV_MUTEX.lock().unwrap();
        set_required_env_vars();
        unsafe { std::env::set_var("ENCRYPTION_SECRET", "a".repeat(32)) }

        let result = Config::from_env();
        assert!(
            result.is_ok(),
            "expected success for 32-byte ENCRYPTION_SECRET"
        );

        clear_required_env_vars();
    }
}
