//! Updating an installed Hoyo game (6d). A diff update when Sophon's patch
//! build covers the installed version: per changed file, an hdiff patch (or
//! the whole new file) cut from a patch bundle by a range request, applied
//! with `hpatchz` from HDiffPatch, md5-checked, then put in place; files the
//! new version drops are deleted. Otherwise a full update: the installed and
//! the new manifests are compared by md5 and only changed files are fetched.
//! Preload fetches the next version's patches ahead of time; the update then
//! takes them from disk. Repair checks every file and fetches the bad ones.
//! All of them keep a log, so a stopped one goes on where it was.

use super::api::{Branch, Region};
use super::sophon::{self, Category, STATE_DIR, md5_hex};
use crate::jobs::Progress;
use anyhow::{Context as _, Result, bail};
use futures_util::StreamExt;
use md5::{Digest, Md5};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// HDiffPatch, fetched once when `hpatchz` is not on PATH
const HDIFF_VERSION: &str = "v5.1.3";
const HDIFF_URL: &str = "https://github.com/sisong/HDiffPatch/releases/download/v5.1.3/hdiffpatch_v5.1.3_bin_linux64.zip";
/// The zip's sha256, as GitHub lists it for the release asset
const HDIFF_SHA256: &str = "628963bf2ee9108a97260fa5eef44acd9ec94369b76090a957c9182b3abbb558";
const PARALLEL: usize = 8;
/// What HDiffPatch data starts with (`HDIFF13&zstd…`)
const HDIFF_MAGIC: &[u8] = b"HDIFF";

/// One category of a patch build
#[derive(Debug, Clone, PartialEq)]
pub struct PatchCategory {
    pub field: String,
    pub manifest: String,
    pub checksum: String,
    pub manifest_url: String,
    pub diff_url: String,
    /// The download, by the version it starts from
    pub sizes: HashMap<String, u64>,
}

fn num(v: &Value) -> u64 {
    v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).unwrap_or(0)
}

pub fn parse_patch_build(data: &Value) -> Vec<PatchCategory> {
    data["manifests"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| PatchCategory {
            field: m["matching_field"].as_str().unwrap_or_default().into(),
            manifest: m["manifest"]["id"].as_str().unwrap_or_default().into(),
            checksum: m["manifest"]["checksum"].as_str().unwrap_or_default().into(),
            manifest_url: m["manifest_download"]["url_prefix"].as_str().unwrap_or_default().into(),
            diff_url: m["diff_download"]["url_prefix"].as_str().unwrap_or_default().into(),
            sizes: m["stats"].as_object().into_iter().flatten().map(|(tag, s)| (tag.clone(), num(&s["compressed_size"]))).collect(),
        })
        .collect()
}

/// The patch build of a branch: what takes each older version to `branch.tag`
pub async fn patch_build(http: &reqwest::Client, region: Region, branch: &Branch) -> Result<Vec<PatchCategory>> {
    let url = format!("{}/getPatchBuild", region.sophon());
    let q = [("branch", branch.branch.as_str()), ("package_id", &branch.package_id), ("password", &branch.password), ("tag", &branch.tag)];
    let v: Value = http.post(&url).query(&q).send().await?.error_for_status()?.json().await.context("Sophon getPatchBuild")?;
    if v["retcode"].as_i64() != Some(0) {
        bail!("Sophon getPatchBuild: {}", v["message"].as_str().unwrap_or("failed"));
    }
    Ok(parse_patch_build(&v["data"]))
}

// --- patch manifests

