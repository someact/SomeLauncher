//! Library queries: the `games` and `game_sources` tables as `proto::game` values.

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Row, params};
use somelauncher_proto::game::{Build, Game, Kind, Meta, NewGame, Source, SourceKind};
use std::path::PathBuf;

const COLUMNS: &str =
    "id, name, kind, install_dir, exe, prefix, runner, tags, categories, favorite, hidden, playtime, last_played, added, build";

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
        meta: None,
        art: Default::default(),
        build: json(row, 14)?,
    })
}

/// What `build` works out from the sources: the installed Steam build (or a
/// Hoyo game's version), and
/// whether a newer one is out (or Steam itself has an update waiting).
fn fill_build(g: &mut Game) {
    let steam = g.sources.iter().find(|s| s.source == SourceKind::Steam);
    let installed = steam.filter(|s| s.data["installed"] == true).and_then(|s| s.data["buildid"].as_str()).filter(|b| !b.is_empty() && *b != "0");
    // A Hoyo game: the version read from its files
    let hoyo = g.sources.iter().find(|s| s.source == SourceKind::Hoyo).and_then(|s| s.data["version"].as_str());
    g.build.installed = installed.or(hoyo).map(String::from);
    // StateFlags bit 2: update required
    let steam_says = steam.is_some_and(|s| s.data["installed"] == true && s.data["stateFlags"].as_i64().unwrap_or(0) & 2 != 0);
    let newer = matches!((&g.build.installed, &g.build.latest), (Some(i), Some(l)) if i != l);
    g.build.update_available = steam_says || newer;
}

/// The stored part of a build.
fn stored_build(b: &Build) -> String {
    let stored = Build { installed: None, update_available: false, ..b.clone() };
    serde_json::to_string(&stored).unwrap_or_else(|_| "{}".into())
}

fn meta(conn: &Connection, id: i64) -> Result<Option<Meta>> {
    let raw: Option<String> = conn.prepare_cached("SELECT data FROM meta WHERE game_id = ?1")?.query_row([id], |r| r.get(0)).optional()?;
    Ok(raw.map(|d| serde_json::from_str(&d)).transpose()?)
}

pub fn set_meta(conn: &Connection, id: i64, meta: Option<&Meta>) -> Result<()> {
    match meta {
        Some(m) => conn.execute(
            "INSERT INTO meta (game_id, data) VALUES (?1, ?2) ON CONFLICT(game_id) DO UPDATE SET data = excluded.data",
            params![id, serde_json::to_string(m)?],
        )?,
        None => conn.execute("DELETE FROM meta WHERE game_id = ?1", [id])?,
    };
    Ok(())
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
    game.meta = meta(conn, id)?;
    game.art = crate::meta::art::of(conn, id)?;
    fill_build(&mut game);
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
        g.meta = meta(conn, g.id)?;
        g.art = crate::meta::art::of(conn, g.id)?;
        fill_build(g);
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
         tags = ?8, categories = ?9, favorite = ?10, hidden = ?11, build = ?12 WHERE id = ?1",
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
            stored_build(&g.build),
        ],
    )?;
    anyhow::ensure!(n == 1, "no game {}", g.id);
    Ok(())
}

pub fn set_played(conn: &Connection, id: i64, playtime: i64, last_played: Option<i64>) -> Result<()> {
    conn.execute("UPDATE games SET playtime = ?2, last_played = ?3 WHERE id = ?1", params![id, playtime, last_played])?;
    Ok(())
}

/// A game already in the library at the same place: the same exe, or the same folder and kind.
pub fn same_install(conn: &Connection, g: &NewGame) -> Result<Option<i64>> {
    let canon = |p: &std::path::Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let exe = g.exe.as_deref().map(canon);
    let dir = g.install_dir.as_deref().map(canon);
    if exe.is_none() && dir.is_none() {
        return Ok(None);
    }
    let mut stmt = conn.prepare_cached("SELECT id, kind, install_dir, exe FROM games WHERE install_dir IS NOT NULL OR exe IS NOT NULL")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let (other_dir, other_exe) = (r.get::<_, Option<String>>(2)?, r.get::<_, Option<String>>(3)?);
        let same_exe = exe.is_some() && other_exe.map(|e| canon(e.as_ref())) == exe;
        let same_dir = dir.is_some() && r.get::<_, String>(1)? == g.kind.as_str() && other_dir.map(|d| canon(d.as_ref())) == dir;
        if same_exe || same_dir {
            return Ok(Some(r.get(0)?));
        }
    }
    Ok(None)
}

