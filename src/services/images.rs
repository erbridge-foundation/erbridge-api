use std::{sync::Arc, time::Duration};
use tracing::warn;

use crate::state::AppState;

const CACHE_TTL: Duration = Duration::from_secs(60 * 60);

pub enum ImageError {
    UpstreamRequestFailed,
    UpstreamErrorStatus,
    UpstreamBodyFailed,
}

pub async fn fetch_image(
    state: &Arc<AppState>,
    category: &str,
    id: i64,
    variation: &str,
    size: Option<u32>,
    upstream_url: &str,
) -> Result<(Vec<u8>, String), ImageError> {
    let dir = &state.config.image_cache_dir;

    if let Some((data, content_type)) = cache_get(dir, category, id, variation, size) {
        return Ok((data, content_type.to_string()));
    }

    let resp = match state.http.get(upstream_url).send().await {
        Ok(r) => r,
        Err(e) => {
            warn!(error = %e, %upstream_url, "image proxy: upstream request failed");
            return Err(ImageError::UpstreamRequestFailed);
        }
    };

    if !resp.status().is_success() {
        warn!(status = %resp.status(), %upstream_url, "image proxy: upstream returned error");
        return Err(ImageError::UpstreamErrorStatus);
    }

    let content_type = resp
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_else(|| default_content_type_for(category))
        .to_string();

    let data = match resp.bytes().await {
        Ok(b) => b.to_vec(),
        Err(e) => {
            warn!(error = %e, "image proxy: failed to read upstream body");
            return Err(ImageError::UpstreamBodyFailed);
        }
    };

    cache_set(dir, category, id, variation, size, &data, &content_type);
    Ok((data, content_type))
}

fn cache_stem(category: &str, id: i64, variation: &str, size: Option<u32>) -> String {
    match size {
        Some(s) => format!("{}-{}-{}-{}", category, id, variation, s),
        None => format!("{}-{}-{}", category, id, variation),
    }
}

fn cache_get(
    dir: &std::path::Path,
    category: &str,
    id: i64,
    variation: &str,
    size: Option<u32>,
) -> Option<(Vec<u8>, &'static str)> {
    let stem = cache_stem(category, id, variation, size);
    for ext in ["jpg", "png"] {
        let path = dir.join(format!("{}.{}", stem, ext));
        let meta = std::fs::metadata(&path).ok()?;
        let age = meta.modified().ok()?.elapsed().ok()?;
        if age > CACHE_TTL {
            return None;
        }
        let data = std::fs::read(&path).ok()?;
        return Some((data, content_type_for_ext(ext)));
    }
    None
}

fn cache_set(
    dir: &std::path::Path,
    category: &str,
    id: i64,
    variation: &str,
    size: Option<u32>,
    data: &[u8],
    content_type: &str,
) {
    let stem = cache_stem(category, id, variation, size);
    let ext = ext_for_content_type(content_type);
    let path = dir.join(format!("{}.{}", stem, ext));
    if let Err(e) = std::fs::create_dir_all(dir) {
        warn!(error = %e, "failed to create image cache dir");
        return;
    }
    let _ = std::fs::write(path, data);
}

pub fn cache_clear(dir: &std::path::Path) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let expired = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .map(|age| age > CACHE_TTL)
            .unwrap_or(false);
        if expired {
            if let Err(e) = std::fs::remove_file(&path) {
                warn!(error = %e, path = %path.display(), "failed to delete expired cache file");
            }
        }
    }
}

fn ext_for_content_type(content_type: &str) -> &'static str {
    if content_type.contains("jpeg") {
        "jpg"
    } else {
        "png"
    }
}

fn content_type_for_ext(ext: &str) -> &'static str {
    if ext == "jpg" {
        "image/jpeg"
    } else {
        "image/png"
    }
}

fn default_content_type_for(category: &str) -> &'static str {
    if category == "characters" {
        "image/jpeg"
    } else {
        "image/png"
    }
}
