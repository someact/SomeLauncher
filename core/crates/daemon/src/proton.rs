//! Proton builds: discovery in every place one can live (Steam's
//! `compatibilitytools.d`, system ones, Steam's own Proton apps, Lutris' and
//! Twintail's runners), lookup by any of the names launchers use, the default
//! (newest GE-Proton), and installs from GitHub releases.

use crate::import::{steam, vdf};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Proton {
    /// The folder name (`GE-Proton11-7`, `cachyos-11.0-20260703-slr`): what we store
    pub name: String,
    /// The name in `compatibilitytool.vdf` (`proton-cachyos-11.0-20260703-slr-x86_64`), what Steam stores
    pub internal: String,
    pub dir: PathBuf,
    /// `user` (compatibilitytools.d in a Steam install), `system`, `steam` (Valve's Proton apps), `lutris`, `twintail`
    pub source: &'static str,
}

/// Where downloaded builds go: the first Steam install's `compatibilitytools.d`
/// (umu-run and Steam both look there).
pub fn install_dir(home: &Path) -> PathBuf {
    steam::roots(home).into_iter().next().unwrap_or_else(|| home.join(".local/share/Steam")).join("compatibilitytools.d")
}

fn internal_name(dir: &Path) -> Option<String> {
    let v = vdf::parse(&std::fs::read_to_string(dir.join("compatibilitytool.vdf")).ok()?).ok()?;
    v.at(&["compatibilitytools", "compat_tools"])?.entries().first().map(|(k, _)| k.clone())
}

fn scan_dir(dir: &Path, source: &'static str, out: &mut Vec<Proton>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut dirs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    dirs.sort();
    for d in dirs {
        scan_one(&d, source, out);
    }
}

/// Every Proton build on this machine, each once.
pub fn discover(home: &Path) -> Vec<Proton> {
    let mut out = Vec::new();
    let roots = steam::roots(home);
    for root in &roots {
        scan_dir(&root.join("compatibilitytools.d"), "user", &mut out);
    }
    // Where our downloads go, also when no Steam is installed
    scan_dir(&install_dir(home), "user", &mut out);
    for dir in ["/usr/share/steam/compatibilitytools.d", "/usr/local/share/steam/compatibilitytools.d", "/opt/steam/compatibilitytools.d"] {
        scan_dir(Path::new(dir), "system", &mut out);
    }
    for root in &roots {
        for lib in steam::libraries(root) {
            let common = lib.join("steamapps/common");
            let Ok(entries) = std::fs::read_dir(&common) else { continue };
            let mut protons: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("Proton")))
                .collect();
            protons.sort();
            for p in protons {
                scan_one(&p, "steam", &mut out);
            }
        }
    }
    scan_dir(&home.join(".local/share/lutris/runners/proton"), "lutris", &mut out);
    scan_dir(&home.join(".local/share/twintaillauncher/compatibility/runners"), "twintail", &mut out);
    out
}

fn scan_one(dir: &Path, source: &'static str, out: &mut Vec<Proton>) {
    let Ok(canon) = dir.canonicalize() else { return };
    if canon.join("proton").is_file() && !out.iter().any(|p| p.dir == canon) {
        let name = dir.file_name().unwrap_or_default().to_string_lossy().into_owned();
        out.push(Proton { internal: internal_name(&canon).unwrap_or_else(|| name.clone()), name, dir: canon, source });
    }
}

/// Finds a build by folder name, internal name or path. A path that is not
/// among the discovered ones still works when it holds a `proton` script.
pub fn resolve(list: &[Proton], wanted: &str) -> Option<Proton> {
    let w = wanted.trim();
    if w.contains('/') {
        let dir = Path::new(w).canonicalize().ok()?;
        return list.iter().find(|p| p.dir == dir).cloned().or_else(|| {
            dir.join("proton").is_file().then(|| Proton {
                name: dir.file_name().unwrap_or_default().to_string_lossy().into_owned(),
                internal: internal_name(&dir).unwrap_or_default(),
                dir,
                source: "path",
            })
        });
    }
    let by = |f: &dyn Fn(&Proton) -> bool| list.iter().find(|p| f(p)).cloned();
    by(&|p| p.name == w).or_else(|| by(&|p| p.internal == w)).or_else(|| by(&|p| p.name.eq_ignore_ascii_case(w) || p.internal.eq_ignore_ascii_case(w)))
}

/// The numbers in a name, for ordering versions: `GE-Proton10-34` → [10, 34].
fn version_key(name: &str) -> Vec<u64> {
    let name = name.replace("x86_64", "").replace("aarch64", "");
    name.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect()
}

/// The `defaultProton` setting when it resolves, else the newest GE-Proton, else the newest of any kind.
pub fn default(list: &[Proton], setting: Option<&str>) -> Option<Proton> {
    if let Some(p) = setting.and_then(|s| resolve(list, s)) {
        return Some(p);
    }
    let newest = |f: &dyn Fn(&&Proton) -> bool| list.iter().filter(f).max_by_key(|p| version_key(&p.name)).cloned();
    newest(&|p| p.name.starts_with("GE-Proton")).or_else(|| newest(&|p| p.source != "steam")).or_else(|| newest(&|_| true))
}

/// Downloadable builds, by the `proton.available` source name.
pub const RELEASES: &[(&str, &str, &str)] = &[
    // source, GitHub repo, the asset suffix for this machine
    ("ge", "GloriousEggroll/proton-ge-custom", "x86_64.tar.gz"),
    ("cachyos", "CachyOS/proton-cachyos", "x86_64.tar.xz"),
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub source: String,
    pub tag: String,
    pub published: String,
    pub url: String,
    pub checksum_url: Option<String>,
    pub size: u64,
    pub installed: bool,
}

