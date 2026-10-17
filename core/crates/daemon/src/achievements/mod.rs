//! Achievements (Phase 7): each game's list in the `achievements` table,
//! read again from its source by [`State::refresh_achievements`]; a game's
//! `achievements` field is the progress worked out from it on load.
//! Sources: Steam (7a, `steam.rs`), gbe_fork/Goldberg saves (7b, `gbe.rs`).

pub mod gbe;
pub mod steam;

use crate::rpc::{State, internal};
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension as _, params};
use serde::Serialize;
use serde_json::{Value, json};
use somelauncher_proto::Error;
use somelauncher_proto::game::{Progress, SourceKind};
use std::sync::Arc;
use std::time::Duration;

/// Web API lists are asked for again after this; Steam's cache on disk at every start
const STALE: i64 = 12 * 3600;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Achievement {
    /// The id the game uses (`ACH_WIN_ONE_GAME`)
    pub api_name: String,
    pub name: String,
    pub description: String,
    /// Steam's hidden achievements: the description is a secret until unlocked
    pub hidden: bool,
    pub icon: Option<String>,
    pub icon_gray: Option<String>,
    /// Unix seconds; `Some(0)` unlocked at an unknown time; None locked
    pub unlocked_at: Option<i64>,
}

pub fn progress(conn: &Connection, game: i64) -> Result<Option<Progress>> {
    let row: Option<(u32, u32, String, Option<i64>)> = conn
        .query_row(
            "SELECT count(*), count(unlocked_at), max(source), max(unlocked_at) FROM achievements WHERE game_id = ?1 HAVING count(*) > 0",
            [game],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    Ok(row.map(|(total, unlocked, source, last)| Progress { unlocked, total, source, last: last.filter(|t| *t > 0) }))
}

pub fn list(conn: &Connection, game: i64) -> Result<Vec<Achievement>> {
    let mut stmt = conn.prepare("SELECT api_name, name, description, hidden, icon, icon_gray, unlocked_at FROM achievements WHERE game_id = ?1 ORDER BY ord")?;
    let rows = stmt.query_map([game], |r| {
        Ok(Achievement {
            api_name: r.get(0)?,
            name: r.get(1)?,
            description: r.get(2)?,
            hidden: r.get(3)?,
            icon: r.get(4)?,
            icon_gray: r.get(5)?,
            unlocked_at: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Replaces a game's list; returns the achievements unlocked since the last one
/// (none the first time, which only fills the list)
pub fn replace(conn: &mut Connection, game: i64, source: &str, list: &[Achievement]) -> Result<Vec<Achievement>> {
    let before = list_map(conn, game)?;
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM achievements WHERE game_id = ?1", [game])?;
    for (i, a) in list.iter().enumerate() {
        tx.execute(
            "INSERT INTO achievements (game_id, api_name, ord, name, description, hidden, icon, icon_gray, unlocked_at, source)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![game, a.api_name, i as i64, a.name, a.description, a.hidden, a.icon, a.icon_gray, a.unlocked_at, source],
        )?;
    }
    tx.commit()?;
    if before.is_empty() {
        return Ok(Vec::new());
    }
    Ok(list.iter().filter(|a| a.unlocked_at.is_some() && before.get(&a.api_name).is_some_and(|b| b.is_none())).cloned().collect())
}

fn list_map(conn: &Connection, game: i64) -> Result<std::collections::HashMap<String, Option<i64>>> {
    Ok(list(conn, game)?.into_iter().map(|a| (a.api_name, a.unlocked_at)).collect())
}

impl State {
    fn steam_account(&self) -> Option<String> {
        self.setting_str("steamId").ok().flatten().filter(|s| !s.is_empty()).or_else(|| crate::import::steam::roots(&self.home).iter().find_map(|r| crate::import::steam::last_user(r)))
    }

    /// A game's list read again from its source: the Web API (with a key and
    /// `api`), else Steam's cache. Emits `library.changed` when it changed, and
    /// `achievement.unlocked` for each new unlock. Ok(false) when no source knows the game.
    pub(crate) async fn refresh_achievements(&self, id: i64, api: bool) -> Result<bool> {
        let Some(game) = crate::games::get(&self.db.lock().unwrap(), id)? else { return Ok(false) };
        let found = if game.sources.iter().any(|s| s.source == SourceKind::Steam) {
            let appid = game.sources.iter().find(|s| s.source == SourceKind::Steam).map(|s| s.source_id.clone()).unwrap_or_default();
            self.steam_list(id, &appid, api).await?
        } else if let Some(emu) = self.gbe_of(&game) {
            let save = gbe::save_file(&emu, game.prefix.as_deref(), &self.home);
            match gbe::read(&emu, save.as_deref())? {
                Some(l) => Some(("gbe", l)),
                // No list of its own: Steam's for its app id, with the emulator's unlocks
                None => self.steam_list(id, &emu.appid, api).await?.map(|(_, l)| ("gbe", gbe::overlay(l, save.as_deref()))),
            }
        } else {
            None
        };
        let Some((source, list)) = found.filter(|(_, l)| !l.is_empty()) else { return Ok(false) };
        let before = progress(&self.db.lock().unwrap(), id)?;
        let new = replace(&mut self.db.lock().unwrap(), id, source, &list)?;
        if progress(&self.db.lock().unwrap(), id)? != before {
            self.emit("library.changed", json!({ "ids": [id] }));
        }
        for a in new {
            self.emit("achievement.unlocked", json!({ "id": id, "game": game.name, "achievement": a, "source": source }));
        }
        Ok(true)
    }

    /// The emulator in a non-Steam game's folder
    pub(crate) fn gbe_of(&self, game: &somelauncher_proto::game::Game) -> Option<gbe::Emu> {
        use somelauncher_proto::game::Kind;
        if !matches!(game.kind, Kind::Proton | Kind::Native) {
            return None;
        }
        gbe::find(game.install_dir.as_deref()?, game.exe.as_deref())
    }

    /// A Steam app's list: the Web API (with a key and `api`), else Steam's cache
    async fn steam_list(&self, id: i64, appid: &str, api: bool) -> Result<Option<(&'static str, Vec<Achievement>)>> {
        let lang = steam::language(&self.home);
        let account = self.steam_account();
        let key = self.setting_str("steamApiKey")?.filter(|k| !k.is_empty());
        let mut found = None;
        if api && let (Some(key), Some(sid)) = (&key, &account) {
            match steam::api(&self.http, key, sid, appid, &lang).await {
                Ok(l) => found = l.map(|l| ("steamApi", l)),
                Err(e) => tracing::warn!(id, "achievements from the Web API: {e:#}"),
            }
        }
        if found.is_none() {
            let acc = account.as_deref().and_then(steam::account_id);
            for root in crate::import::steam::roots(&self.home) {
                if let Some(l) = steam::local(&root, acc, appid, &lang)? {
                    found = Some(("steam", l));
                    break;
                }
            }
        }
        Ok(found)
    }

    /// Every game a source may know (Steam games, games with gbe_fork), as a background job (the Web API when the last pass is old)
    pub(crate) fn refresh_all_achievements(self: &Arc<Self>, force: bool) -> Result<String> {
        let last = crate::db::setting(&self.db.lock().unwrap(), "achievementsChecked")?.and_then(|v| v.as_i64()).unwrap_or(0);
        let api = force || crate::games::now() - last > STALE;
        let id = "achievements".to_string();
        let state = Arc::downgrade(self);
        crate::jobs::spawn(self, &id, "Achievements", move |progress| async move {
            let Some(s) = state.upgrade() else { return Ok(()) };
            let games: Vec<i64> = crate::games::list(&s.db.lock().unwrap(), true)?
                .into_iter()
                .filter(|g| g.sources.iter().any(|s| s.source == SourceKind::Steam) || s.gbe_of(g).is_some())
                .map(|g| g.id)
                .collect();
            let has_key = s.setting_str("steamApiKey")?.is_some_and(|k| !k.is_empty());
            if api && has_key {
                crate::db::set_setting(&s.db.lock().unwrap(), "achievementsChecked", &json!(crate::games::now()))?;
            }
            drop(s);
            let total = games.len() as u64;
            for (i, game) in games.into_iter().enumerate() {
                let Some(s) = state.upgrade() else { return Ok(()) };
                if let Err(e) = s.refresh_achievements(game, api).await {
                    tracing::warn!(game, "achievements: {e:#}");
                }
                progress.update(i as u64 + 1, total);
                drop(s);
                if api && has_key {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            }
            Ok(())
        })?;
        Ok(id)
    }

    /// `achievements.list {id}`
    pub(crate) fn achievements_list(&self, params: &Value) -> Result<Value, Error> {
        let id = params["id"].as_i64().ok_or_else(|| Error::new("bad_params", "missing integer `id`"))?;
        let db = self.db.lock().unwrap();
        self.game(&db, id)?;
        Ok(json!(list(&db, id).map_err(internal)?))
    }

    /// `achievements.refresh {id?, force?}`
    pub(crate) async fn achievements_refresh(self: &Arc<Self>, params: &Value) -> Result<Value, Error> {
        match params["id"].as_i64() {
            Some(id) => {
                self.game(&self.db.lock().unwrap(), id)?;
                self.refresh_achievements(id, true).await.map_err(internal)?;
                let db = self.db.lock().unwrap();
                Ok(json!(self.game(&db, id)?))
            }
            None => {
                let id = self.refresh_all_achievements(params["force"].as_bool().unwrap_or(true)).map_err(|e| Error::new("busy", format!("{e:#}")))?;
                Ok(json!({ "id": id }))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(name: &str, at: Option<i64>) -> Achievement {
        Achievement { api_name: name.into(), name: name.into(), description: String::new(), hidden: false, icon: None, icon_gray: None, unlocked_at: at }
    }

    #[test]
    fn new_unlocks_are_found_after_the_first_fill() {
        let mut db = crate::games::test_db();
        let id = crate::games::insert(&db, &somelauncher_proto::game::NewGame::new("Muse Dash", somelauncher_proto::game::Kind::Steam)).unwrap();
        assert_eq!(progress(&db, id).unwrap(), None);
        assert!(replace(&mut db, id, "steam", &[a("A", Some(5)), a("B", None)]).unwrap().is_empty(), "the first fill is not news");
        assert_eq!(progress(&db, id).unwrap(), Some(Progress { unlocked: 1, total: 2, source: "steam".into(), last: Some(5) }));
        let new = replace(&mut db, id, "steam", &[a("A", Some(5)), a("B", Some(9))]).unwrap();
        assert_eq!(new.iter().map(|a| a.api_name.as_str()).collect::<Vec<_>>(), ["B"]);
        assert_eq!(list(&db, id).unwrap()[1].unlocked_at, Some(9));
    }
}
