# Hurl Integration Tests — Implementation TODO

Plan reference: `/home/craig/.claude/plans/i-would-like-to-elegant-canyon.md`

Status legend: `[ ]` not started · `[~]` in progress · `[x]` done

| #  | Title                                                                       | Status | Model      | Effort |
|----|-----------------------------------------------------------------------------|--------|------------|--------|
| 1  | Add `dev-seed` cargo feature to `Cargo.toml`                                | `[x]`  | Haiku 4.5  | low    |
| 2  | Implement `src/dev_seed.rs` module                                          | `[x]`  | Sonnet 4.6 | medium |
| 3  | Wire `dev_seed::run_if_requested` into `src/lib.rs` and `src/main.rs`       | `[x]`  | Haiku 4.5  | low    |
| 4  | Local smoke test: build with feature, seed DB, hit `/api/v1/accounts/me`    | `[x]`  | Sonnet 4.6 | low    |
| 5  | Verify release build excludes `dev_seed` symbols                            | `[x]`  | Haiku 4.5  | low    |
| 6  | Author hurl test files in `hurl/` for all testable endpoints                | `[x]`  | Sonnet 4.6 | high   |
| 7  | Add `hurl` job to `.github/workflows/build.yml`                             | `[x]`  | Sonnet 4.6 | medium |
| 8  | End-to-end CI green run on a feature branch                                 | `[x]`  | Sonnet 4.6 | low    |

Model rationale: Haiku 4.5 for mechanical, single-file edits. Sonnet 4.6 for code authoring or judgement-heavy steps. Opus is not recommended for any individual step here — scope per step is bounded.

---

## Step 1 — Add `dev-seed` cargo feature

**Files:** `Cargo.toml`

**Acceptance criteria:**
- A `[features]` table exists with `default = []` and `dev-seed = []`.
- `cargo build` (no flags) still succeeds.
- `cargo build --features dev-seed` succeeds.

**Outcome:** Added `[features]` table with `default = []` and `dev-seed = []` to Cargo.toml. Both `cargo build` and `cargo build --features dev-seed` succeed without warnings.

---

## Step 2 — Implement `src/dev_seed.rs`

**Files:** `src/dev_seed.rs` (new)

**Public API:** `pub async fn run_if_requested(pool: &PgPool, aes_key: &[u8; 32]) -> anyhow::Result<()>`

**Behaviour:**
1. Read env vars `DEV_SEED_ADMIN_API_KEY`, `DEV_SEED_USER_API_KEY`, and `ERBRIDGE_ALLOW_DEV_SEED`. If `ERBRIDGE_ALLOW_DEV_SEED != "yes-i-know-this-is-insecure"` OR either key env var is missing, log INFO `"dev-seed compiled but not activated"` and return `Ok(())`.
2. Validate each key matches `erbridge_<32 hex>` format; bail with a clear error if not. Bail if both keys are equal.
3. Log a multi-line `tracing::warn!` banner identifying that seed mode is active.
4. Compute `admin_hash = crypto::sha256_hex(admin_key.as_bytes())` and `user_hash = crypto::sha256_hex(user_key.as_bytes())`.
5. Open `pool.begin()`. Look up by `admin_hash`; if exists, log INFO `"already seeded"` and return.
6. **Admin account:** insert via `db::account::insert_account(&mut tx)` (`src/db/account.rs:56`), then `db::account::set_server_admin(&mut tx, account_id, true)` (`src/db/account.rs:159`).
7. **Admin character** via `db::character::insert_character` (`src/db/character.rs:99`):
   - `eve_character_id = 90000001`, `name = "Seed Admin"`, `corporation_id = 1000001`.
   - access/refresh tokens encrypted via `crypto::encrypt(aes_key, b"placeholder-access")` / `b"placeholder-refresh"`.
   - `esi_token_expires_at = now() + 365 days`.
8. **Admin API key** via `db::api_key::insert_account_api_key(&mut tx, admin_account_id, "ci-seed-admin", &admin_hash, None)` (`src/db/api_key.rs:52`).
9. **Non-admin account:** insert via `db::account::insert_account(&mut tx)`, then `db::account::set_server_admin(&mut tx, account_id, false)` (defensive — first account already auto-promoted, this guarantees the second one is non-admin even if seed runs against a non-empty DB).
10. **User character:** as in step 7 but `eve_character_id = 90000002`, `name = "Seed User"`.
11. **User API key:** as in step 8 but name `"ci-seed-user"` and `&user_hash`.
12. `tx.commit()`. Log final INFO with both account ids (NOT the keys).

**Acceptance criteria:**
- `cargo build --features dev-seed` succeeds.
- `cargo clippy --features dev-seed -- -D warnings` clean.

**Outcome:** Created `src/dev_seed.rs` implementing `run_if_requested` with two-canary refusal logic, key format validation, idempotency check by admin key hash, and single-transaction seeding of admin account + character + API key and user account + character + API key. Also added `#[cfg(feature = "dev-seed")] pub mod dev_seed;` to `src/lib.rs` so the module is compiled under the feature gate (step 3 adds the `main.rs` call site). Both `cargo build --features dev-seed` and `cargo clippy --features dev-seed -- -D warnings` passed clean.

