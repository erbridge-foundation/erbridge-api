# erbridge-api Codebase Reference

ER Bridge backend — an EVE Online wormhole mapping tool. Rust/Axum HTTP API backed by PostgreSQL.
Status: early-stage work in progress, not yet usable. License: AGPL-3.0.

---

## Project Structure

```
erbridge-api/
├── Cargo.toml
├── migrations/
│   ├── 0001_create_account.sql
│   ├── 0002_create_eve_character.sql
│   └── 0003_create_sde_solar_system.sql
└── src/
    ├── main.rs                     # Entry point + startup sequence
    ├── lib.rs                      # Router construction + module declarations
    ├── config.rs                   # Config struct, env parsing, key derivation
    ├── state.rs                    # AppState (shared via Arc<AppState>)
    ├── middleware.rs               # require_active_account middleware (not yet applied)
    ├── extractors.rs               # AccountId extractor (JWT cookie → Uuid)
    ├── crypto.rs                   # AES-256-GCM encrypt/decrypt
    ├── db/
    │   ├── account.rs              # Account CRUD
    │   ├── character.rs            # EVE character CRUD + ghost claim
    │   └── sde_solar_system.rs     # SDE solar system queries
    ├── dto/
    │   ├── auth.rs                 # AuthMode, SessionClaims, StateClaims, MeResponse
    │   ├── envelope.rs             # ApiResponse<T> — {"data":…} or {"error":"…"}
    │   └── health.rs               # HealthResponse, ComponentState
    ├── esi/
    │   ├── mod.rs                  # esi_request() retry wrapper (429 + backoff)
    │   ├── discovery.rs            # EVE SSO well-known discovery → EsiMetadata
    │   ├── jwks.rs                 # JWK set fetch + RS256 EVE JWT verification
    │   ├── character.rs            # GET /characters/{id}/  (corp + alliance IDs)
    │   ├── search.rs               # GET /characters/{id}/search/
    │   ├── token.rs                # ensure_token_fresh() — check expiry + refresh
    │   └── universe.rs             # POST /universe/names/  (ID → name resolution)
    ├── handlers/
    │   ├── auth.rs                 # login, add_character, callback, logout, me (unrouted)
    │   ├── health.rs               # GET /api/health
    │   └── images.rs               # GET /api/v1/images/{category}/{id}/{variation}
    ├── services/
    │   ├── auth.rs                 # login_or_register, attach_character_to_account
    │   ├── images.rs               # Filesystem image cache (1-hour TTL)
    │   └── sde_solar_system.rs     # SDE download, parse, bulk upsert + update check
    └── tasks/
        └── image_cache_cleanup.rs  # Background task: purge stale cache files every 2h
```

---

## Startup Sequence (`main.rs`)

1. Load `.env` via dotenvy
2. Init tracing (`RUST_LOG`, default `erbridge_api=info`)
3. Parse `Config::from_env()`
4. Connect to PostgreSQL (max 5 connections), run sqlx migrations
5. Build `reqwest::Client` (10s timeout, rustls)
6. `services::sde_solar_system::load_sde_if_needed()` — download SDE on first run
7. `esi::discovery::discover()` — fetch EVE SSO OpenID Connect well-known document
8. `esi::jwks::fetch_jwks()` — load JWK set, store in `Arc<RwLock<JwkSet>>`
9. Spawn background tasks: image cache cleanup, SDE update check
10. Build router via `erbridge_api::router()`, bind `0.0.0.0:8080`

---

## API Routes

| Method | Path | Handler | Auth |
|--------|------|---------|------|
| GET | `/api/health` | `handlers::health::health` | No |
| GET | `/auth/login` | `handlers::auth::login` | No |
| GET | `/auth/callback` | `handlers::auth::callback` | No |
| GET | `/api/v1/images/{category}/{id}/{variation}` | `handlers::images::image` | No |
| GET | `/api/v1/me` | `handlers::auth::me` | `require_active_account` |
| DELETE | `/api/v1/me` | `handlers::character::delete_account` | `require_active_account` |
| GET | `/auth/characters/add` | `handlers::auth::add_character` | `require_active_account` |
| POST | `/auth/logout` | `handlers::auth::logout` | `require_active_account` |

