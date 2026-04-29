# Server Admin Role — Implementation TODO

Tracks progress for the server-admin feature described in `DECISIONS_context.md`
("Server admin role: scope and bootstrap").

## Progress

| #   | Step                                                                                       | Status |
| --- | ------------------------------------------------------------------------------------------ | ------ |
| 1   | Migration `0015_account_server_admin.sql` (column + partial index)                         | [x]    |
| 2   | Migration `0016_blocked_eve_character.sql` (table + FK)                                    | [x]    |
| 3   | `ServerAdmin` extractor + `require_server_admin` middleware                                | [x]    |
| 4   | First-account bootstrap branch in `services::auth::login_or_register` (+ audit emit)       | [x]    |
| 5   | `db::account` helpers: `set_server_admin`, `count_server_admins`, blocked-character CRUD   | [x]    |
| 6   | `AuditEvent` variants for admin actions (8 new variants — see notes)                       | [x]    |
| 7   | `/api/v1/admin/maps` — list / change-owner / hard-delete                                   | [x]    |
| 8   | `/api/v1/admin/acls` — list / change-owner / hard-delete (no member exposure)              | [x]    |
| 9   | `/api/v1/admin/characters/{eve_id}/block`, `/unblock`, `GET /blocked`                      | [x]    |
| 10  | Login + add-character gates: reject blocked EVE ids                                        | [x]    |
| 11  | `require_active_account` extension: reject accounts with no usable characters              | [x]    |
| 12  | `/api/v1/admin/accounts` — list / grant-admin / revoke-admin (with last-admin guard)       | [x]    |
| 13  | Fold existing `admin_purge_account` / `admin_restore_account` stubs into admin router      | [x]    |
| 14  | `/api/v1/admin/audit-log` — paginated read with filters (`event_type`, `actor`, `before`)  | [x]    |
| 15  | Wire `admin_router` into `lib.rs` under `require_active_account` + `require_server_admin`  | [x]    |
| 16  | Integration tests — admin gating, bootstrap, blocked-character flow, last-admin guard      | [x]    |
| 17  | `cargo sqlx prepare` + `cargo test` clean                                                  | [x]    |
| 18  | Update `CODEBASE.md` and `CODEBASE_context.md` (routes table, schema, file list)                                     | [x]    |

## Notes

### Step 6 — audit variants

Add to `AuditEvent`:

- `ServerAdminGranted { account_id, source }` — `source` ∈ `"first_account_bootstrap" | "admin_grant"` (already added in step 4 — only the `AdminGrant` source is unused so far)
- `ServerAdminRevoked { account_id }`
- `AdminMapOwnershipChanged { map_id, old_owner, new_owner }`
- `AdminMapHardDeleted { map_id, name }`
- `AdminAclOwnershipChanged { acl_id, old_owner, new_owner }`
- `AdminAclHardDeleted { acl_id, name }`
- `EveCharacterBlocked { eve_character_id, reason }`
- `EveCharacterUnblocked { eve_character_id }`

Per `DECISIONS_context.md` audit-log scope rule, all of these are admin/compliance
actions and belong in `audit_log` (not `map_events`).

### Step 4 — bootstrap

Replace the existing INSERT in `login_or_register` with:

```sql
INSERT INTO account (is_server_admin)
SELECT NOT EXISTS (SELECT 1 FROM account)
RETURNING id, is_server_admin
```

If `is_server_admin` came back true, emit `ServerAdminGranted` with
`actor = NULL`, `details.source = "first_account_bootstrap"` in the same
transaction.

### Step 11 — banned-account check

One ban = account banned. Add a query that returns true if the account has
*any* blocked character: `SELECT EXISTS (SELECT 1 FROM eve_character ec JOIN blocked_eve_character b ON b.eve_character_id = ec.eve_character_id WHERE ec.account_id = $1)`.
True → `403 account is blocked`. The same check is also applied in the login
and add-character flows post-account-resolve so banned accounts cannot get a
session cookie via an unblocked alt.

### Step 12 — last-admin guard

`revoke-admin` runs inside a transaction:

```sql
SELECT count(*) FROM account WHERE is_server_admin = TRUE
```

If revoking would drop the count to zero, return `409 Conflict` with
`{"error": "cannot revoke the last server admin"}`. Self-revoke is permitted
otherwise.

### Step 15 — router wiring

`admin_router` mounts under both `require_active_account` and
`require_server_admin` (in that order). Merge into `build_router` alongside the
existing `authenticated` router.
