use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display, EnumString)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    Login,
    Add,
}

/// Claims stored inside the erbridge session JWT.
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionClaims {
    pub account_id: Uuid,
    /// Expiry (Unix timestamp seconds).
    pub exp: u64,
}

/// Claims stored inside the OAuth state JWT (ADR-014).
#[derive(Debug, Serialize, Deserialize)]
pub struct StateClaims {
    pub client_id: String,
    pub mode: AuthMode,
    /// Present only in `Add` mode (US-003).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<Uuid>,
    /// Expiry (Unix timestamp seconds).
    pub exp: u64,
}

/// Data payload for `GET /api/v1/me`.
#[derive(Serialize, Deserialize)]
pub struct MeResponse {
    pub account_id: Uuid,
    /// The main character.
    pub character: MeCharacter,
    /// All characters on the account (main first).
    pub characters: Vec<MeCharacter>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct MeCharacter {
    pub id: Uuid,
    pub eve_character_id: i64,
    pub name: String,
    pub corporation_id: i64,
    pub alliance_id: Option<i64>,
    pub is_main: bool,
}