#[derive(Debug, Clone, PartialEq)]
pub struct Patch {
    /// The bundle it is in, and where
    pub bundle: String,
    pub offset: u64,
    pub length: u64,
    /// The old file it applies to; None: a new file (HDiff data from nothing, or the file itself)
    pub original: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PatchFile {
    pub name: String,
    pub size: u64,
    pub md5: String,
    /// By the version it starts from
    pub patches: HashMap<String, Patch>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PatchManifest {
    pub files: Vec<PatchFile>,
    /// Files to delete, by the version they go from
    pub deletes: HashMap<String, Vec<String>>,
}

use super::sophon::proto::{Field, Reader};

fn patch(bytes: &[u8]) -> Result<Patch> {
    let mut p = Patch { bundle: String::new(), offset: 0, length: 0, original: None };
    let mut r = Reader(bytes);
    while let Some((n, f)) = r.next()? {
        match n {
            1 => p.bundle = f.text(),
            6 => p.offset = f.int(),
            7 => p.length = f.int(),
            8 => p.original = Some(f.text()).filter(|s| !s.is_empty()),
            _ => {}
        }
    }
    Ok(p)
}

/// `{1 tag, 2 message}`: a value for one starting version
fn by_tag<'a>(bytes: &'a [u8]) -> Result<(String, &'a [u8])> {
    let (mut tag, mut body) = (String::new(), &[][..]);
    let mut r = Reader(bytes);
    while let Some((n, f)) = r.next()? {
        match (n, f) {
            (1, f) => tag = f.text(),
            (2, Field::Bytes(b)) => body = b,
            _ => {}
        }
    }
    Ok((tag, body))
}

pub fn decode_patch_manifest(bytes: &[u8]) -> Result<PatchManifest> {
    let mut m = PatchManifest::default();
    let mut r = Reader(bytes);
    while let Some((n, f)) = r.next()? {
        let Field::Bytes(b) = f else { continue };
        match n {
            1 => {
                let mut file = PatchFile { name: String::new(), size: 0, md5: String::new(), patches: HashMap::new() };
                let mut fr = Reader(b);
                while let Some((n, f)) = fr.next()? {
                    match (n, f) {
                        (1, f) => file.name = f.text(),
                        (2, f) => file.size = f.int(),
                        (3, f) => file.md5 = f.text(),
                        (4, Field::Bytes(b)) => {
                            let (tag, body) = by_tag(b)?;
                            file.patches.insert(tag, patch(body)?);
                        }
                        _ => {}
                    }
                }
                m.files.push(file);
            }
            2 => {
                let (tag, body) = by_tag(b)?;
                let mut names = Vec::new();
                let mut dr = Reader(body);
                while let Some((n, f)) = dr.next()? {
                    if let (1, Field::Bytes(entry)) = (n, f) {
                        let mut er = Reader(entry);
                        while let Some((n, f)) = er.next()? {
                            if n == 1 {
                                names.push(f.text());
                            }
                        }
                    }
                }
                m.deletes.entry(tag).or_default().extend(names);
            }
            _ => {}
        }
    }
    Ok(m)
}

async fn patch_manifest(http: &reqwest::Client, cat: &PatchCategory) -> Result<PatchManifest> {
    let url = format!("{}/{}", cat.manifest_url, cat.manifest);
    let raw = http.get(&url).send().await?.error_for_status()?.bytes().await.context("downloading the patch manifest")?;
    let checksum = cat.checksum.clone();
    tokio::task::spawn_blocking(move || {
        let bytes = zstd::stream::decode_all(&raw[..]).context("unpacking the patch manifest")?;
        if !checksum.is_empty() && md5_hex(&bytes) != checksum {
            bail!("the patch manifest is damaged");
        }
        decode_patch_manifest(&bytes)
    })
    .await?
}

// --- hpatchz

/// `hpatchz`: on PATH, else our copy, else fetched now (once)
pub async fn hpatchz(http: &reqwest::Client) -> Result<PathBuf> {
    if let Some(p) = crate::launch::which("hpatchz") {
        return Ok(p);
    }
    let dir = somelauncher_proto::paths::data_dir().join("tools").join(format!("hdiffpatch-{HDIFF_VERSION}"));
    let bin = dir.join("hpatchz");
    if bin.is_file() {
        return Ok(bin);
    }
    let bytes = http.get(HDIFF_URL).send().await?.error_for_status()?.bytes().await.context("downloading HDiffPatch")?;
    let sum = hex::encode(sha2::Sha256::digest(&bytes));
    if sum != HDIFF_SHA256 {
        bail!("HDiffPatch's download does not match its checksum");
    }
    let tmp = crate::jobs::TempDir(dir.with_extension("tmp"));
    std::fs::create_dir_all(&tmp.0)?;
    let zip = tmp.0.join("hdiffpatch.zip");
    std::fs::write(&zip, &bytes)?;
    let out = tokio::process::Command::new("bsdtar").arg("-xf").arg(&zip).arg("-C").arg(&tmp.0).output().await.context("running bsdtar")?;
    if !out.status.success() {
        bail!("unpacking HDiffPatch: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    let found = walk(&tmp.0).into_iter().find(|p| p.file_name().is_some_and(|n| n == "hpatchz")).context("no hpatchz in HDiffPatch's archive")?;
    std::fs::create_dir_all(&dir)?;
    std::fs::copy(&found, &bin)?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))?;
    tracing::info!("installed hpatchz {HDIFF_VERSION} at {}", bin.display());
    Ok(bin)
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() { out.extend(walk(&p)) } else { out.push(p) }
    }
    out
}

