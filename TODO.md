# erbridge-api — Hardening TODO

Generated 2026-04-27 from a deep audit of `src/`, `migrations/`, and `tests/`. Each item is
self-contained and scoped so a Sonnet model on a low effort setting can execute it without
re-discovering the codebase. Every item names the exact files to touch and the verification step.

Run after each change:

```sh
cargo fmt --all
cargo sqlx prepare       # if any sqlx query changed
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
```

Items are grouped by theme. Inside each group, **P1** = ship-blocker / correctness, **P2** =
strong improvement, **P3** = polish.

---

## Progress tracker

| Section | Items | Done | Notes |
|---------|-------|------|-------|
| A. Audit log gaps | A1 A2 A3 A4 A5 ~~A6~~ | ✓ all | A6 rejected |
| B. Connection pool & background tasks | B1 B2 B3 | ✓ all | |
| C. Missing CHECK constraints | C1 C2 C3 | ✓ all | |
| D. Missing indexes | D1 D2 D3 D4 D5 | ✓ all | |
| E. Too-many-arguments refactors | E1 E2 E3 | ✓ all | |
| F. Error-handling consistency | F1 F2 F3 | ✓ all | |
| G. Image-proxy subsystem | G1 G2 | — none | |
| H. Router / handler gaps | H1 H2 | — none | |
| I. ESI & token handling | I1 I2 I3 | — none | |
| J. Crypto | J1 J2 | — none | |
| K. Config validation | K1 | ✓ all | |
| L. Observability | L1 L2 L3 | ✓ all | |
| M. N+1 / soft-delete leakage | M1 M2 M3 M4 | — none | |
| N. DTO / validation polish | N1 N2 N3 | — none | |
| O. Misc | O1 O2 O3 | — none | |
| P. Tests | P1 P2 P3 | — none | |

---

## A. Audit log gaps (correctness) ✓ DONE

The `AuditEvent` enum in `src/audit.rs` defines variants that are **never instantiated** in
production code. These are silent compliance/security gaps — actions happen with no audit row.

### A1 [P1] Audit `CharacterRemoved` on `DELETE /api/v1/characters/{id}`
- **Where:** `src/handlers/character.rs:84-99` calls `db::character::delete_character` directly,
  outside any transaction. `AuditEvent::CharacterRemoved` (defined in `src/audit.rs`) is never
  fired.
- **Fix:** Promote the operation to a service function `services::character::remove_character`
  that opens a transaction, calls the DB layer (refactor `db::character::delete_character` to
  accept `&mut Transaction<'_, Postgres>`), then `audit::record_in_tx(&mut tx,
  Some(account_id), AuditEvent::CharacterRemoved { account_id, eve_character_id, name })`,
  then commits. Update the handler to call the service.
- **Verify:** Add an integration test in `tests/test_character_handlers.rs` that deletes a
  character and asserts a row appears in `audit_log` with `event_type = 'character_removed'`.

### A2 [P1] Audit `CharacterSetMain` on `PUT /api/v1/characters/{id}/main`
- **Where:** `src/handlers/character.rs:105-120` → `db::character::set_main_character`
  (`src/db/character.rs:408`). No audit call. `AuditEvent::CharacterSetMain` (audit.rs:31-34)
  is unused outside its own test.
- **Fix:** Same shape as A1: thread a transaction, refactor `set_main_character` to take
  `&mut Transaction`, audit before commit.
- **Verify:** Extend `tests/test_character_handlers.rs` to assert the audit row.

### A3 [P1] Audit ACL lifecycle: create / rename / delete
- **Where:** `src/services/acl.rs` — `create`, `rename`, `delete` functions. Zero `audit::`
  calls in this file.
- **Fix:** Add `AuditEvent::AclCreated { acl_id, name }`, `AclRenamed { acl_id, old_name,
  new_name }`, `AclDeleted { acl_id, name }` variants in `src/audit.rs`. Wire `record_in_tx`
  inside the existing transaction in each service function.
