# ── Health ────────────────────────────────────────────────────────────────────

GET {{base_url}}/api/health
HTTP 200
[Asserts]
jsonpath "$.status" == "ok"
jsonpath "$.components.database" == "ok"

# ── GET /api/v1/me — admin key ────────────────────────────────────────────────

GET {{base_url}}/api/v1/me
Authorization: Bearer {{admin_api_key}}
HTTP 200
[Asserts]
jsonpath "$.data.account_id" exists
jsonpath "$.data.character" exists
[Captures]
admin_account_id: jsonpath "$.data.account_id"

# ── GET /api/v1/me — user key ─────────────────────────────────────────────────

GET {{base_url}}/api/v1/me
Authorization: Bearer {{user_api_key}}
HTTP 200
[Asserts]
jsonpath "$.data.account_id" exists
jsonpath "$.data.character" exists
[Captures]
user_account_id: jsonpath "$.data.account_id"

# ── Tenancy isolation: each key sees its own account ─────────────────────────
# The two account IDs must differ.

GET {{base_url}}/api/v1/me
Authorization: Bearer {{admin_api_key}}
HTTP 200
[Asserts]
jsonpath "$.data.account_id" == "{{admin_account_id}}"
jsonpath "$.data.account_id" != "{{user_account_id}}"

GET {{base_url}}/api/v1/me
Authorization: Bearer {{user_api_key}}
HTTP 200
[Asserts]
jsonpath "$.data.account_id" == "{{user_account_id}}"
jsonpath "$.data.account_id" != "{{admin_account_id}}"

# ── GET /api/v1/characters — both keys ───────────────────────────────────────

GET {{base_url}}/api/v1/characters
Authorization: Bearer {{admin_api_key}}
HTTP 200
[Asserts]
jsonpath "$.data" exists

GET {{base_url}}/api/v1/characters
Authorization: Bearer {{user_api_key}}
HTTP 200
[Asserts]
jsonpath "$.data" exists

# ── No auth → 401 ─────────────────────────────────────────────────────────────

GET {{base_url}}/api/v1/me
HTTP 401
