//! Background jobs (downloads, installs): they outlive the request that
//! started them and every client hears about them through events:
//! `download.progress {id, label, done, total}` (a few per second at most) and
//! `download.done {id, label, error?}`.

use crate::rpc::State;
use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha512};
use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobInfo {
    pub id: String,
    pub label: String,
    pub done: u64,
    pub total: u64,
}

#[derive(Default)]
pub struct Jobs {
    running: Mutex<HashMap<String, (JobInfo, tokio::task::AbortHandle)>>,
}

impl Jobs {
    pub fn list(&self) -> Vec<JobInfo> {
        let mut v: Vec<_> = self.running.lock().unwrap().values().map(|(i, _)| i.clone()).collect();
        v.sort_by(|a, b| a.id.cmp(&b.id));
        v
    }
    /// Aborts a job; its temporary files go with it (see `TempDir`).
    pub fn cancel(&self, id: &str) -> bool {
        match self.running.lock().unwrap().remove(id) {
            Some((_, handle)) => {
                handle.abort();
                true
            }
            None => false,
        }
    }
}

/// What a running job reports progress through.
#[derive(Clone)]
pub struct Progress {
    state: Weak<State>,
    id: String,
    label: String,
    last: Arc<Mutex<Option<Instant>>>,
}

impl Progress {
    pub fn update(&self, done: u64, total: u64) {
        let Some(state) = self.state.upgrade() else { return };
        if let Some((info, _)) = state.jobs.running.lock().unwrap().get_mut(&self.id) {
            (info.done, info.total) = (done, total);
        }
        let mut last = self.last.lock().unwrap();
        if last.is_some_and(|t| t.elapsed() < Duration::from_millis(250)) && done < total {
            return;
        }
        *last = Some(Instant::now());
        state.emit("download.progress", json!({ "id": self.id, "label": self.label, "done": done, "total": total }));
    }
}

/// Starts `job` in the background unless a job with this id already runs.
pub fn spawn<F, Fut>(state: &Arc<State>, id: &str, label: &str, job: F) -> Result<()>
where
    F: FnOnce(Progress) -> Fut,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    let mut running = state.jobs.running.lock().unwrap();
    if running.contains_key(id) {
        bail!("{label} is already running");
    }
    let progress = Progress { state: Arc::downgrade(state), id: id.into(), label: label.into(), last: Default::default() };
    let fut = job(progress);
    let (weak, id_owned, label_owned) = (Arc::downgrade(state), id.to_string(), label.to_string());
    let task = tokio::spawn(async move {
        let result = fut.await;
        let Some(state) = weak.upgrade() else { return };
        state.jobs.running.lock().unwrap().remove(&id_owned);
        match result {
            Ok(()) => {
                tracing::info!(id = id_owned, "{label_owned} done");
                state.emit("download.done", json!({ "id": id_owned, "label": label_owned }));
            }
            Err(e) => {
                tracing::warn!(id = id_owned, "{label_owned} failed: {e:#}");
                state.emit("download.done", json!({ "id": id_owned, "label": label_owned, "error": format!("{e:#}") }));
            }
        }
    });
    let info = JobInfo { id: id.into(), label: label.into(), done: 0, total: 0 };
    running.insert(id.into(), (info, task.abort_handle()));
    Ok(())
}

/// A directory removed when dropped, also when the job is aborted.
pub struct TempDir(pub PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Streams `url` into `dest`, reporting progress.
pub async fn fetch(http: &reqwest::Client, url: &str, dest: &Path, progress: &Progress) -> Result<()> {
    let mut resp = http.get(url).send().await?.error_for_status().with_context(|| format!("downloading {url}"))?;
    let total = resp.content_length().unwrap_or(0);
    let mut file = tokio::fs::File::create(dest).await.with_context(|| format!("creating {}", dest.display()))?;
    let mut done = 0u64;
    while let Some(chunk) = resp.chunk().await? {
        file.write_all(&chunk).await?;
        done += chunk.len() as u64;
        progress.update(done, total.max(done));
    }
    file.flush().await?;
    Ok(())
}

/// Checks a file against a `<sha512 hex>  <name>` checksum line.
pub async fn verify_sha512(path: &Path, checksum_line: &str) -> Result<()> {
    let expected = checksum_line.split_whitespace().next().unwrap_or_default().to_lowercase();
    let path = path.to_path_buf();
    let actual = tokio::task::spawn_blocking(move || -> Result<String> {
        let mut f = std::fs::File::open(&path)?;
        let mut h = Sha512::new();
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = std::io::Read::read(&mut f, &mut buf)?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
        }
        Ok(hex::encode(h.finalize()))
    })
    .await??;
    if actual != expected {
        bail!("checksum mismatch: the download is damaged");
    }
    Ok(())
}

/// Unpacks a tarball (any compression `tar` knows) into `into`.
pub async fn untar(archive: &Path, into: &Path) -> Result<()> {
    tokio::fs::create_dir_all(into).await?;
    let out = tokio::process::Command::new("tar").arg("-xf").arg(archive).arg("-C").arg(into).output().await.context("running tar")?;
    if !out.status.success() {
        bail!("tar failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}
