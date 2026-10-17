//! Sophon, HoYoPlay's chunked downloads (6c). `getBuild` lists a version's
//! manifests, one per category (the game, each voice pack); a manifest
//! (zstd-compressed protobuf) lists every file with its md5 and the chunks it
//! is made of; each chunk (zstd) is fetched, checked against its md5 and
//! written at its offset in the file. A log of finished chunks in the install
//! folder lets a stopped install go on where it was.

use super::api::{Branch, Region};
use crate::jobs::Progress;
use anyhow::{Context as _, Result, bail};
use futures_util::StreamExt;
use md5::{Digest, Md5};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;
use std::io::Write as _;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Chunks fetched at once
const PARALLEL: usize = 16;
const RETRIES: u32 = 3;
/// Where our bookkeeping lives inside an install
pub const STATE_DIR: &str = ".somelauncher";

/// One category of a build: the game, or a voice pack
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    /// `game`, or the voice pack's language (`en-us`)
    pub field: String,
    pub manifest: String,
    /// md5 of the decompressed manifest
    pub checksum: String,
    pub manifest_url: String,
    pub chunk_url: String,
    pub compressed: bool,
    /// What the download weighs and what it takes on disk
    pub download_size: u64,
    pub size: u64,
}

fn num(v: &Value) -> u64 {
    v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).unwrap_or(0)
}

pub fn parse_build(data: &Value) -> Vec<Category> {
    data["manifests"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| Category {
            field: m["matching_field"].as_str().unwrap_or_default().into(),
            manifest: m["manifest"]["id"].as_str().unwrap_or_default().into(),
            checksum: m["manifest"]["checksum"].as_str().unwrap_or_default().into(),
            manifest_url: m["manifest_download"]["url_prefix"].as_str().unwrap_or_default().into(),
            chunk_url: m["chunk_download"]["url_prefix"].as_str().unwrap_or_default().into(),
            compressed: m["chunk_download"]["compression"].as_i64() == Some(1),
            download_size: num(&m["stats"]["compressed_size"]),
            size: num(&m["stats"]["uncompressed_size"]),
        })
        .collect()
}

/// The categories of a branch's build (its current version, or `tag`)
pub async fn build(http: &reqwest::Client, region: Region, branch: &Branch) -> Result<Vec<Category>> {
    let url = format!("{}/getBuild", region.sophon());
    let q = [("branch", branch.branch.as_str()), ("package_id", &branch.package_id), ("password", &branch.password), ("tag", &branch.tag)];
    let v: Value = http.get(&url).query(&q).send().await?.error_for_status()?.json().await.context("Sophon getBuild")?;
    if v["retcode"].as_i64() != Some(0) {
        bail!("Sophon getBuild: {}", v["message"].as_str().unwrap_or("failed"));
    }
    Ok(parse_build(&v["data"]))
}

// --- manifests

#[derive(Debug, Clone, PartialEq)]
pub struct Asset {
    pub name: String,
    pub chunks: Vec<Chunk>,
    pub dir: bool,
    pub size: u64,
    pub md5: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    pub name: String,
    /// md5 of the decompressed chunk
    pub md5: String,
    pub offset: u64,
    pub size: u64,
    pub size_decompressed: u64,
}

/// A tiny protobuf reader: enough for the manifest's three messages
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn varint(&mut self) -> Result<u64> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let (&b, rest) = self.0.split_first().context("truncated varint")?;
            self.0 = rest;
            v |= u64::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
        bail!("varint too long")
    }
    /// (field, wire type 0 → value, wire type 2 → bytes)
    fn next(&mut self) -> Result<Option<(u64, Field<'a>)>> {
        if self.0.is_empty() {
            return Ok(None);
        }
        let key = self.varint()?;
        let field = match key & 7 {
            0 => Field::Int(self.varint()?),
            1 => {
                let (b, rest) = self.0.split_at_checked(8).context("truncated fixed64")?;
                self.0 = rest;
                Field::Int(u64::from_le_bytes(b.try_into().unwrap()))
            }
            2 => {
                let len = self.varint()? as usize;
                let (b, rest) = self.0.split_at_checked(len).context("truncated field")?;
                self.0 = rest;
                Field::Bytes(b)
            }
            5 => {
                let (b, rest) = self.0.split_at_checked(4).context("truncated fixed32")?;
                self.0 = rest;
                Field::Int(u32::from_le_bytes(b.try_into().unwrap()).into())
            }
            t => bail!("protobuf wire type {t}"),
        };
        Ok(Some((key >> 3, field)))
    }
}