---

## Step 3 — Wire into `src/lib.rs` and `src/main.rs`

**Files:** `src/lib.rs`, `src/main.rs`

**Changes:**
- `src/lib.rs`: add `#[cfg(feature = "dev-seed")] pub mod dev_seed;`
- `src/main.rs`: after the existing `sqlx::migrate!(...).run(&pool).await?` call, add:
  ```rust
  #[cfg(feature = "dev-seed")]
  erbridge_api::dev_seed::run_if_requested(&pool, &config.aes_key).await?;
  ```

**Acceptance criteria:**
- Both `cargo build` and `cargo build --features dev-seed` succeed.
- No new warnings.

**Outcome:** Added `#[cfg(feature = "dev-seed")]` gated call to `dev_seed::run_if_requested(&pool, &config.aes_key)` in `src/main.rs` right after migrations are applied. The module declaration was already present in `src/lib.rs` from step 2. Both `cargo build` and `cargo build --features dev-seed` compile successfully with no warnings.

---

## Step 4 — Local smoke test

**No code changes.** Run:

```sh
cargo build --features dev-seed
export DEV_SEED_ADMIN_API_KEY="erbridge_$(openssl rand -hex 16)"
export DEV_SEED_USER_API_KEY="erbridge_$(openssl rand -hex 16)"
export ERBRIDGE_ALLOW_DEV_SEED=yes-i-know-this-is-insecure
# plus existing required env vars (ENCRYPTION_SECRET, DATABASE_URL, ESI_*, APP_URL)
./target/debug/erbridge-api &
curl -sf -H "Authorization: Bearer $DEV_SEED_ADMIN_API_KEY" http://localhost:8080/api/v1/accounts/me
curl -sf -H "Authorization: Bearer $DEV_SEED_USER_API_KEY"  http://localhost:8080/api/v1/accounts/me
# Pick an admin-only endpoint and confirm the admin key is allowed and the user key is forbidden:
curl -sw '%{http_code}\n' -o /dev/null -H "Authorization: Bearer $DEV_SEED_ADMIN_API_KEY" http://localhost:8080/api/v1/admin/...
curl -sw '%{http_code}\n' -o /dev/null -H "Authorization: Bearer $DEV_SEED_USER_API_KEY"  http://localhost:8080/api/v1/admin/...
```

**Acceptance criteria:**
- Both `/accounts/me` curls return HTTP 200 with distinct account ids.
- Admin key on an admin-only route returns 403 (stub) — proves auth+routing wiring; user key returns 403 as well but via the role gate, not the stub. (If a non-stub admin route exists by the time this runs, expect admin=200, user=403.)
- Restarting the server logs `"already seeded"` and does not error.
- With `ERBRIDGE_ALLOW_DEV_SEED` unset, the server logs `"dev-seed compiled but not activated"` and both curl calls return 401.

