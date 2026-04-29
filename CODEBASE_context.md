# erbridge-api Codebase Context

Rust/Axum HTTP API backed by PostgreSQL. EVE Online wormhole mapping tool.

---

## Source Layout

```
src/
  main.rs                      # entry point
  lib.rs                       # router construction + module declarations
  config.rs                    # Config struct + env parsing
  state.rs                     # AppState (Arc<AppState>)
  middleware.rs                # require_active_account, require_server_admin
  extractors.rs                # AccountId, ServerAdmin (JWT cookie → Uuid)
  crypto.rs                    # AES-256-GCM encrypt/decrypt
  permissions.rs               # effective_permission() — map ACL resolution
  audit.rs                     # AuditEvent enum + record_in_tx()
  db/
    account.rs                 # account CRUD
    character.rs               # EVE character CRUD + ghost claim + online status
    sde_solar_system.rs        # SDE solar system queries
    acl.rs                     # ACL CRUD + orphan lifecycle
    acl_member.rs              # ACL member CRUD
    map.rs                     # map CRUD (soft-delete)
    map_acl.rs                 # map–ACL junction (attach/detach)
    map_types.rs               # enums: ConnectionStatus, LifeState, MassState, SignatureStatus, Side
    connection.rs              # connection + connection_end CRUD, status recomputation
    signature.rs               # signature CRUD
    map_event.rs               # append-only map event log
    map_checkpoint.rs          # map state snapshots (JSONB)
    route.rs                   # recursive CTE route finder (system_edges view)
  dto/
    admin.rs                   # AccountResponse, BlockedCharacterResponse, AuditLogEntry + request types
    auth.rs                    # AuthMode, SessionClaims, StateClaims, MeResponse
    envelope.rs                # ApiResponse<T>
    health.rs                  # HealthResponse, ComponentState
    character.rs               # CharacterResponse, CharacterListResponse
    acl.rs                     # AclResponse, AclMemberResponse + request types
    map.rs                     # MapResponse, ConnectionResponse, SignatureResponse + request types
  esi/
    mod.rs                     # esi_request() retry wrapper
    discovery.rs               # EVE SSO well-known → EsiMetadata
    jwks.rs                    # JWK set fetch + RS256 verification
    character.rs               # GET /characters/{id}/
    search.rs                  # GET /characters/{id}/search/
    token.rs                   # ensure_token_fresh()
    universe.rs                # POST /universe/names/
  handlers/
    admin.rs                   # /api/v1/admin/*
    auth.rs                    # login, add_character, callback, logout, me
    character.rs               # list, remove, set_main, delete_account
    health.rs                  # GET /api/health
    acl.rs                     # ACL + member CRUD
    map.rs                     # map/connection/signature/route handlers
  services/
    admin.rs                   # accounts, blocked chars, maps, acls, audit log
    auth.rs                    # login_or_register, attach_character_to_account
    acl.rs                     # ACL + member management with permission checks
    map.rs                     # map management + connection/signature/route ops
    sde_solar_system.rs        # SDE download, parse, bulk upsert + update check
  tasks/
    character_online_poll.rs   # background: poll ESI online status
    character_location_poll.rs # background: poll ESI location, broadcast events
    map_checkpoint.rs          # background: snapshot map state to JSONB
tests/
  common/mod.rs                # setup_db, test_state, make_session_jwt
  audit_log.rs
  map_handler_test.rs
  map_service_test.rs
  test_acl_handlers.rs
  test_admin_audit_log.rs
  test_admin_integration.rs
  test_api_key.rs
  test_auth_handlers.rs
  test_character_handlers.rs
  test_db_acl.rs
  test_esi_retry.rs
  test_permissions.rs
  test_service_acl.rs
```

---

## API Routes

### Public

| Method | Path | Handler |
|--------|------|---------|
| GET | `/api/health` | `handlers::health::health` |
| GET | `/auth/login` | `handlers::auth::login` |
| GET | `/auth/callback` | `handlers::auth::callback` |

### Authenticated (`require_active_account`)

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

### Server Admin (`require_active_account` + `require_server_admin`)

