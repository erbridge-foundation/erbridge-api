# Test Plan

Exhaustive unit and integration test coverage for erbridge-api.
Dev dependencies already present: `axum-test`, `pg-embed`, `wiremock`, `portpicker`, `cookie`.

---

## Conventions

- **Unit tests:** `#[cfg(test)]` modules in the same file as the code under test.
- **Integration tests:** `tests/` directory, one file per feature area, using `axum-test` for
  HTTP-level testing, `pg-embed` for an embedded PostgreSQL instance, `wiremock` for ESI/EVE SSO
  mocking.
- **Test DB:** A `TestDb` helper struct in `tests/common/mod.rs` should own the `pg-embed`
  instance, run migrations, and hand out a `PgPool`. Each test should use a fresh schema or
  `TRUNCATE` tables in a `setup` fixture.
- **Test App:** A `TestApp` helper builds the full Axum router via `erbridge_api::router()` with
  a real DB pool and wiremock-backed HTTP client.

---

## Unit Tests

### `src/crypto.rs` — already covered; verify edge cases

| Test | What it checks |
|------|---------------|
| ✅ `round_trip` | encrypt → decrypt returns original bytes |
| ✅ `different_nonces` | two encryptions of same plaintext produce different ciphertext |
| ✅ `wrong_key_fails` | decrypt with wrong key returns Err |
| ✅ `tampered_ciphertext_fails` | bit-flip in ciphertext returns Err |
| ✅ `too_short_fails` | ciphertext shorter than nonce returns Err |
| ✅ `empty_plaintext_round_trip` | zero-length plaintext survives encrypt/decrypt |
| ➕ `nonce_length` | encrypted output is exactly `plaintext.len() + 12 + 16` bytes |

### `src/config.rs` — already covered; add more

| Test | What it checks |
|------|---------------|
| ✅ `key_derivation_deterministic` | same secret → same keys on repeated calls |
| ✅ `keys_distinct` | `aes_key ≠ jwt_key` |
| ✅ `parse_esi_clients_single_vars` | `ESI_CLIENT_ID`/`ESI_CLIENT_SECRET` → one client |
| ➕ `parse_esi_clients_multi_vars` | `ESI_CLIENT_ID_1`…`ESI_CLIENT_ID_3` → three clients, correct order |
| ➕ `parse_esi_clients_multi_overrides_single` | multi-client vars present → single vars ignored |
| ➕ `missing_required_var_panics` | `Config::from_env()` without `DATABASE_URL` panics or returns Err |
| ➕ `esi_base_url_default` | no `ESI_BASE_URL` → `https://esi.evetech.net/latest` |
| ➕ `deletion_grace_days_default` | no env var → 30 |

### `src/esi/jwks.rs` — already covered; verify completeness

| Test | What it checks |
|------|---------------|
| ✅ `parse_character_id_valid` | `CHARACTER:12345:...` → `12345` |
| ✅ `parse_character_id_large` | i64-max value parses |
| ✅ `parse_character_id_missing_prefix` | returns Err |
| ✅ `parse_character_id_non_numeric` | returns Err |
| ✅ `parse_character_id_empty` | returns Err |
| ✅ `issuer_accepted_*` (3 forms) | all three accepted issuer forms pass |
| ✅ `issuer_rejected_*` | other forms fail |
| ➕ `audience_must_contain_eve_online` | token without `"EVE Online"` in aud → Err |
| ➕ `audience_must_contain_client_id` | token without `client_id` in aud → Err |
| ➕ `expired_token_rejected` | `exp` in the past → Err |

### `src/esi/mod.rs` — already covered; add meaningful assertions

| Test | What it checks |
|------|---------------|
| ✅ `backoff_within_bounds` | backoff values stay ≤ 30s |
| ✅ `backoff_grows_with_attempt` | later attempts produce longer waits |
| ➕ `retry_after_header_respected` | wiremock returns 429 with `Retry-After: 2` → waits ~2s before retrying |
| ➕ `max_retries_exceeded_returns_last_error` | 5× 429 with no success → returns Err |
| ➕ `success_on_second_attempt` | first attempt 429, second 200 → returns Ok |

### `src/esi/search.rs` — already covered

| Test | What it checks |
|------|---------------|
| ✅ `search_category_display_parse_round_trip` | all variants |
| ✅ `into_categorised_populated` | populated response |
| ✅ `into_categorised_empty` | empty response |

### `src/db/account.rs`

