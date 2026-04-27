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
│   ├── 0003_create_sde_solar_system.sql
│   ├── 0004_create_audit_log.sql
│   ├── 0005_create_maps_core.sql
│   ├── 0006_create_map_events_checkpoints.sql
│   ├── 0007_create_system_edges_view.sql
│   ├── 0008_create_acl.sql
│   ├── 0009_create_acl_member.sql
│   └── 0010_create_map_acl.sql
├── tests/
│   ├── common/mod.rs               # Shared test helpers (pg-embed, wiremock setup)
│   ├── audit_log.rs                # Integration tests for audit log
│   ├── map_handler_test.rs         # Integration tests for map HTTP handlers
│   ├── map_service_test.rs         # Integration tests for map service layer
│   ├── test_acl_handlers.rs        # Integration tests for ACL HTTP handlers
│   ├── test_db_acl.rs              # Integration tests for ACL DB layer
│   ├── test_permissions.rs         # Integration tests for permission resolution
│   └── test_service_acl.rs         # Integration tests for ACL service layer
└── src/
    ├── main.rs                     # Entry point + startup sequence
    ├── lib.rs                      # Router construction + module declarations
    ├── config.rs                   # Config struct, env parsing, key derivation
    ├── state.rs                    # AppState (shared via Arc<AppState>)
    ├── middleware.rs               # require_active_account middleware
    ├── extractors.rs               # AccountId extractor (JWT cookie → Uuid)
    ├── crypto.rs                   # AES-256-GCM encrypt/decrypt
    ├── permissions.rs              # effective_permission() — map ACL resolution (ADR-026)
    ├── audit.rs                    # AuditEvent enum + record_in_tx()
    ├── db/
    │   ├── mod.rs
    │   ├── account.rs              # Account CRUD
    │   ├── character.rs            # EVE character CRUD + ghost claim + online status
    │   ├── sde_solar_system.rs     # SDE solar system queries
    │   ├── acl.rs                  # ACL CRUD + orphan lifecycle (ADR-028)
    │   ├── acl_member.rs           # ACL member CRUD (Character/Corporation/Alliance)
    │   ├── map.rs                  # Map CRUD (soft-delete)
    │   ├── map_acl.rs              # Map–ACL junction (attach/detach)
    │   ├── map_types.rs            # Enums: ConnectionStatus, LifeState, MassState, SignatureStatus, Side
    │   ├── connection.rs           # Connection + ConnectionEnd CRUD, status recomputation
    │   ├── signature.rs            # Signature CRUD
    │   ├── map_event.rs            # Append-only map event log
    │   ├── map_checkpoint.rs       # Map state snapshots (JSONB)
    │   └── route.rs                # Recursive CTE route finder (system_edges view)
    ├── dto/
    │   ├── mod.rs
    │   ├── auth.rs                 # AuthMode, SessionClaims, StateClaims, MeResponse
    │   ├── envelope.rs             # ApiResponse<T> — {"data":…} or {"error":"…"}
    │   ├── health.rs               # HealthResponse, ComponentState
    │   ├── character.rs            # CharacterResponse, CharacterListResponse
    │   ├── acl.rs                  # AclResponse, AclMemberResponse + request types
    │   └── map.rs                  # MapResponse, ConnectionResponse, SignatureResponse + request types
    ├── esi/
    │   ├── mod.rs                  # esi_request() retry wrapper (429 + backoff)
    │   ├── discovery.rs            # EVE SSO well-known discovery → EsiMetadata
    │   ├── jwks.rs                 # JWK set fetch + RS256 EVE JWT verification
    │   ├── character.rs            # GET /characters/{id}/ (corp + alliance IDs)
    │   ├── search.rs               # GET /characters/{id}/search/
    │   ├── token.rs                # ensure_token_fresh() — check expiry + refresh
    │   └── universe.rs             # POST /universe/names/ (ID → name resolution)
    ├── handlers/
    │   ├── mod.rs
    │   ├── auth.rs                 # login, add_character, callback, logout, me
    │   ├── character.rs            # list, remove, set_main, delete_account + admin stubs
    │   ├── health.rs               # GET /api/health
    │   ├── images.rs               # GET /api/v1/images/{category}/{id}/{variation}
    │   ├── acl.rs                  # Full ACL + member CRUD handlers
    │   ├── map.rs                  # Full map/connection/signature/route handlers
    │   └── debug.rs                # GET /debug/location-subscribe/:character_id (temp)
    ├── services/
    │   ├── mod.rs
    │   ├── auth.rs                 # login_or_register, attach_character_to_account
    │   ├── acl.rs                  # ACL + member management with permission checks
    │   ├── map.rs                  # Map management + connection/signature/route operations
    │   ├── images.rs               # Filesystem image cache (1-hour TTL)
    │   └── sde_solar_system.rs     # SDE download, parse, bulk upsert + update check
    └── tasks/
        ├── mod.rs
        ├── image_cache_cleanup.rs  # Background task: purge stale cache files every 2h
        ├── character_online_poll.rs # Background task: poll ESI online status per character
        ├── character_location_poll.rs # Background task: poll ESI location, broadcast events
        └── map_checkpoint.rs       # Background task: snapshot map state to JSONB
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
9. Build a minimal `AppState` for pollers (placeholder `online_poll_tx` channel)
10. Spawn background tasks: image cache cleanup, SDE update check, online poller, location poller, map checkpoint
11. Build router via `erbridge_api::router()` (gets the real `online_poll_tx` from the online poller), bind `0.0.0.0:8080`