fn file_md5(path: &Path) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut h = Md5::new();
    let mut buf = vec![0u8; 4 << 20];
    loop {
        match std::io::Read::read(&mut f, &mut buf).ok()? {
            0 => break,
            n => h.update(&buf[..n]),
        }
    }
    Some(hex::encode(h.finalize()))
}

// --- diff updates and preloads

/// Where a preloaded patch waits for its update
fn preload_path(dir: &Path, to: &str, cat: &str, name: &str) -> PathBuf {
    dir.join(STATE_DIR).join(format!("preload-{to}")).join(cat).join(format!("{}.patch", md5_hex(name.as_bytes())))
}

fn log_set(path: &Path) -> (HashSet<String>, Arc<Mutex<std::fs::File>>) {
    let done = std::fs::read_to_string(path).map(|t| t.lines().map(String::from).collect()).unwrap_or_default();
    let file = std::fs::OpenOptions::new().create(true).append(true).open(path).expect("the log opens");
    (done, Arc::new(Mutex::new(file)))
}

async fn fetch_range(http: &reqwest::Client, url: &str, offset: u64, length: u64) -> Result<Vec<u8>> {
    let mut last = None;
    for attempt in 0..3u32 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(1 << attempt)).await;
        }
        let r = async {
            let resp = http.get(url).header("Range", format!("bytes={}-{}", offset, offset + length - 1)).send().await?.error_for_status()?;
            let bytes = resp.bytes().await?;
            if bytes.len() as u64 != length {
                bail!("got {} bytes of {}", bytes.len(), length);
            }
            Ok(bytes.to_vec())
        }
        .await;
        match r {
            Ok(b) => return Ok(b),
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap()).with_context(|| format!("downloading {url}"))
}

/// The patch manifests of these categories
pub async fn patch_manifests(http: &reqwest::Client, cats: &[PatchCategory]) -> Result<Vec<(PatchCategory, PatchManifest)>> {
    let mut out = Vec::new();
    for c in cats {
        out.push((c.clone(), patch_manifest(http, c).await.with_context(|| format!("the {} patch manifest", c.field))?));
    }
    Ok(out)
}