| Test | What it checks |
|------|---------------|
| ➕ `insert_account` | creates row with `status = 'active'` |
| ➕ `reactivate_account_from_pending_delete` | updates status, clears `delete_requested_at` |
| ➕ `reactivate_account_already_active` | returns false, no change |
| ➕ `reactivate_account_not_found` | returns false |
| ➕ `request_account_deletion` | sets `pending_delete`, records timestamp |
| ➕ `request_account_deletion_not_active` | returns false |
| ➕ `purge_expired_accounts` | only deletes rows past grace period |
| ➕ `purge_expired_accounts_within_grace` | rows within grace period untouched |
| ➕ `get_account_status_active` | returns `AccountStatus::Active` |
| ➕ `get_account_status_pending_delete` | returns `AccountStatus::PendingDelete` |
| ➕ `get_account_status_not_found` | returns `None` |
| ➕ `invalid_status_in_db_returns_err` | corrupted `status` column → Err |

### `src/db/character.rs`

| Test | What it checks |
|------|---------------|
| ➕ `insert_character_encrypts_tokens` | raw DB row has non-null encrypted blobs |
| ➕ `insert_character_decrypt_round_trip` | returned `Character` has plaintext tokens matching input |
| ➕ `find_character_by_eve_id_exists` | returns `Some` with correct fields |
| ➕ `find_character_by_eve_id_not_found` | returns `None` |
| ➕ `find_characters_by_account_ordered` | main first, then by `created_at` |
| ➕ `claim_ghost_character` | sets `account_id`, `is_main`, tokens; was previously NULL |
| ➕ `update_character_tokens_changes_encrypted_blobs` | new tokens stored; raw bytes differ |
| ➕ `delete_character_success` | non-main character deleted, returns `Deleted` |
| ➕ `delete_character_is_main` | main character not deleted, returns `IsMain` |
| ➕ `delete_character_not_found` | returns `NotFound` |
| ➕ `delete_character_wrong_account` | character owned by other account → `NotFound` |
| ➕ `set_main_character_promotes_and_demotes` | new main is_main=true; old main is_main=false |
| ➕ `set_main_character_wrong_account_returns_err` | character not on account → Err |
| ➕ `bulk_update_corp_alliance` | multiple characters updated in one call |
| ➕ `bulk_update_corp_alliance_empty_input` | no-op, no error |
| ➕ `partial_unique_index_enforced` | inserting a second is_main character → DB error |
| ➕ `find_all_characters_for_refresh_excludes_ghosts` | fix for LOW-3; only claimed chars returned |

### `src/handlers/auth.rs` — unit-testable helpers

| Test | What it checks |
|------|---------------|
| ➕ `make_session_cookie_attributes` | `HttpOnly`, `Secure`, `SameSite=Lax`, `Path=/` all set |
| ➕ `make_session_cookie_jwt_round_trip` | cookie JWT can be decoded with same key, claims match |
| ➕ `now_secs_not_zero` | sanity: returns a reasonable Unix timestamp |

### `src/handlers/images.rs`

| Test | What it checks |
|------|---------------|
| ➕ `is_valid_combination_valid` | all 7 valid combos return true |
| ➕ `is_valid_combination_invalid` | `("characters", "logo")`, `("types", "portrait")`, etc. return false |
| ➕ `is_valid_combination_empty` | empty strings return false |

### `src/esi/token.rs`

| Test | What it checks |
|------|---------------|
| ➕ `token_still_fresh` | expiry > 60s from now → `Fresh(token)` returned, no HTTP call |
| ➕ `token_refresh_age_exceeded` | `updated_at` > max_days ago → `RefreshExpired` |
| ➕ `token_no_expiry_triggers_refresh` | `esi_token_expires_at = None` → refresh attempted |
| ➕ `ghost_character_no_access_token_returns_err` | `access_token = None` → Err |
| ➕ `token_refresh_updates_db` | after refresh, DB row has new encrypted tokens |

### `src/services/auth.rs`

| Test | What it checks |
|------|---------------|
| ➕ `login_or_register_first_login` | creates account + character, `is_main=true` |
| ➕ `login_or_register_subsequent_login` | updates tokens, returns same `account_id` |
| ➕ `login_or_register_ghost_claim` | ghost row → creates account, claims row |
| ➕ `login_or_register_reactivates_pending_delete` | pending-delete account reactivated on login |
| ➕ `attach_character_new_character` | inserts with `is_main=false` |
| ➕ `attach_character_ghost_claim` | ghost row claimed for existing account |
| ➕ `attach_character_same_account_refresh` | duplicate add → tokens updated |
| ➕ `attach_character_different_account_returns_err` | character already on other account → Err |

---

## Integration Tests

All integration tests live in `tests/`. Structure:

