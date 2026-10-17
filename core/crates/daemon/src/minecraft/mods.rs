//! An instance's mods (`mods/`): each jar's name, version, description,
//! authors and icon, and turning one off (`<file>.disabled`, as Prism does).
//! Names and Modrinth/CurseForge ids come from Prism's packwiz index
//! (`mods/.index/*.pw.toml`) when it has the mod; the rest from the jar's own
//! `fabric.mod.json`, `quilt.mod.json` or `META-INF/(neoforge.)mods.toml`.
//! Reading 365 jars takes a while, so what a jar says is cached by its name,
//! size and time (`$XDG_CACHE_HOME/somelauncher/mods/`), icons as small PNGs.

use anyhow::{Context as _, Result, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

const DISABLED: &str = ".disabled";
/// Icons are kept this size at most
const ICON: u32 = 96;

pub fn cache_dir() -> PathBuf {
    somelauncher_proto::paths::cache_dir().join("mods")
}

/// What Prism's index says about a mod: (name, Modrinth project, CurseForge project, side)
#[derive(Default, Clone)]
struct Indexed {
    name: Option<String>,
    modrinth: Option<String>,
    curseforge: Option<i64>,
    side: Option<String>,
}

/// `.index/*.pw.toml` by the jar's file name.
fn index(mods: &Path) -> HashMap<String, Indexed> {
    let mut out = HashMap::new();
    for e in std::fs::read_dir(mods.join(".index")).into_iter().flatten().flatten() {
        let Ok(text) = std::fs::read_to_string(e.path()) else { continue };
        let Ok(t) = text.parse::<toml::Table>() else { continue };
        let Some(file) = t.get("filename").and_then(|v| v.as_str()) else { continue };
        let update = t.get("update").and_then(|u| u.as_table());
        out.insert(
            file.to_string(),
            Indexed {
                name: t.get("name").and_then(|v| v.as_str()).map(String::from),
                modrinth: update.and_then(|u| u.get("modrinth")?.get("mod-id")?.as_str()).map(String::from),
                curseforge: update.and_then(|u| u.get("curseforge")?.get("project-id")?.as_integer()),
                side: t.get("side").and_then(|v| v.as_str()).map(String::from),
            },
        );
    }
    out
}

/// What a jar says about itself: `{name, version, description, authors, icon}` (`icon` the entry's path in the jar).
fn jar_meta(path: &Path) -> Result<Value> {
    let file = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file)).context("not a jar")?;
    let mut read = |name: &str| -> Option<String> {
        let mut e = zip.by_name(name).ok()?;
        let mut s = String::new();
        e.read_to_string(&mut s).ok()?;
        Some(s)
    };
    if let Some(v) = read("fabric.mod.json").and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
        let icon = match &v["icon"] {
            Value::String(s) => Some(s.clone()),
            // Sizes to paths: the biggest
            Value::Object(m) => m.iter().max_by_key(|(k, _)| k.parse::<u32>().unwrap_or(0)).and_then(|(_, p)| p.as_str()).map(String::from),
            _ => None,
        };
        let authors: Vec<String> =
            v["authors"].as_array().into_iter().flatten().filter_map(|a| a.as_str().or_else(|| a["name"].as_str()).map(String::from)).collect();
        return Ok(json!({ "name": v["name"].as_str().or(v["id"].as_str()), "version": v["version"], "description": v["description"], "authors": authors, "icon": icon }));
    }
    if let Some(v) = read("quilt.mod.json").and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
        let q = &v["quilt_loader"];
        let m = &q["metadata"];
        let authors: Vec<String> = m["contributors"].as_object().into_iter().flatten().map(|(k, _)| k.clone()).collect();
        let icon = m["icon"].as_str().map(String::from);
        return Ok(json!({ "name": m["name"].as_str().or(q["id"].as_str()), "version": q["version"], "description": m["description"], "authors": authors, "icon": icon }));
    }
    for name in ["META-INF/neoforge.mods.toml", "META-INF/mods.toml"] {
        let Some(t) = read(name).and_then(|t| t.parse::<toml::Table>().ok()) else { continue };
        let Some(m) = t.get("mods").and_then(|m| m.as_array()).and_then(|a| a.first()) else { continue };
        let get = |k: &str| m.get(k).and_then(|v| v.as_str()).map(|s| s.trim().to_string());
        // `${file.jarVersion}`: the manifest's version
        let version = get("version").filter(|v| !v.starts_with("${")).or_else(|| {
            read("META-INF/MANIFEST.MF")?.lines().find_map(|l| l.strip_prefix("Implementation-Version:").map(|v| v.trim().to_string()))
        });
        let authors: Vec<String> = get("authors").map(|a| a.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_default();
        let logo = get("logoFile").or_else(|| t.get("logoFile").and_then(|v| v.as_str()).map(String::from));
        return Ok(json!({ "name": get("displayName").or(get("modId")), "version": version, "description": get("description"), "authors": authors, "icon": logo }));
    }
    Ok(json!({}))
}