| Method | Path | Handler |
|--------|------|---------|
| GET | `/api/v1/admin/accounts` | `handlers::admin::list_accounts` |
| POST | `/api/v1/admin/accounts/{account_id}/grant-admin` | `handlers::admin::grant_admin` |
| POST | `/api/v1/admin/accounts/{account_id}/revoke-admin` | `handlers::admin::revoke_admin` |
| POST | `/api/v1/admin/accounts/{account_id}/purge` | `handlers::admin::purge_account` |
| POST | `/api/v1/admin/accounts/{account_id}/restore` | `handlers::admin::restore_account` |
| GET | `/api/v1/admin/maps` | `handlers::admin::list_maps` |
| PATCH | `/api/v1/admin/maps/{map_id}/owner` | `handlers::admin::change_map_owner` |
| DELETE | `/api/v1/admin/maps/{map_id}` | `handlers::admin::hard_delete_map` |
| GET | `/api/v1/admin/acls` | `handlers::admin::list_acls` |
| PATCH | `/api/v1/admin/acls/{acl_id}/owner` | `handlers::admin::change_acl_owner` |
| DELETE | `/api/v1/admin/acls/{acl_id}` | `handlers::admin::hard_delete_acl` |
| GET | `/api/v1/admin/characters/blocked` | `handlers::admin::list_blocked_characters` |
| POST | `/api/v1/admin/characters/{eve_id}/block` | `handlers::admin::block_character` |
| DELETE | `/api/v1/admin/characters/{eve_id}/block` | `handlers::admin::unblock_character` |
| GET | `/api/v1/admin/audit-log` | `handlers::admin::list_audit_log` |

Audit-log query params: `event_type`, `actor_account_id`, `before` (cursor), `limit` (default 50, max 200).

### Response Envelope

```
{"data": <T>}           // success
{"error": "<message>"}  // error
```

---

## Authentication

- Session cookie `erbridge_session`: `HttpOnly`, `Secure`, `SameSite=Lax`, `Path=/`; HS256 JWT, 7-day TTL, claims `{ account_id: Uuid, exp: u64 }`
- Key derivation from `ENCRYPTION_SECRET`: `aes_key = SHA256(secret)`, `jwt_key = SHA256("erbridge:jwt:" + secret)`
- EVE SSO flow: state JWT (HS256, 5-min TTL) → redirect → callback → exchange code → verify RS256 EVE token → set cookie
- Accepted EVE JWT issuers: `https://login.eveonline.com`, `https://login.eveonline.com/`, `login.eveonline.com`; audience must include `client_id` and `"EVE Online"`
- Multiple ESI clients via `ESI_CLIENT_ID_N`/`ESI_CLIENT_SECRET_N`; one chosen randomly per redirect, bound in state JWT

---

## Database Schema

### `account`
| Column | Type | Notes |
|--------|------|-------|
| id | UUID PK | gen_random_uuid() |
| created_at / updated_at | TIMESTAMPTZ | |
| status | TEXT | `active` \| `pending_delete` |
| delete_requested_at | TIMESTAMPTZ | nullable |
| is_server_admin | BOOLEAN | default false |

### `eve_character`
| Column | Type | Notes |
|--------|------|-------|
| id | UUID PK | |
| account_id | UUID FK → account | NULL = ghost character |
| eve_character_id | BIGINT UNIQUE | CCP character ID |
| name | TEXT | |
| corporation_id | BIGINT | |
| alliance_id | BIGINT | nullable |
| is_main | BOOLEAN | partial unique index: one per account |
| is_online | BOOLEAN | nullable |
| esi_client_id | TEXT | nullable |
| encrypted_access_token | BYTEA | AES-256-GCM, nonce prepended |
| encrypted_refresh_token | BYTEA | AES-256-GCM, nonce prepended |
| esi_token_expires_at | TIMESTAMPTZ | nullable |
| created_at / updated_at | TIMESTAMPTZ | `updated_at` used as refresh-token age proxy |

Ghost character: `account_id = NULL`. Claimed atomically at first login (sets account_id, tokens, is_main).

### `sde_solar_system`
Key columns: `solar_system_id BIGINT PK`, `name TEXT`, `security_status REAL`, `security_class TEXT`, `wh_class TEXT`, `region_id`, `constellation_id`, `faction_id`. Also `name_i18n`, `planet_ids`, `stargate_ids`, `position` as JSONB.
Singleton metadata: `sde_solar_system_metadata(id=1, sde_version, sde_checksum, loaded_at)`.