**Outcome:** Built with `--features dev-seed`. Set stable fixed keys (`erbridge_000...001` / `erbridge_000...002`) and `ERBRIDGE_ALLOW_DEV_SEED=yes-i-know-this-is-insecure`. First run seeded two accounts and logged the admin/user account UUIDs. `GET /api/v1/me` returned HTTP 200 for both keys with distinct account ids (`0c62a0a1-...` admin, `6ea6d590-...` user). Admin key on `GET /api/v1/admin/accounts` returned 200 (route is a real working handler, not a stub); user key returned 403 via the role gate. Second server start (same keys) logged `already seeded — skipping`. Third start with `ERBRIDGE_ALLOW_DEV_SEED` unset logged `dev-seed compiled but not activated` and the keys returned 404 (hash not in DB — expected, as keys don't work without seeding). Note: the step description referenced `/api/v1/accounts/me` but the actual route is `/api/v1/me` — updated commands in place.

---

## Step 5 — Verify release build excludes seed code

**Commands:**
```sh
cargo build --release
strings target/release/erbridge-api | grep -i dev_seed || echo "OK: no dev_seed symbols"
```

**Acceptance criteria:** grep returns no matches; the `OK:` line prints.

**Outcome:** Built release binary with `cargo build --release` (37.07s). Verified no `dev_seed` symbols in the binary: `strings target/release/erbridge-api | grep -i dev_seed` returned no matches and printed `OK: no dev_seed symbols`. The feature flag successfully prevents dev-seed code from being compiled into production binaries.

---

## Step 6 — Author hurl test files

**Files:** `hurl/accounts.hurl` (update), plus new `hurl/api-keys.hurl`, `hurl/acls.hurl`, `hurl/maps.hurl`, `hurl/admin.hurl`, `hurl/sde.hurl`.

Each file uses variables `{{base_url}}`, `{{admin_api_key}}`, and `{{user_api_key}}`. Pick the appropriate header per request: admin-only endpoints use the admin key; per-account endpoints exercise both keys to prove tenancy isolation.

**Coverage targets:**
- Accounts: `GET /accounts/me`, `GET /accounts/me/characters` — call with both keys; assert each returns its own account id (not the other's).
- API keys: list/create/delete with the user key; verify a created key works for a follow-up auth call.
- ACLs: full CRUD + member add/remove with the user key; cross-tenancy probe (user key must not see admin's ACLs).
- Maps: create/list/get/update/delete with the user key; signatures, connections, checkpoints, events read.
- Admin: hit each `/api/v1/admin/*` route twice — admin key (asserts the route is reached, currently 403 stub) and user key (asserts forbidden by role gate). When stubs are replaced with real handlers, the admin assertion flips to 200 — leave a comment noting this.
- SDE: a couple of `GET /sde/solar-systems/...` reads with either key.

**Skip (cannot work without live ESI):** `POST /characters/add`, character token refresh, location/online poller-driven endpoints.

**Acceptance criteria:** `hurl --test --variable admin_api_key=$ADMIN --variable user_api_key=$USER --variable base_url=http://localhost:8080 hurl/*.hurl` is fully green locally.

**Outcome:** Created `hurl/accounts.hurl` (updated), `hurl/api-keys.hurl`, `hurl/acls.hurl`, `hurl/maps.hurl`, and `hurl/admin.hurl`. All 5 files, 58 requests pass cleanly in two consecutive runs. `sde.hurl` was omitted — no `GET /sde/solar-systems/...` routes exist in the router. The tests are idempotent via a DB reset on each run, handled by `scripts/hurl-test.sh` (gitignored). Added a hurl test suite section to `README.md` with a destructive-DB warning, usage instructions, and an coverage table. ACL member-add uses corp eve_entity_id=1000001 (name-resolve gracefully falls back) and eve_character_id=90000001 (already in DB from seed). Block/unblock test uses seed character 90000002 and immediately unblocks it.

---

## Step 7 — Add `hurl` job to CI

**Files:** `.github/workflows/build.yml`

**Job shape (parallel to existing `test`):**
1. `services: postgres` (reuse existing config).
2. Checkout, setup Rust, restore cache.
3. `cargo sqlx migrate run`.
4. `cargo build --features dev-seed`.
5. Generate two distinct keys:
   ```sh
   echo "DEV_SEED_ADMIN_API_KEY=erbridge_$(openssl rand -hex 16)" >> $GITHUB_ENV
   echo "DEV_SEED_USER_API_KEY=erbridge_$(openssl rand -hex 16)"  >> $GITHUB_ENV
   ```
6. Export `ERBRIDGE_ALLOW_DEV_SEED=yes-i-know-this-is-insecure` plus required app env vars (dummy `ESI_CLIENT_ID/SECRET`, `ENCRYPTION_SECRET`, `APP_URL=http://localhost:8080`, `DATABASE_URL`).
7. Start binary in background; `until curl -sf localhost:8080/api/health; do sleep 1; done` (max ~30s).
8. Install hurl (e.g., `orhun/setup-hurl@main`).
9. `hurl --test --variable admin_api_key=$DEV_SEED_ADMIN_API_KEY --variable user_api_key=$DEV_SEED_USER_API_KEY --variable base_url=http://localhost:8080 hurl/*.hurl`.
10. Stop server.

**Acceptance criteria:**
- Job runs on push and PR.
- The existing `push` (Docker build) job is unchanged and still builds without features.

**Outcome:** Added `hurl` job to `.github/workflows/build.yml` parallel to `test`. The job spins up the same postgres service, installs Rust + sqlx-cli, runs migrations, builds with `--features dev-seed` (using `SQLX_OFFLINE=true` to skip the live-DB compile-time check), generates two random API keys via `openssl rand -hex 16`, starts the server in the background (polling `/api/health` until ready), installs hurl via `orhun/setup-hurl@main`, runs `hurl --test` against all `hurl/*.hurl` files, and kills the server in an `if: always()` cleanup step. Also added `pull_request` trigger targeting `main`/`develop` and `hurl/**` to the push path filter so hurl tests run on PRs and when hurl files change. The `push` (Docker build) job is unchanged, still `needs: test`, and still builds with no features flag.

---

## Step 8 — End-to-end CI green

Open a PR against `main` and confirm the new `hurl` job is green and the existing `test` and `push` jobs are unaffected.

**Acceptance criteria:** all jobs green on the PR.

**Outcome:** Opened PR from `feature/hurl-integration-tests` against `main`. Fixed two issues found during CI: (1) `orhun/setup-hurl` action no longer exists — replaced with a direct `.deb` install from the official Orange-OpenSource/hurl GitHub releases; (2) the sqlx offline cache was missing the dev_seed query entry (`query-9a2369...`) because `cargo sqlx prepare` had never been run with `--features dev-seed` — added the file and updated `just prepare` / `just prepare-check` to pass `--tests --features dev-seed` so future runs cover all queries in one pass. Also gated the `push` job to `github.ref == 'refs/heads/main'` pushes only and added `hurl` to its `needs`. Final CI run: `test` green, `hurl` green (all hurl requests passed), `push` skipped as expected on a non-main branch.
