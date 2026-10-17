//! gbe_fork / Goldberg achievements (7b), for non-Steam games that carry the
//! emulator. The list comes from the game's own `steam_settings/`
//! (`achievements.json`: `name`, `displayName`, `description`, `hidden`,
//! `icon`, `icon_gray`, texts per language or plain, icons as files beside
//! it); the unlocks from the emulator's save, `<saves>/<appid>/achievements.json`
//! (`{"<name>": {"earned": true, "earned_time": <unix>}}`), where `<saves>` is,
//! first found: the local save path in `configs.user.ini` (`[user::saves]
//! local_save_path`, relative to `steam_settings/..`), then in the game's
//! prefix `AppData/Roaming/GSE Saves` (gbe_fork) or `Goldberg SteamEmu Saves`,
//! then the same under `~/.local/share` for Linux builds.

use super::Achievement;
use anyhow::{Context as _, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// How deep a game's folder is searched for `steam_settings` (Unity games keep it in `<Game>_Data/Plugins/x86_64`)
const DEPTH: usize = 4;

#[derive(Debug, Clone, PartialEq)]
pub struct Emu {
    /// The `steam_settings` folder
    pub settings: PathBuf,
    pub appid: String,
}

/// The emulator's settings in a game's folder: the `steam_settings` with an
/// `achievements.json` first, else any with an app id
pub fn find(install_dir: &Path, exe: Option<&Path>) -> Option<Emu> {
    let mut found = Vec::new();
    walk(install_dir, DEPTH, &mut found);
    found.sort_by_key(|d| (!d.join("achievements.json").is_file(), d.components().count()));
    found.into_iter().find_map(|settings| {
        let appid = read_appid(&settings.join("steam_appid.txt"))
            .or_else(|| settings.parent().and_then(|p| read_appid(&p.join("steam_appid.txt"))))
            .or_else(|| exe.and_then(|e| e.parent()).and_then(|p| read_appid(&p.join("steam_appid.txt"))))?;
        Some(Emu { settings, appid })
    })
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let Ok(kind) = e.file_type() else { continue };
        if !kind.is_dir() {
            continue;
        }
        let p = e.path();
        if e.file_name().eq_ignore_ascii_case("steam_settings") {
            out.push(p);
        } else if depth > 0 {
            walk(&p, depth - 1, out);
        }
    }
}

fn read_appid(p: &Path) -> Option<String> {
    let s = std::fs::read_to_string(p).ok()?;
    let id = s.trim().to_string();
    (!id.is_empty() && id.chars().all(|c| c.is_ascii_digit())).then_some(id)
}

/// `key=value` from an ini section (gbe_fork's `configs.*.ini`)
fn ini_value(path: &Path, section: &str, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut inside = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            inside = line.trim_matches(['[', ']']).eq_ignore_ascii_case(section);
        } else if inside && let Some((k, v)) = line.split_once('=') && k.trim().eq_ignore_ascii_case(key) {
            let v = v.trim().to_string();
            return (!v.is_empty()).then_some(v);
        }
    }
    None
}

/// The language the emulator plays in (`configs.user.ini`), else English
pub fn language(emu: &Emu) -> String {
    ini_value(&emu.settings.join("configs.user.ini"), "user::general", "language").unwrap_or_else(|| "english".into())
}

/// Where the save may be, best first (only existing folders count later)
pub fn save_dirs(emu: &Emu, prefix: Option<&Path>, home: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(local) = ini_value(&emu.settings.join("configs.user.ini"), "user::saves", "local_save_path") {
        let base = emu.settings.parent().unwrap_or(&emu.settings);
        out.push(base.join(local.replace('\\', "/")));
    }
    let roots: Vec<PathBuf> = prefix.map(|p| p.join("drive_c/users/steamuser/AppData/Roaming")).into_iter().chain([home.join(".local/share")]).collect();
    for r in roots {
        out.push(r.join("GSE Saves"));
        out.push(r.join("Goldberg SteamEmu Saves"));
    }
    out.into_iter().map(|d| d.join(&emu.appid)).collect()
}

/// The save's `achievements.json`, the first that exists
pub fn save_file(emu: &Emu, prefix: Option<&Path>, home: &Path) -> Option<PathBuf> {
    save_dirs(emu, prefix, home).into_iter().map(|d| d.join("achievements.json")).find(|f| f.is_file())
}

fn text(v: &Value, lang: &str) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Object(m) => m
            .get(lang)
            .or_else(|| m.get("english"))
            .or_else(|| m.iter().find(|(k, _)| *k != "token").map(|(_, v)| v))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    }
}

