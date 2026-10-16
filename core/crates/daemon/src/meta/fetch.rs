//! Art fetching for the daemon: one game on request, every game in a
//! background job, and the user's own choices.

use super::art::{self, Need, Row};
use crate::rpc::State;
use anyhow::Result;
use serde_json::{Value, json};
use somelauncher_proto::game::Game;
use std::sync::Arc;

impl State {
    /// Brings a game's art up to date; returns whether anything changed.
    /// With `resize`, every image is made again from its origin (the screen changed).
    pub(crate) async fn fetch_art(&self, game: &Game, force: bool, resize: bool) -> Result<bool> {
        let key = self.setting_str("steamGridDbKey")?;
        let screen = art::screen_width(std::path::Path::new("/sys/class/drm"));
        let mut changed = false;
        for kind in art::KINDS {
            let current = art::row(&self.db.lock().unwrap(), game.id, kind)?;
            let need = match &current {
                Some(r) if resize => Need::Reload(r.origin.clone()),
                _ => art::need(current.as_ref(), force),
            };
            let origins = match need {
                Need::Nothing => continue,
                Need::Reload(origin) => vec![origin],
                Need::Find => art::origins(&self.http, key.as_deref(), &self.home, game, kind).await,
            };
            let custom = current.as_ref().is_some_and(|r| r.custom);
            let mut stored = None;
            for origin in origins {
                // The same origin as now, with its file there: nothing to do
                if !resize && current.as_ref().is_some_and(|r| r.origin == origin && r.file.as_ref().is_some_and(|f| f.is_file())) {
                    stored = current.clone();
                    break;
                }
                let file = match art::load(&self.http, &origin).await {
                    Ok(bytes) => art::store_sized(game.id, kind, origin.clone(), bytes, screen).await,
                    Err(e) => Err(e),
                };
                match file {
                    Ok(file) => {
                        stored = Some(Row { origin, custom, file: Some(file) });
                        break;
                    }
                    Err(e) => tracing::debug!(game = game.id, kind, "{e:#}"),
                }
            }
            if stored.is_none() {
                art::remove_files(game.id, kind);
            }
            if stored != current {
                art::set_row(&self.db.lock().unwrap(), game.id, kind, stored.as_ref())?;
                changed = true;
            }
        }
        Ok(changed)
    }

    /// Fetches art for one game and tells everyone when it changed.
    pub(crate) async fn refresh_art(&self, id: i64, force: bool) -> Result<()> {
        let Some(game) = crate::games::get(&self.db.lock().unwrap(), id)? else { return Ok(()) };
        if self.fetch_art(&game, force, false).await? {
            self.emit("library.changed", json!({ "ids": [id] }));
        }
        Ok(())
    }

    /// Every game's missing art, as a background job (`download.*` events, `done`/`total` in games).
    pub(crate) fn fetch_all_art(self: &Arc<Self>, force: bool, resize: bool) -> Result<String> {
        let id = "art".to_string();
        let state = Arc::downgrade(self);
        crate::jobs::spawn(self, &id, "Game art", move |progress| async move {
            let Some(s) = state.upgrade() else { return Ok(()) };
            let games = crate::games::list(&s.db.lock().unwrap(), true)?;
            drop(s);
            let total = games.len() as u64;
            for (i, game) in games.iter().enumerate() {
                let Some(s) = state.upgrade() else { return Ok(()) };
                match s.fetch_art(game, force, resize).await {
                    Ok(true) => s.emit("library.changed", json!({ "ids": [game.id] })),
                    Ok(false) => {}
                    Err(e) => tracing::warn!(game = game.id, "art: {e:#}"),
                }
                progress.update(i as u64 + 1, total);
            }
            Ok(())
        })?;
        Ok(id)
    }

