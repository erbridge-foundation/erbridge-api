# ── Health ────────────────────────────────────────────────────────────────────

GET {{base_url}}/api/health
HTTP 200
[Asserts]
jsonpath "$.status" == "ok"
jsonpath "$.components.database" == "ok"
