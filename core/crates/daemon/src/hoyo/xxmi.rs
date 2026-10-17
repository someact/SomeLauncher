//! XXMI (6h): 3DMigoto mods for Genshin Impact (GIMI) and ZZZ (ZZMI), set up
//! as Twintail does. Everything lives in `$XDG_DATA_HOME/somelauncher/xxmi/`:
//! Twintail's loader (`3dmloader.exe`), XXMI Libs (3DMigoto's `d3d11.dll` and
//! `d3dcompiler_47.dll`) in `libs/`, and one folder per game package (`gimi/`,
//! `zzmi/`) with the package's `d3dx.ini`, `Core/`, `ShaderFixes/`, a copy of
//! the libs, and the user's `Mods/`. Each comes from its project's latest
//! GitHub release; an update never touches `Mods/` or `d3dx_user.ini` (the
//! mods' saved settings). A mod is a folder in `Mods/`; one whose name starts
//! with `DISABLED` is skipped by the package's `d3dx.ini`.
//!
//! At launch (`runner.xxmi`) the loader runs beside the game in the same
//! prefix, with the game package's folder as its argument: it reads
//! `[Loader] target`/`module` from that `d3dx.ini`, waits for the game and
//! injects 3DMigoto ([`wrap`]).
//!
//! Mods break HoYoverse's terms of service; nothing here is on by default.

use anyhow::{Context as _, Result, bail};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// What is fetched: the key in `versions.json`, its GitHub repo and how its release asset is named
pub struct Package {
    pub key: &'static str,
    pub repo: &'static str,
    asset: fn(&str) -> bool,
}

pub const PACKAGES: &[Package] = &[
    Package { key: "loader", repo: "TwintailTeam/3dmloader-Package", asset: |n| n == "3dmloader.exe" },
    Package { key: "libs", repo: "SpectrumQT/XXMI-Libs-Package", asset: |n| n.starts_with("XXMI-PACKAGE") && n.ends_with(".zip") },
    Package { key: "gimi", repo: "SilentNightSound/GIMI-Package", asset: |n| n.starts_with("GIMI-PACKAGE") && n.ends_with(".zip") },
    Package { key: "zzmi", repo: "leotorrez/ZZMI-Package", asset: |n| n.starts_with("ZZMI-PACKAGE") && n.ends_with(".zip") },
];

/// The libs each game package folder gets a copy of (copies: links fail inside pressure-vessel)
const LIBS: &[&str] = &["d3d11.dll", "d3dcompiler_47.dll"];
const LOADER: &str = "3dmloader.exe";
const CHECK_EVERY: Duration = Duration::from_secs(3600);

pub fn dir() -> PathBuf {
    somelauncher_proto::paths::data_dir().join("xxmi")
}

/// The game package of a HoYoPlay game: `gimi` for Genshin, `zzmi` for ZZZ
pub fn importer_of(biz: &str) -> Option<&'static str> {
    match super::api::game_of(biz) {
        "hk4e" => Some("gimi"),
        "nap" => Some("zzmi"),
        _ => None,
    }
}

/// Ready to run a game: the loader, and its package folder with 3DMigoto in it
pub fn ready(root: &Path, importer: &str) -> bool {
    let d = root.join(importer);
    root.join(LOADER).is_file() && d.join("d3dx.ini").is_file() && LIBS.iter().all(|l| d.join(l).is_file())
}