    /// The user's own art for one kind (a file or a URL), or back to automatic with None.
    pub(crate) async fn set_art(&self, id: i64, kind: &'static str, origin: Option<&str>) -> Result<()> {
        match origin {
            Some(origin) => {
                let bytes = art::load(&self.http, origin).await?;
                let screen = art::screen_width(std::path::Path::new("/sys/class/drm"));
                let file = art::store_sized(id, kind, origin.to_string(), bytes, screen).await?;
                let row = Row { origin: origin.into(), custom: true, file: Some(file) };
                art::set_row(&self.db.lock().unwrap(), id, kind, Some(&row))?;
                self.emit("library.changed", json!({ "ids": [id] }));
            }
            None => {
                art::set_row(&self.db.lock().unwrap(), id, kind, None)?;
                self.refresh_art(id, false).await?;
                self.emit("library.changed", json!({ "ids": [id] }));
            }
        }
        Ok(())
    }

    /// Store details and Steam tags for a matched game (when missing, or always with
    /// `force`), then its tags; tells everyone when anything changed.
    pub(crate) async fn refresh_meta(&self, id: i64, force: bool) -> Result<()> {
        let Some(game) = crate::games::get(&self.db.lock().unwrap(), id)? else { return Ok(()) };
        let mut changed = false;
        if let Some(mut meta) = game.meta.clone()
            && (force || meta.fetched.is_none())
        {
            super::details(&self.http, &mut meta).await?;
            crate::games::set_meta(&self.db.lock().unwrap(), id, Some(&meta))?;
            changed = true;
        }
        changed |= super::tags::retag(&self.db.lock().unwrap(), id)?;
        if changed {
            self.emit("library.changed", json!({ "ids": [id] }));
        }
        Ok(())
    }

    /// Details for every matched game that has none (all with `force`), as a background
    /// job; slow on purpose (SteamSpy allows a request a second).
    pub(crate) fn refresh_all_meta(self: &Arc<Self>, force: bool) -> Result<String> {
        let id = "meta".to_string();
        let state = Arc::downgrade(self);
        crate::jobs::spawn(self, &id, "Game details", move |progress| async move {
            let Some(s) = state.upgrade() else { return Ok(()) };
            let games: Vec<i64> = crate::games::list(&s.db.lock().unwrap(), true)?
                .into_iter()
                .filter(|g| g.meta.as_ref().is_some_and(|m| force || m.fetched.is_none()))
                .map(|g| g.id)
                .collect();
            drop(s);
            let total = games.len() as u64;
            for (i, game) in games.into_iter().enumerate() {
                let Some(s) = state.upgrade() else { return Ok(()) };
                if let Err(e) = s.refresh_meta(game, force).await {
                    tracing::warn!(game, "details: {e:#}");
                }
                progress.update(i as u64 + 1, total);
                drop(s);
                tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
            }
            Ok(())
        })?;
        Ok(id)
    }

    /// After a new match: details, tags and art, in the background.
    pub(crate) fn matched(self: &Arc<Self>, id: i64) {
        let state = self.clone();
        tokio::spawn(async move {
            if let Err(e) = state.refresh_meta(id, true).await {
                tracing::warn!(id, "details: {e:#}");
            }
            if let Err(e) = state.refresh_art(id, true).await {
                tracing::warn!(id, "art: {e:#}");
            }
        });
    }

    /// Art is sized for the screen: when the widest monitor changed since the
    /// last start, every image is made again for the new one.
    pub async fn check_screen(self: &Arc<Self>) -> Result<()> {
        let width = art::screen_width(std::path::Path::new("/sys/class/drm"));
        let last = crate::db::setting(&self.db.lock().unwrap(), "artScreenWidth")?.and_then(|v| v.as_u64());
        if last == Some(width as u64) {
            return Ok(());
        }
        crate::db::set_setting(&self.db.lock().unwrap(), "artScreenWidth", &json!(width))?;
        if last.is_some() {
            tracing::info!(width, "the screen changed; resizing art");
            self.fetch_all_art(false, true)?;
        }
        Ok(())
    }

    /// The automatic choices for one kind, to pick another from.
    pub(crate) async fn art_options(&self, game: &Game, kind: &str) -> Result<Value> {
        let key = self.setting_str("steamGridDbKey")?;
        Ok(json!(art::origins(&self.http, key.as_deref(), &self.home, game, kind).await))
    }
}
