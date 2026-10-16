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
    pub(crate) async fn fetch_art(&self, game: &Game, force: bool) -> Result<bool> {
        let key = self.setting_str("steamGridDbKey")?;
        let mut changed = false;
        for kind in art::KINDS {
            let current = art::row(&self.db.lock().unwrap(), game.id, kind)?;
            let origins = match art::need(current.as_ref(), force) {
                Need::Nothing => continue,
                Need::Reload(origin) => vec![origin],
                Need::Find => art::origins(&self.http, key.as_deref(), &self.home, game, kind).await,
            };
            let custom = current.as_ref().is_some_and(|r| r.custom);
            let mut stored = None;
            for origin in origins {
                // The same origin as now, with its file there: nothing to do
                if current.as_ref().is_some_and(|r| r.origin == origin && r.file.as_ref().is_some_and(|f| f.is_file())) {
                    stored = current.clone();
                    break;
                }
                match art::load(&self.http, &origin).await.and_then(|bytes| art::store(game.id, kind, &origin, &bytes)) {
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
        if self.fetch_art(&game, force).await? {
            self.emit("library.changed", json!({ "ids": [id] }));
        }
        Ok(())
    }

    /// Every game's missing art, as a background job (`download.*` events, `done`/`total` in games).
    pub(crate) fn fetch_all_art(self: &Arc<Self>, force: bool) -> Result<String> {
        let id = "art".to_string();
        let state = Arc::downgrade(self);
        crate::jobs::spawn(self, &id, "Game art", move |progress| async move {
            let Some(s) = state.upgrade() else { return Ok(()) };
            let games = crate::games::list(&s.db.lock().unwrap(), true)?;
            drop(s);
            let total = games.len() as u64;
            for (i, game) in games.iter().enumerate() {
                let Some(s) = state.upgrade() else { return Ok(()) };
                match s.fetch_art(game, force).await {
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
    pub(crate) async fn set_art(&self, id: i64, kind: &str, origin: Option<&str>) -> Result<()> {
        match origin {
            Some(origin) => {
                let bytes = art::load(&self.http, origin).await?;
                let file = art::store(id, kind, origin, &bytes)?;
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

    /// The automatic choices for one kind, to pick another from.
    pub(crate) async fn art_options(&self, game: &Game, kind: &str) -> Result<Value> {
        let key = self.setting_str("steamGridDbKey")?;
        Ok(json!(art::origins(&self.http, key.as_deref(), &self.home, game, kind).await))
    }
}