/// Fetches the patches from `from` (to `to`) into the preload folder, without touching the game
pub async fn preload(http: &reqwest::Client, sets: &[(PatchCategory, PatchManifest)], dir: &Path, from: &str, to: &str, progress: &Progress) -> Result<()> {
    let log_path = dir.join(STATE_DIR).join(format!("preload-{to}.log"));
    std::fs::create_dir_all(log_path.parent().unwrap())?;
    let (done, log) = log_set(&log_path);
    let jobs: Vec<(PatchCategory, PatchFile, Patch)> = sets
        .iter()
        .flat_map(|(c, m)| m.files.iter().filter_map(move |f| f.patches.get(from).map(|p| (c.clone(), f.clone(), p.clone()))))
        .collect();
    let total: u64 = jobs.iter().map(|j| j.2.length).sum();
    let counter = Arc::new(std::sync::atomic::AtomicU64::new(jobs.iter().filter(|j| done.contains(&j.1.name)).map(|j| j.2.length).sum()));
    progress.update(counter.load(std::sync::atomic::Ordering::Relaxed), total);
    let mut tasks = futures_util::stream::iter(jobs.into_iter().filter(|j| !done.contains(&j.1.name)))
        .map(|(c, f, p)| {
            let (http, dir, log, counter, progress, to) = (http.clone(), dir.to_path_buf(), log.clone(), counter.clone(), progress.clone(), to.to_string());
            async move {
                let bytes = fetch_range(&http, &format!("{}/{}", c.diff_url, p.bundle), p.offset, p.length).await?;
                let path = preload_path(&dir, &to, &c.field, &f.name);
                std::fs::create_dir_all(path.parent().unwrap())?;
                std::fs::write(&path, bytes)?;
                writeln!(log.lock().unwrap(), "{}", f.name)?;
                let now = counter.fetch_add(p.length, std::sync::atomic::Ordering::Relaxed) + p.length;
                progress.update(now, total);
                Ok::<_, anyhow::Error>(())
            }
        })
        .buffer_unordered(PARALLEL);
    while let Some(r) = tasks.next().await {
        r?;
    }
    Ok(())
}

