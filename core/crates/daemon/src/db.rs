//! The sqlite library. Migrations are numbered by `PRAGMA user_version`: each
//! entry in MIGRATIONS runs once, in order, inside one transaction.

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;
use std::path::Path;

const MIGRATIONS: &[&str] = &[
    // 1: daemon settings (game folders, API keys, defaults), one JSON value per key
    "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    // 2: the library (proto::game). runner/tags/categories are JSON; times are unix seconds
    "CREATE TABLE games (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL,
        kind TEXT NOT NULL,
        install_dir TEXT,
        exe TEXT,
        prefix TEXT,
        runner TEXT NOT NULL DEFAULT '{}',
        tags TEXT NOT NULL DEFAULT '[]',
        categories TEXT NOT NULL DEFAULT '[]',
        favorite INTEGER NOT NULL DEFAULT 0,
        hidden INTEGER NOT NULL DEFAULT 0,
        playtime INTEGER NOT NULL DEFAULT 0,
        last_played INTEGER,
        added INTEGER NOT NULL
    );
    CREATE TABLE game_sources (
        game_id INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
        source TEXT NOT NULL,
        source_id TEXT NOT NULL,
        data TEXT,
        PRIMARY KEY (source, source_id)
    );
    CREATE INDEX game_sources_game ON game_sources(game_id);",
    // 3: source entries whose game the user removed, so importers skip them
    "CREATE TABLE ignored_sources (source TEXT NOT NULL, source_id TEXT NOT NULL, PRIMARY KEY (source, source_id));",
    // 4: play sessions; `seen` is the last heartbeat, so a crash loses at most a minute
    "CREATE TABLE sessions (
        id INTEGER PRIMARY KEY,
        game_id INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
        unit TEXT NOT NULL,
        started INTEGER NOT NULL,
        seen INTEGER NOT NULL,
        ended INTEGER
    );
    CREATE INDEX sessions_game ON sessions(game_id);
    CREATE INDEX sessions_open ON sessions(ended) WHERE ended IS NULL;",
];

pub fn open(path: &Path) -> Result<Connection> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", true)?;
    migrate(&mut conn)?;
    Ok(conn)
}

pub fn migrate(conn: &mut Connection) -> Result<()> {
    let current: usize = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    anyhow::ensure!(current <= MIGRATIONS.len(), "library.db is from a newer somelauncherd (schema {current})");
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", i + 1)?;
        tx.commit()?;
        tracing::info!(version = i + 1, "migrated library.db");
    }
    Ok(())
}

pub fn setting(conn: &Connection, key: &str) -> Result<Option<Value>> {
    let raw: Option<String> = conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional()?;
    Ok(raw.map(|s| serde_json::from_str(&s)).transpose()?)
}

pub fn settings(conn: &Connection) -> Result<serde_json::Map<String, Value>> {
    let mut stmt = conn.prepare("SELECT key, value FROM settings ORDER BY key")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut map = serde_json::Map::new();
    for row in rows {
        let (k, v) = row?;
        map.insert(k, serde_json::from_str(&v)?);
    }
    Ok(map)
}

/// A null value removes the key.
pub fn set_setting(conn: &Connection, key: &str, value: &Value) -> Result<()> {
    if value.is_null() {
        conn.execute("DELETE FROM settings WHERE key = ?1", [key])?;
    } else {
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value.to_string()],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn migrations_run_once_and_settings_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        let conn = open(&path).unwrap();
        set_setting(&conn, "gameDirs", &json!(["/games"])).unwrap();
        drop(conn);

        let conn = open(&path).unwrap();
        let v: usize = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
        assert_eq!(v, MIGRATIONS.len());
        assert_eq!(setting(&conn, "gameDirs").unwrap(), Some(json!(["/games"])));
        set_setting(&conn, "gameDirs", &Value::Null).unwrap();
        assert!(settings(&conn).unwrap().is_empty());
    }

    #[test]
    fn refuses_a_newer_schema() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", 999).unwrap();
        assert!(migrate(&mut conn).is_err());
    }
}