enum Field<'a> {
    Int(u64),
    Bytes(&'a [u8]),
}

impl Field<'_> {
    fn text(&self) -> String {
        match self {
            Field::Bytes(b) => String::from_utf8_lossy(b).into_owned(),
            Field::Int(_) => String::new(),
        }
    }
    fn int(&self) -> u64 {
        match self {
            Field::Int(v) => *v,
            Field::Bytes(_) => 0,
        }
    }
}

fn chunk(bytes: &[u8]) -> Result<Chunk> {
    let mut c = Chunk { name: String::new(), md5: String::new(), offset: 0, size: 0, size_decompressed: 0 };
    let mut r = Reader(bytes);
    while let Some((n, f)) = r.next()? {
        match n {
            1 => c.name = f.text(),
            2 => c.md5 = f.text(),
            3 => c.offset = f.int(),
            4 => c.size = f.int(),
            5 => c.size_decompressed = f.int(),
            _ => {}
        }
    }
    Ok(c)
}

fn asset(bytes: &[u8]) -> Result<Asset> {
    let mut a = Asset { name: String::new(), chunks: Vec::new(), dir: false, size: 0, md5: String::new() };
    let mut r = Reader(bytes);
    while let Some((n, f)) = r.next()? {
        match (n, &f) {
            (1, _) => a.name = f.text(),
            (2, Field::Bytes(b)) => a.chunks.push(chunk(b)?),
            // 64: a directory
            (3, _) => a.dir = f.int() == 64,
            (4, _) => a.size = f.int(),
            (5, _) => a.md5 = f.text(),
            _ => {}
        }
    }
    Ok(a)
}

pub fn decode(bytes: &[u8]) -> Result<Vec<Asset>> {
    let mut out = Vec::new();
    let mut r = Reader(bytes);
    while let Some((n, f)) = r.next()? {
        if let (1, Field::Bytes(b)) = (n, f) {
            out.push(asset(b)?);
        }
    }
    Ok(out)
}

pub fn md5_hex(bytes: &[u8]) -> String {
    hex::encode(Md5::digest(bytes))
}

/// A category's manifest, checked against its md5
pub async fn manifest(http: &reqwest::Client, cat: &Category) -> Result<Vec<Asset>> {
    let url = format!("{}/{}", cat.manifest_url, cat.manifest);
    let raw = http.get(&url).send().await?.error_for_status()?.bytes().await.context("downloading the manifest")?;
    let checksum = cat.checksum.clone();
    tokio::task::spawn_blocking(move || {
        let bytes = zstd::stream::decode_all(&raw[..]).context("unpacking the manifest")?;
        if !checksum.is_empty() && md5_hex(&bytes) != checksum {
            bail!("the manifest is damaged");
        }
        decode(&bytes)
    })
    .await?
}

// --- installing

/// Free space on the disk holding `dir` (or its nearest existing parent)
pub fn free_space(dir: &Path) -> Option<u64> {
    let mut p = dir;
    while !p.exists() {
        p = p.parent()?;
    }
    let c = std::ffi::CString::new(p.as_os_str().as_encoded_bytes()).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    (unsafe { libc::statvfs(c.as_ptr(), &mut s) } == 0).then(|| s.f_bavail as u64 * s.f_frsize as u64)
}