/// Folds `from` into `into`: its sources move over, empty fields are filled, tags
/// and categories are joined and the playtimes added (two copies played apart).
pub fn merge(conn: &Connection, from: i64, into: i64) -> Result<Game> {
    anyhow::ensure!(from != into, "a game cannot be merged into itself");
    let tx = conn.unchecked_transaction()?;
    let conn = &*tx;
    let src = get(conn, from)?.with_context(|| format!("no game {from}"))?;
    let mut g = get(conn, into)?.with_context(|| format!("no game {into}"))?;
    g.install_dir = g.install_dir.or(src.install_dir);
    g.exe = g.exe.or(src.exe);
    g.prefix = g.prefix.or(src.prefix);
    if g.runner == Default::default() {
        g.runner = src.runner;
    }
    for t in src.tags {
        if !g.tags.contains(&t) {
            g.tags.push(t);
        }
    }
    for c in src.categories {
        if !g.categories.contains(&c) {
            g.categories.push(c);
        }
    }
    g.favorite |= src.favorite;
    g.hidden &= src.hidden;
    g.playtime += src.playtime;
    g.last_played = g.last_played.max(src.last_played);
    save(conn, &g)?;
    set_played(conn, into, g.playtime, g.last_played)?;
    if g.meta.is_none() && src.meta.is_some() {
        set_meta(conn, into, src.meta.as_ref())?;
    }
    conn.execute("UPDATE game_sources SET game_id = ?2 WHERE game_id = ?1", params![from, into])?;
    conn.execute("DELETE FROM games WHERE id = ?1", [from])?;
    let merged = get(conn, into)?.expect("the game was just saved");
    tx.commit()?;
    Ok(merged)
}

/// Games that look like the same game (same name, ignoring case and punctuation), in groups.
pub fn duplicates(conn: &Connection) -> Result<Vec<Vec<Game>>> {
    let mut groups: std::collections::BTreeMap<String, Vec<Game>> = Default::default();
    for g in list(conn, true)? {
        groups.entry(crate::guess::normalized(&g.name)).or_default().push(g);
    }
    Ok(groups.into_values().filter(|g| g.len() > 1).collect())
}

/// Deletes the game. Its imported sources are remembered so a re-import does not bring it back.
pub fn remove(conn: &Connection, id: i64) -> Result<bool> {
    conn.execute(
        "INSERT OR IGNORE INTO ignored_sources (source, source_id)
         SELECT source, source_id FROM game_sources WHERE game_id = ?1 AND source != 'manual'",
        [id],
    )?;
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
    fn merge_moves_sources_and_adds_playtime() {
        let conn = test_db();
        let mut a = NewGame::new("Palworld", Kind::Steam);
        a.playtime = 100;
        let a = insert(&conn, &a).unwrap();
        set_source(&conn, a, &Source { source: SourceKind::Steam, source_id: "1623730".into(), data: json!(null) }).unwrap();
        let mut b = NewGame::new("PALWORLD", Kind::Proton);
        b.playtime = 50;
        b.tags = vec!["survival".into()];
        b.prefix = Some("/p/pal".into());
        let b = insert(&conn, &b).unwrap();
        set_source(&conn, b, &Source { source: SourceKind::Lutris, source_id: "palworld".into(), data: json!(null) }).unwrap();

        assert_eq!(duplicates(&conn).unwrap().len(), 1);
        let g = merge(&conn, b, a).unwrap();
        assert_eq!((g.name.as_str(), g.kind, g.playtime, g.sources.len()), ("Palworld", Kind::Steam, 150, 2));
        assert_eq!((g.tags, g.prefix.unwrap().to_str()), (vec!["survival".to_string()], Some("/p/pal")));
        assert!(get(&conn, b).unwrap().is_none());
        assert!(duplicates(&conn).unwrap().is_empty());
        assert!(merge(&conn, a, a).is_err());
    }

    #[test]
    fn builds_compare_the_installed_steam_build_with_the_latest() {
        let conn = test_db();
        let id = insert(&conn, &NewGame::new("R.E.P.O.", Kind::Steam)).unwrap();
        let src = |build: &str, flags: i64| Source {
            source: SourceKind::Steam,
            source_id: "3241660".into(),
            data: json!({ "installed": true, "buildid": build, "stateFlags": flags }),
        };
        set_source(&conn, id, &src("100", 4)).unwrap();
        let mut g = get(&conn, id).unwrap().unwrap();
        assert_eq!((g.build.installed.as_deref(), g.build.update_available), (Some("100"), false));
        g.build.latest = Some("101".into());
        g.build.recorded = Some("v1".into());
        save(&conn, &g).unwrap();
        let g = get(&conn, id).unwrap().unwrap();
        assert!(g.build.update_available);
        assert_eq!(g.build.recorded.as_deref(), Some("v1"));
        set_source(&conn, id, &src("101", 4)).unwrap();
        assert!(!get(&conn, id).unwrap().unwrap().build.update_available);
        set_source(&conn, id, &src("101", 6)).unwrap();
        assert!(get(&conn, id).unwrap().unwrap().build.update_available, "Steam has an update waiting");
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