pub fn installed(root: &Path) -> BTreeMap<String, String> {
    std::fs::read(root.join("versions.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save_installed(root: &Path, v: &BTreeMap<String, String>) -> Result<()> {
    std::fs::write(root.join("versions.json"), serde_json::to_vec_pretty(v)?)?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    pub tag: String,
    pub url: String,
    pub size: u64,
}

static LATEST: Mutex<Option<(Instant, BTreeMap<String, Release>)>> = Mutex::new(None);

/// Each package's latest release (asked at most once an hour; `force` asks again)
pub async fn latest(http: &reqwest::Client, force: bool) -> Result<BTreeMap<String, Release>> {
    if !force && let Some((at, l)) = LATEST.lock().unwrap().as_ref() && at.elapsed() < CHECK_EVERY {
        return Ok(l.clone());
    }
    let mut out = BTreeMap::new();
    for p in PACKAGES {
        let url = format!("https://api.github.com/repos/{}/releases/latest", p.repo);
        let v: Value = http.get(&url).header("Accept", "application/vnd.github+json").send().await?.error_for_status().with_context(|| format!("GitHub {}", p.repo))?.json().await?;
        out.insert(p.key.to_string(), release(&v, p.asset).with_context(|| format!("{}: no release asset", p.repo))?);
    }
    *LATEST.lock().unwrap() = Some((Instant::now(), out.clone()));
    Ok(out)
}

fn release(v: &Value, asset: fn(&str) -> bool) -> Option<Release> {
    let tag = v["tag_name"].as_str()?.trim_start_matches('v').to_string();
    let a = v["assets"].as_array()?.iter().find(|a| a["name"].as_str().is_some_and(asset))?;
    Some(Release { tag, url: a["browser_download_url"].as_str()?.to_string(), size: a["size"].as_u64().unwrap_or(0) })
}

/// The packages that are missing or behind
pub fn outdated(have: &BTreeMap<String, String>, latest: &BTreeMap<String, Release>) -> Vec<String> {
    PACKAGES.iter().map(|p| p.key.to_string()).filter(|k| latest.get(k).is_some_and(|r| have.get(k) != Some(&r.tag))).collect()
}

/// Unpacks a zip into `into`, overwriting its files; `keep` paths (relative,
/// first component) are never written. Entries that would leave `into` are refused.
pub fn unzip(bytes: &[u8], into: &Path, keep: &[&str]) -> Result<()> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).context("not a zip")?;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i)?;
        let Some(rel) = f.enclosed_name() else { bail!("{} leaves its folder", f.name().unwrap_or_default()) };
        let first = rel.components().next().map(|c| c.as_os_str().to_string_lossy().to_string()).unwrap_or_default();
        let dest = into.join(&rel);
        if f.is_dir() {
            std::fs::create_dir_all(&dest)?;
            continue;
        }
        if keep.iter().any(|k| first.eq_ignore_ascii_case(k)) {
            continue;
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&dest).with_context(|| format!("writing {}", dest.display()))?;
        std::io::copy(&mut f, &mut out)?;
    }
    Ok(())
}

/// Puts one downloaded package in place
pub fn place(root: &Path, key: &str, bytes: &[u8]) -> Result<()> {
    std::fs::create_dir_all(root)?;
    match key {
        "loader" => {
            let tmp = root.join(".3dmloader.exe.part");
            std::fs::write(&tmp, bytes)?;
            std::fs::rename(&tmp, root.join(LOADER))?;
        }
        "libs" => {
            let libs = root.join("libs");
            unzip(bytes, &libs, &[])?;
            for imp in ["gimi", "zzmi"] {
                if root.join(imp).is_dir() {
                    copy_libs(root, imp)?;
                }
            }
        }
        imp => {
            // The user's: their mods and their mods' saved settings
            let d = root.join(imp);
            unzip(bytes, &d, &["Mods", "d3dx_user.ini"])?;
            std::fs::create_dir_all(d.join("Mods"))?;
            if root.join("libs").is_dir() {
                copy_libs(root, imp)?;
            }
        }
    }
    Ok(())
}

fn copy_libs(root: &Path, importer: &str) -> Result<()> {
    for l in LIBS {
        let from = root.join("libs").join(l);
        if from.is_file() {
            std::fs::copy(&from, root.join(importer).join(l)).with_context(|| format!("copying {l}"))?;
        }
    }
    Ok(())
}

/// Downloads and places what is missing or behind; each package's version is
/// recorded once it is in place, so a failed run goes on where it stopped.
pub async fn update(http: &reqwest::Client, root: &Path, progress: &crate::jobs::Progress) -> Result<Vec<String>> {
    let latest = latest(http, true).await?;
    let mut have = installed(root);
    let todo = outdated(&have, &latest);
    let total: u64 = todo.iter().map(|k| latest[k].size).sum();
    let mut done = 0u64;
    progress.update(0, total);
    for key in &todo {
        let r = &latest[key];
        let bytes = http.get(&r.url).timeout(Duration::from_secs(600)).send().await?.error_for_status().with_context(|| format!("downloading {}", r.url))?.bytes().await?;
        let root2 = root.to_path_buf();
        let key2 = key.clone();
        tokio::task::spawn_blocking(move || place(&root2, &key2, &bytes)).await??;
        have.insert(key.clone(), r.tag.clone());
        save_installed(root, &have)?;
        done += r.size;
        progress.update(done, total);
        tracing::info!("XXMI: {key} {}", r.tag);
    }
    Ok(todo)
}

