//! Hoyo games (Phase 6): Genshin Impact and Zenless Zone Zero, installed,
//! updated and run without their official launcher. `api` is the HoYoPlay
//! launcher API.

pub mod api;

use crate::rpc::{State, internal};
use serde_json::{Value, json};
use somelauncher_proto::Error;
use somelauncher_proto::game::{Game, Kind};

/// The game's exe for each biz
pub const EXES: &[(&str, &str)] =
    &[("hk4e_global", "GenshinImpact.exe"), ("hk4e_cn", "YuanShen.exe"), ("nap_global", "ZenlessZoneZero.exe"), ("nap_cn", "ZenlessZoneZero.exe")];

/// Which HoYoPlay game a library game is: what a source recorded (`data.biz`,
/// Twintail's or an adopted install's), else its exe's name (global for ZZZ,
/// whose exe is the same in both regions)
pub fn biz_of(game: &Game) -> Option<String> {
    if game.kind != Kind::Hoyo {
        return None;
    }
    if let Some(b) = game.sources.iter().find_map(|s| s.data["biz"].as_str().filter(|b| !b.is_empty())) {
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
