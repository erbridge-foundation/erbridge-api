# Architecture Decisions (model context)

## Layering: handler → service → db

Handlers call service functions. Service functions call db functions. Handlers **must not** call `db::*` directly.

- `src/handlers/` — request parsing, auth extractors, response serialisation only
- `src/services/` — all business logic, validation, multi-step transactions, cross-entity invariants
- `src/db/` — pure data access, no business logic

## audit_log scope

`audit_log` records administrative/compliance actions only. Map mutations go to `map_events`.

| Action | `audit_log`? |
|--------|-------------|
| Account register/delete/purge | Yes |
| Character add/remove/set-main | Yes |
| ACL create/rename/delete | Yes |
| ACL member add/update/remove | Yes |
| ACL attach/detach to map | Yes |
| Map create/delete | Yes |
| Map connection/signature mutations | **No** — use `map_events` |

## Server admin role

- `account.is_server_admin` (boolean) gates `/api/v1/admin/*`. No RBAC, no grants table.
- Admins can: list/reassign/hard-delete maps and ACLs, block characters, grant/revoke admin, read `audit_log`.
- Admins **cannot**: read ACL members or map contents (normal ACL path required).
- `/api/v1/maps` always applies ownership + ACL filtering even for admins; `/api/v1/admin/maps` shows all.

## Banning model

- No account-level ban column. Ban lever is `blocked_eve_character` keyed on `eve_character_id`.
- **One ban = account banned**: any blocked character on an account rejects login, add-character, and `require_active_account` for the whole account.
- Login and add-character additionally reject the requesting `eve_character_id` directly, before any account/character lookup.

## Ownership transfer

Reassigning `map.owner_account_id` or `acl.owner_account_id` silently removes the previous owner's owner-derived permission. No automatic ACL adjustment.