> **Note:** The poller `AppState` uses a throwaway channel for `online_poll_tx`; the router `AppState`
> uses the real sender from `spawn_online_poller`. This means the two `AppState` instances differ
> slightly. See known issues.

---

## API Routes

### Public / Unauthenticated

| Method | Path | Handler |
|--------|------|---------|
| GET | `/api/health` | `handlers::health::health` |
| GET | `/auth/login` | `handlers::auth::login` |
| GET | `/auth/callback` | `handlers::auth::callback` |
| GET | `/api/v1/images/{category}/{id}/{variation}` | `handlers::images::image` |
| GET | `/debug/location-subscribe/{character_id}` | `handlers::debug::location_subscribe` (**debug builds only**) |

### Authenticated (`require_active_account` middleware applied)

| Method | Path | Handler | Min Permission |
|--------|------|---------|----------------|
| GET | `/api/v1/me` | `handlers::auth::me` | active account |
| DELETE | `/api/v1/me` | `handlers::character::delete_account` | active account |
| GET | `/auth/characters/add` | `handlers::auth::add_character` | active account |
| POST | `/auth/logout` | `handlers::auth::logout` | active account |
| GET | `/api/v1/characters` | `handlers::character::list_characters` | active account |
| DELETE | `/api/v1/characters/{id}` | `handlers::character::remove_character` | active account |
| PUT | `/api/v1/characters/{id}/main` | `handlers::character::set_main` | active account |
| GET | `/api/v1/maps` | `handlers::map::list_maps_handler` | active account |
| POST | `/api/v1/maps` | `handlers::map::create_map_handler` | active account |
| GET | `/api/v1/maps/{map_id}` | `handlers::map::get_map_handler` | Read |
| PATCH | `/api/v1/maps/{map_id}` | `handlers::map::update_map_handler` | Manage |
| DELETE | `/api/v1/maps/{map_id}` | `handlers::map::delete_map_handler` | Admin |
| POST | `/api/v1/maps/{map_id}/acls` | `handlers::map::attach_acl` | Admin |
| DELETE | `/api/v1/maps/{map_id}/acls/{acl_id}` | `handlers::map::detach_acl` | Admin |
| POST | `/api/v1/maps/{map_id}/connections` | `handlers::map::create_connection` | ReadWrite |
| DELETE | `/api/v1/maps/{map_id}/connections/{conn_id}` | `handlers::map::delete_connection_handler` | ReadWrite |
| POST | `/api/v1/maps/{map_id}/signatures` | `handlers::map::add_signature` | ReadWrite |
| DELETE | `/api/v1/maps/{map_id}/signatures/{sig_id}` | `handlers::map::delete_signature_handler` | ReadWrite |
| POST | `/api/v1/maps/{map_id}/connections/{conn_id}/link` | `handlers::map::link_signature` | ReadWrite |
| PATCH | `/api/v1/maps/{map_id}/connections/{conn_id}/metadata` | `handlers::map::update_connection_metadata` | ReadWrite |
| GET | `/api/v1/maps/{map_id}/routes` | `handlers::map::find_routes` | Read |
| GET | `/api/v1/acls` | `handlers::acl::list_acls` | active account |
| POST | `/api/v1/acls` | `handlers::acl::create` | active account |
| PUT | `/api/v1/acls/{acl_id}` | `handlers::acl::rename` | Admin (ACL) |
| DELETE | `/api/v1/acls/{acl_id}` | `handlers::acl::delete` | Admin (ACL) |
| GET | `/api/v1/acls/{acl_id}/members` | `handlers::acl::list_members` | Manage (ACL) |
| POST | `/api/v1/acls/{acl_id}/members` | `handlers::acl::add` | Manage (ACL) |
| PATCH | `/api/v1/acls/{acl_id}/members/{member_id}` | `handlers::acl::update_member` | Manage (ACL) |
| DELETE | `/api/v1/acls/{acl_id}/members/{member_id}` | `handlers::acl::delete_member` | Manage (ACL) |

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
| is_online | BOOLEAN | nullable; updated by online poller |
| esi_client_id | TEXT | nullable; which ESI client authenticated this character |
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

