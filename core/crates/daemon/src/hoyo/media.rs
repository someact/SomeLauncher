//! The Online page's big media (6g): HoYoPlay's backgrounds, their webm loops
//! and theme pictures, kept on disk so a loop is downloaded once, not on every
//! visit. Only HoYoverse's and miHoYo's hosts; files unused for a month go.

use anyhow::{Context as _, Result, bail};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const KEEP: Duration = Duration::from_secs(30 * 86400);

pub fn dir() -> PathBuf {
    somelauncher_proto::paths::cache_dir().join("hoyo")
}

/// Where a URL is kept: a hash of it, with its extension
pub fn file_for(dir: &Path, url: &str) -> Result<PathBuf> {
    let parsed = reqwest::Url::parse(url).context("not a URL")?;
    let host = parsed.host_str().unwrap_or_default();
    let ours = |d: &str| host == d || host.ends_with(&format!(".{d}"));
    if parsed.scheme() != "https" || !(ours("hoyoverse.com") || ours("mihoyo.com")) {
        bail!("not HoYoPlay media: {url}");
    }
    let ext = Path::new(parsed.path()).extension().and_then(|e| e.to_str()).filter(|e| e.len() <= 5 && e.chars().all(|c| c.is_ascii_alphanumeric())).unwrap_or("bin");
    let hash = hex::encode(&Sha256::digest(url.as_bytes())[..8]);
    Ok(dir.join(format!("{hash}.{ext}")))
}

/// The local copy of `url`, fetched when missing
pub async fn get(http: &reqwest::Client, url: &str) -> Result<PathBuf> {
    let dir = dir();
    let file = file_for(&dir, url)?;
    if file.is_file() {
        // Used: kept another month
        if let Ok(f) = std::fs::File::options().append(true).open(&file) {
            let _ = f.set_modified(SystemTime::now());
        }
        return Ok(file);
    }
    tokio::fs::create_dir_all(&dir).await?;
    // A loop is 10-20 MB: more time than the client's default
    let mut resp = http.get(url).timeout(Duration::from_secs(600)).send().await?.error_for_status().with_context(|| format!("downloading {url}"))?;
    let part = file.with_extension(format!("part{}", std::process::id()));
    let mut out = tokio::fs::File::create(&part).await?;
    use tokio::io::AsyncWriteExt as _;
    let fetched: Result<()> = async {
        while let Some(chunk) = resp.chunk().await? {
            out.write_all(&chunk).await?;
        }
        out.flush().await?;
        Ok(())
    }
    .await;
    if let Err(e) = fetched {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(e);
    }
    tokio::fs::rename(&part, &file).await?;
    prune(&dir, KEEP);
    Ok(file)
}

/// Removes what was not used for `keep`
pub fn prune(dir: &Path, keep: Duration) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > keep);
        if old {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_hoyo_hosts() {
        let d = Path::new("/c");
        let f = file_for(d, "https://fastcdn.hoyoverse.com/static-resource-v2/x/loop.webm").unwrap();
        assert_eq!(f.extension().unwrap(), "webm");
        assert!(file_for(d, "https://launcher-webstatic.hoyoverse.com/a.webp").is_ok());
        assert!(file_for(d, "https://hyp-static.mihoyo.com/a.webp").is_ok());
        assert!(file_for(d, "https://evilhoyoverse.com/a.webp").is_err());
        assert!(file_for(d, "http://fastcdn.hoyoverse.com/a.webp").is_err());
        assert!(file_for(d, "file:///etc/passwd").is_err());
        assert_ne!(file_for(d, "https://a.hoyoverse.com/1.png").unwrap(), file_for(d, "https://a.hoyoverse.com/2.png").unwrap());
    }

    #[test]
    fn prune_keeps_recent_files() {
        let d = tempfile::tempdir().unwrap();
        let old = d.path().join("old.webm");
        std::fs::write(&old, b"x").unwrap();
        std::fs::File::options().append(true).open(&old).unwrap().set_modified(SystemTime::now() - Duration::from_secs(40 * 86400)).unwrap();
        std::fs::write(d.path().join("new.webm"), b"x").unwrap();
        prune(d.path(), KEEP);
        assert!(!old.exists());
        assert!(d.path().join("new.webm").exists());
    }
}
