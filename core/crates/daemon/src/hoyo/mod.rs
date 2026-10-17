//! Hoyo games (Phase 6): Genshin Impact and Zenless Zone Zero, installed,
//! updated and run without their official launcher. `api` is the HoYoPlay
//! launcher API.

pub mod api;
pub mod files;
pub mod install;
pub mod manage;
pub mod sophon;
pub mod update;

use crate::rpc::{State, internal};
use serde_json::{Value, json};
use somelauncher_proto::Error;
use somelauncher_proto::game::{Game, Kind, NewGame, Source, SourceKind};
use std::path::Path;
use std::sync::Arc;

/// The game's exe for each biz
pub const EXES: &[(&str, &str)] =
    &[("hk4e_global", "GenshinImpact.exe"), ("hk4e_cn", "YuanShen.exe"), ("nap_global", "ZenlessZoneZero.exe"), ("nap_cn", "ZenlessZoneZero.exe")];

/// Which HoYoPlay game a library game is: what a source recorded (`data.biz`,
/// ours first, then Twintail's), else its exe's name (global for ZZZ, whose
/// exe is the same in both regions)
pub fn biz_of(game: &Game) -> Option<String> {
    if game.kind != Kind::Hoyo {
        return None;
    }
    let mut sources: Vec<&Source> = game.sources.iter().collect();
    sources.sort_by_key(|s| s.source != SourceKind::Hoyo);
    if let Some(b) = sources.iter().find_map(|s| s.data["biz"].as_str().filter(|b| !b.is_empty())) {
        return Some(b.to_string());
    }
    let exe = game.exe.as_ref()?.file_name()?.to_str()?;
    EXES.iter().find(|(_, e)| e.eq_ignore_ascii_case(exe)).map(|(b, _)| b.to_string())
}

impl State {
    /// The API's language: the `language` setting, else the locale
    pub(crate) fn hoyo_language(&self) -> String {
        let set = self.setting_str("language").ok().flatten();
        let locale = set.or_else(|| std::env::var("LC_MESSAGES").ok()).or_else(|| std::env::var("LANG").ok()).unwrap_or_default();
        api::language(&locale)
    }

    pub(crate) async fn hoyo_games(&self, params: &Value) -> Result<Value, Error> {
        let region = match params["region"].as_str() {
            Some("cn") => api::Region::Cn,
            _ => api::Region::Global,
        };
        let force = params["force"].as_bool().unwrap_or(false);
        let games = api::games(&self.http, region, &self.hoyo_language(), force).await.map_err(network)?;
        let library = crate::games::list(&self.db.lock().unwrap(), true).map_err(internal)?;
        Ok(Value::Array(
            games
                .into_iter()
                .map(|g| {
                    let installed: Vec<i64> = library.iter().filter(|l| biz_of(l).as_deref() == Some(g.biz.as_str())).map(|l| l.id).collect();
                    let mut v = serde_json::to_value(&g).unwrap_or_default();
                    v["games"] = json!(installed);
                    v
                })
                .collect(),
        ))
    }

    pub(crate) async fn hoyo_content(&self, params: &Value) -> Result<Value, Error> {
        let biz = params["biz"].as_str().ok_or_else(|| Error::new("bad_params", "missing string `biz`"))?;
        let force = params["force"].as_bool().unwrap_or(false);
        let c = api::content(&self.http, biz, &self.hoyo_language(), force).await.map_err(network)?;
        serde_json::to_value(c).map_err(|e| internal(e.into()))
    }
}

/// A `hoyo` source's id: its install folder (one game can be installed twice;
/// source ids are unique per kind)
pub fn source_id(dir: &Path) -> String {
    dir.to_string_lossy().into_owned()
}

pub fn name_of(biz: &str) -> &'static str {
    if api::game_of(biz) == "nap" { "Zenless Zone Zero" } else { "Genshin Impact" }
}

impl State {
    /// `hoyo.adopt {path}` or `{id}`: an install already on disk becomes ours to
    /// update and run; its version and voice packs are read from its files.
    /// A library game in that folder (from Twintail, or added by hand) is kept
    /// and gets our source; otherwise a new game is added.
    pub(crate) fn hoyo_adopt(self: &Arc<Self>, params: &Value) -> Result<Value, Error> {
        let given = params["id"].as_i64();
        let path = match (params["path"].as_str(), given) {
            (Some(p), _) => Path::new(p).to_path_buf(),
            (None, Some(id)) => self.game(&self.db.lock().unwrap(), id)?.install_dir.ok_or_else(|| Error::new("not_found", format!("game {id} has no install folder")))?,
            _ => return Err(Error::new("bad_params", "give `path` or `id`")),
        };
        let install = files::detect(&path).ok_or_else(|| Error::new("not_found", format!("no Genshin Impact or Zenless Zone Zero in {}", path.display())))?;
        let id = self.adopt(&install, given).map_err(internal)?;
        self.emit("library.changed", json!({ "ids": [id] }));
        self.art_later(id);
        let game = self.game(&self.db.lock().unwrap(), id)?;
        Ok(json!(game))
    }