### `audit_log`
| Column | Type | Notes |
|--------|------|-------|
| id | BIGSERIAL PK | |
| occurred_at | TIMESTAMPTZ | default now() |
| actor_account_id | UUID FK → account | nullable |
| event_type | TEXT | snake_case |
| details | JSONB | |

### `blocked_eve_character`
| Column | Type | Notes |
|--------|------|-------|
| eve_character_id | BIGINT PK | CCP character ID |
| reason | TEXT | nullable |
| blocked_at | TIMESTAMPTZ | default now() |

### `map`
| Column | Type | Notes |
|--------|------|-------|
| id | UUID PK | |
| name | TEXT | |
| slug | TEXT UNIQUE | kebab-case |
| owner_account_id | UUID FK → account | ON DELETE SET NULL |
| description | TEXT | nullable |
| deleted | BOOLEAN | soft-delete |
| last_checkpoint_seq | BIGINT | |
| last_checkpoint_at | TIMESTAMPTZ | nullable |
| retention_days | INT | default 14 |
| created_at / updated_at | TIMESTAMPTZ | |

### `map_connections`
| Column | Type | Notes |
|--------|------|-------|
| connection_id | UUID PK | |
| map_id | UUID FK → map | CASCADE |
| status | TEXT | `partial`\|`linked`\|`fully_linked`\|`collapsed`\|`expired` |
| life_state | TEXT | `fresh`\|`eol`\|NULL |
| mass_state | TEXT | `stable`\|`reduced`\|`critical`\|NULL |
| extra | JSONB | default `{}` |
| created_at / updated_at | TIMESTAMPTZ | |

### `map_connection_ends`
| Column | Type | Notes |
|--------|------|-------|
| connection_id | UUID FK → map_connections | CASCADE, composite PK |
| side | TEXT | `a`\|`b`, composite PK |
| system_id | BIGINT FK → sde_solar_system | |
| signature_id | UUID UNIQUE FK → map_signatures | nullable, ON DELETE SET NULL, DEFERRABLE |
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
| connection_id / connection_side | UUID / TEXT | nullable, composite FK → map_connection_ends, DEFERRABLE |
| wormhole_code | TEXT | nullable |
| derived_life_state / derived_mass_state | TEXT | propagated from connection |
| extra | JSONB | default `{}` |
| created_at / updated_at | TIMESTAMPTZ | |

Partial unique index: `(map_id, system_id, sig_code) WHERE status IN ('visible','resolved')`.

### `map_events`
| Column | Type | Notes |
|--------|------|-------|
| seq | BIGSERIAL PK | |
| map_id | UUID FK → map | CASCADE |
| entity_type / entity_id | TEXT | e.g. `"connection"` / UUID string |
| event_type | TEXT | PascalCase |
| actor_account_id | TEXT | UUID string, nullable |
| payload | JSONB | |
| occurred_at | TIMESTAMPTZ | default now() |

### `map_checkpoints`
| Column | Type | Notes |
|--------|------|-------|
| checkpoint_id | BIGSERIAL PK | |
| map_id | UUID FK → map | CASCADE |
| last_included_seq | BIGINT UNIQUE per map | |
| checkpoint_version | INT | |
| event_count | INT | |
| checksum | TEXT | nullable |
| state | JSONB | full snapshot |
| created_at | TIMESTAMPTZ | |

### `acl`
| Column | Type | Notes |
|--------|------|-------|
| id | UUID PK | |
| name | TEXT | |
| owner_account_id | UUID FK → account | nullable |
| pending_delete_at | TIMESTAMPTZ | set at creation, cleared on first map attach |
| created_at / updated_at | TIMESTAMPTZ | |

### `acl_member`
| Column | Type | Notes |
|--------|------|-------|
| id | UUID PK | |
| acl_id | UUID FK → acl | CASCADE |
| member_type | TEXT | `character`\|`corporation`\|`alliance` |
| eve_entity_id | BIGINT | corp/alliance CCP ID; NULL for character members |
| character_id | UUID FK → eve_character | NULL for corp/alliance members |
| name | TEXT | resolved at insert |
| permission | TEXT | `read`\|`read_write`\|`manage`\|`admin`\|`deny` |
| created_at / updated_at | TIMESTAMPTZ | |

