use serde::Serialize;

/// Standard API response envelope (ADR-021).
///
/// Success:  `{ "data": T }`
/// Error:    `{ "error": "..." }`
#[derive(Serialize)]
#[serde(untagged)]
pub enum ApiResponse<T: Serialize> {
    Ok { data: T },
    Error { error: String },
}

impl<T: Serialize> ApiResponse<T> {
    pub fn ok(data: T) -> Self {
        Self::Ok { data }
    }
}

impl ApiResponse<()> {
    pub fn error(msg: impl Into<String>) -> Self {
        Self::Error { error: msg.into() }
    }
}