- **Verify:** Extend `tests/test_service_acl.rs` to assert each operation produces the
  corresponding audit row.

### A4 [P1] Audit ACL membership changes
- **Where:** `src/services/acl.rs` — `add_member`, `update_member`, `delete_member`.
- **Fix:** Add `AuditEvent::AclMemberAdded`, `AclMemberPermissionChanged`, `AclMemberRemoved`
  with `{ acl_id, member_id, member_type, permission }` payloads. Audit inside each existing
  transaction.
- **Verify:** Extend `tests/test_service_acl.rs`.

### A5 [P2] Audit map–ACL attach/detach
- **Where:** `src/services/map.rs` — `attach_acl_to_map` and `detach_acl_from_map`.
- **Fix:** Add `AuditEvent::AclAttachedToMap` / `AclDetachedFromMap` variants with `{ map_id,
  acl_id }`. Wire `record_in_tx` inside the existing transaction.
- **Verify:** Extend `tests/map_service_test.rs`.

### A6 [P2] ~~Audit map mutation operations (connections, signatures)~~ — REJECTED

Map mutations are gameplay actions, not admin/compliance actions. They are
already fully recorded in `map_events`. Adding them to `audit_log` would
conflate two logs with different purposes and retention requirements.
See `DECISIONS.md` for the full `audit_log` scope rule.

---

## B. Connection pool & background-task lifecycle (reliability)

### B1 [P1] Bump database connection pool size
- **Where:** `src/main.rs:28` uses `.max_connections(5)`.
- **Why:** With 4 long-lived background loops (online poller, location poller, checkpoint
  task, purge task) each potentially holding a connection, the request handlers can starve.
- **Fix:** Add `DATABASE_MAX_CONNECTIONS` env var (default 20) to `src/config.rs`, validate
  it as `1..=200`, plumb it into `main.rs`. Also set `.acquire_timeout(Duration::from_secs(5))`
  so requests fail fast under contention rather than hanging.
- **Verify:** `cargo test --all` passes; manually start the server and confirm `tracing` shows
  the pool size in the startup log (add the log line if missing).

### B2 [P1] Graceful shutdown for background tasks
- **Where:** `src/main.rs:66-72` spawns 5 tasks and discards the `JoinHandle`s. The HTTP
  server uses plain `axum::serve(listener, app).await` with no shutdown signal.
- **Fix:** Introduce `tokio_util::sync::CancellationToken` (add `tokio-util` to Cargo.toml).
  Pass a child token into each `spawn_*` function so loops can break on cancellation.
  Use `axum::serve(...).with_graceful_shutdown(shutdown_signal(token.clone()))` where
  `shutdown_signal` awaits SIGTERM/SIGINT. After the server exits, `token.cancel()` then
  `tokio::time::timeout(Duration::from_secs(10), join_set.join_all())` for the task handles.
