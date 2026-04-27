use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use reqwest::Client;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::db::sde_solar_system::{self, SdeSolarSystem, SdeSolarSystemMetadata};

const SDE_METADATA_URL: &str =
    "https://developers.eveonline.com/static-data/tranquility/latest.jsonl";
const SDE_ZIP_URL: &str =
    "https://developers.eveonline.com/static-data/tranquility/eve-online-static-data-{}-jsonl.zip";
const SDE_ENTRY_NAME: &str = "mapSolarSystems.jsonl";

/// Called at startup. If the DB has no SDE data, triggers an initial download.
/// If data is already present, skips immediately.
pub async fn load_sde_if_needed(pool: &PgPool, http: &Client) -> Result<()> {
    let meta = sde_solar_system::current_sde_metadata(pool)
        .await
        .context("current_sde_metadata")?;

    if let Some(ref m) = meta {
        info!(version = %m.sde_version, "SDE already loaded, skipping startup load");
        return Ok(());
    }

    info!("No SDE data in DB, running initial download");
    run_sde_update_check_inner(pool, http, SDE_METADATA_URL, SDE_ZIP_URL).await
}

/// Spawns a daily background task that polls for a new SDE build number and
/// applies updates when the version differs from what is recorded in the DB.
pub fn spawn_sde_update_check(pool: PgPool, http: Client, cancel: CancellationToken) {
    tokio::spawn(async move {
        // Offset so it doesn't fire immediately at startup.
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(10 * 60)) => {}
            _ = cancel.cancelled() => {
                info!("SDE update check: shutting down");
                return;
            }
        }

        let mut interval = tokio::time::interval(Duration::from_secs(24 * 60 * 60));
        interval.tick().await;

        loop {
            tokio::select! {
                _ = interval.tick() => {}
                _ = cancel.cancelled() => {
                    info!("SDE update check: shutting down");
                    return;
                }
            }

            if let Err(e) = run_sde_update_check(&pool, &http).await {
                warn!(error = %e, "SDE update check failed");
            }
        }
    });
}

async fn run_sde_update_check(pool: &PgPool, http: &Client) -> Result<()> {
    run_sde_update_check_inner(pool, http, SDE_METADATA_URL, SDE_ZIP_URL).await
}

pub async fn run_sde_update_check_inner(
    pool: &PgPool,
    http: &Client,
    metadata_url: &str,
    zip_url_template: &str,
) -> Result<()> {
    info!("checking for SDE update");

    let build_number = fetch_build_number(http, metadata_url)
        .await
        .context("fetch build number")?;

    let version_str = build_number.to_string();

    let meta = sde_solar_system::current_sde_metadata(pool)
        .await
        .context("current_sde_metadata")?;

    if meta.as_ref().is_some_and(|m| m.sde_version == version_str) {
        info!(build_number, "SDE is already current, no update needed");
        return Ok(());
    }

    if let Some(ref prev) = meta {
        info!(
            prev_version = %prev.sde_version,
            new_version = build_number,
            "SDE update detected, downloading"
        );
    }

    let jsonl_bytes = download_and_extract_sde(http, build_number, zip_url_template)
        .await
        .context("download and extract SDE")?;

    let checksum = hex_sha256(&jsonl_bytes);
    let systems = parse_jsonl(&jsonl_bytes).context("parse SDE JSONL")?;
    let count = systems.len();

    let mut tx = pool.begin().await.context("begin tx")?;
    sde_solar_system::bulk_upsert_solar_systems(&mut tx, &systems)
        .await
        .context("bulk_upsert_solar_systems")?;
    sde_solar_system::set_sde_metadata(&mut tx, &version_str, &checksum)
        .await
        .context("set_sde_metadata")?;
    tx.commit().await.context("commit tx")?;

    info!(build_number, count, "SDE update applied");
    Ok(())
}

// ---------------------------------------------------------------------------
// Download helpers
// ---------------------------------------------------------------------------

