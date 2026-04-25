use std::{path::PathBuf, time::Duration};
use tokio::time;

const IMAGE_CACHE_PURGE_INTERVAL: Duration = Duration::from_hours(2);

pub fn spawn_image_cache_cleanup(dir: PathBuf) {
    tokio::spawn(async move {
        let mut interval = time::interval(IMAGE_CACHE_PURGE_INTERVAL);
        interval.tick().await;

        loop {
            interval.tick().await;
            crate::services::images::cache_clear(&dir);
        }
    });
}
