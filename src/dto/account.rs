use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

/// POST /api/v1/account/api-keys request body.
/// `name` length matches the convention used in `dto/acl.rs::AclNameRequest` and `dto/map.rs`.
#[derive(Deserialize, Validate)]
pub struct CreateApiKeyRequest {
    #[validate(length(min = 1, max = 100))]
    pub name: String,
}

/// A single key entry in list responses. Never includes the plaintext key.
#[derive(Serialize)]
pub struct ApiKeyEntry {
    pub id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

/// Returned by POST /api/v1/account/api-keys (one-time plaintext reveal).
#[derive(Serialize)]
pub struct ApiKeyCreatedResponse {
    pub id: Uuid,
    pub name: String,
    /// The raw token — shown once only, never stored.
    pub api_key: String,
    pub created_at: DateTime<Utc>,
}

/// Returned by GET /api/v1/account/api-keys.
#[derive(Serialize)]
pub struct ApiKeyListResponse {
    pub api_keys: Vec<ApiKeyEntry>,
}