async fn fetch_build_number(http: &Client, metadata_url: &str) -> Result<i64> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct SdeBuildInfo {
        build_number: i64,
    }

    let info: SdeBuildInfo = http
        .get(metadata_url)
        .send()
        .await
        .context("fetch SDE metadata endpoint")?
        .error_for_status()
        .context("SDE metadata endpoint returned error status")?
        .json()
        .await
        .context("parse SDE metadata JSON")?;

    Ok(info.build_number)
}

const SDE_DOWNLOAD_DIR: &str = "/tmp/erbridge-sde";

async fn download_and_extract_sde(
    http: &Client,
    build_number: i64,
    zip_url_template: &str,
) -> Result<Vec<u8>> {
    use futures::StreamExt;
    use tokio::io::AsyncWriteExt;

    let url = zip_url_template.replace("{}", &build_number.to_string());
    info!(build_number, url, "downloading SDE ZIP");

    tokio::fs::create_dir_all(SDE_DOWNLOAD_DIR)
        .await
        .context("create SDE download directory")?;

    let zip_path = Path::new(SDE_DOWNLOAD_DIR).join(format!("sde-{build_number}.zip"));

    let response = http
        .get(&url)
        .timeout(Duration::from_secs(300))
        .send()
        .await
        .context("send SDE ZIP request")?
        .error_for_status()
        .context("SDE ZIP returned error status")?;

    {
        let mut file = tokio::fs::File::create(&zip_path)
            .await
            .context("create SDE ZIP file")?;

        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("read SDE ZIP chunk")?;
            file.write_all(&chunk)
                .await
                .context("write SDE ZIP chunk")?;
        }

        file.flush().await.context("flush SDE ZIP file")?;
    }

    info!(path = %zip_path.display(), "SDE ZIP downloaded, extracting {}", SDE_ENTRY_NAME);

    let result = tokio::task::spawn_blocking({
        let zip_path = zip_path.clone();
        move || extract_entry_from_zip(&zip_path, SDE_ENTRY_NAME)
    })
    .await
    .context("extract task panicked")?
    .context("extract mapSolarSystems.jsonl from ZIP");

    if let Err(e) = tokio::fs::remove_file(&zip_path).await {
        warn!(path = %zip_path.display(), error = %e, "failed to remove SDE ZIP after extraction");
    }

    result
}

