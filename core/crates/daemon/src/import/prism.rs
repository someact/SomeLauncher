//! Prism Launcher: every instance (`instances/<id>/instance.cfg` +
//! `mmc-pack.json`) with its Minecraft version, mod loader, group
//! (`instgroups.json`), icon and Prism's playtime. Instance settings stay in
//! Prism's files; Phase 8 edits them there.

use super::Found;
use anyhow::Result;
use serde_json::{Value, json};
use somelauncher_proto::game::{Kind, NewGame, Source, SourceKind};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// `[section]` + `key=value` lines as Qt's QSettings writes them; sections are ignored.
fn ini(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| {
            let v = v.trim();
            let v = match v.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
                Some(quoted) => quoted.replace("\\\"", "\"").replace("\\\\", "\\"),
                None => v.to_string(),
            };
            (k.trim().to_string(), v)
        })
        .collect()
}

/// Prism data dirs (native, flatpak) with their instances dir.
fn installs(home: &Path) -> Vec<(PathBuf, PathBuf)> {
    [".local/share/PrismLauncher", ".var/app/org.prismlauncher.PrismLauncher/data/PrismLauncher"]
        .iter()
        .map(|d| home.join(d))
        .filter(|root| root.join("prismlauncher.cfg").is_file() || root.join("instances").is_dir())
        .map(|root| {
            let cfg = std::fs::read_to_string(root.join("prismlauncher.cfg")).map(|t| ini(&t)).unwrap_or_default();
            let dir = PathBuf::from(cfg.get("InstanceDir").map(String::as_str).filter(|d| !d.is_empty()).unwrap_or("instances"));
            let instances = if dir.is_absolute() { dir } else { root.join(dir) };
            (root, instances)
        })
        .collect()
}

/// Instance id → group name.
fn groups(instances: &Path) -> HashMap<String, String> {
    let Ok(text) = std::fs::read_to_string(instances.join("instgroups.json")) else { return HashMap::new() };
    let Ok(v) = serde_json::from_str::<Value>(&text) else { return HashMap::new() };
    let mut out = HashMap::new();
    for (group, g) in v["groups"].as_object().into_iter().flatten() {
        for id in g["instances"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            out.insert(id.to_string(), group.clone());
        }
    }
    out
}