/// Applies the patches from `from`: preloaded ones from disk, the rest fetched now
pub async fn apply_diff(http: &reqwest::Client, sets: &[(PatchCategory, PatchManifest)], dir: &Path, from: &str, to: &str, progress: &Progress) -> Result<()> {
    // Patches are HDiff data (also new files: a diff from nothing); fetched only when there are any
    let hpatchz = if sets.iter().any(|(_, m)| m.files.iter().any(|f| f.patches.contains_key(from))) { Some(hpatchz(http).await?) } else { None };
    let log_path = dir.join(STATE_DIR).join(format!("update-{from}-{to}.log"));
    std::fs::create_dir_all(log_path.parent().unwrap())?;
    let (done, log) = log_set(&log_path);
    let jobs: Vec<(PatchCategory, PatchFile, Patch)> = sets
        .iter()
        .flat_map(|(c, m)| m.files.iter().filter_map(move |f| f.patches.get(from).map(|p| (c.clone(), f.clone(), p.clone()))))
        .collect();
    let total: u64 = jobs.iter().map(|j| j.2.length).sum();
    let counter = Arc::new(std::sync::atomic::AtomicU64::new(jobs.iter().filter(|j| done.contains(&j.1.name)).map(|j| j.2.length).sum()));
    progress.update(counter.load(std::sync::atomic::Ordering::Relaxed), total);
    let mut tasks = futures_util::stream::iter(jobs.into_iter().filter(|j| !done.contains(&j.1.name)))
        .map(|(c, f, p)| {
            let (http, dir, log, counter, progress, to, hpatchz) =
                (http.clone(), dir.to_path_buf(), log.clone(), counter.clone(), progress.clone(), to.to_string(), hpatchz.clone());
            async move {
                let preloaded = preload_path(&dir, &to, &c.field, &f.name);
                let bytes = match std::fs::read(&preloaded) {
                    Ok(b) if b.len() as u64 == p.length => b,
                    _ => fetch_range(&http, &format!("{}/{}", c.diff_url, p.bundle), p.offset, p.length).await?,
                };
                let target = sophon::asset_path(&dir, &f.name)?;
                let staged = sophon::staged_path(&target);
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                if bytes.starts_with(HDIFF_MAGIC) {
                    // From the old file, or from nothing for a new one
                    let empty = staged.with_extension("empty");
                    let old = match &p.original {
                        Some(original) => sophon::asset_path(&dir, original)?,
                        None => {
                            std::fs::write(&empty, b"")?;
                            empty.clone()
                        }
                    };
                    let diff = staged.with_extension("hdiff");
                    std::fs::write(&diff, &bytes)?;
                    let out = tokio::process::Command::new(hpatchz.as_ref().unwrap()).arg("-f").arg(&old).arg(&diff).arg(&staged).output().await?;
                    let _ = std::fs::remove_file(&diff);
                    let _ = std::fs::remove_file(&empty);
                    if !out.status.success() {
                        bail!("patching {}: {}", f.name, String::from_utf8_lossy(&out.stdout).lines().last().unwrap_or_default());
                    }
                } else {
                    // The new file itself
                    std::fs::write(&staged, &bytes)?;
                }
                let (check, md5) = (staged.clone(), f.md5.clone());
                let ok = tokio::task::spawn_blocking(move || file_md5(&check).is_some_and(|m| m == md5)).await?;
                if !ok {
                    let _ = std::fs::remove_file(&staged);
                    bail!("{} does not match after patching", f.name);
                }
                std::fs::rename(&staged, &target)?;
                let _ = std::fs::remove_file(&preloaded);
                writeln!(log.lock().unwrap(), "{}", f.name)?;
                let now = counter.fetch_add(p.length, std::sync::atomic::Ordering::Relaxed) + p.length;
                progress.update(now, total);
                Ok::<_, anyhow::Error>(())
            }
        })
        .buffer_unordered(PARALLEL);
    while let Some(r) = tasks.next().await {
        r?;
    }
    // Files the new version no longer has (never one it still lists)
    let keep: HashSet<&str> = sets.iter().flat_map(|(_, m)| m.files.iter().map(|f| f.name.as_str())).collect();
    for (_, m) in sets {
        for name in m.deletes.get(from).into_iter().flatten().filter(|n| !keep.contains(n.as_str())) {
            if let Ok(p) = sophon::asset_path(dir, name) {
                let _ = std::fs::remove_file(p);
            }
        }
    }
    let _ = std::fs::remove_file(&log_path);
    let _ = std::fs::remove_dir_all(dir.join(STATE_DIR).join(format!("preload-{to}")));
    let _ = std::fs::remove_file(dir.join(STATE_DIR).join(format!("preload-{to}.log")));
    Ok(())
}

// --- full updates and repairs

/// A full update (no patches from `from`): files whose md5 changed between the
/// two manifests are fetched again, files the new one drops are deleted
pub async fn apply_full(http: &reqwest::Client, old: &[(Category, Vec<sophon::Asset>)], new: Vec<(Category, Vec<sophon::Asset>)>, dir: &Path, to: &str, progress: &Progress) -> Result<()> {
    let before: HashMap<&str, &str> = old.iter().flat_map(|(_, a)| a.iter().map(|a| (a.name.as_str(), a.md5.as_str()))).collect();
    let keep: HashSet<String> = new.iter().flat_map(|(_, a)| a.iter().map(|a| a.name.clone())).collect();
    let changed: Vec<(Category, Vec<sophon::Asset>)> = new
        .into_iter()
        .map(|(c, assets)| {
            let assets = assets
                .into_iter()
                .filter(|a| !a.dir && (before.get(a.name.as_str()) != Some(&a.md5.as_str()) || !dir.join(&a.name).is_file()))
                .collect();
            (c, assets)
        })
        .collect();
    sophon::fetch_files(http, changed, dir, &format!("full-{to}.log"), true, progress).await?;
    for (name, _) in before.iter().filter(|(n, _)| !keep.contains(**n)) {
        if let Ok(p) = sophon::asset_path(dir, name) {
            let _ = std::fs::remove_file(p);
        }
    }
    let _ = std::fs::remove_file(dir.join(STATE_DIR).join(format!("full-{to}.log")));
    Ok(())
}

