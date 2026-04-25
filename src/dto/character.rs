use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
pub struct CharacterResponse {
    pub id: Uuid,
    pub eve_character_id: i64,
    pub name: String,
    pub corporation_id: i64,
    pub corporation_name: String,
    pub alliance_id: Option<i64>,
    pub alliance_name: Option<String>,
    pub is_main: bool,
}

/// Data payload for `GET /api/v1/characters`.
#[derive(Serialize, Deserialize)]
pub struct CharacterListResponse {
    pub characters: Vec<CharacterResponse>,
}