/// The Minecraft version and the mod loader (name, version) from `mmc-pack.json`.
fn components(dir: &Path) -> (Option<String>, Option<(&'static str, String)>) {
    let Ok(text) = std::fs::read_to_string(dir.join("mmc-pack.json")) else { return (None, None) };
    let Ok(v) = serde_json::from_str::<Value>(&text) else { return (None, None) };
    let mut minecraft = None;
    let mut loader = None;
    for c in v["components"].as_array().into_iter().flatten() {
        let version = c["version"].as_str().unwrap_or_default().to_string();
        let name = match c["uid"].as_str().unwrap_or_default() {
            "net.minecraft" => {
                minecraft = Some(version);
                continue;
            }
            "net.fabricmc.fabric-loader" => "Fabric",
            "org.quiltmc.quilt-loader" => "Quilt",
            "net.neoforged" => "NeoForge",
            "net.minecraftforge" => "Forge",
            "com.mumfier.liteloader" => "LiteLoader",
            _ => continue,
        };
        loader = Some((name, version));
    }
    (minecraft, loader)
}

/// A custom icon in Prism's icons dir; built-in keys have no file.
fn icon(root: &Path, key: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(root.join("icons")).ok()?;
    entries.flatten().map(|e| e.path()).find(|p| p.file_stem().and_then(|s| s.to_str()) == Some(key))
}

pub fn scan(home: &Path) -> Result<Vec<Found>> {
    let mut out: Vec<Found> = Vec::new();
    for (root, instances) in installs(home) {
        let groups = groups(&instances);
        let flatpak = root.to_string_lossy().contains("org.prismlauncher.PrismLauncher");
        let Ok(entries) = std::fs::read_dir(&instances) else { continue };
        let mut dirs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.join("instance.cfg").is_file()).collect();
        dirs.sort();
        for dir in dirs {
            let id = dir.file_name().unwrap_or_default().to_string_lossy().into_owned();
            if out.iter().any(|f| f.source.source_id == id) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(dir.join("instance.cfg")) else { continue };
            let cfg = ini(&text);
            let get = |k: &str| cfg.get(k).map(String::as_str).unwrap_or_default();
            let num = |k: &str| get(k).parse::<i64>().unwrap_or(0);

            let name = Some(get("name")).filter(|n| !n.is_empty()).unwrap_or(&id);
            let mut game = NewGame::new(name, Kind::Minecraft);
            game.install_dir = Some(dir.clone());
            game.playtime = num("totalTimePlayed");
            game.last_played = Some(num("lastLaunchTime") / 1000).filter(|t| *t > 0);

            let (minecraft, loader) = components(&dir);
            let key = get("iconKey");
            let data = json!({
                "root": root,
                "flatpak": flatpak,
                "group": groups.get(&id),
                "minecraft": minecraft,
                "loader": loader.as_ref().map(|l| l.0),
                "loaderVersion": loader.map(|l| l.1),
                "iconKey": key,
                "icon": icon(&root, key),
                "pack": Some(get("ManagedPackName")).filter(|p| !p.is_empty()),
            });
            out.push(Found { source: Source { source: SourceKind::Prism, source_id: id, data }, game, follow_paths: true });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_instances_groups_loaders_and_playtime() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join(".local/share/PrismLauncher");
        let inst = root.join("instances");
        for (id, cfg, pack) in [
            ("Vibe", "[General]\nname=\"Vibe, modded\"\niconKey=axiom\ntotalTimePlayed=113025\nlastLaunchTime=1788192932419\n", r#"{"components":[{"uid":"net.minecraft","version":"26.2"},{"uid":"net.fabricmc.fabric-loader","version":"0.19.5"}]}"#),
            ("1.21.1", "[General]\nname=1.21.1\niconKey=grass\n", r#"{"components":[{"uid":"net.minecraft","version":"1.21.1"}]}"#),
        ] {
            std::fs::create_dir_all(inst.join(id)).unwrap();
            std::fs::write(inst.join(id).join("instance.cfg"), cfg).unwrap();
            std::fs::write(inst.join(id).join("mmc-pack.json"), pack).unwrap();
        }
        std::fs::create_dir_all(inst.join("broken")).unwrap();
        std::fs::create_dir_all(root.join("icons")).unwrap();
        std::fs::write(root.join("icons/axiom.png"), "").unwrap();
        std::fs::write(root.join("prismlauncher.cfg"), "[General]\nInstanceDir=instances\n").unwrap();
        std::fs::write(inst.join("instgroups.json"), r#"{"formatVersion":"1","groups":{"Modded":{"hidden":false,"instances":["Vibe"]}}}"#).unwrap();

        let found = scan(home.path()).unwrap();
        assert_eq!(found.iter().map(|f| f.source.source_id.as_str()).collect::<Vec<_>>(), ["1.21.1", "Vibe"]);
        let vibe = &found[1];
        assert_eq!((vibe.game.name.as_str(), vibe.game.kind), ("Vibe, modded", Kind::Minecraft));
        assert_eq!((vibe.game.playtime, vibe.game.last_played), (113025, Some(1788192932)));
        let d = &vibe.source.data;
        assert_eq!((&d["group"], &d["minecraft"], &d["loader"], &d["loaderVersion"]), (&json!("Modded"), &json!("26.2"), &json!("Fabric"), &json!("0.19.5")));
        assert!(d["icon"].as_str().unwrap().ends_with("icons/axiom.png"));
        let plain = &found[0].source.data;
        assert_eq!((&plain["loader"], &plain["group"], &plain["icon"]), (&Value::Null, &Value::Null, &Value::Null));
    }
}
