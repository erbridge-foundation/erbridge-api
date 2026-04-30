# ── POST /api/v1/maps — create a map (user owns it) ──────────────────────────

POST {{base_url}}/api/v1/maps
Authorization: Bearer {{user_api_key}}
Content-Type: application/json
{
  "name": "Hurl Test Map",
  "slug": "hurl-test-map"
}
HTTP 201
[Asserts]
jsonpath "$.data.id" exists
jsonpath "$.data.name" == "Hurl Test Map"
jsonpath "$.data.slug" == "hurl-test-map"
[Captures]
map_id: jsonpath "$.data.id"

# ── GET /api/v1/maps — list maps ─────────────────────────────────────────────

GET {{base_url}}/api/v1/maps
Authorization: Bearer {{user_api_key}}
HTTP 200
[Asserts]
jsonpath "$.data.maps" count > 0

# ── GET /api/v1/maps/:map_id — get map by id ─────────────────────────────────

GET {{base_url}}/api/v1/maps/{{map_id}}
Authorization: Bearer {{user_api_key}}
HTTP 200
[Asserts]
jsonpath "$.data.id" == "{{map_id}}"
jsonpath "$.data.name" == "Hurl Test Map"

# ── PATCH /api/v1/maps/:map_id — update map ──────────────────────────────────

PATCH {{base_url}}/api/v1/maps/{{map_id}}
Authorization: Bearer {{user_api_key}}
Content-Type: application/json
{
  "name": "Hurl Test Map Updated",
  "slug": "hurl-test-map"
}
HTTP 200
[Asserts]
jsonpath "$.data.name" == "Hurl Test Map Updated"

# ── POST /api/v1/maps/:map_id/connections — create a connection ───────────────
# Requires solar system IDs present in sde_solar_system.
# Jita (30000142) and Perimeter (30000144) are well-known EVE systems.
# CI step 7 must ensure SDE data is loaded before running hurl.

POST {{base_url}}/api/v1/maps/{{map_id}}/connections
Authorization: Bearer {{user_api_key}}
Content-Type: application/json
{
  "system_a_id": 30000142,
  "system_b_id": 30000144
}
HTTP 201
[Asserts]
jsonpath "$.data.connection.connection_id" exists
jsonpath "$.data.connection.status" == "partial"
jsonpath "$.data.end_a.system_id" == 30000142
jsonpath "$.data.end_b.system_id" == 30000144
[Captures]
connection_id: jsonpath "$.data.connection.connection_id"

# ── POST /api/v1/maps/:map_id/signatures — add a signature ───────────────────

POST {{base_url}}/api/v1/maps/{{map_id}}/signatures
Authorization: Bearer {{user_api_key}}
Content-Type: application/json
{
  "system_id": 30000142,
  "sig_code": "ABC-123",
  "sig_type": "wormhole"
}
HTTP 201
[Asserts]
jsonpath "$.data.signature_id" exists
jsonpath "$.data.sig_code" == "ABC-123"
[Captures]
sig_id: jsonpath "$.data.signature_id"

# ── POST …/connections/:conn_id/link — link signature to connection end ───────

POST {{base_url}}/api/v1/maps/{{map_id}}/connections/{{connection_id}}/link
Authorization: Bearer {{user_api_key}}
Content-Type: application/json
{
  "signature_id": "{{sig_id}}",
  "side": "a"
}
HTTP 204

# ── PATCH …/connections/:conn_id/metadata — update connection metadata ────────

PATCH {{base_url}}/api/v1/maps/{{map_id}}/connections/{{connection_id}}/metadata
Authorization: Bearer {{user_api_key}}
Content-Type: application/json
{
  "life_state": "eol",
  "mass_state": "reduced"
}
HTTP 204

# ── GET /api/v1/maps/:map_id/routes ──────────────────────────────────────────

GET {{base_url}}/api/v1/maps/{{map_id}}/routes?start_system_id=30000142
Authorization: Bearer {{user_api_key}}
HTTP 200
[Asserts]
jsonpath "$.data.routes" exists

# ── DELETE …/signatures/:sig_id ───────────────────────────────────────────────

DELETE {{base_url}}/api/v1/maps/{{map_id}}/signatures/{{sig_id}}
Authorization: Bearer {{user_api_key}}
HTTP 204

# ── DELETE …/connections/:conn_id ─────────────────────────────────────────────

DELETE {{base_url}}/api/v1/maps/{{map_id}}/connections/{{connection_id}}
Authorization: Bearer {{user_api_key}}
HTTP 204

# ── DELETE /api/v1/maps/:map_id ───────────────────────────────────────────────

DELETE {{base_url}}/api/v1/maps/{{map_id}}
Authorization: Bearer {{user_api_key}}
HTTP 204

# ── Deleted map is gone ───────────────────────────────────────────────────────

GET {{base_url}}/api/v1/maps/{{map_id}}
Authorization: Bearer {{user_api_key}}
HTTP 404

# ── ACL attach/detach lifecycle ───────────────────────────────────────────────

POST {{base_url}}/api/v1/maps
Authorization: Bearer {{user_api_key}}
Content-Type: application/json
{
  "name": "Map for ACL attach test",
  "slug": "hurl-acl-attach-map"
}
HTTP 201
[Captures]
acl_map_id: jsonpath "$.data.id"

POST {{base_url}}/api/v1/acls
Authorization: Bearer {{user_api_key}}
Content-Type: application/json
{
  "name": "hurl-attach-acl"
}
HTTP 201
[Captures]
attach_acl_id: jsonpath "$.data.id"

POST {{base_url}}/api/v1/maps/{{acl_map_id}}/acls
Authorization: Bearer {{user_api_key}}
Content-Type: application/json
{
  "acl_id": "{{attach_acl_id}}"
}
HTTP 204

DELETE {{base_url}}/api/v1/maps/{{acl_map_id}}/acls/{{attach_acl_id}}
Authorization: Bearer {{user_api_key}}
HTTP 204

DELETE {{base_url}}/api/v1/maps/{{acl_map_id}}
Authorization: Bearer {{user_api_key}}
HTTP 204