### `map_acl`
| Column | Type | Notes |
|--------|------|-------|
| map_id | UUID FK → map | composite PK |
| acl_id | UUID FK → acl | composite PK |

### `system_edges` (VIEW)
Bidirectional edge list from `map_connections` + `map_connection_ends`. Columns: `map_id`, `connection_id`, `status`, `life_state`, `mass_state`, `from_system_id`, `to_system_id`.

---

## Key Patterns

### Permission Model
`permissions::effective_permission(pool, account_id, map_id) -> Option<Permission>`
- Map owner → `Admin` unconditionally
- Any `deny` match (by character, corp, or alliance across any attached ACL) → `None`
- Otherwise → most permissive grant across all matching ACL entries

`Permission` enum: `Read < ReadWrite < Manage < Admin` (derived `Ord`).

ACL-level permission for ACL management is resolved inline in `services::acl`, not via `effective_permission`.

### Layering
`handler → service → db`. Handlers must not call `db::*` directly. See `DECISIONS_context.md`.

### Bulk DB Operations
`UNNEST($1::type[], …)` for single-query bulk ops — see `db::character::bulk_update_corp_alliance`, `db::sde_solar_system::bulk_upsert_solar_systems`.

### ESI Retry
`esi::mod::esi_request`: up to 4 retries on 429. Respects `Retry-After`; otherwise exponential backoff ±25% jitter, capped at 30s.

### Error Handling
- Service/DB: `anyhow::Result<T>` with `.context("…")` chains
- Handlers: map to `StatusCode` + `warn!(error = %e, …)`
- `services::map`: typed `MapError` via `thiserror` (includes `From<sqlx::Error>` for FK violations)
- `services::acl`: typed `AclError` via `thiserror`

### Map Events
Every map mutation appends to `map_events` in the same transaction. Event types: `MapCreated`, `ConnectionCreated`, `ConnectionDeleted`, `SignatureAdded`, `SignatureDeleted`, `SignatureLinkedToConnectionEnd`, `ConnectionMetadataUpdated`.

### ACL Orphan Lifecycle
`pending_delete_at` set at ACL creation; cleared on first map attach. Purge task hard-deletes ACLs past their grace period.

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
| `ACCOUNT_DELETION_GRACE_DAYS` | No | `30` | |
| `ESI_BASE_URL` | No | `https://esi.evetech.net/latest` | Override for tests |
| `ESI_REFRESH_TOKEN_MAX_DAYS` | No | `7` | |
| `ESI_POLL_CONCURRENCY` | No | `10` | |
| `ESI_POLL_BATCH_SIZE` | No | `10` | |
| `ESI_POLL_BATCH_DELAY_MS` | No | `500` | Clamped ≥ 100 |
| `MAP_CHECKPOINT_INTERVAL_MINS` | No | `60` | |

---

## Background Tasks

| Task | Interval | Action |
|------|----------|--------|
| SDE update check | 10-min delay then every 24h | Re-download + upsert if CCP build number changed |
| Online poller | Adaptive (ESI Cache-Control, default 60s) | Poll `/characters/{id}/online/`; update `is_online` on change |
| Location poller | Adaptive (ESI Cache-Control, default 5s) | Poll `/characters/{id}/location/` for active subscribers; broadcast `LocationEvent` |
| Map checkpoint | `MAP_CHECKPOINT_INTERVAL_MINS` | Snapshot maps with new events to JSONB |
| Purge | Every 24h | Hard-delete expired accounts + orphaned ACLs |

---

## External Services

| Service | URL | Purpose |
|---------|-----|---------|
| EVE SSO | `https://login.eveonline.com` | OAuth2 + JWK set |
| ESI | `https://esi.evetech.net/latest` | Character/corp/alliance data, online/location polling |
| CCP SDE | `https://developers.eveonline.com/static-data/tranquility/` | Solar system static data |

Required ESI scopes: `esi-location.read_location.v1`, `esi-location.read_ship_type.v1`, `esi-location.read_online.v1`, `esi-search.search_structures.v1`, `esi-ui.write_waypoint.v1`.

---

## Key Dependencies

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
| `regex` 1 | Slug validation |
| `validator` 0.20 | Request struct validation |
| Dev: `axum-test`, `pg-embed`, `wiremock`, `portpicker`, `cookie` | Test infrastructure |
