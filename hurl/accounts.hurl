GET http://localhost:8080/api/health
HTTP 200
[Asserts]
jsonpath "$.status" == "ok"
jsonpath "$.components.database" == "ok"

GET http://localhost:8080/api/v1/characters
Authorization: Bearer {{api_token}}
Content-Type: application/json
HTTP 200
