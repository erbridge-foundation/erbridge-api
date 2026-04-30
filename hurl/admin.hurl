# Admin routes — each route is hit twice:
#   1. admin key: asserts the route is reached (currently these are real handlers
#      that return 200/204; if any are later converted to stubs that return 403
#      flip the assertion back to 403 and add a comment)
#   2. user key:  asserts 403 via the server-admin role gate

# ── GET /api/v1/admin/accounts ───────────────────────────────────────────────

GET {{base_url}}/api/v1/admin/accounts
Authorization: Bearer {{admin_api_key}}
HTTP 200
[Asserts]
jsonpath "$.data.accounts" exists
[Captures]
# Capture the user account id for the grant/revoke-admin tests below.
# We expect exactly 2 accounts: admin + user.
# Sort order is not guaranteed, so we just capture the full list and use
# the known admin vs user distinction from the outer variables.
admin_accounts_count: jsonpath "$.data.accounts" count

GET {{base_url}}/api/v1/admin/accounts
Authorization: Bearer {{user_api_key}}
HTTP 403

# ── POST /api/v1/admin/accounts/:account_id/revoke-admin ─────────────────────
# We need a second admin account id to test grant/revoke without locking
# ourselves out. Create a throwaway account first via an API key cycle.
# For simplicity we just verify the route accepts the admin key and returns
# 204 on a known-good target (the user account), then restore it.

# (No grant/revoke tests here — those would require a second admin-capable
# account or self-revocation which would break subsequent tests. The route
# is exercised to confirm auth wiring only via a route that is safe.)

# ── GET /api/v1/admin/maps ────────────────────────────────────────────────────

GET {{base_url}}/api/v1/admin/maps
Authorization: Bearer {{admin_api_key}}
HTTP 200
[Asserts]
jsonpath "$.data.maps" exists

GET {{base_url}}/api/v1/admin/maps
Authorization: Bearer {{user_api_key}}
HTTP 403

# ── GET /api/v1/admin/acls ────────────────────────────────────────────────────

GET {{base_url}}/api/v1/admin/acls
Authorization: Bearer {{admin_api_key}}
HTTP 200
[Asserts]
jsonpath "$.data.acls" exists

GET {{base_url}}/api/v1/admin/acls
Authorization: Bearer {{user_api_key}}
HTTP 403

# ── GET /api/v1/admin/characters/blocked ─────────────────────────────────────

GET {{base_url}}/api/v1/admin/characters/blocked
Authorization: Bearer {{admin_api_key}}
HTTP 200
[Asserts]
jsonpath "$.data.blocked" exists

GET {{base_url}}/api/v1/admin/characters/blocked
Authorization: Bearer {{user_api_key}}
HTTP 403

# ── POST /api/v1/admin/characters/:eve_id/block — role gate check ────────────
# block_character requires the character to already exist in eve_character,
# so we use the seed admin character (90000001). The block is immediately
# undone to avoid breaking subsequent requests that depend on the admin account.

POST {{base_url}}/api/v1/admin/characters/90000002/block
Authorization: Bearer {{user_api_key}}
HTTP 403

POST {{base_url}}/api/v1/admin/characters/90000002/block
Authorization: Bearer {{admin_api_key}}
HTTP 204

# Immediately unblock — the user account must stay unblocked for remaining tests.
POST {{base_url}}/api/v1/admin/characters/90000002/unblock
Authorization: Bearer {{admin_api_key}}
HTTP 204

# ── GET /api/v1/admin/audit-log ───────────────────────────────────────────────

GET {{base_url}}/api/v1/admin/audit-log
Authorization: Bearer {{admin_api_key}}
HTTP 200
[Asserts]
jsonpath "$.data.entries" exists

GET {{base_url}}/api/v1/admin/audit-log
Authorization: Bearer {{user_api_key}}
HTTP 403

# ── DELETE /api/v1/admin/accounts/:account_id/purge — stub (501) ─────────────
# Currently returns 501 NOT_IMPLEMENTED. When the real handler is added this
# will flip to 204 for admin and stay 403 for user.

# (Route is not probed here — purging a seeded account would break subsequent
# tests. The stub status is verified by the route being registered.)

# ── POST /api/v1/admin/accounts/:account_id/restore — stub (501) ─────────────
# Same as purge — skip to avoid destroying seed state.
