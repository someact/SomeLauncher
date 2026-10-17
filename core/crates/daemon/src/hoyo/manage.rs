//! Keeping an installed Hoyo game current (6d): what an update takes
//! (`hoyo.updatePlan`), the update itself, preloading the next version, and
//! repair. Each is a background job on the game (`hoyo-<id>`, one at a time).

use super::api::{self, Branch, HoyoGame};
use super::{files, name_of, sophon, update};
use crate::rpc::State;
use anyhow::{Context as _, Result};
use serde_json::{Value, json};
use somelauncher_proto::Error;
use somelauncher_proto::game::{Game, SourceKind};
use std::path::PathBuf;
use std::sync::Arc;

/// An installed Hoyo game, as these jobs need it
pub struct Installed {
    pub game: Game,
    pub biz: String,
    pub dir: PathBuf,
    pub version: String,
    pub voices: Vec<String>,
}

/// The categories of a branch this install has: every resource, its voice packs
fn wanted(branch: &Branch, voices: &[String]) -> impl Fn(&str) -> bool {
    let (res, voices) = (branch.resources.clone(), voices.to_vec());
    move |field| res.iter().any(|r| r == field) || voices.iter().any(|v| v == field)
}

fn err(e: anyhow::Error) -> Error {
    Error::new("network", format!("{e:#}"))
}

impl State {
    fn installed(&self, id: i64) -> Result<Installed, Error> {
        let game = self.game(&self.db.lock().unwrap(), id)?;
        let src = game.sources.iter().find(|s| s.source == SourceKind::Hoyo).cloned();
        let Some(src) = src.filter(|s| s.data["installing"].is_null()) else {
            return Err(Error::new("not_found", format!("{} is not an installed Hoyo game (adopt or install it)", game.name)));
        };
        let dir = game.install_dir.clone().ok_or_else(|| Error::new("not_found", "no install folder"))?;
        let version = src.data["version"].as_str().map(String::from).ok_or_else(|| Error::new("not_found", format!("the version of {} is unknown", game.name)))?;
        let voices = serde_json::from_value(src.data["voices"].clone()).unwrap_or_default();
        let biz = src.data["biz"].as_str().map(String::from).ok_or_else(|| Error::new("not_found", "no biz recorded"))?;
        Ok(Installed { biz, dir, version, voices, game })
    }

    async fn hoyo_api_game(&self, biz: &str, force: bool) -> Result<HoyoGame, Error> {
        api::games(&self.http, api::Region::of(biz), &self.hoyo_language(), force)
            .await
            .map_err(err)?
            .into_iter()
            .find(|g| g.biz == biz)
            .ok_or_else(|| Error::new("not_found", format!("HoYoPlay has no game {biz}")))
    }

    /// `hoyo.updatePlan {id}`: how far behind the game is, and what catching up and preloading take
    pub(crate) async fn hoyo_update_plan(&self, params: &Value) -> Result<Value, Error> {
        let id = params["id"].as_i64().ok_or_else(|| Error::new("bad_params", "missing integer `id`"))?;
        let ins = self.installed(id)?;
        let g = self.hoyo_api_game(&ins.biz, params["force"].as_bool().unwrap_or(false)).await?;
        let main = g.main.clone().ok_or_else(|| Error::new("not_found", "HoYoPlay lists no version"))?;
        let mut out = json!({ "id": id, "installed": ins.version, "latest": main.tag, "upToDate": ins.version == main.tag });
        if ins.version != main.tag {
            out["update"] = self.update_size(&g, &main, &ins).await?;
        }
        if let Some(pre) = &g.preload {
            let mut p = self.update_size(&g, pre, &ins).await?;
            p["version"] = json!(pre.tag);
            p["done"] = json!(ins.dir.join(sophon::STATE_DIR).join(format!("preload-{}.done", pre.tag)).is_file());
            out["preload"] = p;
        }
        Ok(out)
    }

    /// `{method, downloadSize}`: a diff when every category has patches from the
    /// installed version, else a full update (its size is unknown until compared)
    async fn update_size(&self, g: &HoyoGame, to: &Branch, ins: &Installed) -> Result<Value, Error> {
        let cats = update::patch_build(&self.http, g.region, to).await.map_err(err)?;
        let want = wanted(to, &ins.voices);
        let mine: Vec<_> = cats.iter().filter(|c| want(&c.field)).collect();
        let sizes: Option<Vec<u64>> = mine.iter().map(|c| c.sizes.get(&ins.version).copied()).collect();
        Ok(match sizes {
            Some(s) if !mine.is_empty() => json!({ "method": "diff", "downloadSize": s.iter().sum::<u64>() }),
            _ => json!({ "method": "full", "downloadSize": null }),
        })
    }

    fn start_job<F, Fut>(self: &Arc<Self>, id: i64, label: String, job: F) -> Result<Value, Error>
    where
        F: FnOnce(crate::jobs::Progress) -> Fut,
        Fut: std::future::Future<Output = Result<()>> + Send + 'static,
    {
        let job_id = format!("hoyo-{id}");
        crate::jobs::spawn(self, &job_id, &label, job).map_err(|e| Error::new("busy", format!("{e:#}")))?;
        Ok(json!({ "id": job_id }))
    }

