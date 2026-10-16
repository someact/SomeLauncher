//! Importers read other launchers' files and hand back `Found` entries; `apply`
//! merges them into the library. Re-importing is safe: an entry seen before
//! updates its game, an entry whose game the user removed is skipped, and
//! nothing the user edited is overwritten.

pub mod lutris;
pub mod steam;
pub mod twintail;
pub mod vdf;

use crate::games;
use anyhow::Result;
use rusqlite::{Connection, params};
use serde_json::Value;
use somelauncher_proto::game::{NewGame, Source, SourceKind};
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct Found {
    pub source: Source,
    pub game: NewGame,
    /// The source owns the install paths (Steam moves and uninstalls games itself):
    /// they are replaced on every import instead of only filling gaps
    pub follow_paths: bool,
}

#[derive(Debug, Default, Clone, PartialEq, serde::Serialize)]
pub struct Summary {
    pub added: Vec<i64>,
    pub updated: Vec<i64>,
}

impl Summary {
    pub fn ids(&self) -> Vec<i64> {
        self.added.iter().chain(&self.updated).copied().collect()
    }
}

pub fn is_ignored(conn: &Connection, source: SourceKind, source_id: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM ignored_sources WHERE source = ?1 AND source_id = ?2)",
        params![source.as_str(), source_id],
        |r| r.get(0),
    )?)
}

pub fn apply(conn: &mut Connection, found: Vec<Found>) -> Result<Summary> {
    let tx = conn.transaction()?;
    let mut summary = Summary::default();
    for f in found {
        let (kind, sid) = (f.source.source, f.source.source_id.as_str());
        if is_ignored(&tx, kind, sid)? {
            continue;
        }
        let Some(id) = games::by_source(&tx, kind, sid)? else {
            let id = games::insert(&tx, &f.game)?;
            games::set_source(&tx, id, &f.source)?;
            summary.added.push(id);
            continue;
        };
        let Some(old) = games::get(&tx, id)? else { continue };
        let mut g = old.clone();
        let n = f.game;
        if f.follow_paths {
            (g.install_dir, g.exe, g.prefix) = (n.install_dir, n.exe, n.prefix);
        } else {
            g.install_dir = g.install_dir.or(n.install_dir);
            g.exe = g.exe.or(n.exe);
            g.prefix = g.prefix.or(n.prefix);
        }
        g.playtime = g.playtime.max(n.playtime);
        g.last_played = g.last_played.max(n.last_played);
        if let Some(src) = g.sources.iter_mut().find(|s| s.source == kind && s.source_id == sid) {
            src.data = f.source.data.clone();
        }
        if g != old {
            games::save(&tx, &g)?;
            games::set_played(&tx, id, g.playtime, g.last_played)?;
            games::set_source(&tx, id, &f.source)?;
            summary.updated.push(id);
        }
    }
    tx.commit()?;
    Ok(summary)
}

/// Games from `source` that a full scan no longer saw: `update` may change the
/// game and that source's data; changed games are saved and returned.
pub fn sweep(
    conn: &mut Connection,
    source: SourceKind,
    seen: &HashSet<String>,
    mut update: impl FnMut(&mut somelauncher_proto::game::Game, &mut Value),
) -> Result<Vec<i64>> {
    let tx = conn.transaction()?;
    let rows: Vec<(i64, String)> = tx
        .prepare("SELECT game_id, source_id FROM game_sources WHERE source = ?1")?
        .query_map([source.as_str()], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut changed = Vec::new();
    for (id, sid) in rows {
        if seen.contains(&sid) {
            continue;
        }
        let Some(old) = games::get(&tx, id)? else { continue };
        let mut g = old.clone();
        let Some(pos) = g.sources.iter().position(|s| s.source == source && s.source_id == sid) else { continue };
        let mut data = std::mem::take(&mut g.sources[pos].data);
        update(&mut g, &mut data);
        g.sources[pos].data = data;
        if g != old {
            games::save(&tx, &g)?;
            games::set_source(&tx, id, &g.sources[pos])?;
            changed.push(id);
        }
    }
    tx.commit()?;
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use somelauncher_proto::game::Kind;

    fn found(id: &str, dir: &str, playtime: i64, follow: bool) -> Found {
        let mut game = NewGame::new(format!("Game {id}"), Kind::Steam);
        game.install_dir = Some(dir.into());
        game.playtime = playtime;
        Found { source: Source { source: SourceKind::Steam, source_id: id.into(), data: json!({ "v": playtime }) }, game, follow_paths: follow }
    }

    #[test]
    fn reimport_updates_without_touching_user_edits() {
        let mut conn = games::test_db();
        let s = apply(&mut conn, vec![found("1", "/a", 60, false)]).unwrap();
        assert_eq!(s.added.len(), 1);
        let id = s.added[0];
        let mut g = games::get(&conn, id).unwrap().unwrap();
        g.name = "Renamed".into();
        games::save(&conn, &g).unwrap();

        assert_eq!(apply(&mut conn, vec![found("1", "/a", 60, false)]).unwrap(), Summary::default(), "nothing changed");
        let s = apply(&mut conn, vec![found("1", "/b", 120, false)]).unwrap();
        assert_eq!(s.updated, [id]);
        let g = games::get(&conn, id).unwrap().unwrap();
        assert_eq!((g.name.as_str(), g.install_dir.unwrap().to_str().unwrap(), g.playtime), ("Renamed", "/a", 120));
        assert_eq!(g.sources[0].data, json!({ "v": 120 }));

        apply(&mut conn, vec![found("1", "/b", 120, true)]).unwrap();
        assert_eq!(games::get(&conn, id).unwrap().unwrap().install_dir.unwrap().to_str(), Some("/b"));
    }

    #[test]
    fn removed_games_stay_removed() {
        let mut conn = games::test_db();
        let id = apply(&mut conn, vec![found("1", "/a", 0, true)]).unwrap().added[0];
        games::remove(&conn, id).unwrap();
        assert!(apply(&mut conn, vec![found("1", "/a", 0, true)]).unwrap().added.is_empty());
    }

    #[test]
    fn sweep_touches_only_unseen_games() {
        let mut conn = games::test_db();
        apply(&mut conn, vec![found("1", "/a", 0, true), found("2", "/b", 0, true)]).unwrap();
        let seen = HashSet::from(["1".to_string()]);
        let changed = sweep(&mut conn, SourceKind::Steam, &seen, |g, data| {
            g.install_dir = None;
            *data = json!({ "installed": false });
        })
        .unwrap();
        assert_eq!(changed.len(), 1);
        let g = games::get(&conn, changed[0]).unwrap().unwrap();
        assert_eq!((g.install_dir, g.sources[0].data.clone()), (None, json!({ "installed": false })));
    }
}