// --- mods

/// A mod's name without its `DISABLED` mark
pub fn mod_name(folder: &str) -> &str {
    let lower = folder.to_ascii_lowercase();
    if lower.starts_with("disabled") {
        folder[8..].trim_start_matches([' ', '_', '-'])
    } else {
        folder
    }
}

pub fn enabled(folder: &str) -> bool {
    !folder.to_ascii_lowercase().starts_with("disabled")
}

/// The mods in a `Mods/` folder: `{name, folder, enabled, preview}`, by name.
/// The preview is the folder's picture named `preview*` (or `thumb*`, `cover*`).
pub fn list(mods: &Path) -> Vec<Value> {
    let Ok(entries) = std::fs::read_dir(mods) else { return Vec::new() };
    let mut out: Vec<Value> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| {
            let folder = e.file_name().to_string_lossy().into_owned();
            json!({ "name": mod_name(&folder), "folder": folder, "enabled": enabled(&folder), "preview": preview(&e.path()), "path": e.path() })
        })
        .collect();
    out.sort_by_key(|m| m["name"].as_str().unwrap_or_default().to_lowercase());
    out
}

fn preview(dir: &Path) -> Option<PathBuf> {
    // Mods ship textures and help overlays as pictures too: only one named as a preview counts
    let named = |p: &Path| {
        let stem = p.file_stem().unwrap_or_default().to_string_lossy().to_lowercase();
        ["preview", "thumb", "cover"].iter().any(|n| stem.starts_with(n))
    };
    let picture = |p: &Path| p.extension().and_then(|e| e.to_str()).is_some_and(|e| ["png", "jpg", "jpeg", "webp", "gif"].contains(&e.to_ascii_lowercase().as_str()));
    let mut pics: Vec<PathBuf> = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.is_file() && picture(p) && named(p)).collect();
    pics.sort();
    pics.into_iter().next()
}

/// Turns a mod on or off by its folder's name; the new folder name
pub fn set_enabled(mods: &Path, folder: &str, on: bool) -> Result<String> {
    if folder.is_empty() || folder.contains('/') || folder == "." || folder == ".." {
        bail!("not a mod folder: {folder}");
    }
    let from = mods.join(folder);
    if !from.is_dir() {
        bail!("no mod {folder}");
    }
    let to = if on { mod_name(folder).to_string() } else if enabled(folder) { format!("DISABLED_{folder}") } else { folder.to_string() };
    if to != folder {
        if mods.join(&to).exists() {
            bail!("{to} already exists in the mods folder");
        }
        std::fs::rename(&from, mods.join(&to))?;
    }
    Ok(to)
}

