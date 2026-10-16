//! "Needs Steam": Windows games whose co-op goes through the real Steam client
//! (OnlineFix, gbe_fork experimental), the steps of LUTRIS_SETUP.md done for
//! the user. Steam must run and be logged in; umu-run must keep Proton's
//! `lsteamclient` (`UMU_USE_STEAM=1`); the prefix needs real copies (symlinks
//! fail inside pressure-vessel) of Steam's Windows DLLs; the game folder needs
//! `steam_appid.txt`; the loader DLLs need `n,b` overrides.

use crate::import::steam;
use anyhow::{Context, Result, bail};
use somelauncher_proto::game::Game;
use std::path::{Path, PathBuf};

/// DLL overrides set to `n,b` unless the game sets them itself.
pub const OVERRIDES: &[&str] = &["winmm", "OnlineFix64", "SteamOverlay64", "StubDRM64", "steam_api64"];

/// Files copied into `drive_c/Program Files (x86)/Steam`: name there, then where to take it from in the Steam install.
const FILES: &[(&str, &[&str])] = &[
    ("steamclient.dll", &["steamclient.dll", "legacycompat/steamclient.dll"]),
    ("steamclient64.dll", &["steamclient64.dll", "legacycompat/steamclient64.dll"]),
    ("GameOverlayRenderer64.dll", &["GameOverlayRenderer64.dll", "legacycompat/GameOverlayRenderer64.dll"]),
    ("Steam.dll", &["legacycompat/Steam.dll"]),
    ("steam.exe", &["legacycompat/SteamService.exe"]),
];

/// The app id for `steam_appid.txt`: the game's setting, else OnlineFix's fake id, else Spacewar (480).
pub fn app_id(game: &Game) -> String {
    if let Some(id) = game.runner.steam_app_id.as_deref().filter(|s| !s.is_empty()) {
        return id.into();
    }
    let ini = game.exe.as_ref().and_then(|e| e.parent()).and_then(|d| std::fs::read_to_string(d.join("OnlineFix.ini")).ok());
    ini.and_then(|t| t.lines().find_map(|l| l.trim().strip_prefix("FakeAppId=").map(|v| v.trim().to_string())))
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "480".into())
}

/// Copies Steam's Windows files into the prefix. A symlink or a file of another
/// size is replaced; an equal file is left alone. Returns what was copied.
pub fn prepare_prefix(steam_root: &Path, prefix: &Path) -> Result<Vec<&'static str>> {
    let dest = prefix.join("drive_c/Program Files (x86)/Steam");
    std::fs::create_dir_all(&dest).with_context(|| format!("creating {}", dest.display()))?;
    let mut copied = Vec::new();
    for (name, sources) in FILES {
        let Some(src) = sources.iter().map(|s| steam_root.join(s)).find(|p| p.is_file()) else {
            bail!("{name} is missing from the Steam install ({})", steam_root.display());
        };
        let target = dest.join(name);
        let same = std::fs::symlink_metadata(&target)
            .is_ok_and(|m| m.file_type().is_file() && std::fs::metadata(&src).is_ok_and(|s| s.len() == m.len()));
        if same {
            continue;
        }
        let _ = std::fs::remove_file(&target);
        std::fs::copy(&src, &target).with_context(|| format!("copying {}", src.display()))?;
        copied.push(*name);
    }
    Ok(copied)
}

/// Writes `steam_appid.txt` next to the exe unless one is there.
pub fn write_app_id(game: &Game) -> Result<()> {
    let Some(dir) = game.exe.as_ref().and_then(|e| e.parent()) else { return Ok(()) };
    let file = dir.join("steam_appid.txt");
    if !file.exists() {
        std::fs::write(&file, app_id(game)).with_context(|| format!("writing {}", file.display()))?;
    }
    Ok(())
}

/// The Steam install whose DLLs go into prefixes.
pub fn steam_root(home: &Path) -> Result<PathBuf> {
    steam::roots(home).into_iter().next().context("no Steam install found")
}

#[cfg(test)]
mod tests {
    use super::*;
    use somelauncher_proto::game::{Kind, NewGame};

    fn game(exe: &Path) -> Game {
        let n = NewGame::new("How to Fish", Kind::Proton);
        Game {
            id: 1,
            name: n.name,
            kind: n.kind,
            install_dir: None,
            exe: Some(exe.into()),
            prefix: None,
            runner: n.runner,
            tags: vec![],
            categories: vec![],
            favorite: false,
            hidden: false,
            playtime: 0,
            last_played: None,
            added: 0,
            sources: vec![],
        }
    }

    #[test]
    fn copies_real_files_over_symlinks_and_keeps_equal_ones() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("Steam");
        std::fs::create_dir_all(root.join("legacycompat")).unwrap();
        for f in ["steamclient.dll", "steamclient64.dll", "GameOverlayRenderer64.dll"] {
            std::fs::write(root.join(f), f).unwrap();
        }
        std::fs::write(root.join("legacycompat/Steam.dll"), "steam").unwrap();
        std::fs::write(root.join("legacycompat/SteamService.exe"), "service").unwrap();
        let prefix = t.path().join("pfx");
        let dest = prefix.join("drive_c/Program Files (x86)/Steam");
        std::fs::create_dir_all(&dest).unwrap();
        std::os::unix::fs::symlink(root.join("steamclient64.dll"), dest.join("steamclient64.dll")).unwrap();

        assert_eq!(prepare_prefix(&root, &prefix).unwrap().len(), 5);
        assert!(!std::fs::symlink_metadata(dest.join("steamclient64.dll")).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(dest.join("steam.exe")).unwrap(), "service");
        assert!(prepare_prefix(&root, &prefix).unwrap().is_empty(), "nothing to do the second time");

        std::fs::remove_file(root.join("legacycompat/Steam.dll")).unwrap();
        std::fs::remove_file(dest.join("Steam.dll")).unwrap();
        assert!(prepare_prefix(&root, &prefix).is_err());
    }

    #[test]
    fn app_id_comes_from_the_game_then_onlinefix_then_spacewar() {
        let t = tempfile::tempdir().unwrap();
        let exe = t.path().join("game.exe");
        let mut g = game(&exe);
        assert_eq!(app_id(&g), "480");
        std::fs::write(t.path().join("OnlineFix.ini"), "RealAppId=4001890\nFakeAppId=1234\n").unwrap();
        assert_eq!(app_id(&g), "1234");
        g.runner.steam_app_id = Some("999".into());
        assert_eq!(app_id(&g), "999");
        write_app_id(&g).unwrap();
        assert_eq!(std::fs::read_to_string(t.path().join("steam_appid.txt")).unwrap(), "999");
        g.runner.steam_app_id = Some("1".into());
        write_app_id(&g).unwrap();
        assert_eq!(std::fs::read_to_string(t.path().join("steam_appid.txt")).unwrap(), "999", "an existing file is kept");
    }
}
