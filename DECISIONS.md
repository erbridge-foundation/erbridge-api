# Architecture Decisions

Decisions that are not obvious from the code and are likely to be revisited
or questioned in future. See also ADRs referenced inline in `CODEBASE.md`.

---

## audit_log scope: admin/compliance actions only

**Decision:** `audit_log` records changes to *administrative objects* — accounts,
characters, ACLs, maps. It does **not** record day-to-day gameplay mutations
(connections, signatures, link operations).

**Rationale:** Map mutations (create/delete connection, add/delete signature,
link signature, update metadata) are already fully recorded in `map_events`,
which is the authoritative, replayable event log for map state. Writing the
same operations to `audit_log` would turn a compliance/security log into a
high-volume activity log, with different retention and query requirements.

**Boundary:**

| Action | Logged in `audit_log`? | Why |
|--------|------------------------|-----|
| Account register/delete/purge | Yes | Account lifecycle |
| Character add/remove/set-main | Yes | Account object mutation |
| ACL create/rename/delete | Yes | Access control object lifecycle |
| ACL member add/update/remove | Yes | Access control change |
| ACL attach/detach to map | Yes | Access control change (admin permission required) |
| Map create/delete | Yes | Resource lifecycle |
| Map connection/signature mutations | **No** | Gameplay; covered by `map_events` |

**If you are tempted to add a new `AuditEvent` variant for a map mutation:**
ask whether it belongs in `map_events` instead. The test is: is this an
access-control or administrative action, or is it something a regular user
does during normal gameplay?

---

## Server admin role: scope and bootstrap

**Decision:** A single boolean flag `account.is_server_admin` grants access to
the `/api/v1/admin/*` route tree. There is no multi-tier RBAC, no per-permission
grant model, and no separate principals table.

**Capability boundaries:** Server admins are operators, not auditors of private
data. They can list and reassign/hard-delete maps and ACLs, block EVE characters,
grant/revoke admin, and read `audit_log`. They **cannot** read ACL members or
map contents — those go through the normal ACL permission path, which the admin
role does not satisfy. Admins see the full map list only via
`/api/v1/admin/maps`; `/api/v1/maps` continues to behave as for a normal user
(ownership + ACL filtering).

**Bootstrap rule:** The first account to register on a fresh instance becomes a
server admin atomically inside the registration transaction:

```sql
INSERT INTO account (is_server_admin)
SELECT NOT EXISTS (SELECT 1 FROM account)
RETURNING id, is_server_admin;
```

No env var, no startup allowlist, no advisory lock. If two registrations race
on a brand-new instance both could come back as admin — that's acceptable on
first run since the operator can revoke via the admin API. There is **no
re-bootstrap on zero admins**: a deployment that drops to zero admins requires
manual recovery (`UPDATE account SET is_server_admin = TRUE WHERE id = '…'`)
rather than silently re-arming the rule. The last-admin guard on
`revoke-admin` prevents accidentally falling into that state via the API.

**Banning model:** There is no account-level ban column. The single lever is
`blocked_eve_character` keyed on `eve_character_id` — EVE characters are what
authenticate, the account is an internal grouping. **One ban = account
banned**: if *any* character on an account is in `blocked_eve_character`, the
whole account is rejected — login (via any character on that account),
add-character, and `require_active_account` all refuse. This stops a banned
actor from continuing to operate via unblocked alts on the same account.
The login and add-character flows additionally reject the requesting
`eve_character_id` directly before doing any account/character lookup.
`account.status` keeps its existing `active | pending_delete` shape.

**Ownership transfer:** When an admin reassigns `map.owner_account_id` or
`acl.owner_account_id`, the previous owner silently loses owner-derived
permission. No automatic ACL adjustment is made — that is the point of the
operation.
