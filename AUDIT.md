# Codebase Audit

Issues grouped by severity. Each entry identifies the file and line, the problem, and the fix.

---

## Critical

### ~~CRIT-1: Middleware is wired to no routes~~ ✓ Fixed

`require_active_account` is now applied via `.layer()` on an `authenticated` router group
(`lib.rs:43–54`) that covers `/api/v1/me`, `/auth/characters/add`, and `/auth/logout`.

---

### ~~CRIT-2: `me` endpoint is unrouted~~ ✓ Fixed

`GET /api/v1/me` is now registered in the `authenticated` group (`lib.rs:45`) and protected by
`require_active_account`. Note: path changed from `/api/v1/auth/me` to `/api/v1/me`.

---

### CRIT-3: Token refresh picks only the first ESI client
**File:** `src/esi/token.rs:81–85`

`ensure_token_fresh` uses `config.esi_clients.first()` for refresh calls. With the multi-client
pool, the original login may have used client N, but the refresh always uses client 1. EVE's token
endpoint validates the `client_id` in the Basic auth against the token's issuing client — if those
differ, the refresh will fail.

**Fix:** Store which `client_id` was used to obtain each token (either as an extra column on
`eve_character`, or encoded in the stored token alongside it), and use that specific client for
refresh.

---

## High

### ~~HIGH-1: Typo in middleware function name~~ ✓ Fixed

`require_actclive_account` renamed to `require_active_account` in `middleware.rs` and `lib.rs`.

---

### ~~HIGH-2: `set_main_character` does not update `updated_at` when demoting~~ ✓ Fixed

Demotion query now includes `updated_at = now()` (`character.rs:402`). Promotion query also sets
`updated_at = now()` (`character.rs:410`).

---

### HIGH-3: `delete_character` has a TOCTOU race
**File:** `src/db/character.rs:349–378`

`delete_character` performs a SELECT to check `is_main`, then a separate DELETE. Under concurrent
requests, a character could be promoted to main between the SELECT and DELETE, silently deleting
the main character.

**Fix:** Collapse into a single query:
```sql
DELETE FROM eve_character
WHERE id = $1 AND account_id = $2 AND is_main = false
RETURNING id, is_main
```
Interpret no rows returned + a separate existence check as either NotFound or IsMain.
Or use `DELETE … WHERE id = $1 AND account_id = $2 AND NOT is_main RETURNING id`.

---

### HIGH-4: `tenant` query parameter forwarded unsanitised to upstream URL
**File:** `src/handlers/images.rs:46–47`

`params.tenant` is appended to the upstream `images.evetech.net` URL via string concatenation
without encoding. A value like `"foo&other=bar"` would inject extra query parameters into the
upstream URL. While images.evetech.net is a trusted upstream and the impact is low today, it is an
SSRF-adjacent concern if the base URL becomes configurable.

**Fix:** Build the upstream URL with `url::Url::parse` + `query_pairs_mut()` so all parameters
are percent-encoded.

---

### HIGH-5: No `purge_expired_accounts` background task
**File:** `src/db/account.rs:84` and `src/main.rs`

`purge_expired_accounts` is implemented but never called. Accounts in `pending_delete` status
accumulate indefinitely; the 30-day grace period is documented but never enforced.

**Fix:** Spawn a background task (similar to `image_cache_cleanup`) that calls
`purge_expired_accounts` once per day. Log the count of accounts purged.

---

### HIGH-6: Image proxy forwards response body from upstream without size limit
**File:** `src/services/images.rs` (implied by `fetch_image`)

The upstream body is buffered into a `Vec<u8>` with no size cap. A malicious or malfunctioning
upstream (or a crafted Category/ID that resolves to a huge file) could cause unbounded memory
allocation.

**Fix:** Use `response.bytes_with_limit(N)` or stream with a size cap (e.g., 10 MB for images).

---

## Medium

### MED-1: Unused imports in `lib.rs` will cause compiler warnings / dead code
**File:** `src/lib.rs:13–18`

`HashMap`, `patch`, `put` are still imported but unused. (`from_fn_with_state`, `delete`, `get`,
`post` are now used by the authenticated route group — partially resolved by CRIT-1/CRIT-2 fixes.)

**Fix:** Remove `HashMap`, `patch`, `put` until the corresponding routes are implemented.

---

### MED-2: Unused dependencies in `Cargo.toml`
**File:** `Cargo.toml`

`once_cell`, `regex`, `tokio-stream`, and `validator` are listed as dependencies but no current
code uses them. This increases compile times and binary size.

**Fix:** Remove until they are needed. Add back with a commit message explaining the planned use.

---

### MED-3: `now_secs()` uses `unwrap()` on system time
**File:** `src/handlers/auth.rs:39–43`

`SystemTime::now().duration_since(UNIX_EPOCH).unwrap()` panics if the system clock is before the
Unix epoch (extremely unlikely but technically possible on misconfigured systems). More importantly,
`unwrap()` is a code smell in production handlers.

**Fix:** Use `chrono::Utc::now().timestamp() as u64` or handle the error explicitly.

---

### MED-4: `me` handler returns 500 if account has no main character
**File:** `src/handlers/auth.rs:354–361`

If an account somehow has no `is_main = true` character (e.g., data integrity issue or the main
was deleted without promoting another), `me` returns a 500 with a warning log. This is reasonable
for a data integrity violation, but the 500 is generic and gives the client no actionable
information.

**Fix:** Return a more specific 409 Conflict or 422 with an `{"error": "account has no main character"}` body.

---

### MED-5: `callback` error responses are inconsistent
**File:** `src/handlers/auth.rs:131`

The `callback` handler uses a mixture of plain text bodies (`(StatusCode, &str).into_response()`)
and the `ApiResponse` envelope used by all other endpoints. Browser-facing redirect errors look
different from API endpoint errors.

