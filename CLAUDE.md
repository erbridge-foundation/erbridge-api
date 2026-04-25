# Claude Instructions for erbridge-api

## Start here before exploring

Read `CODEBASE.md` for a complete reference of the project structure, API routes, database schema,
authentication flow, key patterns, and configuration. Do not spend tokens re-exploring the source
unless you need the exact current code for a specific detail — `CODEBASE.md` covers everything at
the architectural level.

## Directories to ignore

**Do not read or explore the `zz-ref/` directory** unless the user explicitly instructs you to.
It contains reference/scratch material that is not part of the active codebase.

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

- `lib.rs` has unused imports (`patch`, `put`, `HashMap`) — placeholders for planned routes
- `validator`, `once_cell`, `regex`, `tokio-stream` are imported but not yet actively used
- `handlers::character::delete_account` is referenced in `lib.rs` but the `handlers::character` module does not yet exist

## Testing

Dev dependencies are set up for integration tests: `axum-test`, `pg-embed`, `wiremock`, `portpicker`, `cookie`.
Unit tests live in `#[cfg(test)]` modules within `src/`. No integration tests exist yet.
