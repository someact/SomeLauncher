//! Steam build checks: the latest public build of an app from a PICS mirror
//! (api.steamcmd.net; SteamDB has no API), compared with the installed one.

use crate::rpc::State;
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

/// Checks at daemon start are skipped when the last one is newer than this.
const STALE: i64 = 12 * 3600;

/// The latest public build id and its time, or None when the mirror does not know the app.
pub async fn latest(http: &reqwest::Client, appid: &str) -> Result<Option<(String, i64)>> {
    let body: Value = http
        .get(format!("https://api.steamcmd.net/v1/info/{appid}"))
        .send()
        .await?
        .error_for_status()
        .context("api.steamcmd.net")?
        .json()
        .await?;
    let public = &body["data"][appid]["depots"]["branches"]["public"];
    let Some(build) = public["buildid"].as_str() else { return Ok(None) };
    let time = public["timeupdated"].as_str().and_then(|t| t.parse().ok()).unwrap_or(0);
    Ok(Some((build.to_string(), time)))
}

impl State {
    /// Looks up the latest build of one installed Steam game.
    pub(crate) async fn check_build(&self, id: i64) -> Result<()> {
        let Some(game) = crate::games::get(&self.db.lock().unwrap(), id)? else { return Ok(()) };
        let Some(appid) = crate::launch::steam_app(&game).map(|a| a.0) else { return Ok(()) };
        let found = latest(&self.http, &appid).await?;
        // The game may have changed meanwhile: write only the build
        let Some(mut game) = crate::games::get(&self.db.lock().unwrap(), id)? else { return Ok(()) };
        let before = game.build.clone();
        game.build.checked = Some(crate::games::now());
        if let Some((build, time)) = found {
            (game.build.latest, game.build.latest_time) = (Some(build), Some(time));
        }
        crate::games::save(&self.db.lock().unwrap(), &game)?;
        if game.build.latest != before.latest {
            self.emit("library.changed", json!({ "ids": [id] }));
        }
        Ok(())
    }

    /// Every installed Steam game, as a background job.
    pub(crate) fn check_all_builds(self: &Arc<Self>) -> Result<String> {
        let id = "builds".to_string();
        let state = Arc::downgrade(self);
        crate::jobs::spawn(self, &id, "Update check", move |progress| async move {
            let Some(s) = state.upgrade() else { return Ok(()) };
            let games: Vec<i64> =
                crate::games::list(&s.db.lock().unwrap(), true)?.into_iter().filter(|g| g.build.installed.is_some()).map(|g| g.id).collect();
            if !games.is_empty() {
                crate::db::set_setting(&s.db.lock().unwrap(), "buildsChecked", &json!(crate::games::now()))?;
            }
            drop(s);
            let total = games.len() as u64;
            for (i, game) in games.into_iter().enumerate() {
                let Some(s) = state.upgrade() else { return Ok(()) };
                if let Err(e) = s.check_build(game).await {
                    tracing::warn!(game, "build check: {e:#}");
                }
                progress.update(i as u64 + 1, total);
                drop(s);
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
            Ok(())
        })?;
        Ok(id)
    }

    /// At daemon start: a check unless the last one is recent.
    pub fn check_builds_if_stale(self: &Arc<Self>) -> Result<()> {
        let last = crate::db::setting(&self.db.lock().unwrap(), "buildsChecked")?.and_then(|v| v.as_i64()).unwrap_or(0);
        if crate::games::now() - last > STALE {
            self.check_all_builds()?;
        }
        Ok(())
    }
}