/// The game's list, with the save's unlocks; None without an `achievements.json` in its settings
pub fn read(emu: &Emu, save: Option<&Path>) -> Result<Option<Vec<Achievement>>> {
    let schema = emu.settings.join("achievements.json");
    let Ok(bytes) = std::fs::read(&schema) else { return Ok(None) };
    let list: Vec<Value> = serde_json::from_slice(&bytes).with_context(|| format!("reading {}", schema.display()))?;
    let earned: serde_json::Map<String, Value> = match save {
        Some(f) => std::fs::read(f).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default(),
        None => Default::default(),
    };
    let lang = language(emu);
    let icon = |v: &Value| {
        v.as_str().filter(|s| !s.is_empty()).map(|rel| {
            let p = emu.settings.join(rel.replace('\\', "/"));
            if p.is_file() { p.to_string_lossy().into_owned() } else { rel.to_string() }
        })
    };
    let flag = |v: &Value| v.as_i64() == Some(1) || v.as_str() == Some("1") || v.as_bool() == Some(true);
    Ok(Some(
        list.iter()
            .filter_map(|a| {
                let name = a["name"].as_str()?.to_string();
                let e = earned.get(&name).unwrap_or(&Value::Null);
                let unlocked = flag(&e["earned"]);
                Some(Achievement {
                    name: Some(text(&a["displayName"], &lang)).filter(|s| !s.is_empty()).unwrap_or_else(|| name.clone()),
                    description: text(&a["description"], &lang),
                    hidden: flag(&a["hidden"]),
                    icon: icon(&a["icon"]),
                    icon_gray: icon(&a["icon_gray"]).or_else(|| icon(&a["icongray"])),
                    unlocked_at: unlocked.then(|| e["earned_time"].as_i64().unwrap_or(0)),
                    api_name: name,
                })
            })
            .collect(),
    ))
}

/// Another list with the save's unlocks (a game whose settings have no list of their own)
pub fn overlay(mut list: Vec<Achievement>, save: Option<&Path>) -> Vec<Achievement> {
    let earned: serde_json::Map<String, Value> = save.and_then(|f| std::fs::read(f).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    for a in &mut list {
        let e = earned.get(&a.api_name).unwrap_or(&Value::Null);
        a.unlocked_at = (e["earned"] == true).then(|| e["earned_time"].as_i64().unwrap_or(0));
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_unity_games_settings_and_its_save_in_the_prefix() {
        let d = tempfile::tempdir().unwrap();
        let game = d.path().join("Game");
        let settings = game.join("Game_Data/Plugins/x86_64/steam_settings");
        std::fs::create_dir_all(settings.join("achievement_images")).unwrap();
        std::fs::write(settings.join("steam_appid.txt"), "774171\n").unwrap();
        std::fs::write(settings.join("achievement_images/a.jpg"), b"").unwrap();
        std::fs::write(settings.join("configs.user.ini"), "[user::general]\nlanguage=japanese\n").unwrap();
        std::fs::write(
            settings.join("achievements.json"),
            r#"[{"name": "welcome", "displayName": {"english": "Welcome", "japanese": "ようこそ"}, "description": "Do the tutorial", "hidden": 0, "icon": "achievement_images/a.jpg", "icon_gray": "achievement_images/b.jpg"},
                {"name": "secret", "displayName": "Secret", "hidden": "1"}]"#,
        )
        .unwrap();
        let emu = find(&game, None).unwrap();
        assert_eq!(emu.appid, "774171");

        let prefix = d.path().join("pfx");
        assert_eq!(save_file(&emu, Some(&prefix), d.path()), None);
        assert!(read(&emu, None).unwrap().unwrap().iter().all(|a| a.unlocked_at.is_none()), "no save yet: all locked");
        assert!(overlay(read(&emu, None).unwrap().unwrap(), None).iter().all(|a| a.unlocked_at.is_none()));
        let saves = prefix.join("drive_c/users/steamuser/AppData/Roaming/GSE Saves/774171");
        std::fs::create_dir_all(&saves).unwrap();
        std::fs::write(saves.join("achievements.json"), r#"{"welcome": {"earned": true, "earned_time": 1700000000}, "secret": {"earned": false, "earned_time": 0}}"#).unwrap();
        let save = save_file(&emu, Some(&prefix), d.path()).unwrap();
        let a = read(&emu, Some(&save)).unwrap().unwrap();
        assert_eq!((a[0].name.as_str(), a[0].unlocked_at), ("ようこそ", Some(1700000000)));
        assert!(a[0].icon.as_deref().unwrap().ends_with("steam_settings/achievement_images/a.jpg"));
        assert_eq!(a[0].icon_gray.as_deref(), Some("achievement_images/b.jpg"), "a missing file stays as it was named");
        assert_eq!((a[1].hidden, a[1].unlocked_at), (true, None));

        // A local save path wins
        std::fs::write(settings.join("configs.user.ini"), "[user::saves]\nlocal_save_path=./saves\n").unwrap();
        assert_eq!(save_dirs(&emu, Some(&prefix), d.path())[0], settings.parent().unwrap().join("./saves/774171"));
    }
}
