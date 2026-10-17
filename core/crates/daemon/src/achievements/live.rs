//! Unlocks while a game runs (7d), for gbe_fork/Goldberg games: the save's
//! folder is watched with inotify (no polling) and each change reads the list
//! again, which emits `achievement.unlocked` for what is new. The folder may
//! not exist yet (the emulator makes `<saves>/<appid>/` on its first unlock or
//! launch): then the nearest folder above it that exists is watched until it
//! does. Steam games are left to Steam's own popup.

use super::gbe;
use crate::rpc::State;
use futures_util::StreamExt as _;
use inotify::{Inotify, WatchMask};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// Writes come in bursts (a file is truncated, then written): wait for quiet
const SETTLE: Duration = Duration::from_millis(300);

/// What to watch now: the first save folder that exists, else the nearest
/// existing folder above the first candidate (to see it appear); and whether
/// that is the save folder itself
pub fn target(candidates: &[PathBuf]) -> Option<(PathBuf, bool)> {
    if let Some(d) = candidates.iter().find(|d| d.is_dir()) {
        return Some((d.clone(), true));
    }
    let first = candidates.first()?;
    first.ancestors().skip(1).find(|a| a.is_dir()).map(|a| (a.to_path_buf(), false))
}

impl State {
    /// Starts watching a running game's save; the task ends when aborted (the game ended)
    pub(crate) fn watch_achievements(self: &Arc<Self>, id: i64) -> Option<tokio::task::AbortHandle> {
        let game = crate::games::get(&self.db.lock().unwrap(), id).ok().flatten()?;
        let emu = self.gbe_of(&game)?;
        let dirs = gbe::save_dirs(&emu, game.prefix.as_deref(), &self.home);
        let state = Arc::downgrade(self);
        let task = tokio::spawn(async move {
            // The list as it is now, so the first unlock of this session is news
            if let Some(s) = state.upgrade()
                && let Err(e) = s.refresh_achievements(id, false).await
            {
                tracing::warn!(id, "achievements: {e:#}");
            }
            loop {
                let Some((dir, is_save)) = target(&dirs) else { return };
                if let Err(e) = watch_once(&dir, is_save).await {
                    tracing::debug!(id, "watching {}: {e:#}", dir.display());
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }
                // Something changed: the save folder appeared, or its file was written
                tokio::time::sleep(SETTLE).await;
                let Some(s) = state.upgrade() else { return };
                if is_save && let Err(e) = s.refresh_achievements(id, false).await {
                    tracing::warn!(id, "achievements: {e:#}");
                }
            }
        });
        tracing::debug!(id, "watching achievements");
        Some(task.abort_handle())
    }
}

/// Waits for one change worth a look: a write in the save folder, or any new
/// folder above it (it may be the save folder appearing)
async fn watch_once(dir: &Path, is_save: bool) -> std::io::Result<()> {
    let inotify = Inotify::init()?;
    let mask = if is_save { WatchMask::CLOSE_WRITE | WatchMask::MOVED_TO | WatchMask::CREATE } else { WatchMask::CREATE | WatchMask::MOVED_TO };
    inotify.watches().add(dir, mask)?;
    let mut events = inotify.into_event_stream([0u8; 4096])?;
    while let Some(ev) = events.next().await {
        let ev = ev?;
        let name = ev.name.as_ref().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        // In the save folder only the achievements file counts; above it any new folder
        if !is_save || name == "achievements.json" {
            return Ok(());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watches_the_save_or_the_folder_it_will_appear_in() {
        let d = tempfile::tempdir().unwrap();
        let save = d.path().join("pfx/drive_c/users/steamuser/AppData/Roaming/GSE Saves/774171");
        let other = d.path().join("home/GSE Saves/774171");
        std::fs::create_dir_all(d.path().join("pfx/drive_c/users")).unwrap();
        assert_eq!(target(&[save.clone(), other.clone()]), Some((d.path().join("pfx/drive_c/users"), false)));
        std::fs::create_dir_all(&other).unwrap();
        assert_eq!(target(&[save.clone(), other.clone()]), Some((other.clone(), true)));
        std::fs::create_dir_all(&save).unwrap();
        assert_eq!(target(&[save.clone(), other]), Some((save, true)));
    }

    #[tokio::test]
    async fn a_write_of_the_achievements_file_wakes_it() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().to_path_buf();
        let w = {
            let dir = dir.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(50)).await;
                std::fs::write(dir.join("stats.json"), b"{}").unwrap();
                tokio::time::sleep(Duration::from_millis(50)).await;
                std::fs::write(dir.join("achievements.json"), b"{}").unwrap();
            })
        };
        tokio::time::timeout(Duration::from_secs(5), watch_once(&dir, true)).await.unwrap().unwrap();
        assert!(dir.join("achievements.json").exists(), "woke for achievements.json, not stats.json");
        w.await.unwrap();
    }
}