Unused imports in `lib.rs`: `patch`, `put`, `HashMap` — markers for planned routes.
`handlers::character` module is referenced but not yet implemented.

### Response Envelope

Success: `{"data": <T>}`  
Error: `{"error": "<message>"}`  
Health: `{"status": "ok"|"degraded", "version": "0.1.0", "components": {"database": "ok"|"degraded"}}`

### Image Proxy

`GET /api/v1/images/{category}/{id}/{variation}?size={u32}&tenant={string}`

Valid combos: `characters/{id}/portrait`, `corporations/{id}/logo`, `alliances/{id}/logo`,
`types/{id}/render|icon|bp|bpc|relic`. Proxied to `images.evetech.net`. 404 on invalid combo,
502 on upstream failure. Response: `Cache-Control: public, max-age=3600`.

---

## Authentication

### Session Cookie
- Name: `erbridge_session`; `HttpOnly`, `Secure`, `SameSite=Lax`, `Path=/`
- Value: HS256 JWT signed with `jwt_key`, 7-day TTL
- Claims: `{ account_id: Uuid, exp: u64 }`

### Key Derivation (from `ENCRYPTION_SECRET`)
- `aes_key = SHA256(secret)` — AES-256-GCM for token storage
- `jwt_key = SHA256("erbridge:jwt:" + secret)` — HS256 session/state JWTs

### EVE SSO OAuth Flow
1. `/auth/login` → build state JWT (HS256, 5-min TTL, contains `client_id` + `mode`) → redirect to EVE SSO
2. EVE SSO → `/auth/callback?code=…&state=…`
3. Verify state JWT, exchange code, verify EVE access token (RS256 against CCP JWK set)
4. `services::auth::login_or_register` → set `erbridge_session` cookie → redirect to `{FRONTEND_URL}/`
5. `/auth/logout` → clear cookie → 204

### ESI Client Pool
Multiple clients via `ESI_CLIENT_ID_N`/`ESI_CLIENT_SECRET_N`. One chosen randomly per redirect,
bound in the state JWT so the callback uses the correct secret.

### EVE JWT Verification (ADR-030)
Accepted issuers: `https://login.eveonline.com`, `https://login.eveonline.com/`, `login.eveonline.com`.
Audience must include `client_id` and `"EVE Online"`. On verification failure, re-fetches JWK set
once and retries (handles CCP key rotation).

---

## Database Schema

### `account`
| Column | Type | Notes |
|--------|------|-------|
| id | UUID PK | gen_random_uuid() |
| created_at / updated_at | TIMESTAMPTZ | |
| status | TEXT | `active` \| `pending_delete` |
| delete_requested_at | TIMESTAMPTZ | nullable |

### `eve_character`
| Column | Type | Notes |
|--------|------|-------|
| id | UUID PK | |
| account_id | UUID FK → account | NULL = "ghost character" |
| eve_character_id | BIGINT UNIQUE | CCP's character ID |
| name | TEXT | |
| corporation_id | BIGINT | |
| alliance_id | BIGINT | nullable |
| is_main | BOOLEAN | one-per-account enforced by partial unique index |
| encrypted_access_token | BYTEA | AES-256-GCM; nonce prepended |
| encrypted_refresh_token | BYTEA | AES-256-GCM; nonce prepended |
| esi_token_expires_at | TIMESTAMPTZ | nullable |
| created_at / updated_at | TIMESTAMPTZ | `updated_at` used as refresh-token age proxy |

Partial unique index: `eve_character_one_main_per_account ON eve_character(account_id) WHERE is_main = true`.

**Ghost character (ADR-031):** `account_id = NULL`. Allows adding a character to an ACL before first
login. On first login the row is atomically claimed (account_id, tokens, is_main set in one transaction).

### `sde_solar_system`
Large table. Key columns: `solar_system_id BIGINT PK`, `name TEXT`, `security_status REAL`,
`security_class TEXT`, `wh_class TEXT` (preserved across SDE updates via COALESCE), `region_id`,
`constellation_id`, `faction_id`. Also stores `name_i18n`, `planet_ids`, `stargate_ids`, `position`
as JSONB. Companion singleton table `sde_solar_system_metadata(id=1, sde_version, sde_checksum, loaded_at)`.

---

## Key Patterns and ADRs