/// The newest releases of one source with an asset for this machine.
pub async fn releases(http: &reqwest::Client, source: &str, installed: &[Proton]) -> Result<Vec<Release>> {
    let Some(&(_, repo, suffix)) = RELEASES.iter().find(|r| r.0 == source) else { bail!("no Proton source {source}") };
    #[derive(serde::Deserialize)]
    struct Gh {
        tag_name: String,
        #[serde(default)]
        published_at: String,
        #[serde(default)]
        prerelease: bool,
        assets: Vec<Asset>,
    }
    #[derive(serde::Deserialize)]
    struct Asset {
        name: String,
        browser_download_url: String,
        size: u64,
    }
    let list: Vec<Gh> = http
        .get(format!("https://api.github.com/repos/{repo}/releases?per_page=15"))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()
        .context("GitHub releases")?
        .json()
        .await?;
    Ok(list
        .into_iter()
        .filter(|r| !r.prerelease)
        .filter_map(|r| {
            let asset = r.assets.iter().find(|a| a.name.ends_with(suffix))?;
            let stem = asset.name.trim_end_matches(".tar.gz").trim_end_matches(".tar.xz");
            let checksum = r.assets.iter().find(|a| a.name == format!("{stem}.sha512sum")).map(|a| a.browser_download_url.clone());
            let installed = installed.iter().any(|p| [&p.name, &p.internal].iter().any(|n| *n == &r.tag_name || *n == stem));
            Some(Release {
                source: source.into(),
                tag: r.tag_name,
                published: r.published_at,
                url: asset.browser_download_url.clone(),
                checksum_url: checksum,
                size: asset.size,
                installed,
            })
        })
        .collect())
}

/// Downloads, checks and unpacks a release into `dest` (a `compatibilitytools.d`).
pub async fn install(http: reqwest::Client, release: Release, dest: PathBuf, progress: crate::jobs::Progress) -> Result<()> {
    use crate::jobs;
    tokio::fs::create_dir_all(&dest).await?;
    let staging = jobs::TempDir(dest.join(format!(".somelauncher-{}", release.tag)));
    let _ = tokio::fs::remove_dir_all(&staging.0).await;
    tokio::fs::create_dir_all(&staging.0).await?;
    let archive = staging.0.join("archive");
    jobs::fetch(&http, &release.url, &archive, &progress).await?;
    if let Some(url) = &release.checksum_url {
        let line = http.get(url).send().await?.error_for_status()?.text().await?;
        jobs::verify_sha512(&archive, &line).await?;
    }
    let unpacked = staging.0.join("unpacked");
    jobs::untar(&archive, &unpacked).await?;
    let mut top = std::fs::read_dir(&unpacked)?.flatten().map(|e| e.path()).filter(|p| p.is_dir());
    let (Some(dir), None) = (top.next(), top.next()) else { bail!("unexpected archive layout") };
    if !dir.join("proton").is_file() {
        bail!("the archive holds no Proton build");
    }
    let target = dest.join(dir.file_name().unwrap_or_default());
    if target.exists() {
        bail!("{} is already installed", target.display());
    }
    tokio::fs::rename(&dir, &target).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(dir: &Path, internal: Option<&str>) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("proton"), "").unwrap();
        if let Some(i) = internal {
            std::fs::write(
                dir.join("compatibilitytool.vdf"),
                format!("\"compatibilitytools\" {{ \"compat_tools\" {{ \"{i}\" // comment\n {{ \"install_path\" \".\" }} }} }}"),
            )
            .unwrap();
        }
    }

    #[test]
    fn finds_resolves_and_picks_the_newest_ge() {
        let home = tempfile::tempdir().unwrap();
        let ctd = home.path().join(".local/share/Steam/compatibilitytools.d");
        std::fs::create_dir_all(home.path().join(".local/share/Steam/steamapps")).unwrap();
        tool(&ctd.join("GE-Proton10-34"), Some("GE-Proton10-34"));
        tool(&ctd.join("GE-Proton11-7"), Some("GE-Proton11-7-x86_64"));
        tool(&ctd.join("cachyos-11.0-20260703-slr"), Some("proton-cachyos-11.0-20260703-slr-x86_64"));
        tool(&home.path().join(".local/share/Steam/steamapps/common/Proton Hotfix"), None);
        tool(&home.path().join(".local/share/twintaillauncher/compatibility/runners/11.0-proton-cachyos"), None);
        std::fs::create_dir_all(ctd.join("not-a-tool")).unwrap();

        let list = discover(home.path());
        let names: Vec<_> = list.iter().map(|p| (p.name.as_str(), p.source)).collect();
        assert_eq!(
            names,
            [
                ("GE-Proton10-34", "user"),
                ("GE-Proton11-7", "user"),
                ("cachyos-11.0-20260703-slr", "user"),
                ("Proton Hotfix", "steam"),
                ("11.0-proton-cachyos", "twintail")
            ]
        );
        assert_eq!(resolve(&list, "proton-cachyos-11.0-20260703-slr-x86_64").unwrap().name, "cachyos-11.0-20260703-slr");
        assert_eq!(resolve(&list, "ge-proton11-7").unwrap().name, "GE-Proton11-7");
        assert_eq!(resolve(&list, ctd.join("GE-Proton10-34").to_str().unwrap()).unwrap().source, "user");
        assert!(resolve(&list, "GE-Proton9-1").is_none());

        assert_eq!(default(&list, None).unwrap().name, "GE-Proton11-7");
        assert_eq!(default(&list, Some("Proton Hotfix")).unwrap().name, "Proton Hotfix");
        assert_eq!(default(&list, Some("gone")).unwrap().name, "GE-Proton11-7");
    }
}
