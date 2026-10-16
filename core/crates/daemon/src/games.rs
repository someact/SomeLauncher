//! Library queries: the `games` and `game_sources` tables as `proto::game` values.

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Row, params};
use somelauncher_proto::game::{Game, Kind, NewGame, Source, SourceKind};
use std::path::PathBuf;

const COLUMNS: &str =
    "id, name, kind, install_dir, exe, prefix, runner, tags, categories, favorite, hidden, playtime, last_played, added";

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn path(p: &Option<PathBuf>) -> Option<String> {
    p.as_ref().map(|p| p.to_string_lossy().into_owned())
}

fn json<T: serde::de::DeserializeOwned>(row: &Row, i: usize) -> rusqlite::Result<T> {
    let raw: String = row.get(i)?;
    serde_json::from_str(&raw).map_err(|e| rusqlite::Error::FromSqlConversionFailure(i, rusqlite::types::Type::Text, e.into()))
}

fn from_row(row: &Row) -> rusqlite::Result<Game> {
    let kind: String = row.get(2)?;
    Ok(Game {
        id: row.get(0)?,
        name: row.get(1)?,
        kind: Kind::parse(&kind).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, format!("unknown kind {kind}").into())
        })?,
        install_dir: row.get::<_, Option<String>>(3)?.map(PathBuf::from),
        exe: row.get::<_, Option<String>>(4)?.map(PathBuf::from),
        prefix: row.get::<_, Option<String>>(5)?.map(PathBuf::from),
        runner: json(row, 6)?,
        tags: json(row, 7)?,
        categories: json(row, 8)?,
        favorite: row.get(9)?,
        hidden: row.get(10)?,
        playtime: row.get(11)?,
        last_played: row.get(12)?,
        added: row.get(13)?,
        sources: Vec::new(),
    })
}

fn sources(conn: &Connection, id: i64) -> Result<Vec<Source>> {
    let mut stmt = conn.prepare_cached("SELECT source, source_id, data FROM game_sources WHERE game_id = ?1 ORDER BY source")?;
    let rows = stmt.query_map([id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?)))?;
    let mut out = Vec::new();
    for row in rows {
        let (source, source_id, data) = row?;
        let Some(source) = SourceKind::parse(&source) else { continue };
        let data = data.map(|d| serde_json::from_str(&d)).transpose()?.unwrap_or_default();
        out.push(Source { source, source_id, data });
    }
    Ok(out)
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<Game>> {
    let game = conn
        .query_row(&format!("SELECT {COLUMNS} FROM games WHERE id = ?1"), [id], from_row)
        .optional()?;
    let Some(mut game) = game else { return Ok(None) };
    game.sources = sources(conn, id)?;
    Ok(Some(game))
}

/// Every game, by name. Hidden games only when asked for.
pub fn list(conn: &Connection, include_hidden: bool) -> Result<Vec<Game>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM games WHERE ?1 OR hidden = 0 ORDER BY name COLLATE NOCASE, id"
    ))?;
    let mut games = stmt.query_map([include_hidden], from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    for g in &mut games {
        g.sources = sources(conn, g.id)?;
    }
    Ok(games)
}

pub fn insert(conn: &Connection, g: &NewGame) -> Result<i64> {
    conn.execute(
        "INSERT INTO games (name, kind, install_dir, exe, prefix, runner, tags, categories, playtime, last_played, added)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            g.name,
            g.kind.as_str(),
            path(&g.install_dir),
            path(&g.exe),
            path(&g.prefix),
            serde_json::to_string(&g.runner)?,
            serde_json::to_string(&g.tags)?,
            serde_json::to_string(&g.categories)?,
            g.playtime,
            g.last_played,
            now(),
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Writes every editable field of `g` back (not playtime, last played or sources).
pub fn save(conn: &Connection, g: &Game) -> Result<()> {
    let n = conn.execute(
        "UPDATE games SET name = ?2, kind = ?3, install_dir = ?4, exe = ?5, prefix = ?6, runner = ?7,
         tags = ?8, categories = ?9, favorite = ?10, hidden = ?11 WHERE id = ?1",
        params![
            g.id,
            g.name,
            g.kind.as_str(),
            path(&g.install_dir),
            path(&g.exe),
            path(&g.prefix),
            serde_json::to_string(&g.runner)?,
            serde_json::to_string(&g.tags)?,
            serde_json::to_string(&g.categories)?,
            g.favorite,
            g.hidden,
        ],
    )?;
    anyhow::ensure!(n == 1, "no game {}", g.id);
    Ok(())
}

pub fn remove(conn: &Connection, id: i64) -> Result<bool> {
    Ok(conn.execute("DELETE FROM games WHERE id = ?1", [id])? == 1)
}

/// Links a source entry to a game, replacing what that source said before.
pub fn set_source(conn: &Connection, id: i64, s: &Source) -> Result<()> {
    let data = (!s.data.is_null()).then(|| s.data.to_string());
    conn.execute(
        "INSERT INTO game_sources (game_id, source, source_id, data) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(source, source_id) DO UPDATE SET game_id = excluded.game_id, data = excluded.data",
        params![id, s.source.as_str(), s.source_id, data],
    )
    .with_context(|| format!("linking {}:{} to game {id}", s.source.as_str(), s.source_id))?;
    Ok(())
}

/// The game a source entry belongs to, if it was imported before.
pub fn by_source(conn: &Connection, source: SourceKind, source_id: &str) -> Result<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT game_id FROM game_sources WHERE source = ?1 AND source_id = ?2",
            params![source.as_str(), source_id],
            |r| r.get(0),
        )
        .optional()?)
}

#[cfg(test)]
pub(crate) fn test_db() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    crate::db::migrate(&mut conn).unwrap();
    conn
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn insert_get_save_remove() {
        let conn = test_db();
        let mut new = NewGame::new("Valheim", Kind::Proton);
        new.exe = Some("/games/Valheim/valheim.exe".into());
        new.runner.dll_overrides.insert("winhttp".into(), "n,b".into());
        let id = insert(&conn, &new).unwrap();
        set_source(&conn, id, &Source { source: SourceKind::Lutris, source_id: "valheim".into(), data: json!({ "runner": "wine" }) })
            .unwrap();

        let mut g = get(&conn, id).unwrap().unwrap();
        assert_eq!(g.exe, new.exe);
        assert_eq!(g.runner, new.runner);
        assert_eq!(g.sources[0].source, SourceKind::Lutris);
        assert_eq!(by_source(&conn, SourceKind::Lutris, "valheim").unwrap(), Some(id));

        g.hidden = true;
        g.tags = vec!["survival".into()];
        save(&conn, &g).unwrap();
        assert!(list(&conn, false).unwrap().is_empty());
        assert_eq!(list(&conn, true).unwrap(), vec![g]);

        assert!(remove(&conn, id).unwrap());
        assert_eq!(by_source(&conn, SourceKind::Lutris, "valheim").unwrap(), None, "sources go with the game");
    }

    #[test]
    fn list_sorts_by_name_ignoring_case() {
        let conn = test_db();
        for n in ["zenless", "Genshin", "apex"] {
            insert(&conn, &NewGame::new(n, Kind::Native)).unwrap();
        }
        let names: Vec<_> = list(&conn, false).unwrap().into_iter().map(|g| g.name).collect();
        assert_eq!(names, ["apex", "Genshin", "zenless"]);
    }
}