### `audit_log`
| Column | Type | Notes |
|--------|------|-------|
| id | BIGSERIAL PK | |
| occurred_at | TIMESTAMPTZ | default now() |
| actor_account_id | UUID FK → account | nullable (system events) |
| event_type | TEXT | snake_case string |
| details | JSONB | event-specific payload |

Indexes on `occurred_at DESC`, `actor_account_id`.

### `map`
| Column | Type | Notes |
|--------|------|-------|
| id | UUID PK | gen_random_uuid() |
| name | TEXT | |
| slug | TEXT UNIQUE | kebab-case, validated by regex |
| owner_account_id | UUID FK → account | ON DELETE SET NULL |
| description | TEXT | nullable |
| deleted | BOOLEAN | soft-delete flag |
| last_checkpoint_seq | BIGINT | tracks latest checkpointed event seq |
| last_checkpoint_at | TIMESTAMPTZ | nullable |
| retention_days | INT | default 14 |
| created_at / updated_at | TIMESTAMPTZ | |

### `map_connections`
| Column | Type | Notes |
|--------|------|-------|
| connection_id | UUID PK | |
| map_id | UUID FK → map | CASCADE |
| status | TEXT | `tentative`\|`partial`\|`linked`\|`fully_linked`\|`collapsed`\|`expired` |
| life_state | TEXT | `fresh`\|`eol`\|NULL |
| mass_state | TEXT | `stable`\|`reduced`\|`critical`\|NULL |
| extra | JSONB | default `{}` |
| created_at / updated_at | TIMESTAMPTZ | |

Initial default status is `partial` (not `tentative` — see known issues).

### `map_connection_ends`
| Column | Type | Notes |
|--------|------|-------|
| connection_id | UUID FK → map_connections | CASCADE, composite PK |
| side | TEXT | `a`\|`b`, composite PK |
| system_id | BIGINT FK → sde_solar_system | |
| signature_id | UUID UNIQUE FK → map_signatures | nullable, ON DELETE SET NULL; DEFERRABLE |
| wormhole_code | TEXT | nullable |
| created_at / updated_at | TIMESTAMPTZ | |

### `map_signatures`
| Column | Type | Notes |
|--------|------|-------|
| signature_id | UUID PK | |
| map_id | UUID FK → map | CASCADE |
| system_id | BIGINT FK → sde_solar_system | |
| sig_code | TEXT | |
| sig_type | TEXT | |
| status | TEXT | `visible`\|`resolved`\|`expired`\|`deleted` |
| connection_id / connection_side | UUID / TEXT | nullable; composite FK → map_connection_ends; DEFERRABLE |
| wormhole_code | TEXT | nullable |
| derived_life_state / derived_mass_state | TEXT | propagated from connection |
| extra | JSONB | default `{}` |
| created_at / updated_at | TIMESTAMPTZ | |