/// What is left to fetch: (asset index, chunk index) pairs, and their bytes
struct Work {
    files: Vec<Asset>,
    todo: Vec<(usize, usize)>,
    done_bytes: u64,
    total_bytes: u64,
}

fn done_key(asset: &Asset, chunk: &Chunk) -> String {
    format!("{}\t{}", asset.name, chunk.offset)
}

/// The finished chunks of an install so far (its log)
fn read_log(path: &Path) -> HashSet<String> {
    std::fs::read_to_string(path).map(|t| t.lines().map(String::from).collect()).unwrap_or_default()
}

fn plan_work(assets: Vec<Asset>, done: &HashSet<String>) -> Work {
    let files: Vec<Asset> = assets.into_iter().filter(|a| !a.dir).collect();
    let mut todo = Vec::new();
    let (mut done_bytes, mut total_bytes) = (0, 0);
    for (ai, a) in files.iter().enumerate() {
        for (ci, c) in a.chunks.iter().enumerate() {
            total_bytes += c.size;
            if done.contains(&done_key(a, c)) {
                done_bytes += c.size;
            } else {
                todo.push((ai, ci));
            }
        }
    }
    Work { files, todo, done_bytes, total_bytes }
}

/// The path of an asset inside `dir`; names never leave it
pub fn asset_path(dir: &Path, name: &str) -> Result<PathBuf> {
    let rel = Path::new(name);
    if rel.is_absolute() || rel.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        bail!("refusing the asset path {name}");
    }
    Ok(dir.join(rel))
}

async fn fetch_chunk(http: &reqwest::Client, cat: &Category, c: &Chunk) -> Result<Vec<u8>> {
    let url = format!("{}/{}", cat.chunk_url, c.name);
    let mut last = None;
    for attempt in 0..RETRIES {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(1 << attempt)).await;
        }
        let got = async { Ok::<_, anyhow::Error>(http.get(&url).send().await?.error_for_status()?.bytes().await?) }.await;
        match got {
            Ok(raw) => {
                let (compressed, expect, size) = (cat.compressed, c.md5.clone(), c.size_decompressed as usize);
                let data = tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
                    let data = if compressed { zstd::bulk::decompress(&raw, size).context("unpacking a chunk")? } else { raw.to_vec() };
                    if md5_hex(&data) != expect {
                        bail!("chunk damaged");
                    }
                    Ok(data)
                })
                .await?;
                match data {
                    Ok(d) => return Ok(d),
                    Err(e) => last = Some(e),
                }
            }
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap()).with_context(|| format!("chunk {}", c.name))
}