/// Copies Twintail's mods for a package into ours, those we do not have (by
/// name, on or off alike), and its saved mod settings when we have none. The names copied.
pub fn import_from(theirs: &Path, ours: &Path) -> Result<Vec<String>> {
    let have: Vec<String> = list(&ours.join("Mods")).iter().filter_map(|m| m["name"].as_str().map(str::to_lowercase)).collect();
    std::fs::create_dir_all(ours.join("Mods"))?;
    let mut copied = Vec::new();
    for m in list(&theirs.join("Mods")) {
        let (name, folder) = (m["name"].as_str().unwrap_or_default(), m["folder"].as_str().unwrap_or_default());
        if have.contains(&name.to_lowercase()) {
            continue;
        }
        copy_dir(&theirs.join("Mods").join(folder), &ours.join("Mods").join(folder))?;
        copied.push(name.to_string());
    }
    let user = ours.join("d3dx_user.ini");
    if !user.exists() && theirs.join("d3dx_user.ini").is_file() {
        std::fs::copy(theirs.join("d3dx_user.ini"), user)?;
    }
    Ok(copied)
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)?.flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        let kind = e.file_type()?;
        if kind.is_dir() {
            copy_dir(&src, &dst)?;
        } else if kind.is_file() || src.is_file() {
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

/// Twintail's package folders (native, then flatpak)
pub fn twintail_dirs(home: &Path, importer: &str) -> Vec<PathBuf> {
    [".local/share/twintaillauncher", ".var/app/app.twintaillauncher.ttl/data/twintaillauncher"]
        .iter()
        .map(|d| home.join(d).join("extras/xxmi").join(importer))
        .filter(|d| d.join("Mods").is_dir())
        .collect()
}

// --- launch

/// Runs `command` (`umu-run <game.exe> …`) with the loader beside it: the
/// loader starts a moment after the game, in the same prefix and env, from
/// the XXMI folder, given the package's folder name. When the game command
/// ends, whatever is still left in this game's own scope (a loader still
/// waiting for a game that never came) is stopped, so the session ends too;
/// the script's own ancestors there (systemd-run, the wrappers) are spared,
/// they end with it.
pub fn wrap(root: &Path, importer: &str, umu: &str, command: Vec<String>, env: &mut BTreeMap<String, String>) -> Vec<String> {
    env.insert("SOMELAUNCHER_XXMI".into(), root.to_string_lossy().into_owned());
    env.insert("SOMELAUNCHER_XXMI_PACKAGE".into(), importer.into());
    env.insert("SOMELAUNCHER_UMU".into(), umu.into());
    const SCRIPT: &str = r#"(sleep 1; cd "$SOMELAUNCHER_XXMI" && exec "$SOMELAUNCHER_UMU" "$SOMELAUNCHER_XXMI/3dmloader.exe" "$SOMELAUNCHER_XXMI_PACKAGE") &
"$@"
status=$?
cg=$(cut -d: -f3 /proc/self/cgroup)
case "$cg" in
*/somelauncher-game-*)
    keep=" $$ "
    p=$$
    while [ "$p" -gt 1 ] 2>/dev/null; do
        p=$(sed 's/.*) //' "/proc/$p/stat" 2>/dev/null | cut -d' ' -f2) || break
        keep="$keep$p "
    done
    for p in $(cat "/sys/fs/cgroup$cg/cgroup.procs"); do
        case "$keep" in *" $p "*) ;; *) kill "$p" 2>/dev/null ;; esac
    done ;;