fn extract_entry_from_zip(zip_path: &Path, entry_name: &str) -> Result<Vec<u8>> {
    use std::io::Read;

    let file = std::fs::File::open(zip_path).context("open SDE ZIP file")?;
    let reader = std::io::BufReader::new(file);
    let mut archive = zip::ZipArchive::new(reader).context("open ZIP archive")?;

    // Try direct lookup first; fall back to suffix match for nested paths.
    let idx = (0..archive.len())
        .find(|&i| {
            archive
                .by_index(i)
                .map(|f| f.name() == entry_name || f.name().ends_with(&format!("/{entry_name}")))
                .unwrap_or(false)
        })
        .with_context(|| format!("{entry_name} not found in ZIP"))?;

    let mut entry = archive.by_index(idx).context("open ZIP entry")?;
    let mut buf = Vec::with_capacity(entry.size() as usize);
    entry
        .read_to_end(&mut buf)
        .with_context(|| format!("read {entry_name} from ZIP"))?;

    Ok(buf)
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
struct SdeRecord {
    #[serde(rename = "_key")]
    key: i64,
    #[serde(rename = "border")]
    border: Option<bool>,
    #[serde(rename = "corridor", default)]
    corridor: Option<bool>,
    #[serde(rename = "disallowedAnchorCategories", default)]
    disallowed_anchor_categories: Option<Vec<i64>>,
    #[serde(rename = "disallowedAnchorGroups", default)]
    disallowed_anchor_groups: Option<Vec<i64>>,
    #[serde(rename = "factionID")]
    faction_id: Option<i64>,
    #[serde(rename = "fringe", default)]
    fringe: Option<bool>,
    #[serde(rename = "hub", default)]
    hub: Option<bool>,
    #[serde(rename = "international", default)]
    international: Option<bool>,
    #[serde(rename = "luminosity")]
    luminosity: Option<f64>,
    #[serde(rename = "name", default)]
    name: HashMap<String, String>,
    #[serde(rename = "regionID")]
    region_id: Option<i64>,
    #[serde(rename = "constellationID")]
    constellation_id: Option<i64>,
    #[serde(rename = "securityClass")]
    security_class: Option<String>,
    #[serde(rename = "securityStatus")]
    security_status: Option<f64>,
    #[serde(rename = "starID")]
    star_id: Option<i64>,
    #[serde(rename = "planetIDs", default)]
    planet_ids: Option<Vec<i64>>,
    position: Option<serde_json::Value>,
    #[serde(rename = "position2D", default)]
    position_2d: Option<serde_json::Value>,
    radius: Option<f64>,
    #[serde(rename = "regional", default)]
    regional: Option<bool>,
    #[serde(rename = "stargateIDs", default)]
    stargate_ids: Option<Vec<i64>>,
    #[serde(rename = "visualEffect")]
    visual_effect: Option<String>,
    #[serde(rename = "wormholeClassID")]
    wormhole_class_id: Option<i64>,
}

pub fn parse_jsonl(bytes: &[u8]) -> Result<Vec<SdeSolarSystem>> {
    let reader = BufReader::new(bytes);
    let mut systems = Vec::new();

    for (i, line) in reader.lines().enumerate() {
        let line = line.with_context(|| format!("read line {i}"))?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let rec: SdeRecord =
            serde_json::from_str(line).with_context(|| format!("parse line {i}"))?;

        let en_name = match rec.name.get("en") {
            Some(n) => n.clone(),
            None => {
                warn!(
                    solar_system_id = rec.key,
                    "SDE record missing English name, using empty string"
                );
                String::new()
            }
        };

        let name_i18n = serde_json::to_value(&rec.name).ok();
        let planet_ids = rec
            .planet_ids
            .as_ref()
            .map(|v| serde_json::to_value(v).unwrap_or(serde_json::Value::Null));
        let stargate_ids = rec
            .stargate_ids
            .as_ref()
            .map(|v| serde_json::to_value(v).unwrap_or(serde_json::Value::Null));
        let disallowed_anchor_categories = rec
            .disallowed_anchor_categories
            .as_ref()
            .map(|v| serde_json::to_value(v).unwrap_or(serde_json::Value::Null));
        let disallowed_anchor_groups = rec
            .disallowed_anchor_groups
            .as_ref()
            .map(|v| serde_json::to_value(v).unwrap_or(serde_json::Value::Null));

        systems.push(SdeSolarSystem {
            solar_system_id: rec.key,
            name: en_name,
            region_id: rec.region_id,
            constellation_id: rec.constellation_id,
            faction_id: rec.faction_id,
            star_id: rec.star_id,
            security_status: rec.security_status.map(|f| f as f32),
            security_class: rec.security_class,
            wh_class: None,
            wormhole_class_id: rec.wormhole_class_id,
            luminosity: rec.luminosity.map(|f| f as f32),
            radius: rec.radius,
            border: rec.border,
            corridor: rec.corridor,
            fringe: rec.fringe,
            hub: rec.hub,
            international: rec.international,
            regional: rec.regional,
            visual_effect: rec.visual_effect,
            name_i18n,
            planet_ids,
            stargate_ids,
            disallowed_anchor_categories,
            disallowed_anchor_groups,
            position: rec.position,
            position_2d: rec.position_2d,
        });
    }

    if systems.is_empty() {
        return Err(anyhow!(
            "parsed 0 systems from JSONL — likely wrong file or empty input"
        ));
    }

    Ok(systems)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn hex_sha256(data: &[u8]) -> String {
    let hash = Sha256::digest(data);
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

// ---------------------------------------------------------------------------
// Metadata accessor (used by tests and diagnostics)
// ---------------------------------------------------------------------------

pub async fn current_metadata(pool: &PgPool) -> Result<Option<SdeSolarSystemMetadata>> {
    sde_solar_system::current_sde_metadata(pool).await
}