/// The jar's icon, made small and written to `to`.
fn save_icon(jar: &Path, entry: &str, to: &Path) -> Result<()> {
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(std::fs::File::open(jar)?))?;
    let mut bytes = Vec::new();
    zip.by_name(entry.trim_start_matches('/'))?.read_to_end(&mut bytes)?;
    let img = image::load_from_memory(&bytes)?;
    // Forge's logos can be wide banners: made small, keeping their shape
    let img = if img.width() > ICON || img.height() > ICON { img.resize(ICON, ICON, image::imageops::FilterType::Triangle) } else { img };
    img.save_with_format(to, image::ImageFormat::Png)?;
    Ok(())
}

/// A jar's metadata (with `iconFile`, the cached icon), read once per version of the file.
fn cached_meta(jar: &Path, base: &str, cache: &Path) -> Value {
    let stat = std::fs::metadata(jar).ok();
    let size = stat.as_ref().map(|m| m.len()).unwrap_or(0);
    let time = stat.and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
    let key = hex::encode(&Sha256::digest(format!("{base}\0{size}\0{time}"))[..12]);
    let meta_path = cache.join(format!("{key}.json"));
    if let Some(v) = std::fs::read(&meta_path).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) {
        return v;
    }
    let mut meta = jar_meta(jar).unwrap_or_else(|e| {
        tracing::debug!("{}: {e:#}", jar.display());
        json!({})
    });
    let icon_path = cache.join(format!("{key}.png"));
    let icon = meta["icon"].as_str().filter(|i| !i.is_empty()).and_then(|entry| match save_icon(jar, entry, &icon_path) {
        Ok(()) => Some(icon_path.clone()),
        Err(e) => {
            tracing::debug!("{} icon {entry}: {e:#}", jar.display());
            None
        }
    });
    meta["iconFile"] = json!(icon);
    if std::fs::create_dir_all(cache).is_ok() {
        let _ = std::fs::write(&meta_path, serde_json::to_vec(&meta).unwrap_or_default());
    }
    meta
}

/// Every mod in `mods/`, by name: `{file, enabled, name, version, description, authors, icon, url, side}`.
/// Jars not seen before are read on all cores.
pub fn list(mods: &Path, cache: &Path) -> Vec<Value> {
    let index = index(mods);
    // (file, base name without `.disabled`, path)
    let files: Vec<(String, String, PathBuf)> = std::fs::read_dir(mods)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let file = e.file_name().to_string_lossy().into_owned();
            let base = file.strip_suffix(DISABLED).unwrap_or(&file).to_string();
            ((base.ends_with(".jar") || base.ends_with(".zip")) && e.path().is_file()).then(|| (file, base, e.path()))
        })
        .collect();
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(16);
    let chunk = files.len().div_ceil(threads).max(1);
    let metas: Vec<Value> = std::thread::scope(|s| {
        let handles: Vec<_> = files.chunks(chunk).map(|part| s.spawn(move || part.iter().map(|(_, base, path)| cached_meta(path, base, cache)).collect::<Vec<_>>())).collect();
        handles.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
    });
    let mut out: Vec<Value> = files
        .iter()
        .zip(metas)
        .map(|((file, base, _), meta)| {
            let idx = index.get(base).cloned().unwrap_or_default();
            let stem = base.trim_end_matches(".jar").trim_end_matches(".zip");
            let name = idx.name.clone().or_else(|| meta["name"].as_str().map(String::from)).unwrap_or_else(|| stem.to_string());
            let url = idx
                .modrinth
                .as_ref()
                .map(|id| format!("https://modrinth.com/mod/{id}"))
                .or_else(|| idx.curseforge.map(|id| format!("https://www.curseforge.com/projects/{id}")));
            json!({
                "file": file,
                "enabled": !file.ends_with(DISABLED),
                "name": name,
                "version": meta["version"],
                "description": meta["description"].as_str().map(|d| d.split_whitespace().collect::<Vec<_>>().join(" ")),
                "authors": meta["authors"],
                "icon": meta["iconFile"],
                "url": url,
                "side": idx.side,
            })
        })
        .collect();
    out.sort_by_key(|m| m["name"].as_str().unwrap_or_default().to_lowercase());
    out
}