```
tests/
├── common/
│   ├── mod.rs        # TestDb, TestApp helpers, mock builders
│   ├── db.rs         # pg-embed setup, migration runner
│   └── mocks.rs      # wiremock stubs for EVE SSO + ESI
├── test_health.rs
├── test_auth_login.rs
├── test_auth_callback.rs
├── test_auth_add_character.rs
├── test_auth_logout.rs
├── test_auth_me.rs
├── test_images.rs
└── test_sde.rs
```

### `tests/test_health.rs`

| Test | Scenario |
|------|---------|
| `health_ok` | DB up → `200 {"status":"ok","version":"0.1.0","components":{"database":"ok"}}` |
| `health_degraded` | DB pool exhausted / wrong URL → `503 {"status":"degraded",…,"database":"degraded"}` |

### `tests/test_auth_login.rs`

| Test | Scenario |
|------|---------|
| `login_redirects_to_eve_sso` | `GET /auth/login` → `302` to `authorization_endpoint` |
| `login_redirect_contains_required_params` | redirect URL has `response_type=code`, `client_id`, `redirect_uri`, `scope`, `state` |
| `login_state_param_is_valid_jwt` | state JWT decodes with server key, contains `mode=login`, `exp` in future |
| `login_state_param_expires_in_5_minutes` | `exp = now + 300` |
| `login_state_jwt_has_no_account_id` | login mode → `account_id = null` in state claims |

### `tests/test_auth_callback.rs`

| Test | Scenario |
|------|---------|
| `callback_happy_path_new_user` | Valid code+state → account + character created, `erbridge_session` cookie set, redirect to `{FRONTEND_URL}/` |
| `callback_happy_path_returning_user` | Existing character → tokens updated, session cookie refreshed, same `account_id` |
| `callback_happy_path_ghost_claim` | Ghost row in DB → claimed, new account created |
| `callback_reactivates_pending_delete` | Character on pending-delete account → reactivated on login |
| `callback_missing_code_param` | No `code` in query → `400` |
| `callback_missing_state_param` | No `state` in query → `400` |
| `callback_invalid_state_jwt` | Garbage `state` → `400 "invalid state"` |
| `callback_expired_state_jwt` | State JWT with `exp` in past → `400 "invalid state"` |
| `callback_state_client_id_not_found` | `client_id` in state JWT not in ESI clients list → `400 "invalid state"` |
| `callback_token_exchange_esi_returns_400` | wiremock: EVE token endpoint returns `400` → `502` to client |
| `callback_token_exchange_esi_unavailable` | wiremock: no response / timeout → `502` |
| `callback_invalid_eve_jwt` | EVE access token has bad signature → `400 "invalid EVE token"` |
| `callback_eve_jwt_wrong_audience` | EVE token aud doesn't include `client_id` → `400` |
| `callback_jwks_rotation` | First verify fails (wrong kid), re-fetch succeeds → cookie issued |
| `callback_esi_character_info_fails` | ESI `/characters/{id}/` returns 500 → `502` |

### `tests/test_auth_add_character.rs`

| Test | Scenario |
|------|---------|
| `add_character_no_cookie` | No session cookie → `401` |
| `add_character_expired_cookie` | Expired JWT → `401` |
| `add_character_redirects` | Valid cookie → `302` to EVE SSO |
| `add_character_state_contains_account_id` | State JWT `account_id` = logged-in user's ID |
| `add_character_state_contains_mode_add` | State JWT `mode = "add"` |
| `add_character_callback_attaches` | Full round-trip: add flow → new character on account |
| `add_character_callback_already_same_account` | Same character re-added → tokens updated, no duplicate |
| `add_character_callback_belongs_to_other_account` | Character on other account → `400` |

### `tests/test_auth_logout.rs`

| Test | Scenario |
|------|---------|
| `logout_returns_204` | `POST /auth/logout` → `204 No Content` |
| `logout_clears_session_cookie` | Response `Set-Cookie` removes `erbridge_session` |
| `logout_without_cookie_still_returns_204` | Idempotent — no cookie present → still `204` |

### `tests/test_auth_me.rs`

| Test | Scenario |
|------|---------|
| `me_no_cookie` | No session → `401` |
| `me_returns_account_and_characters` | Valid session → `200 {"data":{"account_id":…,"character":…,"characters":[…]}}` |
| `me_main_character_in_character_field` | The `character` top-level field equals the `is_main=true` character |
| `me_multiple_characters` | Two characters → both in `characters` array, one marked main |
| `me_no_main_returns_500` | Data integrity violation (no main) → `500` |
| `me_pending_delete_account` | Account in `pending_delete` → `403` (once middleware is applied) |

### `tests/test_images.rs`

