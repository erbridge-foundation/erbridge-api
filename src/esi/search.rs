use anyhow::{Context, Result};
use serde::Deserialize;
use strum::{Display, EnumString};
use url::Url;

use super::esi_request;

// ---------------------------------------------------------------------------
// Search category enum
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Hash, Display, EnumString)]
#[strum(serialize_all = "snake_case")]
pub enum SearchCategory {
    Agent,
    Alliance,
    Character,
    Constellation,
    Corporation,
    Faction,
    InventoryType,
    Region,
    SolarSystem,
    Station,
    Structure,
}

// ---------------------------------------------------------------------------
// ESI response shape
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Default)]
pub struct EsiSearchResponse {
    #[serde(default)]
    pub agent: Vec<i64>,
    #[serde(default)]
    pub alliance: Vec<i64>,
    #[serde(default)]
    pub character: Vec<i64>,
    #[serde(default)]
    pub constellation: Vec<i64>,
    #[serde(default)]
    pub corporation: Vec<i64>,
    #[serde(default)]
    pub faction: Vec<i64>,
    #[serde(default)]
    pub inventory_type: Vec<i64>,
    #[serde(default)]
    pub region: Vec<i64>,
    #[serde(default)]
    pub solar_system: Vec<i64>,
    #[serde(default)]
    pub station: Vec<i64>,
    #[serde(default)]
    pub structure: Vec<i64>,
}

impl EsiSearchResponse {
    /// Returns all IDs paired with their category, consuming self.
    pub fn into_categorised(self) -> Vec<(SearchCategory, i64)> {
        let mut out = Vec::new();
        for id in self.agent {
            out.push((SearchCategory::Agent, id));
        }
        for id in self.alliance {
            out.push((SearchCategory::Alliance, id));
        }
        for id in self.character {
            out.push((SearchCategory::Character, id));
        }
        for id in self.constellation {
            out.push((SearchCategory::Constellation, id));
        }
        for id in self.corporation {
            out.push((SearchCategory::Corporation, id));
        }
        for id in self.faction {
            out.push((SearchCategory::Faction, id));
        }
        for id in self.inventory_type {
            out.push((SearchCategory::InventoryType, id));
        }
        for id in self.region {
            out.push((SearchCategory::Region, id));
        }
        for id in self.solar_system {
            out.push((SearchCategory::SolarSystem, id));
        }
        for id in self.station {
            out.push((SearchCategory::Station, id));
        }
        for id in self.structure {
            out.push((SearchCategory::Structure, id));
        }
        out
    }
}

// ---------------------------------------------------------------------------
// ESI call
// ---------------------------------------------------------------------------

/// Searches EVE entities using the authenticated character's ESI token.
///
/// ESI requires the `esi-search.search_structures.v1` scope on the token, and
/// the character_id must match the token holder.
pub async fn search(
    http: &reqwest::Client,
    esi_base: &str,
    character_id: i64,
    access_token: &str,
    categories: &[SearchCategory],
    query: &str,
    strict: bool,
) -> Result<EsiSearchResponse> {
    let url = format!("{esi_base}/characters/{character_id}/search/");

    let mut full_url = Url::parse(&url).context("failed to parse ESI search URL")?;
    {
        let mut pairs = full_url.query_pairs_mut();
        for cat in categories {
            pairs.append_pair("categories", &cat.to_string());
        }
        pairs.append_pair("search", query);
        pairs.append_pair("strict", if strict { "true" } else { "false" });
    }
    let url_str = full_url.to_string();

    esi_request(|| {
        let req = http
            .get(&url_str)
            .bearer_auth(access_token)
            .header("X-Compatibility-Date", "2025-12-16");
        async move { req.send().await }
    })
    .await
    .map_err(|e| anyhow::anyhow!(e))
    .context("ESI search failed after retries")?
    .json::<EsiSearchResponse>()
    .await
    .context("failed to parse ESI search response")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn category_display_is_snake_case() {
        assert_eq!(SearchCategory::Agent.to_string(), "agent");
        assert_eq!(SearchCategory::InventoryType.to_string(), "inventory_type");
        assert_eq!(SearchCategory::SolarSystem.to_string(), "solar_system");
    }

    #[test]
    fn category_round_trip() {
        for (s, expected) in [
            ("agent", SearchCategory::Agent),
            ("alliance", SearchCategory::Alliance),
            ("character", SearchCategory::Character),
            ("constellation", SearchCategory::Constellation),
            ("corporation", SearchCategory::Corporation),
            ("faction", SearchCategory::Faction),
            ("inventory_type", SearchCategory::InventoryType),
            ("region", SearchCategory::Region),
            ("solar_system", SearchCategory::SolarSystem),
            ("station", SearchCategory::Station),
            ("structure", SearchCategory::Structure),
        ] {
            let parsed =
                SearchCategory::from_str(s).unwrap_or_else(|_| panic!("failed to parse {s}"));
            assert_eq!(parsed, expected);
            assert_eq!(parsed.to_string(), s);
        }
    }

    #[test]
    fn category_unknown_string_errors() {
        assert!(SearchCategory::from_str("unknown").is_err());
        assert!(SearchCategory::from_str("").is_err());
        assert!(SearchCategory::from_str("Character").is_err()); // must be lowercase
    }

    #[test]
    fn into_categorised_maps_all_fields() {
        let resp = EsiSearchResponse {
            character: vec![111, 222],
            corporation: vec![333],
            alliance: vec![444],
            ..Default::default()
        };
        let result = resp.into_categorised();
        assert_eq!(result.len(), 4);
        assert!(result.contains(&(SearchCategory::Character, 111)));
        assert!(result.contains(&(SearchCategory::Character, 222)));
        assert!(result.contains(&(SearchCategory::Corporation, 333)));
        assert!(result.contains(&(SearchCategory::Alliance, 444)));
    }

    #[test]
    fn into_categorised_empty_response() {
        let result = EsiSearchResponse::default().into_categorised();
        assert!(result.is_empty());
    }
}