Partial unique index on `(map_id, system_id, sig_code) WHERE status IN ('visible','resolved')`.

### `map_events`
| Column | Type | Notes |
|--------|------|-------|
| seq | BIGSERIAL PK | monotone per table (not per map) |
| map_id | UUID FK → map | CASCADE |
| entity_type / entity_id | TEXT | e.g. `"connection"` / UUID string |
| event_type | TEXT | PascalCase string |
| actor_account_id | TEXT | UUID string; nullable |
| payload | JSONB | |
| occurred_at | TIMESTAMPTZ | default now() |

### `map_checkpoints`
| Column | Type | Notes |
|--------|------|-------|
| checkpoint_id | BIGSERIAL PK | |
| map_id | UUID FK → map | CASCADE |
| last_included_seq | BIGINT UNIQUE per map | highest event seq in snapshot |
| checkpoint_version | INT | schema version |
| event_count | INT | events since prior checkpoint |
| checksum | TEXT | nullable |
| state | JSONB | full snapshot (connections, ends, signatures) |
| created_at | TIMESTAMPTZ | |

### `acl`
| Column | Type | Notes |
|--------|------|-------|
| id | UUID PK | |
| name | TEXT | |
| owner_account_id | UUID FK → account | nullable |
| pending_delete_at | TIMESTAMPTZ | nullable; set at creation; cleared on first map attach |
| created_at / updated_at | TIMESTAMPTZ | |

### `acl_member`
| Column | Type | Notes |
|--------|------|-------|
| id | UUID PK | |
| acl_id | UUID FK → acl | CASCADE |
| member_type | TEXT | `character`\|`corporation`\|`alliance` |
| eve_entity_id | BIGINT | corp/alliance CCP ID; NULL for character members |
| character_id | UUID FK → eve_character | NULL for corp/alliance members |
| name | TEXT | display name (resolved at insert) |
| permission | TEXT | `read`\|`read_write`\|`manage`\|`admin`\|`deny` |
| created_at / updated_at | TIMESTAMPTZ | |

### `map_acl`
| Column | Type | Notes |
|--------|------|-------|
| map_id | UUID FK → map | composite PK |
| acl_id | UUID FK → acl | composite PK |

### `system_edges` (VIEW)
Bidirectional edge list derived from `map_connections` + `map_connection_ends`. Used by the
recursive CTE route finder. Columns: `map_id`, `connection_id`, `status`, `life_state`,
`mass_state`, `from_system_id`, `to_system_id`.

---

## Key Patterns and ADRs

| ADR | Pattern |
|-----|---------|
| ADR-009 | Accept three issuer forms for EVE JWTs |
| ADR-021 | `ApiResponse<T>` envelope — `{"data":…}` or `{"error":"…"}` |
| ADR-026 | Map permission resolution: owner → Admin; deny is hard stop; else most-permissive grant |
| ADR-028 | ACL orphan lifecycle: `pending_delete_at` set at creation; cleared on first map attach |
| ADR-029 | Refresh token age: use `eve_character.updated_at`; re-auth if `> ESI_REFRESH_TOKEN_MAX_DAYS` (default 7d) |
| ADR-030 | JWKS rotation retry on verification failure |
| ADR-031 | Ghost character — `account_id = NULL` rows claimable at first login |

### Permission Model (ADR-026)

`permissions::effective_permission(pool, account_id, map_id) -> Option<Permission>`

- Owner of the map → `Admin` always
- Any `deny` entry in any attached ACL matching the account (by character, corp, or alliance) → `None`
- Otherwise → most permissive grant across all matching ACL entries

`Permission` enum: `Read < ReadWrite < Manage < Admin` (derived `Ord`).

ACL-level permission (for ACL management operations) is separate from map-level permission and is
resolved inline in `services::acl`.

### Bulk DB Operations
`db::character::bulk_update_corp_alliance` and `db::sde_solar_system::bulk_upsert_solar_systems`
use `UNNEST($1::type[], …)` for single-query bulk ops.