/// Turns a mod on or off by renaming it (`x.jar` ↔ `x.jar.disabled`); its new file name.
pub fn set_enabled(mods: &Path, file: &str, on: bool) -> Result<String> {
    if file.is_empty() || file.contains('/') || file.starts_with('.') {
        bail!("not a mod file: {file}");
    }
    let base = file.strip_suffix(DISABLED).unwrap_or(file);
    let (from, to) = if on { (format!("{base}{DISABLED}"), base.to_string()) } else { (base.to_string(), format!("{base}{DISABLED}")) };
    if mods.join(&to).exists() {
        return Ok(to);
    }
    if !mods.join(&from).is_file() {
        bail!("no mod {file}");
    }
    std::fs::rename(mods.join(&from), mods.join(&to))?;
    Ok(to)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn jar(path: &Path, files: &[(&str, &[u8])]) {
        let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (name, data) in files {
            z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(data).unwrap();
        }
        z.finish().unwrap();
    }
    fn png() -> Vec<u8> {
        let mut b = Vec::new();
        image::RgbaImage::from_pixel(128, 128, image::Rgba([0, 200, 0, 255])).write_to(&mut std::io::Cursor::new(&mut b), image::ImageFormat::Png).unwrap();
        b
    }

    #[test]
    fn lists_mods_from_the_index_and_the_jars_and_toggles_them() {
        let dir = tempfile::tempdir().unwrap();
        let (mods, cache) = (dir.path().join("mods"), dir.path().join("cache"));
        std::fs::create_dir_all(mods.join(".index")).unwrap();
        let icon = png();
        jar(
            &mods.join("appleskin-1.0.jar"),
            &[("fabric.mod.json", br#"{"id":"appleskin","name":"appleskin jar name","version":"3.0.10","description":"Food  info","authors":["squeek",{"name":"b"}],"icon":{"16":"s.png","128":"big.png"}}"#), ("big.png", &icon)],
        );
        std::fs::write(mods.join(".index/appleskin.pw.toml"), "filename = 'appleskin-1.0.jar'\nname = 'AppleSkin'\nside = 'both'\n[update.modrinth]\nmod-id = 'EsAfCjCV'\nversion = 'x'\n").unwrap();
        jar(
            &mods.join("netherite.jar.disabled"),
            &[
                ("META-INF/mods.toml", b"modLoader=\"javafml\"\n[[mods]]\nmodId=\"advancednetherite\"\nversion=\"${file.jarVersion}\"\ndisplayName=\"Advanced Netherite\"\nauthors=\"Autovw, Liz\"\ndescription='''\nMore tiers.\n'''\n"),
                ("META-INF/MANIFEST.MF", b"Manifest-Version: 1.0\nImplementation-Version: 2.1.3\n"),
            ],
        );
        jar(&mods.join("plain.jar"), &[("x.class", b"")]);
        std::fs::write(mods.join("readme.txt"), "").unwrap();

        let list = list(&mods, &cache);
        let names: Vec<&str> = list.iter().map(|m| m["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["Advanced Netherite", "AppleSkin", "plain"], "the index's name first; a jar without metadata by its file name");
        let apple = &list[1];
        assert_eq!((&apple["version"], &apple["description"], &apple["url"], &apple["authors"]), (&json!("3.0.10"), &json!("Food info"), &json!("https://modrinth.com/mod/EsAfCjCV"), &json!(["squeek", "b"])));
        let icon_file = apple["icon"].as_str().unwrap();
        assert_eq!(image::open(icon_file).unwrap().width(), ICON, "the biggest icon, made small");
        let neth = &list[0];
        assert_eq!((&neth["enabled"], &neth["version"], &neth["authors"], &neth["description"]), (&json!(false), &json!("2.1.3"), &json!(["Autovw", "Liz"]), &json!("More tiers.")));

        // Toggling keeps the cache (same name, size and time) and the index match
        assert_eq!(set_enabled(&mods, "netherite.jar.disabled", true).unwrap(), "netherite.jar");
        assert_eq!(set_enabled(&mods, "appleskin-1.0.jar", false).unwrap(), "appleskin-1.0.jar.disabled");
        assert_eq!(set_enabled(&mods, "appleskin-1.0.jar", false).unwrap(), "appleskin-1.0.jar.disabled", "already off");
        let list = super::list(&mods, &cache);
        assert_eq!((&list[0]["enabled"], &list[1]["enabled"], &list[1]["name"]), (&json!(true), &json!(false), &json!("AppleSkin")));
        assert_eq!(list[1]["icon"].as_str().unwrap(), icon_file);
        assert!(set_enabled(&mods, "../x.jar", true).is_err());
        assert!(set_enabled(&mods, "missing.jar", true).is_err());
    }
}