| ADR | Pattern |
|-----|---------|
| ADR-009 | Accept three issuer forms for EVE JWTs |
| ADR-021 | `ApiResponse<T>` envelope — `{"data":…}` or `{"error":"…"}` |
| ADR-029 | Refresh token age: use `eve_character.updated_at`; re-auth if `> ESI_REFRESH_TOKEN_MAX_DAYS` (default 7d) |
| ADR-030 | JWKS rotation retry on verification failure |
| ADR-031 | Ghost character — `account_id = NULL` rows claimable at first login |

### Bulk DB Operations
`db::character::bulk_update_corp_alliance` and `db::sde_solar_system::bulk_upsert_solar_systems`
use `UNNEST($1::type[], …)` for single-query bulk ops.

### ESI Retry (`esi::mod::esi_request`)
Up to 4 retries on 429. Respects `Retry-After` header; otherwise exponential backoff with ±25% jitter, capped at 30s.

### Error Handling
- Service/DB: `anyhow::Result<T>` with `.context("…")` chains
- Handlers: map errors to `StatusCode` + `warn!(error = %e, …)` log
- `services::images`: typed `ImageError` enum via `thiserror`

---

## Configuration

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `DATABASE_URL` | Yes | — | PostgreSQL connection string |
| `APP_URL` | Yes | — | Public-facing base URL (no trailing slash) |
| `ENCRYPTION_SECRET` | Yes | — | Source for AES + JWT key derivation |
| `ESI_CLIENT_ID` / `ESI_CLIENT_SECRET` | Yes* | — | Single ESI client |
| `ESI_CLIENT_ID_N` / `ESI_CLIENT_SECRET_N` | Yes* | — | Multi-client (overrides single) |
| `ESI_CALLBACK_URL` | No | `{APP_URL}/auth/callback` | OAuth callback |
| `FRONTEND_URL` | No | `{APP_URL}` | Post-login redirect |
| `IMAGE_CACHE_DIR` | No | `{TMPDIR}/erbridge-images` | Image cache directory |
| `ACCOUNT_DELETION_GRACE_DAYS` | No | `30` | Days before hard-deleting pending-delete accounts |
| `ESI_BASE_URL` | No | `https://esi.evetech.net/latest` | ESI base URL (override for tests) |
| `ESI_REFRESH_TOKEN_MAX_DAYS` | No | `7` | Refresh token age limit |

---

## Background Tasks

| Task | Interval | Action |
|------|----------|--------|
| Image cache cleanup | Every 2h | Remove cache files older than 1h |
| SDE update check | 10-min initial delay, then every 24h | Compare CCP build number; re-download + upsert if changed |

---

## External Services

| Service | URL | Purpose |
|---------|-----|---------|
| EVE SSO | `https://login.eveonline.com` | OAuth2 + JWK set |
| ESI | `https://esi.evetech.net/latest` | Character/corp/alliance data, search |
| EVE Image Server | `https://images.evetech.net` | Character portraits, logos |
| CCP SDE | `https://developers.eveonline.com/static-data/tranquility/` | Solar system static data |

Required ESI scopes: `esi-location.read_location.v1`, `esi-location.read_ship_type.v1`,
`esi-location.read_online.v1`, `esi-search.search_structures.v1`, `esi-ui.write_waypoint.v1`.

---

## Dependencies (key)

| Crate | Role |
|-------|------|
| `axum` 0.8 / `axum-extra` 0.12 | HTTP framework + cookie jar |
| `tokio` 1 (full) | Async runtime |
| `sqlx` 0.8 | PostgreSQL driver, compile-time queries, migrations |
| `serde` / `serde_json` | Serialization |
| `uuid` 1 | v4 + v7 UUIDs |
| `jsonwebtoken` 10 | JWT encode/decode, JWK set |
| `aes-gcm` 0.10 | Token encryption |
| `sha2` 0.11 | Key derivation |
| `reqwest` 0.13 | HTTP client (rustls) |
| `chrono` 0.4 | Timestamps |
| `anyhow` / `thiserror` | Error handling |
| `tracing` / `tracing-subscriber` | Structured logging |
| `strum` 0.28 | Enum ↔ string for DB status fields |
| `zip` 2 | SDE ZIP extraction |
| Dev: `axum-test`, `pg-embed`, `wiremock`, `portpicker`, `cookie` | Test infrastructure |