/// Downloads every file of `categories` into `dir`, resuming from the log in
/// `dir/.somelauncher/<log>`. Files are made at their full size and filled in
/// place. Progress is in downloaded bytes, over all categories together.
pub async fn install(http: &reqwest::Client, categories: &[Category], dir: &Path, log: &str, progress: &Progress) -> Result<()> {
    let state = dir.join(STATE_DIR);
    std::fs::create_dir_all(&state).with_context(|| format!("creating {}", state.display()))?;
    let log_path = state.join(log);
    let done = read_log(&log_path);

    // ZZZ has over a hundred categories: their manifests come a few at a time
    let manifests: Vec<Result<Vec<Asset>>> = futures_util::stream::iter(categories.to_vec())
        .map(|cat| {
            let http = http.clone();
            async move { manifest(&http, &cat).await.with_context(|| format!("the {} manifest", cat.field)) }
        })
        .buffered(8)
        .collect()
        .await;
    let mut works = Vec::new();
    for (cat, assets) in categories.iter().zip(manifests) {
        works.push((cat.clone(), plan_work(assets?, &done)));
    }
    let total: u64 = works.iter().map(|(_, w)| w.total_bytes).sum();
    let need: u64 = works.iter().map(|(_, w)| w.files.iter().map(|a| a.size).sum::<u64>()).sum();
    let have = works.iter().map(|(_, w)| w.done_bytes).sum::<u64>();
    if have == 0
        && let Some(free) = free_space(dir)
        && free < need
    {
        bail!("not enough space: {} GB needed, {} GB free", need / 1_000_000_000 + 1, free / 1_000_000_000);
    }

    let counter = Arc::new(std::sync::atomic::AtomicU64::new(have));
    progress.update(have, total);
    let log_file = Arc::new(Mutex::new(std::fs::OpenOptions::new().create(true).append(true).open(&log_path)?));
    for (cat, work) in works {
        // Every file at its size first (and its folder), so chunks can land anywhere in it
        for a in &work.files {
            let path = asset_path(dir, &a.name)?;
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let f = std::fs::OpenOptions::new().create(true).write(true).truncate(false).open(&path)?;
            if f.metadata()?.len() != a.size {
                f.set_len(a.size)?;
            }
        }
        let work = Arc::new(work);
        let cat = Arc::new(cat);
        let mut tasks = futures_util::stream::iter(work.todo.clone())
            .map(|(ai, ci)| {
                let (work, cat, http, dir) = (work.clone(), cat.clone(), http.clone(), dir.to_path_buf());
                let (log_file, counter, progress) = (log_file.clone(), counter.clone(), progress.clone());
                async move {
                    let a = &work.files[ai];
                    let c = &a.chunks[ci];
                    let data = fetch_chunk(&http, &cat, c).await?;
                    let path = asset_path(&dir, &a.name)?;
                    let offset = c.offset;
                    tokio::task::spawn_blocking(move || -> Result<()> {
                        std::fs::OpenOptions::new().write(true).open(&path)?.write_all_at(&data, offset)?;
                        Ok(())
                    })
                    .await??;
                    writeln!(log_file.lock().unwrap(), "{}", done_key(a, c))?;
                    let now = counter.fetch_add(c.size, std::sync::atomic::Ordering::Relaxed) + c.size;
                    progress.update(now, total);
                    Ok::<_, anyhow::Error>(())
                }
            })
            .buffer_unordered(PARALLEL);
        while let Some(r) = tasks.next().await {
            r?;
        }
    }
    Ok(())
}