**Fix:** Since `/auth/callback` is a redirect endpoint (browser-facing), plain-text error bodies
are arguably acceptable; alternatively, redirect to a `{FRONTEND_URL}/error?reason=...` page
instead of returning an HTTP error body.

---

### MED-6: No rate limiting or brute-force protection on auth endpoints
**File:** `src/handlers/auth.rs`

The `/auth/callback` endpoint accepts arbitrary `code` and `state` values with no rate limiting.
An attacker could submit forged state JWTs at high frequency. The 5-minute state JWT expiry and
HS256 signing mitigate this considerably, but there is no per-IP rate limit.

**Fix:** Add tower middleware (e.g., `tower_governor`) or a simple per-IP counter in AppState for
the callback endpoint.

---

### MED-7: SDE download saves to `/tmp` with predictable filename
**File:** `src/services/sde_solar_system.rs`

The SDE ZIP is written to `/tmp/erbridge-sde/sde-{buildNumber}.zip`. On a multi-tenant machine
this path is world-writable, allowing a local attacker to pre-place a malicious ZIP file before
the download completes (symlink attack).

**Fix:** Create the temp file with `tempfile::NamedTempFile` (add `tempfile` crate) so the path
is unpredictable and the file handle is held exclusively.

---

### MED-8: `bulk_update_corp_alliance` touches `updated_at` for all characters
**File:** `src/db/character.rs:309–340`

The UNNEST bulk update sets `updated_at = now()` for every character being refreshed.
Since `updated_at` is the refresh-token-age proxy (ADR-029), this means a scheduled corp/alliance
refresh for a ghost character (no tokens) resets its "token age" to now, potentially masking a
stale token situation.

**Fix:** Either only update `updated_at` when tokens actually change, or use a separate
`corp_alliance_refreshed_at` column so the token-age proxy isn't confused.

---

## Low

### LOW-1: Missing `CONTENT_LENGTH` header on image proxy response
**File:** `src/handlers/images.rs:74–83`

The image proxy returns `Content-Type` and `Cache-Control` headers but not `Content-Length`.
Browsers and intermediary caches can't pre-allocate buffers efficiently.

**Fix:** Add `(header::CONTENT_LENGTH, data.len().to_string())` to the response headers.

---

### LOW-2: `require_active_account` middleware double-decodes JWT
**File:** `src/middleware.rs`

The `AccountId` extractor already decodes and validates the session JWT. The middleware then calls
`get_account_status` with the already-extracted `account_id`, which is correct — but only because
both the extractor and middleware are applied to the same request. If someone applies the middleware
without the `AccountId` extractor (i.e., on a public route), it will return a 401 silently. This
coupling is invisible.

**Fix:** Document the dependency explicitly, or restructure so the middleware extracts `AccountId`
internally (currently it does via the `AccountId(account_id): AccountId` parameter, so this is
actually fine — but worth a code comment).

---

### LOW-3: `find_all_characters_for_refresh` returns ghost characters
**File:** `src/db/character.rs:297–305`

`find_all_characters_for_refresh` includes ghost characters (those with `account_id = NULL`). Ghost
characters have no tokens. If a background task tries to refresh them, it will get a `bail!` error
from `ensure_token_fresh`.

**Fix:** Add `WHERE account_id IS NOT NULL` to the query.

---

### LOW-4: Stale `#[allow(dead_code)]` or unused struct fields not flagged
**File:** Various

`CharacterForRefresh` and `Account` have fields that may go unused depending on which background
tasks exist. The Rust compiler will flag these when the project is built in release mode; confirm
all public structs' fields are consumed somewhere.

---

### LOW-5: No structured error for `attach_character_to_account` "wrong account" case
**File:** `src/services/auth.rs:61–63`

When a character is already on a different account, `anyhow::ensure!` is used, which produces a
generic `anyhow::Error`. The handler maps this to `400 BAD_REQUEST` with body
`"could not attach character"`, losing the specific reason.

**Fix:** Return a typed error (`thiserror` enum) from `attach_character_to_account` so the handler
can differentiate "character belongs to another account" from other errors and produce a more
descriptive response.

---

### LOW-6: `validate` crate imported but no validation is performed
**File:** `Cargo.toml`, `src/handlers/images.rs`

`validator` is a dev/prod dependency but nothing uses `#[derive(Validate)]` or calls
`.validate()`. Input to image handler path parameters (category, id, variation) is only validated
via pattern match, not the validator crate.

**Fix:** Remove unless actively used. If path/query validation is desired, use it consistently.

---

## Code Quality / Maintenance

### CQ-1: `AuthMode` and `StateClaims` are in `dto` but have service/handler-level logic
**File:** `src/dto/auth.rs`

Minor architecture note: `AuthMode` drives branching in `handlers::auth::callback`. DTOs are
typically passive data structures; the branching logic in the handler is fine, but future ADRs may
want a service-layer function that accepts `AuthMode` explicitly.

### CQ-2: Multiple mutable references to `JwkSet` not truly single-writer guaranteed
**File:** `src/handlers/auth.rs:205`

The JWKS rotation code (ADR-030) drops the read lock, fetches fresh JWKS, then acquires a write
lock. Between drop and acquire, another concurrent callback could also trigger a re-fetch. This is
harmless (both write the same result from CCP) but causes N redundant HTTP fetches under burst
traffic. Consider a `tokio::sync::Mutex` or a "fetch-in-progress" flag if JWKS rotation storms
become a concern.

### CQ-3: Inconsistent `account_id` logging format
**File:** `src/handlers/auth.rs`

Some log calls use `account_id = %account_id` (line 288) while others use
`account_id = %acc.id` (line 222). Standardize to a single field name for grep-ability.
