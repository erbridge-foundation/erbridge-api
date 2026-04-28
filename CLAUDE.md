# Claude Instructions for erbridge-api

## Start here before exploring

Read `CODEBASE.md` for a complete reference of the project structure, API routes, database schema,
authentication flow, key patterns, and configuration. Do not spend tokens re-exploring the source
unless you need the exact current code for a specific detail — `CODEBASE.md` covers everything at
the architectural level.

## Directories to ignore

**Do not read or explore the `zz-ref/` directory** unless the user explicitly instructs you to.
It contains reference/scratch material that is not part of the active codebase.

## Architecture decisions

Read `DECISIONS.md` for decisions that are not obvious from the code —
particularly the `audit_log` scope rule (admin/compliance actions only; map
mutations go to `map_events` instead).

## Project conventions

- All service/DB functions return `anyhow::Result<T>`; handlers map errors to `StatusCode`
- Response envelope: `ApiResponse<T>` → `{"data":…}` or `{"error":"…"}`
- Structured logging via `tracing`; always use `%` format for errors: `error = %e`
- AES-256-GCM for stored ESI tokens (nonce prepended to ciphertext)
- HS256 JWTs for session/state cookies; RS256 for EVE SSO token verification
- Use `UNNEST` pattern for bulk DB operations (see `db::character`, `db::sde_solar_system`)
- sqlx compile-time checked queries — run `cargo sqlx prepare` after changing queries
- No OpenSSL; rustls throughout

## Known issues to be aware of

- `AppState::online_poll_tx` is `Option<Sender<...>>` — `None` in the poller state, `Some` in the router state; send sites use `.as_ref().expect(...)`
- Admin-role routes (`admin_purge_account`, `admin_restore_account`) are stubs (always return 403, not registered in `lib.rs`) — full `/api/v1/admin` endpoint implementation is upcoming work

## Testing

- Dev dependencies are set up for integration tests: `axum-test`, `pg-embed`, `wiremock`, `portpicker`, `cookie`.
- Unit tests live in `#[cfg(test)]` modules within `src/`.
- Integration tests live in `tests/` — see `tests/common/mod.rs` for shared helpers (`setup_db`, `test_state`, `make_session_jwt`).
- Always generate unit and/or integration tests