/// Checks every file of a manifest against its size and md5 (repair, 6d):
/// the names of the ones that do not match
#[allow(dead_code)] // repair (6d)
pub async fn verify(assets: &[Asset], dir: &Path, progress: &Progress) -> Result<Vec<String>> {
    let total: u64 = assets.iter().filter(|a| !a.dir).map(|a| a.size).sum();
    let mut done = 0u64;
    let mut bad = Vec::new();
    for a in assets.iter().filter(|a| !a.dir) {
        let path = asset_path(dir, &a.name)?;
        let (expect, size) = (a.md5.clone(), a.size);
        let ok = tokio::task::spawn_blocking(move || -> bool {
            let Ok(mut f) = std::fs::File::open(&path) else { return false };
            if f.metadata().map(|m| m.len()).unwrap_or(0) != size {
                return false;
            }
            let mut h = Md5::new();
            let mut buf = vec![0u8; 4 << 20];
            loop {
                match std::io::Read::read(&mut f, &mut buf) {
                    Ok(0) => break,
                    Ok(n) => h.update(&buf[..n]),
                    Err(_) => return false,
                }
            }
            hex::encode(h.finalize()) == expect
        })
        .await?;
        if !ok {
            bad.push(a.name.clone());
        }
        done += a.size;
        progress.update(done, total);
    }
    Ok(bad)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn decodes_a_manifest() {
        let mut c = Vec::new();
        bytes(1, b"abc_123", &mut c);
        bytes(2, b"d41d8cd98f00b204e9800998ecf8427e", &mut c);
        int(3, 1050297, &mut c);
        int(4, 10, &mut c);
        int(5, 20, &mut c);
        int(6, 1u64 << 62, &mut c);
        let mut a = Vec::new();
        bytes(1, b"GenshinImpact_Data/x.pck", &mut a);
        bytes(2, &c, &mut a);
        int(4, 1050317, &mut a);
        bytes(5, b"ffff", &mut a);
        let mut d = Vec::new();
        bytes(1, b"Data/Folder", &mut d);
        int(3, 64, &mut d);
        let mut m = Vec::new();
        bytes(1, &a, &mut m);
        bytes(1, &d, &mut m);
        let assets = decode(&m).unwrap();
        assert_eq!(assets.len(), 2);
        assert_eq!((assets[0].name.as_str(), assets[0].size, assets[0].dir), ("GenshinImpact_Data/x.pck", 1050317, false));
        assert_eq!(assets[0].chunks[0], Chunk { name: "abc_123".into(), md5: "d41d8cd98f00b204e9800998ecf8427e".into(), offset: 1050297, size: 10, size_decompressed: 20 });
        assert!(assets[1].dir);
        assert!(decode(&[0x0a, 0x05, 1]).is_err(), "truncated");
    }

    #[test]
    fn resumes_from_the_log_and_refuses_escaping_paths() {
        let chunk = |offset| Chunk { name: "c".into(), md5: String::new(), offset, size: 5, size_decompressed: 5 };
        let a = Asset { name: "f".into(), chunks: vec![chunk(0), chunk(5)], dir: false, size: 10, md5: String::new() };
        let done: HashSet<String> = [done_key(&a, &a.chunks[0])].into();
        let w = plan_work(vec![a], &done);
        assert_eq!((w.todo, w.done_bytes, w.total_bytes), (vec![(0, 1)], 5, 10));
        assert!(asset_path(Path::new("/g"), "../etc/passwd").is_err());
        assert!(asset_path(Path::new("/g"), "/etc/passwd").is_err());
        assert_eq!(asset_path(Path::new("/g"), "a/b").unwrap(), PathBuf::from("/g/a/b"));
    }

    /// The real thing, on two small ZZZ categories: `scripts/dev.sh test -p somelauncherd sophon_real -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn sophon_real_install_resume_and_verify() {
        let http = reqwest::Client::new();
        let games = crate::hoyo::api::games(&http, Region::Global, "en-us", true).await.unwrap();
        let branch = games.into_iter().find(|g| g.biz == "nap_global").unwrap().main.unwrap();
        // SOPHON_TEST_MAX: the largest category to take, in bytes; the two largest under it are used
        let max: u64 = std::env::var("SOPHON_TEST_MAX").ok().and_then(|v| v.parse().ok()).unwrap_or(3_000_000);
        let mut cats: Vec<Category> = build(&http, Region::Global, &branch).await.unwrap().into_iter().filter(|c| c.download_size > 0 && c.download_size < max).collect();
        cats.sort_by_key(|c| std::cmp::Reverse(c.download_size));
        cats.truncate(2);
        assert_eq!(cats.len(), 2, "two small categories");
        let t = tempfile::tempdir().unwrap();
        let progress = Progress::detached();
        // A first run that is cut short after one category, then the whole: the second resumes
        install(&http, &cats[..1], t.path(), "test.log", &progress).await.unwrap();
        let logged = read_log(&t.path().join(STATE_DIR).join("test.log")).len();
        assert!(logged > 0);
        install(&http, &cats, t.path(), "test.log", &progress).await.unwrap();
        for cat in &cats {
            let assets = manifest(&http, cat).await.unwrap();
            assert!(verify(&assets, t.path(), &progress).await.unwrap().is_empty(), "every file matches its md5");
        }
    }

    #[test]
    fn builds_list_their_categories() {
        let data = serde_json::json!({"manifests": [{"matching_field": "game", "manifest": {"id": "m", "checksum": "x"},
            "chunk_download": {"url_prefix": "https://c", "compression": 1}, "manifest_download": {"url_prefix": "https://m"},
            "stats": {"compressed_size": "100", "uncompressed_size": "120"}}]});
        let c = &parse_build(&data)[0];
        assert_eq!((c.field.as_str(), c.compressed, c.download_size, c.size), ("game", true, 100, 120));
    }
}