| Test | Scenario |
|------|---------|
| `image_character_portrait` | `GET /api/v1/images/characters/12345/portrait` → `200`, correct `Content-Type` |
| `image_corporation_logo` | Same for corporations |
| `image_alliance_logo` | Same for alliances |
| `image_type_render` | Same for types/render |
| `image_type_icon` | Same for types/icon |
| `image_type_bp` | Same for types/bp |
| `image_type_bpc` | Same for types/bpc |
| `image_type_relic` | Same for types/relic |
| `image_invalid_combination` | `GET /api/v1/images/characters/1/logo` → `404` |
| `image_unknown_category` | `GET /api/v1/images/wormholes/1/portrait` → `404` |
| `image_response_cache_control_header` | `Cache-Control: public, max-age=3600` present |
| `image_upstream_4xx` | wiremock returns `404` from `images.evetech.net` → `502` |
| `image_upstream_5xx` | wiremock returns `500` → `502` |
| `image_served_from_cache` | Second request: wiremock asserts only one upstream call made |
| `image_cache_expired_refetch` | Cache file older than 1h → upstream refetched |
| `image_with_size_param` | `?size=256` forwarded to upstream |
| `image_with_tenant_param` | `?tenant=tranquility` forwarded to upstream |

### `tests/test_sde.rs`

| Test | Scenario |
|------|---------|
| `sde_load_if_needed_inserts_systems` | Fresh DB, mock SDE ZIP → `sde_solar_system` rows inserted |
| `sde_load_if_needed_skips_if_current` | Checksum matches → no download, no upsert |
| `sde_update_check_triggers_on_version_change` | New build number → re-download and upsert |
| `sde_wh_class_preserved_on_update` | Row has `wh_class` set; SDE update doesn't clear it |
| `sde_bulk_upsert_idempotent` | Running upsert twice → no duplicates, counts unchanged |

---

## Test Infrastructure (`tests/common/`)

### `TestDb`
```rust
pub struct TestDb {
    _pg: pg_embed::PgEmbed,
    pub pool: PgPool,
}

impl TestDb {
    pub async fn new() -> Self { /* spin up embedded PG, run migrations */ }
    pub async fn truncate_all(&self) { /* truncate all tables for test isolation */ }
}
```

### `TestApp`
```rust
pub struct TestApp {
    pub client: axum_test::TestServer,
    pub db: TestDb,
    pub eve_sso_mock: wiremock::MockServer,
    pub esi_mock: wiremock::MockServer,
    pub image_mock: wiremock::MockServer,
}

impl TestApp {
    pub async fn new() -> Self { /* build router with test config pointing at mocks */ }
    pub async fn with_logged_in_user(&self) -> (Uuid, Cookie) { /* inserts account + char, returns session cookie */ }
}
```

### EVE SSO Mocks (`tests/common/mocks.rs`)
Pre-built `wiremock::Mock` stubs for:
- Well-known discovery endpoint
- JWK set endpoint (with test RS256 key pair)
- Token exchange endpoint (returns crafted RS256 access token + refresh token)
- Token refresh endpoint
- ESI `/characters/{id}/` endpoint
- ESI `/universe/names/` endpoint
- `images.evetech.net` (returns a small valid PNG/JPEG)

### Test JWT helpers
Helper to sign a test EVE access token with the test RS256 private key, with configurable claims
(`sub`, `aud`, `iss`, `exp`).

---

## Coverage Targets

| Area | Target |
|------|--------|
| `crypto` | 100% — already near-complete |
| `config` | 100% |
| `esi/jwks` | 100% |
| `db/*` | ≥ 95% — all DB functions have at least happy-path + error tests |
| `services/auth` | 100% — all branches of login/register/attach |
| `handlers/auth` | ≥ 90% — all error paths tested via integration tests |
| `handlers/images` | ≥ 90% — all combinations + cache behaviour |
| `handlers/health` | 100% |
| `esi/token` | ≥ 90% |
| `esi/*` | ≥ 80% |
| `services/sde_solar_system` | ≥ 80% |
| `tasks/*` | ≥ 70% — integration tests cover the side effects |

---

## Implementation Order

1. Set up `tests/common/` infrastructure (TestDb, TestApp, mocks, JWT helpers)
2. Health endpoint tests (simplest, validates infrastructure)
3. Unit tests for remaining DB functions (`db/account`, `db/character`)
4. Unit tests for `services/auth`
5. Integration tests for auth flow (login → callback → me → logout)
6. Unit tests for image handler helpers + integration tests for image proxy
7. Unit tests for `esi/token`, `esi/mod` retry with wiremock
8. Integration tests for add-character flow
9. SDE tests
10. Fix issues identified in AUDIT.md concurrently with or before tests that cover them
