# ── POST /api/v1/acls — create an ACL (user key) ─────────────────────────────

POST {{base_url}}/api/v1/acls
Authorization: Bearer {{user_api_key}}
Content-Type: application/json
{
  "name": "hurl-test-acl"
}
HTTP 201
[Asserts]
jsonpath "$.data.id" exists
jsonpath "$.data.name" == "hurl-test-acl"
[Captures]
user_acl_id: jsonpath "$.data.id"

# ── POST /api/v1/acls — create an ACL (admin key, for cross-tenancy probe) ───

POST {{base_url}}/api/v1/acls
Authorization: Bearer {{admin_api_key}}
Content-Type: application/json
{
  "name": "hurl-admin-acl"
}
HTTP 201
[Captures]
admin_acl_id: jsonpath "$.data.id"

# ── GET /api/v1/acls — list (user sees own ACLs only) ────────────────────────

GET {{base_url}}/api/v1/acls
Authorization: Bearer {{user_api_key}}
HTTP 200
[Asserts]
jsonpath "$.data.acls" count > 0

# ── Cross-tenancy probe: user list must not contain admin's ACL id ────────────

GET {{base_url}}/api/v1/acls
Authorization: Bearer {{user_api_key}}
HTTP 200
[Asserts]
# The full response body must not contain the admin's ACL id string
body not contains "{{admin_acl_id}}"

# ── POST /api/v1/acls/:acl_id/members — add a corporation member ─────────────
# Uses NPC corp 1000001 (CCP Corp) — ESI name-resolve gracefully falls back
# to the ID string when ESI is unavailable, so this always succeeds.

POST {{base_url}}/api/v1/acls/{{user_acl_id}}/members
Authorization: Bearer {{user_api_key}}
Content-Type: application/json
{
  "member_type": "corporation",
  "eve_entity_id": 1000001,
  "permission": "read"
}
HTTP 201
[Asserts]
jsonpath "$.data.id" exists
jsonpath "$.data.permission" == "read"
[Captures]
corp_member_id: jsonpath "$.data.id"

# ── POST /api/v1/acls/:acl_id/members — add the seed admin character ─────────
# The seed inserts eve_character_id=90000001 so the ghost-character lookup
# hits the existing row without an ESI call.

POST {{base_url}}/api/v1/acls/{{user_acl_id}}/members
Authorization: Bearer {{user_api_key}}
Content-Type: application/json
{
  "member_type": "character",
  "eve_entity_id": 90000001,
  "permission": "read"
}
HTTP 201
[Asserts]
jsonpath "$.data.id" exists
[Captures]
char_member_id: jsonpath "$.data.id"

# ── GET /api/v1/acls/:acl_id/members — list members ─────────────────────────

GET {{base_url}}/api/v1/acls/{{user_acl_id}}/members
Authorization: Bearer {{user_api_key}}
HTTP 200
[Asserts]
jsonpath "$.data.members" count == 2

# ── PATCH /api/v1/acls/:acl_id/members/:member_id — update permission ────────

PATCH {{base_url}}/api/v1/acls/{{user_acl_id}}/members/{{corp_member_id}}
Authorization: Bearer {{user_api_key}}
Content-Type: application/json
{
  "permission": "read_write"
}
HTTP 200
[Asserts]
jsonpath "$.data.permission" == "read_write"

# ── PUT /api/v1/acls/:acl_id — rename ────────────────────────────────────────

PUT {{base_url}}/api/v1/acls/{{user_acl_id}}
Authorization: Bearer {{user_api_key}}
Content-Type: application/json
{
  "name": "hurl-test-acl-renamed"
}
HTTP 200
[Asserts]
jsonpath "$.data.name" == "hurl-test-acl-renamed"

# ── DELETE /api/v1/acls/:acl_id/members/:member_id — remove character member ─

DELETE {{base_url}}/api/v1/acls/{{user_acl_id}}/members/{{char_member_id}}
Authorization: Bearer {{user_api_key}}
HTTP 204

# ── DELETE /api/v1/acls/:acl_id — delete the ACL ────────────────────────────

DELETE {{base_url}}/api/v1/acls/{{user_acl_id}}
Authorization: Bearer {{user_api_key}}
HTTP 204

# ── Clean up admin ACL ────────────────────────────────────────────────────────

DELETE {{base_url}}/api/v1/acls/{{admin_acl_id}}
Authorization: Bearer {{admin_api_key}}
HTTP 204
