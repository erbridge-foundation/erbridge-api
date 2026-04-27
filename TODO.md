# TODO

Gaps and issues found during codebase audit on 2026-04-26.

---

## ~~No auth on debug endpoint~~ ✓ Fixed

Gated behind `#[cfg(debug_assertions)]` — route only exists in dev/test builds.

---

## ~~`list_members` has no ACL access check~~ ✓ Fixed

Now requires `Manage` or `Admin` permission (owner or character member with manage/admin).
Returns 403 for non-members, 404 for unknown ACL.

---

## ~~Purge tasks never run~~ ✓ Fixed

New `tasks::purge` module: daily background task calls `purge_expired_accounts` and
`purge_expired_acls` using `ACCOUNT_DELETION_GRACE_DAYS` for both. Wired in `main.rs`.

---

## `tentative` connection status is unreachable

**Files:** `src/db/connection.rs:recompute_connection_status`, `migrations/0005_create_maps_core.sql`

The `recompute_connection_status` function sets status to `tentative` when fewer than 2 ends exist, but `insert_connection` always inserts both ends atomically in the same statement — so the count is always exactly 2. `tentative` can never be reached after insert, and the initial migration default is `partial` anyway.

Fix (pick one):
- Remove the `tentative` branch from the CASE expression and the CHECK constraint if the status is genuinely unused.
- Or: reserve `tentative` for a future "one-sided connection" feature and document this clearly.

---

## ~~No API endpoints to delete connections or signatures~~ ✓ Fixed

- `DELETE /api/v1/maps/{map_id}/connections/{conn_id}` — soft-deletes (status → `collapsed`), min `ReadWrite`
- `DELETE /api/v1/maps/{map_id}/signatures/{sig_id}` — soft-deletes (status → `deleted`), min `ReadWrite`
- `GET /api/v1/maps/{map_id}` also added (wires `services::map::get_map`), min `Read`

---

## ~~Unused service functions~~ ✓ Fixed

`list_maps_for_account` removed. `get_map` wired to new `GET /api/v1/maps/{map_id}` handler.

---

## ~~Fragile dual-`AppState` in `main.rs`~~ ✓ Fixed

`online_poll_tx` changed to `Option<Sender<...>>` in `AppState`. Poller state uses `None`;
router state uses `Some(...)`. Send sites use `.as_ref().expect(...)` to fail loudly.

---

## Minor / low-priority

- ~~**`once_cell::sync::Lazy`**~~ ✓ Already replaced with `std::sync::LazyLock`; `once_cell` removed from `Cargo.toml`.
- ~~**`unsafe` env mutation in tests**~~ ✓ Wrapped in `static ENV_MUTEX: Mutex<()>` to serialize env-mutating tests.
- ~~**`tokio-stream` dependency**~~ ✓ Removed from `Cargo.toml` (no current usage; re-add when SSE streams are implemented).
- **`rand` 0.10** — downgrade to 0.9 causes compile failures; staying on 0.10 for now.
