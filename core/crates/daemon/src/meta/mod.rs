//! Metadata and art (Phase 3). A game is matched to a Steam app or a
//! SteamGridDB game; the user confirms every match from `search`'s candidates,
//! except Steam's own games, which match themselves by app id.

pub mod sgdb;
pub mod steam_store;

use crate::guess::normalized;
use anyhow::{Result, bail};
use rusqlite::Connection;
use serde::Serialize;
use somelauncher_proto::game::{Game, Meta, SourceKind};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    /// `steam` or `steamgriddb`, what `meta.match` takes back
    pub source: &'static str,
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<i32>,
    /// A portrait cover to confirm the match with
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
    /// A small image for when the cover does not load
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumb: Option<String>,
    /// Why it is suggested first (`Steam library`, `OnlineFix.ini`, `steam_appid.txt`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
}

/// Steam app ids the game itself names: its Steam library entry, OnlineFix's
/// real app id, a `steam_appid.txt` (480 is Spacewar, the co-op stand-in).
pub fn known_appids(game: &Game) -> Vec<(String, &'static str)> {
    let mut out: Vec<(String, &'static str)> = Vec::new();
    let mut add = |id: String, why| {
        if !id.is_empty() && id != "480" && id.chars().all(|c| c.is_ascii_digit()) && !out.iter().any(|(i, _)| *i == id) {
            out.push((id, why));
        }
    };
    for s in game.sources.iter().filter(|s| s.source == SourceKind::Steam) {
        add(s.source_id.clone(), "Steam library");
    }
    if let Some(dir) = game.exe.as_ref().and_then(|e| e.parent()) {
        if let Ok(ini) = std::fs::read_to_string(dir.join("OnlineFix.ini")) {
            for line in ini.lines() {
                if let Some(id) = line.trim().strip_prefix("RealAppId=") {
                    add(id.trim().to_string(), "OnlineFix.ini");
                }
            }
        }
        if let Ok(id) = std::fs::read_to_string(dir.join("steam_appid.txt")) {
            add(id.trim().to_string(), "steam_appid.txt");
        }
    }
    out
}

/// Names that are rarely the game itself.
const EXTRAS: &[&str] = &["soundtrack", "ost", "demo", "dlc", "artbook", "playtest", "season pass", "dedicated server"];

/// Lower is a better match for `query`.
fn rank(name: &str, query: &str) -> u8 {
    let (n, q) = (normalized(name), normalized(query));
    let extra = EXTRAS.iter().any(|e| name.to_lowercase().contains(e)) as u8 * 3;
    extra
        + if n == q {
            0
        } else if n.starts_with(&q) || q.starts_with(&n) {
            1
        } else {
            2
        }
}

fn steam_candidate(appid: &str, name: String, thumb: Option<String>, reason: Option<&'static str>) -> Candidate {
    Candidate {
        source: "steam",
        id: appid.into(),
        name,
        year: None,
        cover: Some(steam_store::art_url(appid, "library_600x900.jpg")),
        thumb: thumb.or_else(|| Some(steam_store::art_url(appid, "header.jpg"))),
        reason,
    }
}

/// Candidates for a game (its own ids first) or a free-text query, best first.
pub async fn search(http: &reqwest::Client, key: Option<&str>, query: &str, game: Option<&Game>) -> Result<Vec<Candidate>> {
    let mut known = Vec::new();
    for (appid, why) in game.map(known_appids).unwrap_or_default() {
        let name = match steam_store::details(http, &appid).await {
            Ok(Some(d)) if !d.name.is_empty() => d.name,
            _ => continue,
        };
        known.push(steam_candidate(&appid, name, None, Some(why)));
    }

    let steam = steam_store::search(http, query).await;
    let sgdb = match key {
        Some(key) => Some(sgdb_candidates(http, key, query).await),
        None => None,
    };
    let mut found: Vec<Candidate> = Vec::new();
    match &steam {
        Ok(hits) => found.extend(hits.iter().take(8).map(|h| steam_candidate(&h.id.to_string(), h.name.clone(), h.tiny_image.clone(), None))),
        Err(e) => tracing::warn!("{e:#}"),
    }
    match &sgdb {
        Some(Ok(c)) => found.extend(c.iter().cloned()),
        Some(Err(e)) => tracing::warn!("{e:#}"),
        None => {}
    }
    if found.is_empty() && known.is_empty() {
        steam?;
        if let Some(Err(e)) = sgdb {
            return Err(e);
        }
    }
    // Best first; Steam before SteamGridDB on a tie (it brings genres and tags too)
    found.sort_by_key(|c| (rank(&c.name, query), c.source != "steam"));
    found.retain(|c| !known.iter().any(|k| k.source == c.source && k.id == c.id));
    known.extend(found);
    Ok(known)
}

async fn sgdb_candidates(http: &reqwest::Client, key: &str, query: &str) -> Result<Vec<Candidate>> {
    let games = sgdb::search(http, key, query).await?;
    let ids: Vec<String> = games.iter().take(6).map(|g| g.id.to_string()).collect();
    let covers = futures_util::future::join_all(ids.iter().map(|id| sgdb::images(http, key, "grids", id))).await;
    Ok(games
        .iter()
        .zip(covers)
        .map(|(g, cover)| {
            let first = cover.ok().and_then(|c| c.into_iter().next());
            Candidate {
                source: "steamgriddb",
                id: g.id.to_string(),
                name: g.name.clone(),
                year: g.release_date.map(year_of),
                cover: first.as_ref().map(|i| i.thumb.clone()),
                thumb: None,
                reason: None,
            }
        })
        .collect())
}

/// The year of a unix time.
fn year_of(t: i64) -> i32 {
    // Days to a civil date (Howard Hinnant's algorithm), enough for a year
    let days = t.div_euclid(86_400) + 719_468;
    let era = days.div_euclid(146_097);
    let doe = days - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    (yoe + era * 400 + if mp >= 10 { 1 } else { 0 }) as i32
}

/// The match for a picked candidate, with the ids of the other source filled in when they can be found.
pub async fn resolve(http: &reqwest::Client, key: Option<&str>, source: &str, id: &str) -> Result<Meta> {
    match source {
        "steam" => {
            let details = steam_store::details(http, id).await?;
            let Some(d) = details else { bail!("Steam has no app {id}") };
            let sgdb_id = match key {
                Some(key) => sgdb::by_steam(http, key, id).await.ok().map(|g| g.id.to_string()),
                None => None,
            };
            Ok(Meta { source: "steam".into(), source_id: id.into(), name: d.name, steam_appid: Some(id.into()), sgdb_id })
        }
        "steamgriddb" => {
            let Some(key) = key else { bail!("set a SteamGridDB API key first") };
            let game = sgdb::game(http, key, id).await?;
            // The same game on Steam, by its exact name, for genres and tags
            let hits = steam_store::search(http, &game.name).await.unwrap_or_default();
            let steam_appid = hits.iter().find(|h| normalized(&h.name) == normalized(&game.name)).map(|h| h.id.to_string());
            Ok(Meta { source: "steamgriddb".into(), source_id: id.into(), name: game.name, steam_appid, sgdb_id: Some(id.into()) })
        }
        _ => bail!("no metadata source {source}"),
    }
}

/// Steam games match themselves: their app id is known, nothing to confirm.
/// Returns the games that got a match.
pub fn match_steam_games(conn: &Connection) -> Result<Vec<i64>> {
    let mut changed = Vec::new();
    for g in crate::games::list(conn, true)? {
        if g.meta.is_some() {
            continue;
        }
        let Some(s) = g.sources.iter().find(|s| s.source == SourceKind::Steam) else { continue };
        let meta = Meta { source: "steam".into(), source_id: s.source_id.clone(), name: g.name.clone(), steam_appid: Some(s.source_id.clone()), sgdb_id: None };
        crate::games::set_meta(conn, g.id, Some(&meta))?;
        changed.push(g.id);
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use somelauncher_proto::game::{Kind, NewGame, Source};

    #[test]
    fn known_ids_come_from_steam_onlinefix_and_appid_files() {
        let t = tempfile::tempdir().unwrap();
        let mut n = NewGame::new("How to Fish", Kind::Proton);
        n.exe = Some(t.path().join("How to Fish.exe"));
        let mut g = Game::new(1, n);
        assert!(known_appids(&g).is_empty());
        std::fs::write(t.path().join("OnlineFix.ini"), "[Main]\nRealAppId=4001890\nFakeAppId=480\n").unwrap();
        std::fs::write(t.path().join("steam_appid.txt"), "480\n").unwrap();
        assert_eq!(known_appids(&g), [("4001890".to_string(), "OnlineFix.ini")]);
        g.sources.push(Source { source: SourceKind::Steam, source_id: "4001890".into(), data: Default::default() });
        assert_eq!(known_appids(&g), [("4001890".to_string(), "Steam library")], "each id once");
    }

    #[test]
    fn ranking_prefers_exact_names_over_extras() {
        assert_eq!(rank("How to Fish", "how to fish"), 0);
        assert_eq!(rank("How to Fish Soundtrack", "How to Fish"), 4);
        assert_eq!(rank("R.E.P.O.", "REPO"), 0);
        assert_eq!(rank("Stardew Valley", "Stardew"), 1);
        assert_eq!(rank("Terraria", "Stardew"), 2);
    }

    #[test]
    fn years_from_unix_time() {
        assert_eq!(year_of(0), 1970);
        assert_eq!(year_of(1740528000), 2025);
        assert_eq!(year_of(1735689599), 2024);
    }

    #[test]
    fn steam_games_match_themselves_once() {
        let conn = crate::games::test_db();
        let id = crate::games::insert(&conn, &NewGame::new("R.E.P.O.", Kind::Steam)).unwrap();
        crate::games::set_source(&conn, id, &Source { source: SourceKind::Steam, source_id: "3241660".into(), data: Default::default() }).unwrap();
        crate::games::insert(&conn, &NewGame::new("Valheim", Kind::Native)).unwrap();
        assert_eq!(match_steam_games(&conn).unwrap(), [id]);
        assert!(match_steam_games(&conn).unwrap().is_empty());
        let meta = crate::games::get(&conn, id).unwrap().unwrap().meta.unwrap();
        assert_eq!(meta.steam_appid.as_deref(), Some("3241660"));
    }
}