esac
exit $status"#;
    let mut argv = vec!["sh".to_string(), "-c".into(), SCRIPT.into(), "somelauncher-xxmi".into()];
    argv.extend(command);
    argv
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        let mut z = zip::ZipWriter::new(&mut buf);
        for (name, data) in files {
            if name.ends_with('/') {
                z.add_directory(*name, zip::write::SimpleFileOptions::default()).unwrap();
            } else {
                z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
                z.write_all(data).unwrap();
            }
        }
        z.finish().unwrap();
        buf.into_inner()
    }

    #[test]
    fn packages_keep_the_users_mods_and_get_the_libs() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        place(root, "libs", &zip_of(&[("d3d11.dll", b"v1"), ("d3dcompiler_47.dll", b"c"), ("3dmloader.dll", b"l")])).unwrap();
        std::fs::create_dir_all(root.join("gimi/Mods/Venti")).unwrap();
        std::fs::write(root.join("gimi/d3dx_user.ini"), "mine").unwrap();
        let pkg = zip_of(&[("Core/", b""), ("Mods/", b""), ("Mods/sample.ini", b"x"), ("d3dx.ini", b"[Loader]"), ("d3dx_user.ini", b"theirs"), ("Core/GIMI/main.ini", b"m")]);
        place(root, "gimi", &pkg).unwrap();
        assert!(root.join("gimi/Mods/Venti").is_dir());
        assert!(!root.join("gimi/Mods/sample.ini").exists());
        assert_eq!(std::fs::read_to_string(root.join("gimi/d3dx_user.ini")).unwrap(), "mine");
        assert_eq!(std::fs::read(root.join("gimi/d3d11.dll")).unwrap(), b"v1");
        assert!(!ready(root, "gimi"), "no loader yet");
        place(root, "loader", b"MZ").unwrap();
        assert!(ready(root, "gimi"));
        // New libs reach the package folders too
        place(root, "libs", &zip_of(&[("d3d11.dll", b"v2"), ("d3dcompiler_47.dll", b"c")])).unwrap();
        assert_eq!(std::fs::read(root.join("gimi/d3d11.dll")).unwrap(), b"v2");
        assert!(unzip(&zip_of(&[("../evil", b"x")]), &root.join("z"), &[]).is_err());
    }

    #[test]
    fn mods_turn_on_and_off_by_their_folder_name() {
        let d = tempfile::tempdir().unwrap();
        let mods = d.path();
        std::fs::create_dir_all(mods.join("Venti")).unwrap();
        std::fs::write(mods.join("Venti/preview.png"), b"").unwrap();
        std::fs::write(mods.join("Venti/0c.png"), b"").unwrap();
        std::fs::create_dir_all(mods.join("DISABLED Nahida")).unwrap();
        std::fs::write(mods.join("DISABLED Nahida/NahidaBody.png"), b"").unwrap();
        let l = list(mods);
        assert!(l[0]["preview"].is_null(), "a texture is not a preview");
        assert_eq!(l[0]["name"], "Nahida");
        assert_eq!(l[0]["enabled"], false);
        assert!(l[1]["preview"].as_str().unwrap().ends_with("preview.png"));
        assert_eq!(set_enabled(mods, "Venti", false).unwrap(), "DISABLED_Venti");
        assert_eq!(set_enabled(mods, "DISABLED Nahida", true).unwrap(), "Nahida");
        assert!(set_enabled(mods, "../x", true).is_err());
        assert_eq!(mod_name("disabled-Foo"), "Foo");
    }

    #[test]
    fn twintail_mods_are_copied_once() {
        let d = tempfile::tempdir().unwrap();
        let (theirs, ours) = (d.path().join("t"), d.path().join("o"));
        std::fs::create_dir_all(theirs.join("Mods/Venti/tex")).unwrap();
        std::fs::write(theirs.join("Mods/Venti/tex/a.dds"), b"x").unwrap();
        std::fs::write(theirs.join("d3dx_user.ini"), b"s").unwrap();
        assert_eq!(import_from(&theirs, &ours).unwrap(), ["Venti"]);
        assert!(ours.join("Mods/Venti/tex/a.dds").is_file());
        assert!(import_from(&theirs, &ours).unwrap().is_empty());
    }

    #[test]
    fn releases_and_what_is_behind() {
        let v = json!({"tag_name": "v1.2.2", "assets": [{"name": "Manifest.json", "browser_download_url": "m"}, {"name": "XXMI-PACKAGE-v1.2.2.zip", "browser_download_url": "z", "size": 5}]});
        let r = release(&v, PACKAGES[1].asset).unwrap();
        assert_eq!((r.tag.as_str(), r.url.as_str(), r.size), ("1.2.2", "z", 5));
        let latest: BTreeMap<_, _> = [("libs".to_string(), r.clone()), ("loader".to_string(), Release { tag: "3.0.3".into(), ..r })].into();
        let have: BTreeMap<_, _> = [("libs".to_string(), "1.2.2".to_string())].into();
        assert_eq!(outdated(&have, &latest), ["loader"]);
        assert_eq!((importer_of("hk4e_global"), importer_of("nap_cn"), importer_of("hkrpg_global")), (Some("gimi"), Some("zzmi"), None));
    }

    #[test]
    fn the_loader_runs_beside_the_game() {
        let mut env = BTreeMap::new();
        let argv = wrap(Path::new("/x"), "gimi", "/usr/bin/umu-run", vec!["/usr/bin/umu-run".into(), "/g/GenshinImpact.exe".into()], &mut env);
        assert_eq!(&argv[..2], ["sh", "-c"]);
        assert_eq!(&argv[4..], ["/usr/bin/umu-run", "/g/GenshinImpact.exe"]);
        assert_eq!(env["SOMELAUNCHER_XXMI_PACKAGE"], "gimi");
    }
}
