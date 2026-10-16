//! Waiting on files with inotify instead of polling.

use futures_util::StreamExt;
use inotify::{Inotify, WatchMask};
use std::path::Path;
use std::time::Duration;

/// Returns true as soon as `check` holds: right away, or after a change to
/// `path` (a file, or a directory to see files in it replaced) makes it hold.
/// False when `timeout` passes first (None waits for ever).
pub async fn until(path: &Path, mask: WatchMask, timeout: Option<Duration>, mut check: impl FnMut() -> bool) -> std::io::Result<bool> {
    let inotify = Inotify::init()?;
    inotify.watches().add(path, mask)?;
    // Checked after the watch exists, so no change can slip in between
    if check() {
        return Ok(true);
    }
    let mut events = inotify.into_event_stream([0u8; 4096])?;
    let wait = async {
        while let Some(ev) = events.next().await {
            ev?;
            if check() {
                return Ok(true);
            }
        }
        Ok(false)
    };
    match timeout {
        Some(t) => tokio::time::timeout(t, wait).await.unwrap_or(Ok(false)),
        None => wait.await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn wakes_on_a_change_and_times_out_without_one() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("flag");
        let mask = WatchMask::CREATE | WatchMask::MODIFY | WatchMask::MOVED_TO;
        let writer = {
            let file = file.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(50)).await;
                std::fs::write(file, "1").unwrap();
            })
        };
        let seen = until(dir.path(), mask, Some(Duration::from_secs(5)), || std::fs::read_to_string(&file).is_ok_and(|s| s == "1")).await;
        assert!(seen.unwrap());
        writer.await.unwrap();
        assert!(!until(dir.path(), mask, Some(Duration::from_millis(50)), || false).await.unwrap());
    }
}