    /// After a job changed the files: the version and voices read again
    fn readopt(&self, id: i64, dir: &std::path::Path) -> Result<()> {
        let install = files::detect(dir).context("the game is not in its folder any more")?;
        self.adopt(&install, Some(id))?;
        self.emit("library.changed", json!({ "ids": [id] }));
        Ok(())
    }

    /// `hoyo.update {id}`: to the latest version, a diff when possible
    pub(crate) async fn hoyo_update(self: &Arc<Self>, params: &Value) -> Result<Value, Error> {
        let id = params["id"].as_i64().ok_or_else(|| Error::new("bad_params", "missing integer `id`"))?;
        let ins = self.installed(id)?;
        if self.running.lock().unwrap().contains_key(&id) {
            return Err(Error::new("busy", format!("{} is running", ins.game.name)));
        }
        let g = self.hoyo_api_game(&ins.biz, true).await?;
        let main = g.main.clone().ok_or_else(|| Error::new("not_found", "HoYoPlay lists no version"))?;
        if ins.version == main.tag {
            return Err(Error::new("bad_params", format!("{} is up to date ({})", ins.game.name, main.tag)));
        }
        let label = format!("Updating {} to {}", name_of(&ins.biz), main.tag);
        let (state, http) = (self.clone(), self.http.clone());
        self.start_job(id, label, move |progress| async move {
            let want = wanted(&main, &ins.voices);
            let cats: Vec<_> = update::patch_build(&http, g.region, &main).await?.into_iter().filter(|c| want(&c.field)).collect();
            let diff = !cats.is_empty() && cats.iter().all(|c| c.sizes.contains_key(&ins.version));
            if diff {
                let sets = update::patch_manifests(&http, &cats).await?;
                update::apply_diff(&http, &sets, &ins.dir, &ins.version, &main.tag, &progress).await?;
            } else {
                tracing::info!(id, "no patches from {}: a full update", ins.version);
                let old_branch = Branch { tag: ins.version.clone(), ..main.clone() };
                let pick = |cats: Vec<sophon::Category>| cats.into_iter().filter(|c| want(&c.field)).collect::<Vec<_>>();
                let old = sophon::manifests(&http, &pick(sophon::build(&http, g.region, &old_branch).await.context("the installed version's build")?)).await?;
                let new = sophon::manifests(&http, &pick(sophon::build(&http, g.region, &main).await?)).await?;
                update::apply_full(&http, &old, new, &ins.dir, &main.tag, &progress).await?;
            }
            super::install::finish(&ins.dir, &ins.biz, &main.tag, &ins.voices)?;
            state.readopt(id, &ins.dir)
        })
    }

    /// `hoyo.preload {id}`: the next version's patches, fetched now and applied by its update
    pub(crate) async fn hoyo_preload(self: &Arc<Self>, params: &Value) -> Result<Value, Error> {
        let id = params["id"].as_i64().ok_or_else(|| Error::new("bad_params", "missing integer `id`"))?;
        let ins = self.installed(id)?;
        let g = self.hoyo_api_game(&ins.biz, true).await?;
        let pre = g.preload.clone().ok_or_else(|| Error::new("not_found", format!("{} has nothing to preload", ins.game.name)))?;
        let label = format!("Preloading {} {}", name_of(&ins.biz), pre.tag);
        let http = self.http.clone();
        self.start_job(id, label, move |progress| async move {
            let want = wanted(&pre, &ins.voices);
            let cats: Vec<_> = update::patch_build(&http, g.region, &pre).await?.into_iter().filter(|c| want(&c.field)).collect();
            anyhow::ensure!(cats.iter().all(|c| c.sizes.contains_key(&ins.version)), "no preload from {}: update first", ins.version);
            let sets = update::patch_manifests(&http, &cats).await?;
            update::preload(&http, &sets, &ins.dir, &ins.version, &pre.tag, &progress).await?;
            std::fs::write(ins.dir.join(sophon::STATE_DIR).join(format!("preload-{}.done", pre.tag)), "")?;
            Ok(())
        })
    }

    /// `hoyo.repair {id}`: every file checked against its version's manifest, bad ones fetched again
    pub(crate) async fn hoyo_repair(self: &Arc<Self>, params: &Value) -> Result<Value, Error> {
        let id = params["id"].as_i64().ok_or_else(|| Error::new("bad_params", "missing integer `id`"))?;
        let ins = self.installed(id)?;
        if self.running.lock().unwrap().contains_key(&id) {
            return Err(Error::new("busy", format!("{} is running", ins.game.name)));
        }
        let g = self.hoyo_api_game(&ins.biz, false).await?;
        let main = g.main.clone().ok_or_else(|| Error::new("not_found", "HoYoPlay lists no version"))?;
        // The installed version's files: a repair does not update
        let branch = Branch { tag: ins.version.clone(), ..main };
        let label = format!("Repairing {}", name_of(&ins.biz));
        let (state, http) = (self.clone(), self.http.clone());
        self.start_job(id, label, move |progress| async move {
            let want = wanted(&branch, &ins.voices);
            let cats: Vec<_> = sophon::build(&http, g.region, &branch).await?.into_iter().filter(|c| want(&c.field)).collect();
            let sets = sophon::manifests(&http, &cats).await?;
            let fixed = update::repair(&http, sets, &ins.dir, &progress).await?;
            tracing::info!(id, fixed, "repair done");
            state.readopt(id, &ins.dir)
        })
    }
}