/// Repair: every file checked against the manifest, the bad ones fetched again.
/// Two phases share one progress: checking (by size), then fetching.
pub async fn repair(http: &reqwest::Client, sets: Vec<(Category, Vec<sophon::Asset>)>, dir: &Path, progress: &Progress) -> Result<usize> {
    let mut bad_sets = Vec::new();
    let mut bad_count = 0;
    for (c, assets) in sets {
        let bad: HashSet<String> = sophon::verify(&assets, dir, progress).await?.into_iter().collect();
        bad_count += bad.len();
        bad_sets.push((c, assets.into_iter().filter(|a| bad.contains(&a.name)).collect::<Vec<_>>()));
    }
    if bad_count > 0 {
        tracing::info!("repair: {bad_count} files to fetch again");
        sophon::fetch_files(http, bad_sets, dir, "repair.log", true, progress).await?;
    }
    let _ = std::fs::remove_file(dir.join(STATE_DIR).join("repair.log"));
    Ok(bad_count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::Progress;

    fn varint(mut v: u64, out: &mut Vec<u8>) {
        loop {
            let b = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                out.push(b);
                return;
            }
            out.push(b | 0x80);
        }
    }
    fn bytes(field: u64, b: &[u8], out: &mut Vec<u8>) {
        varint(field << 3 | 2, out);
        varint(b.len() as u64, out);
        out.extend_from_slice(b);
    }
    fn int(field: u64, v: u64, out: &mut Vec<u8>) {
        varint(field << 3, out);
        varint(v, out);
    }

    #[test]
    fn decodes_a_patch_manifest() {
        let mut p = Vec::new();
        bytes(1, b"bundle1", &mut p);
        int(6, 600, &mut p);
        int(7, 1000, &mut p);
        bytes(8, b"old/name.blk", &mut p);
        let mut tagged = Vec::new();
        bytes(1, b"7.0.0", &mut tagged);
        bytes(2, &p, &mut tagged);
        let mut file = Vec::new();
        bytes(1, b"new/name.blk", &mut file);
        int(2, 5000, &mut file);
        bytes(3, b"abc", &mut file);
        bytes(4, &tagged, &mut file);
        let mut gone = Vec::new();
        bytes(1, b"gone.blk", &mut gone);
        let mut list = Vec::new();
        bytes(1, &gone, &mut list);
        let mut del = Vec::new();
        bytes(1, b"7.0.0", &mut del);
        bytes(2, &list, &mut del);
        let mut m = Vec::new();
        bytes(1, &file, &mut m);
        bytes(2, &del, &mut m);
        let m = decode_patch_manifest(&m).unwrap();
        let f = &m.files[0];
        assert_eq!((f.name.as_str(), f.size, f.md5.as_str()), ("new/name.blk", 5000, "abc"));
        assert_eq!(f.patches["7.0.0"], Patch { bundle: "bundle1".into(), offset: 600, length: 1000, original: Some("old/name.blk".into()) });
        assert_eq!(m.deletes["7.0.0"], vec!["gone.blk".to_string()]);
    }

    #[test]
    fn patch_builds_have_sizes_by_starting_version() {
        let data = serde_json::json!({"manifests": [{"matching_field": "ja-jp", "manifest": {"id": "m", "checksum": "c"},
            "manifest_download": {"url_prefix": "https://m"}, "diff_download": {"url_prefix": "https://d"},
            "stats": {"7.0.0": {"compressed_size": "582248522"}, "6.7.0": {"compressed_size": "1139409242"}}}]});
        let c = &parse_patch_build(&data)[0];
        assert_eq!((c.diff_url.as_str(), c.sizes["7.0.0"]), ("https://d", 582248522));
    }

    /// The real thing on small ZZZ categories: the previous version installed,
    /// then a preload + diff update, a full update, and a repair, all checked
    /// against the current manifests.
    /// `XDG_DATA_HOME=<tmp> scripts/dev.sh test -p somelauncherd update_real -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn update_real_diff_full_and_repair() {
        use crate::hoyo::api;
        let http = reqwest::Client::new();
        let g = api::games(&http, Region::Global, "en-us", true).await.unwrap().into_iter().find(|g| g.biz == "nap_global").unwrap();
        let main = g.main.unwrap();
        let from = main.diff_tags[0].clone();
        let old_branch = Branch { tag: from.clone(), ..main.clone() };
        let progress = Progress::detached();

        // Small categories that patch from the previous version, at least one with a real hdiff
        let mut patches: Vec<PatchCategory> = patch_build(&http, Region::Global, &main).await.unwrap().into_iter().filter(|c| c.sizes.get(&from).is_some_and(|s| *s > 0 && *s < 400_000_000)).collect();
        patches.sort_by_key(|c| c.sizes[&from]);
        let mut chosen = Vec::new();
        for c in patches {
            let m = patch_manifest(&http, &c).await.unwrap();
            let hdiff = m.files.iter().any(|f| f.patches.get(&from).is_some_and(|p| p.original.is_some()));
            if hdiff || chosen.is_empty() {
                chosen.push((c, m));
            }
            if chosen.len() == 2 && chosen.iter().any(|(_, m)| m.files.iter().any(|f| f.patches.get(&from).is_some_and(|p| p.original.is_some()))) {
                break;
            }
        }
        assert!(chosen.iter().any(|(_, m)| m.files.iter().any(|f| f.patches.get(&from).is_some_and(|p| p.original.is_some()))), "a patch against an old file is covered");
        let fields: Vec<String> = chosen.iter().map(|(c, _)| c.field.clone()).collect();
        let pick = |cats: Vec<Category>| cats.into_iter().filter(|c| fields.contains(&c.field)).collect::<Vec<_>>();
        let old = sophon::manifests(&http, &pick(sophon::build(&http, Region::Global, &old_branch).await.unwrap())).await.unwrap();
        let new = sophon::manifests(&http, &pick(sophon::build(&http, Region::Global, &main).await.unwrap())).await.unwrap();

        // Diff: preload, then the update
        let t = tempfile::tempdir().unwrap();
        sophon::fetch_files(&http, old.clone(), t.path(), "old.log", false, &progress).await.unwrap();
        preload(&http, &chosen, t.path(), &from, &main.tag, &progress).await.unwrap();
        apply_diff(&http, &chosen, t.path(), &from, &main.tag, &progress).await.unwrap();
        for (_, assets) in &new {
            assert!(sophon::verify(assets, t.path(), &progress).await.unwrap().is_empty(), "diff update matches {}", main.tag);
        }

        // Full: the same categories by manifest comparison
        let t2 = tempfile::tempdir().unwrap();
        sophon::fetch_files(&http, old.clone(), t2.path(), "old.log", false, &progress).await.unwrap();
        apply_full(&http, &old, new.clone(), t2.path(), &main.tag, &progress).await.unwrap();
        for (_, assets) in &new {
            assert!(sophon::verify(assets, t2.path(), &progress).await.unwrap().is_empty(), "full update matches {}", main.tag);
        }

        // Repair: a damaged file comes back
        let victim = new.iter().flat_map(|(_, a)| a.iter()).find(|a| !a.dir && a.size > 0).unwrap();
        std::fs::write(t2.path().join(&victim.name), b"broken").unwrap();
        assert_eq!(repair(&http, new.clone(), t2.path(), &progress).await.unwrap(), 1);
        for (_, assets) in &new {
            assert!(sophon::verify(assets, t2.path(), &progress).await.unwrap().is_empty(), "repaired");
        }
    }
}
