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