    /// HoYoPlay's art for a game just adopted or being installed, in the background
    pub(crate) fn art_later(self: &Arc<Self>, id: i64) {
        let state = self.clone();
        tokio::spawn(async move {
            if let Err(e) = state.refresh_art(id, false).await {
                tracing::warn!(id, "art: {e:#}");
            }
        });
    }

    pub(crate) fn adopt(&self, install: &files::Install, id: Option<i64>) -> anyhow::Result<i64> {
        let db = self.db.lock().unwrap();
        let existing = match id {
            Some(id) => Some(id),
            None => crate::games::list(&db, true)?.into_iter().find(|g| g.kind == Kind::Hoyo && g.install_dir.as_deref() == Some(&install.dir)).map(|g| g.id),
        };
        let id = match existing {
            Some(id) => id,
            None => {
                let mut n = NewGame::new(name_of(&install.biz), Kind::Hoyo);
                n.install_dir = Some(install.dir.clone());
                n.exe = Some(install.exe.clone());
                crate::games::insert(&db, &n)?
            }
        };
        let data = json!({ "biz": install.biz, "version": install.version, "voices": install.voices });
        crate::games::set_only_source(&db, id, &Source { source: SourceKind::Hoyo, source_id: source_id(&install.dir), data })?;
        crate::meta::tags::retag(&db, id)?;
        tracing::info!(id, biz = install.biz, version = ?install.version, "adopted {}", install.dir.display());
        Ok(id)
    }

    /// Every Hoyo game with an install folder: its version read again from its files
    pub(crate) fn adopt_all(&self) -> Vec<i64> {
        let games = match crate::games::list(&self.db.lock().unwrap(), true) {
            Ok(g) => g,
            Err(e) => {
                tracing::error!("listing games: {e:#}");
                return Vec::new();
            }
        };
        let mut changed = Vec::new();
        for g in games.into_iter().filter(|g| g.kind == Kind::Hoyo) {
            let Some(install) = g.install_dir.as_deref().and_then(files::detect) else { continue };
            let before = g.sources.iter().find(|s| s.source == SourceKind::Hoyo).map(|s| s.data.clone());
            // Being installed: its files are not the game yet
            if before.as_ref().is_some_and(|d| !d["installing"].is_null()) {
                continue;
            }
            let after = json!({ "biz": install.biz, "version": install.version, "voices": install.voices });
            if before.as_ref() != Some(&after) {
                match self.adopt(&install, Some(g.id)) {
                    Ok(id) => changed.push(id),
                    Err(e) => tracing::warn!(id = g.id, "adopting: {e:#}"),
                }
            }
        }
        changed
    }
}

fn network(e: anyhow::Error) -> Error {
    Error::new("network", format!("{e:#}"))
}

/// HoYoPlay's art for a Hoyo game, best first: the current launcher
/// background for `hero`, then the game's own; its logo and icon (logos are
/// localized: in the locale's language)
pub async fn art_origins(http: &reqwest::Client, game: &Game, kind: &str) -> Vec<String> {
    let Some(biz) = biz_of(game) else { return Vec::new() };
    let language = api::language(&std::env::var("LANG").unwrap_or_default());
    let games = match api::games(http, api::Region::of(&biz), &language, false).await {
        Ok(g) => g,
        Err(e) => {
            tracing::debug!(game = game.id, "HoYoPlay art: {e:#}");
            return Vec::new();
        }
    };
    let Some(g) = games.into_iter().find(|g| g.biz == biz) else { return Vec::new() };
    let urls = match kind {
        "hero" => g.backgrounds.first().map(|b| b.image.clone()).into_iter().chain([g.background]).collect(),
        "logo" => vec![g.logo],
        "icon" => vec![g.icon],
        _ => Vec::new(),
    };
    urls.into_iter().filter(|u| !u.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use somelauncher_proto::game::NewGame;

    #[test]
    fn biz_from_a_source_or_the_exe() {
        let mut n = NewGame::new("Genshin Impact", Kind::Hoyo);
        n.exe = Some("/g/Genshin Impact/YuanShen.exe".into());
        let mut g = Game::new(1, n);
        assert_eq!(biz_of(&g).as_deref(), Some("hk4e_cn"));
        g.sources.push(somelauncher_proto::game::Source {
            source: somelauncher_proto::game::SourceKind::Twintail,
            source_id: "x".into(),
            data: json!({"biz": "hk4e_global"}),
        });
        assert_eq!(biz_of(&g).as_deref(), Some("hk4e_global"));
        g.kind = Kind::Proton;
        assert_eq!(biz_of(&g), None);
    }
}