- **Verify:** Send SIGTERM during `cargo run` and confirm clean exit (tasks log "shutting
  down", no panic). Add a unit test that calls each `spawn_*` with a cancelled token and
  asserts the loop exits within ~1s.

### B3 [P2] Panic guard around poller loops
- **Where:** `src/tasks/character_online_poll.rs`, `character_location_poll.rs`,
  `map_checkpoint.rs`, `purge.rs`. A panic inside the loop body kills the task forever.
- **Fix:** Wrap each loop iteration in `match tokio::task::spawn_local(async move { iter
  body }).await` is overkill; instead, wrap the body in `match
  std::panic::AssertUnwindSafe(async { body }).catch_unwind().await { ... }` (requires
  `futures::FutureExt`). On panic, log `error` and `tokio::time::sleep` for a backoff before
  the next iteration.
- **Verify:** Write a unit test that injects a panic and asserts the loop continues.

---

## C. Missing CHECK constraints (DB integrity)

The DB allows arbitrary string values in enum-like TEXT columns. Migrations 0005 and 0006 set
a good precedent (status, life_state, side, mass_state); the rest were missed.

### C1 [P1] Add CHECK on `account.status`
- **Where:** No migration exists. Create `migrations/0011_account_status_check.sql`.
- **Fix:** `ALTER TABLE account ADD CONSTRAINT account_status_check CHECK (status IN
  ('active','pending_delete'));`
- **Verify:** `cargo test --all` (pg-embed runs migrations); attempt to insert garbage status
  in a one-off SQL session.

### C2 [P1] Add CHECK on `acl_member.member_type` and `acl_member.permission`
- **Where:** No migration. Create `migrations/0012_acl_member_check.sql`.
- **Fix:**
  ```sql
  ALTER TABLE acl_member
      ADD CONSTRAINT acl_member_type_check
          CHECK (member_type IN ('character','corporation','alliance')),
      ADD CONSTRAINT acl_member_permission_check
          CHECK (permission IN ('read','read_write','manage','admin','deny'));
  ```
- **Verify:** `cargo test --all`.

### C3 [P2] DB-level guard: manage/admin only for character members
- **Where:** Same migration as C2.
- **Fix:** Add `ALTER TABLE acl_member ADD CONSTRAINT acl_member_role_for_type
  CHECK (member_type = 'character' OR permission NOT IN ('manage','admin'));`. The service
  layer (`services::acl::validate_permission_for_type`) already enforces this; the constraint
  prevents bypass via direct SQL or future bugs.
- **Verify:** Existing tests still pass; add a SQL-level negative test.

---

## D. Missing indexes (performance)

### D1 [P1] Index on `eve_character.account_id`
- **Where:** `migrations/0002_create_eve_character.sql`. No index covers the FK; queries in
  `src/db/character.rs` filter by `account_id` constantly.
- **Fix:** Create `migrations/0013_eve_character_indexes.sql` with `CREATE INDEX
  eve_character_account_id_idx ON eve_character (account_id);`. (The existing partial unique
  index on `is_main` does not satisfy general lookups.)

### D2 [P1] Index on `acl_member.acl_id` and `acl_member.character_id`
- **Where:** `migrations/0009_create_acl_member.sql`. CASCADE FKs without indexes — DELETE on
  parent rows triggers full table scans.
- **Fix:** In a new migration:
  ```sql
  CREATE INDEX acl_member_acl_id_idx       ON acl_member (acl_id);
  CREATE INDEX acl_member_character_id_idx ON acl_member (character_id) WHERE character_id IS NOT NULL;
  ```

### D3 [P1] Index on `map_acl.acl_id`
- **Where:** `migrations/0010_create_map_acl.sql`. Composite PK is `(map_id, acl_id)` so
  `WHERE acl_id = $1` is unindexed (used in `db::map_acl::detach` orphan check at line 55-60).
- **Fix:** `CREATE INDEX map_acl_acl_id_idx ON map_acl (acl_id);`

### D4 [P2] Index on `audit_log.event_type`
- **Where:** `migrations/0004_create_audit_log.sql`. Future analytics queries by event_type
  will scan the full table.
- **Fix:** `CREATE INDEX audit_log_event_type_idx ON audit_log (event_type);`

### D5 [P3] Document why `acl.owner_account_id` is unindexed
- **Where:** `migrations/0008_create_acl.sql`. Currently no index. If we plan to query "all
  ACLs owned by X" frequently, add one. If not, add a one-line comment in the migration
  noting the omission is intentional.

---

## E. `#[allow(clippy::too_many_arguments)]` refactors

Each of the 3 sites bundles arguments that naturally form a struct. This makes the API
self-documenting and prevents argument-order bugs.

### E1 [P2] `db::character::claim_ghost_character` → struct input
- **Where:** `src/db/character.rs:147-206` (12 parameters).
- **Fix:** Define a struct in the same file:
  ```rust
  pub struct ClaimGhostCharacterInput<'a> {
      pub eve_character_id: i64,
      pub account_id: Uuid,
      pub is_main: bool,
      pub name: &'a str,
      pub corporation_id: i64,
      pub alliance_id: Option<i64>,
      pub esi_client_id: &'a str,
      pub access_token: &'a str,
      pub refresh_token: &'a str,
      pub esi_token_expires_at: DateTime<Utc>,
  }
  ```
  Change the signature to `pub async fn claim_ghost_character(tx: &mut Transaction<'_,
  Postgres>, aes_key: &[u8;32], input: ClaimGhostCharacterInput<'_>) -> Result<Character>`.
  Drop the `#[allow(clippy::too_many_arguments)]` attribute. Update the call site in
  `src/services/auth.rs` (search for `claim_ghost_character(`).
- **Verify:** `cargo clippy --all-targets -- -D warnings`; existing tests pass.

### E2 [P2] `db::character::update_character_tokens` → struct input
- **Where:** `src/db/character.rs:209-270` (9 parameters).
- **Fix:** Same pattern: `CharacterTokenUpdate` struct holding `corporation_id`,
  `alliance_id`, `esi_client_id`, `access_token`, `refresh_token`, `esi_token_expires_at`.
  Keep `pool`, `aes_key`, and `eve_character_id` as direct args (they're orthogonal). Drop the
  `#[allow]`. Update callers in `src/services/auth.rs`.

### E3 [P2] `services::acl::add_member` → group `MemberType`/`eve_entity_id`/`permission`
- **Where:** `src/services/acl.rs:136-179` (8 parameters).
- **Fix:** Define `pub struct AddMemberInput { pub member_type: MemberType, pub
  eve_entity_id: Option<i64>, pub permission: AclPermission }`. Signature becomes `pub async
  fn add_member(pool, http, esi_base, acl_id, requesting_account_id, input: AddMemberInput)`.
  Drop the `#[allow]`. Update the handler call site in `src/handlers/acl.rs::add`.

---

## F. Error-handling consistency

### F1 [P2] Migrate `services::auth` to a typed `AuthError` via thiserror
- **Where:** `src/services/auth.rs` uses `anyhow::Result` everywhere. Handlers
  (`src/handlers/auth.rs`) map errors to status codes by hand, often losing detail.
- **Fix:** Define `pub enum AuthError` with variants like `OAuthExchangeFailed(String)`,
  `JwtVerificationFailed`, `EsiCharacterFetchFailed`, `DatabaseError(#[from] sqlx::Error)`,
  `Internal(#[from] anyhow::Error)`. Match the existing pattern in `src/services/map.rs:20-42`
  (`MapError`) and `src/services/acl.rs` (`AclError`). Update return types and handler `match`
  blocks.
- **Verify:** Tests pass; clippy clean.

### F2 [P2] Implement `IntoResponse` for typed errors
- **Where:** `MapError`, `AclError`, `ImageError` (the latter referenced in CODEBASE.md
  but the file `src/services/images.rs` does not exist — see G3). Handlers currently use
  helper functions (`map.rs:36-53`) or inline `match` (`acl.rs`).
- **Fix:** Add `impl IntoResponse for MapError { fn into_response(self) -> Response { let
  (status, msg) = match self { ... }; (status, Json(ApiResponse::<()>::error(msg))).into_response() } }`
  and likewise for `AclError`. Then handlers can be `Result<Json<ApiResponse<T>>, MapError>`
  and the `?` operator does the right thing. Delete the per-handler `map_err` helpers.
- **Verify:** Tests pass; handler signatures simplified.

### F3 [P3] Replace ad-hoc error helpers in `handlers::auth::callback` with `ApiResponse`
- **Where:** `src/handlers/auth.rs::callback` builds raw `(StatusCode, msg).into_response()`.
- **Fix:** Wrap into `(StatusCode, Json(ApiResponse::<()>::error(msg))).into_response()` so
  the response shape matches the documented envelope (CODEBASE.md ADR-021).
- **Verify:** Update / extend any test that asserts the body shape.

---

## G. Image-proxy subsystem is undocumented-as-missing

CODEBASE.md and the project memory both claim an image proxy exists. **It does not.** No
`src/handlers/images.rs`, no `src/services/images.rs`, no `src/tasks/image_cache_cleanup.rs`,
no route registration in `lib.rs`, no `ImageError` type. Either implement it or correct the
docs.

### G1 [P2] Decide: implement or remove from docs
- **Action:** Open the question: do we want the proxy now? If **yes**, file the implementation
  ticket (out of scope for this hardening pass). If **no**, edit `CODEBASE.md` to remove the
  references in: project structure tree, API routes table, "Image Proxy" subsection, the
  `tasks::image_cache_cleanup.rs` line in the file tree, the "Image cache cleanup" row in the
  Background Tasks table, and the "EVE Image Server" row in External Services (or move it to
  a "planned" section).
- **Verify:** `grep -nri "image" CODEBASE.md src/` returns no doc claims that don't match
  code reality.

### G2 [P2] Update memory `project_state_2026_04.md` accordingly
- **Where:** `~/.claude/projects/.../memory/project_state_2026_04.md` claims the EVE image
  proxy is "fully implemented" and lists `parse_max_age` deduplication as a "next likely task"
  (it's already done — confirmed in `src/esi/cache.rs`).
- **Action:** Rewrite the relevant lines to reflect actual state. Specifically remove "EVE
  image proxy with filesystem cache" from the implemented list, remove the `parse_max_age`
  deduplication from gaps, and remove the `once_cell::sync::Lazy` task (already migrated to
  `std::sync::LazyLock` in `src/dto/map.rs:13`).

---

## H. Router / handler gaps

### H1 [P2] Decide on admin route stubs
- **Where:** `src/handlers/character.rs:168-190` defines `admin_purge_account` and
  `admin_restore_account` that always return 403. They are not registered in `lib.rs` and
  reference an "admin role story" that does not exist.
- **Fix:** Either (a) delete both stubs and the corresponding `AdminPurge`/`AdminRestore`
  audit variants until the admin role story exists, or (b) leave them and add a
  `// TODO(US-admin-roles)` block at the top of `handlers::character` documenting the
  contract. Recommend (a) — dead code rots faster than it gets revived. CODEBASE.md already
  documents them as stubs in "Known Issues" so removal is consistent.

### H2 [P2] Authentication on `/debug/location-subscribe/{character_id}`
- **Where:** `src/lib.rs:162-168` registers the debug route in the **public** router (no
  middleware). Even guarded by `#[cfg(debug_assertions)]`, this leaks character locations in
  any debug build to anyone with the URL.
- **Fix:** Move the route under the `authenticated` router (still gated by
  `#[cfg(debug_assertions)]`). Verify `handlers::debug::location_subscribe` handles the
  `AccountId` extractor or assert character belongs to caller.
- **Verify:** Build with `cargo build --debug` and curl without a session cookie — should
  return 401.

---

## I. ESI & token handling

### I1 [P2] Retry on 5xx from ESI, not only 429
- **Where:** `src/esi/mod.rs:28-56` (`esi_request`). Currently retries 429 only;
  `error_for_status()` short-circuits 5xx.
- **Fix:** In the loop, check `response.status().is_server_error()` and retry up to N times
  using the same exponential-backoff helper as the 429 path. Cap retries to 3 for 5xx
  (different from 429 budget) and log each retry with the status.
- **Verify:** Add a wiremock test that returns 503 twice then 200 and asserts success.

### I2 [P2] Typed ESI errors
- **Where:** `src/esi/mod.rs`. Callers cannot tell "401 token revoked" from "503 ESI down"
  from "network error".
- **Fix:** Define `pub enum EsiError { RateLimited { retry_after_ms: u64 }, Unauthorized,
  Forbidden, NotFound, ServerError { status: u16 }, Network(reqwest::Error), Decode(String) }`
  with `#[derive(thiserror::Error)]`. Return `Result<Response, EsiError>` from `esi_request`
  and update the small number of call sites in `src/esi/{character,search,token,universe}.rs`.
- **Verify:** Tests in `tests/` pass; clippy clean.

### I3 [P3] Bound the in-memory poller scheduling state
- **Where:** `src/tasks/character_online_poll.rs` and `character_location_poll.rs` keep a
  `HashMap<String, Instant>` of next-poll timestamps. As characters come and go, stale keys
  accumulate. With `__unassigned__` bucket plus per-client buckets this is small in practice
  but unbounded in theory.
- **Fix:** Periodically (e.g., once per outer loop iteration when iteration count is a
  multiple of 100) prune entries older than 24h.
- **Verify:** Add a unit test that inserts an old entry and asserts it's pruned.

---

## J. Crypto

### J1 [P3] Validate `ENCRYPTION_SECRET` minimum length at startup
- **Where:** `src/config.rs:48-54` SHA-256-hashes any input. Empty or 1-byte secrets are
  silently accepted.
- **Fix:** In `Config::from_env`, after reading the env var: `if encryption_secret.len() < 32
  { anyhow::bail!("ENCRYPTION_SECRET must be at least 32 bytes (got {})",
  encryption_secret.len()); }`.
- **Verify:** Add a config test asserting failure for short secrets.

### J2 [P3] `subtle::ConstantTimeEq` helper for token comparisons
- **Where:** Currently no token byte-comparison happens (we encrypt/decrypt only). Pre-emptive:
  if any future code compares secrets, ensure constant-time. Add `subtle = "2"` to
  Cargo.toml and a `crypto::constant_time_eq(a: &[u8], b: &[u8]) -> bool` helper. Document
  that `==` MUST NOT be used on token bytes.
- **Verify:** Unit test in `crypto.rs`.

---

## K. Config validation

### K1 [P2] Bounds-check numeric env vars
- **Where:** `src/config.rs` parses `ESI_POLL_CONCURRENCY`, `ESI_POLL_BATCH_SIZE`,
  `MAP_CHECKPOINT_INTERVAL_MINS`, `ACCOUNT_DELETION_GRACE_DAYS`,
  `ESI_REFRESH_TOKEN_MAX_DAYS` with `unwrap_or(default)` — invalid values like `0` are
  silently accepted and panic later (e.g., `Semaphore::new(0)`).
- **Fix:** After each parse, clamp or `bail!` on out-of-range values:
  - `ESI_POLL_CONCURRENCY`: `1..=100`
  - `ESI_POLL_BATCH_SIZE`: `1..=100`
  - `MAP_CHECKPOINT_INTERVAL_MINS`: `1..=1440`
  - `ACCOUNT_DELETION_GRACE_DAYS`: `1..=365`
  - `ESI_REFRESH_TOKEN_MAX_DAYS`: `1..=30`
  Use the existing `ESI_POLL_BATCH_DELAY_MS` clamping pattern (`config.rs:91-106`) as the
  template.
- **Verify:** Extend `config.rs` tests to cover each clamp.

---

## L. Observability

### L1 [P2] Add `tower-http` `TraceLayer` with request IDs
- **Where:** `src/lib.rs::build_router` does not apply any tower-http layer. There is no
  request ID, no per-request span, no latency log.
- **Fix:** Add `tower-http = { version = "0.6", features = ["trace", "request-id"] }` and
  `tower = "0.5"` to Cargo.toml. In `build_router`, add layers (outermost first):
  ```rust
  use tower_http::trace::{TraceLayer, DefaultMakeSpan, DefaultOnResponse};
  use tower_http::request_id::{MakeRequestUuid, SetRequestIdLayer, PropagateRequestIdLayer};
  
  let request_id = HeaderName::from_static("x-request-id");
  router
      .layer(SetRequestIdLayer::new(request_id.clone(), MakeRequestUuid))
      .layer(TraceLayer::new_for_http()
          .make_span_with(DefaultMakeSpan::new().include_headers(false))
          .on_response(DefaultOnResponse::new().level(tracing::Level::INFO)))
      .layer(PropagateRequestIdLayer::new(request_id))
  ```
- **Verify:** Run server, hit `/api/health`, observe a JSON span line with `request_id`,
  `method`, `uri`, `status`, `latency`. Ensure JWT/cookie values are NOT logged
  (`include_headers(false)` is essential).

### L2 [P3] Verify no token/JWT/cookie ever logged
- **Where:** Audit `tracing::*!` calls across the project. Specifically check
  `src/handlers/auth.rs`, `src/services/auth.rs`, `src/esi/token.rs`.
- **Fix:** Where errors include the body of an OAuth response, redact the `access_token` /
  `refresh_token` fields before logging. Suggested pattern: `tracing::error!(error = %e,
  "...")` is fine; never `tracing::error!(?response_body)`.
- **Verify:** `grep -rn "tracing::.*token\|tracing::.*access\|tracing::.*refresh" src/`
  returns nothing alarming.

### L3 [P3] `#[tracing::instrument]` on hot service functions
- **Where:** `services::map`, `services::acl` have many functions with no `#[instrument]`.
  Adding spans makes the request-id propagation in L1 actually useful.
- **Fix:** Add `#[tracing::instrument(skip(pool, http, aes_key), err)]` (skipping arguments
  that are large or sensitive) to every public service function. Use `err` so error returns
  are recorded automatically.

---

## M. N+1 / soft-delete leakage in DB layer

### M1 [P2] `handlers::character::list_characters` ESI lookup is fine, but cap result size
- **Where:** `src/handlers/character.rs:29-78`. The handler dedups corp/alliance IDs into a
  single ESI `POST /universe/names/` call — already non-N+1. ESI's `/universe/names/` accepts
  up to 1000 IDs; if a user has many characters across many corps, batching is needed.
- **Fix:** In `src/esi/universe.rs::resolve_names`, if `ids.len() > 1000`, chunk into
  batches of 1000 and concat results.
- **Verify:** Unit test with a 1500-element input.

### M2 [P2] Soft-delete leak in `db::map_acl::find_acls_for_map`
- **Where:** `src/db/map_acl.rs` — query joins `map_acl` to `acl` but does not filter
  `map.deleted = false` on the parent map. A soft-deleted map's ACL list is still readable
  via this function.
- **Fix:** Add `JOIN map m ON m.id = ma.map_id WHERE m.deleted = false` to the query. (Verify
  callers expect this; if any caller intentionally wants deleted maps' ACLs, add a separate
  `find_acls_for_map_including_deleted` function.)
- **Verify:** Add a test that soft-deletes a map and asserts its ACLs are no longer returned.

### M3 [P2] Soft-delete leak in `db::map_event::find_events_since` and `db::map_checkpoint`
- **Where:** Neither filters `map.deleted = false`. Background checkpoint task (which iterates
  these) may continue snapshotting deleted maps.
- **Fix:** In `tasks::map_checkpoint`, exclude deleted maps from the candidate query
  (`SELECT id FROM map WHERE deleted = false AND ...`). For the read functions, document the
  expectation that callers verify map liveness.
- **Verify:** Add a test that soft-deletes a map mid-checkpoint cycle and asserts no new
  checkpoint row appears.

### M4 [P3] TOCTOU on character delete
- **Where:** `src/db/character.rs::delete_character` (lines ~373-403) reads `is_main` then
  deletes in a separate query.
- **Fix:** Single statement: `DELETE FROM eve_character WHERE id = $1 AND account_id = $2
  AND is_main = false RETURNING id`. Use the absence of a returned row to distinguish "not
  found" from "is main" by re-querying with the is_main filter dropped only if the first
  query returned zero rows.
- **Verify:** Existing tests in `tests/test_character_handlers.rs` should still pass; add one
  that races the delete (not strictly needed, but a note in the file is helpful).

---

## N. DTO / validation polish

### N1 [P3] Move handler-side defaults into DTO via `#[serde(default = "...")]`
- **Where:** `src/handlers/map.rs:374-376` does `params.max_depth.unwrap_or(10)`,
  `params.exclude_eol.unwrap_or(false)`, `params.exclude_mass_critical.unwrap_or(false)`.
- **Fix:** In `src/dto/map.rs::RouteQueryParams`, change fields to non-`Option` with
  `#[serde(default = "default_max_depth")]` etc., and define module-level `fn
  default_max_depth() -> u32 { 10 }`. Handler simplifies to `params.max_depth`.
- **Verify:** Existing route tests pass.

### N2 [P3] Validate `member_type` and `permission` strings at deserialization time
- **Where:** `src/dto/acl.rs::AddMemberRequest` and `UpdateMemberRequest` accept raw
  `String`s, parsed inside the handler with `MemberType::from_str` / `AclPermission::from_str`.
- **Fix:** Change the field types to `MemberType` and `AclPermission` directly. Both already
  derive `strum::EnumString`; add `#[serde(deserialize_with = "...")]` or implement
  `Deserialize` via strum to fail at parse time. Then the handler doesn't need the parse step.
- **Verify:** Existing handler tests pass; add a negative test for invalid strings returning
  400 from the framework rather than 500 from the handler.

### N3 [P3] `LazyLock` `.unwrap()` → `.expect("…")`
- **Where:** `src/dto/map.rs:13` — `Regex::new(r"...").unwrap()`.
- **Fix:** `.expect("slug regex must compile")` so the panic message is informative if the
  pattern is ever changed incorrectly.

---

## O. Misc

### O1 [P3] `handlers::auth.rs::now_secs` unwrap
- **Where:** `src/handlers/auth.rs:42` — `SystemTime::now().duration_since(UNIX_EPOCH)
  .unwrap()`. Cannot fail under any normal clock, but this kind of unwrap drifts over time.
- **Fix:** `.unwrap_or(Duration::ZERO).as_secs()` — JWT validation will reject a `0` `iat`
  cleanly if the clock is broken.

### O2 [P3] Update CLAUDE.md / CODEBASE.md known-issues block
- **Where:** `CODEBASE.md::Known Issues and Gaps` and `CLAUDE.md::Known issues to be aware of`
  carry several stale claims (e.g., the unused "tentative" status was removed in a recent
  commit per `git log`; image proxy as documented does not exist).
- **Fix:** After items A–N land, sweep both files. Remove resolved items, add genuinely
  new known issues (e.g., admin stubs after H1, image proxy gap after G1).

### O3 [P3] Bound `unwrap()` audit
- **Where:** Several `.unwrap()` calls remain in production code paths:
  `src/esi/jwks.rs:54,85,102` (bail patterns — fine, these are inside `?` chains),
  `src/permissions.rs:48`, `src/db/character.rs:419`, `src/services/sde_solar_system.rs:306-318`.
  Most look safe but should each carry a `// SAFETY:` comment or be converted to `.expect(...)`
  with a meaningful message.
- **Fix:** Walk each site, convert to `.expect("...")` if the invariant is real, or to a
  proper `?` if it isn't.

---

## P. Tests

### P1 [P2] Tests for character handler audit (after A1, A2)
- Already implied above; tracking it explicitly.

### P2 [P3] Concurrency test for ACL orphan lifecycle
- **Where:** `src/db/map_acl.rs::detach` — the COUNT/UPDATE pattern is racy under
  concurrent attach. Per ADR-028 it's acceptable but should be tested.
- **Fix:** Add a test in `tests/test_db_acl.rs` that simulates two concurrent operations
  (one detach, one attach) on the same ACL and asserts the final state is consistent.

### P3 [P3] Wire-mock test for ESI 5xx retry (after I1)
- Already implied; tracking explicitly.

---

## Verification checklist after the full pass

```sh
cargo fmt --all
cargo sqlx prepare        # if any sqlx query changed
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
```

After clippy is clean, search for the previously needed allow attributes — none should remain:

```sh
grep -rn "allow(clippy::too_many_arguments)" src/
```

Confirm no token/secret in logs:

```sh
grep -rn 'tracing::.*\(access\|refresh\|secret\|token\)' src/ \
    | grep -v 'esi_token_expires_at\|jwt_key\|aes_key' \
    || true
```
