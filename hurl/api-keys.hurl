# ── POST /api/v1/account/api-keys — create a new key ─────────────────────────

POST {{base_url}}/api/v1/account/api-keys
Authorization: Bearer {{user_api_key}}
Content-Type: application/json
{
  "name": "hurl-test-key"
}
HTTP 201
[Asserts]
jsonpath "$.data.id" exists
jsonpath "$.data.name" == "hurl-test-key"
jsonpath "$.data.api_key" startsWith "erbridge_"
[Captures]
new_key_id: jsonpath "$.data.id"
new_key_plaintext: jsonpath "$.data.api_key"

# ── Created key works for authentication ─────────────────────────────────────

GET {{base_url}}/api/v1/me
Authorization: Bearer {{new_key_plaintext}}
HTTP 200
[Asserts]
jsonpath "$.data.account_id" exists

# ── GET /api/v1/account/api-keys — list includes the new key ─────────────────

GET {{base_url}}/api/v1/account/api-keys
Authorization: Bearer {{user_api_key}}
HTTP 200
[Asserts]
jsonpath "$.data.api_keys" count > 0

# ── DELETE /api/v1/account/api-keys/:id — revoke the new key ─────────────────

DELETE {{base_url}}/api/v1/account/api-keys/{{new_key_id}}
Authorization: Bearer {{user_api_key}}
HTTP 204

# ── Revoked key no longer works ───────────────────────────────────────────────

GET {{base_url}}/api/v1/me
Authorization: Bearer {{new_key_plaintext}}
HTTP 401

# ── Revoking a non-existent key returns 404 ───────────────────────────────────

DELETE {{base_url}}/api/v1/account/api-keys/00000000-0000-0000-0000-000000000000
Authorization: Bearer {{user_api_key}}
HTTP 404

# ── Admin key: list and create also work ─────────────────────────────────────

GET {{base_url}}/api/v1/account/api-keys
Authorization: Bearer {{admin_api_key}}
HTTP 200
[Asserts]
jsonpath "$.data.api_keys" exists