### ESI Retry (`esi::mod::esi_request`)
Up to 4 retries on 429. Respects `Retry-After` header; otherwise exponential backoff with ±25% jitter, capped at 30s.

### Error Handling
- Service/DB: `anyhow::Result<T>` with `.context("…")` chains
- Handlers: map errors to `StatusCode` + `warn!(error = %e, …)` log
- `services::map`: typed `MapError` enum via `thiserror` (includes `From<sqlx::Error>` for FK violations)
- `services::acl`: typed `AclError` enum via `thiserror`
- `services::images`: typed `ImageError` enum via `thiserror`

### Map Events
Every mutation to a map appends to `map_events` within the same transaction. Event types include:
`MapCreated`, `ConnectionCreated`, `ConnectionDeleted`, `SignatureAdded`, `SignatureDeleted`,
`SignatureLinkedToConnectionEnd`, `ConnectionMetadataUpdated`.

### Map Checkpoints (`tasks::map_checkpoint`)
Runs on `MAP_CHECKPOINT_INTERVAL_MINS` (default 60). Finds maps where `max(map_events.seq) > map.last_checkpoint_seq`,
snapshots connections + ends + signatures as JSONB, inserts a `map_checkpoints` row, updates `map.last_checkpoint_seq`.

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
| `ESI_POLL_CONCURRENCY` | No | `10` | Max concurrent ESI requests for location poller |
| `ESI_POLL_BATCH_SIZE` | No | `10` | Characters per batch for online poller |
| `ESI_POLL_BATCH_DELAY_MS` | No | `500` | Min ms between online poll batches (clamped ≥ 100) |
| `MAP_CHECKPOINT_INTERVAL_MINS` | No | `60` | How often the checkpoint task runs |

---

## Background Tasks

| Task | Interval | Action |
|------|----------|--------|
| Image cache cleanup | Every 2h | Remove cache files older than 1h |
| SDE update check | 10-min initial delay, then every 24h | Compare CCP build number; re-download + upsert if changed |
| Online poller | Adaptive (ESI Cache-Control max-age, default 60s) | Poll ESI `/characters/{id}/online/` per character; update `is_online` on change |
| Location poller | Adaptive (ESI Cache-Control max-age, default 5s) | Poll ESI `/characters/{id}/location/` only for characters with active subscribers; broadcast `LocationEvent` |
| Map checkpoint | `MAP_CHECKPOINT_INTERVAL_MINS` (default 60 min) | Snapshot map state to JSONB for maps with new events |
| Purge | Every 24h | Hard-delete expired accounts + orphaned ACLs past grace period |

---

## External Services

| Service | URL | Purpose |
|---------|-----|---------|
| EVE SSO | `https://login.eveonline.com` | OAuth2 + JWK set |
| ESI | `https://esi.evetech.net/latest` | Character/corp/alliance data, search, online/location polling |
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
| `dashmap` 6 | Concurrent map for location subscriptions |
| `once_cell` 1 | Lazy regex init in dto/map.rs |
| `regex` 1 | Slug validation |
| `validator` 0.20 | Request struct validation (`#[validate]`) |
| `futures` 0.3 | Async utilities |
| `tokio-stream` 0.1 | Stream adapters |
| Dev: `axum-test`, `pg-embed`, `wiremock`, `portpicker`, `cookie` | Test infrastructure |

---

## Known Issues and Gaps

### Router / Handler Gaps
- Admin-role routes (`admin_purge_account`, `admin_restore_account`) are implemented as stubs that always return 403; they are not registered in `lib.rs`.

### Architectural / Design Issues
- **ACL permission bypass in `require_acl_permission`:** Only direct character membership is checked. Corporation/alliance ACL membership does not grant ACL management rights — that is intentional per the design (manage/admin are character-only), but the code does not have a comment to that effect.
- **`map_connection_ends.signature_id` is UNIQUE but `connection_id` has only two ends:** The UNIQUE constraint means one signature can only be linked to one end, which is correct, but there is no DB constraint preventing two different ends from being in the `fully_linked` state while one signature is already doubly-referenced.

### Code Quality
- `rand` 0.10 is used for ESI client selection; version 0.10 is a pre-release series (downgrade to 0.9 causes compile failures, so it stays).
