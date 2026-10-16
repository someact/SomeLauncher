//! Lutris: installed games from `pga.db` (name, runner, playtime in hours) and
//! each game's `games/<configpath>.yml` (exe, prefix, wine version, env, DLL
//! overrides, args). Wine games become `proton` games, Linux ones `native`;
//! Lutris' Steam games are left to the Steam importer.

use super::Found;
use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};
use serde::Deserialize;
use serde_json::json;
use serde_yaml_ng::Value as Yaml;
use somelauncher_proto::game::{Kind, NewGame, Source, SourceKind};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Lutris' data dirs (with `pga.db`) and where each keeps its game configs.
fn installs(home: &Path) -> Vec<(PathBuf, Vec<PathBuf>)> {
    let native = (home.join(".local/share/lutris"), vec![home.join(".local/share/lutris/games"), home.join(".config/lutris/games")]);
    let flatpak_root = home.join(".var/app/net.lutris.Lutris");
    let flatpak = (
        flatpak_root.join("data/lutris"),
        vec![flatpak_root.join("data/lutris/games"), flatpak_root.join("config/lutris/games")],
    );
    [native, flatpak].into_iter().filter(|(data, _)| data.join("pga.db").is_file()).collect()
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Config {
    game: GameSection,
    system: SystemSection,
    wine: WineSection,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct GameSection {
    exe: Option<String>,
    prefix: Option<String>,
    working_dir: Option<String>,
    args: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SystemSection {
    env: BTreeMap<String, Yaml>,
    mangohud: Option<bool>,
    gamemode: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct WineSection {
    version: Option<String>,
    overrides: BTreeMap<String, Yaml>,
}

/// YAML scalars as the strings Lutris would pass on (`1` and `'1'` both become "1").
fn scalar(v: &Yaml) -> Option<String> {
    match v {
        Yaml::String(s) => Some(s.clone()),
        Yaml::Number(n) => Some(n.to_string()),
        Yaml::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

struct Row {
    name: String,
    slug: String,
    runner: String,
    directory: Option<String>,
    configpath: String,
    hours: f64,
    lastplayed: Option<i64>,
}

fn rows(db: &Path) -> Result<Vec<Row>> {
    let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .with_context(|| format!("opening {}", db.display()))?;
    let mut stmt = conn.prepare(
        "SELECT name, slug, runner, directory, configpath, playtime, lastplayed FROM games
         WHERE installed = 1 AND configpath IS NOT NULL AND configpath != '' ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Row {
            name: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
            slug: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
            runner: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
            directory: r.get::<_, Option<String>>(3)?.filter(|d| !d.is_empty()),
            configpath: r.get(4)?,
            hours: r.get::<_, Option<f64>>(5)?.unwrap_or(0.0),
            lastplayed: r.get::<_, Option<i64>>(6)?.filter(|t| *t > 0),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn game(row: Row, config: Config, data_dir: &Path) -> Option<Found> {
    let kind = match row.runner.as_str() {
        "wine" | "proton" => Kind::Proton,
        "linux" => Kind::Native,
        // steam: the Steam importer has it; dosbox, emulators, …: not run by SomeLauncher
        _ => return None,
    };
    let g = config.game;
    let working_dir = g.working_dir.filter(|d| !d.is_empty()).map(PathBuf::from);
    let directory = row.directory.map(PathBuf::from);
    // A relative exe is relative to the game directory
    let exe = g.exe.filter(|e| !e.is_empty()).map(PathBuf::from).map(|e| match (&directory, e.is_relative()) {
        (Some(dir), true) => dir.join(e),
        _ => e,
    });
    let mut new = NewGame::new(row.name, kind);
    new.install_dir = directory.or_else(|| working_dir.clone()).or_else(|| exe.as_ref().and_then(|e| e.parent().map(Path::to_path_buf)));
    new.exe = exe;
    new.prefix = (kind == Kind::Proton).then_some(g.prefix).flatten().filter(|p| !p.is_empty()).map(PathBuf::from);
    new.runner.working_dir = working_dir;
    new.runner.args = g.args.as_deref().and_then(shlex::split).unwrap_or_default();
    new.runner.env = config.system.env.iter().filter_map(|(k, v)| Some((k.clone(), scalar(v)?))).collect();
    if kind == Kind::Proton {
        new.runner.proton = config.wine.version.filter(|v| !v.is_empty());
        new.runner.dll_overrides = config.wine.overrides.iter().filter_map(|(k, v)| Some((k.clone(), scalar(v)?))).collect();
    }
    new.runner.mangohud = config.system.mangohud;
    new.runner.gamemode = config.system.gamemode;
    new.playtime = (row.hours * 3600.0).round() as i64;
    new.last_played = row.lastplayed;

    let art = |dir: &str| ["jpg", "png"].iter().map(|ext| data_dir.join(dir).join(format!("{}.{ext}", row.slug))).find(|p| p.is_file());
    let data = json!({ "runner": row.runner, "configpath": row.configpath, "coverart": art("coverart"), "banner": art("banners") });
    Some(Found { source: Source { source: SourceKind::Lutris, source_id: row.slug, data }, game: new, follow_paths: false })
}

/// Installed Lutris games, once per slug.
pub fn scan(home: &Path) -> Result<Vec<Found>> {
    let mut out: Vec<Found> = Vec::new();
    for (data_dir, config_dirs) in installs(home) {
        for row in rows(&data_dir.join("pga.db"))? {
            if row.slug.is_empty() || out.iter().any(|f| f.source.source_id == row.slug) {
                continue;
            }
            let file = format!("{}.yml", row.configpath);
            let Some(path) = config_dirs.iter().map(|d| d.join(&file)).find(|p| p.is_file()) else {
                tracing::debug!(slug = row.slug, "no Lutris config {file}");
                continue;
            };
            let config: Config = match std::fs::read_to_string(&path).map_err(anyhow::Error::from).and_then(|t| Ok(serde_yaml_ng::from_str(&t)?)) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("{}: {e:#}", path.display());
                    continue;
                }
            };
            out.extend(game(row, config, &data_dir));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> tempfile::TempDir {
        let home = tempfile::tempdir().unwrap();
        let data = home.path().join(".local/share/lutris");
        std::fs::create_dir_all(data.join("games")).unwrap();
        std::fs::create_dir_all(data.join("coverart")).unwrap();
        std::fs::write(data.join("coverart/palworld.jpg"), "").unwrap();
        let db = Connection::open(data.join("pga.db")).unwrap();
        db.execute_batch(
            "CREATE TABLE games (id INTEGER PRIMARY KEY, name TEXT, slug TEXT, runner TEXT, directory TEXT, installed INTEGER,
                configpath TEXT, playtime REAL, lastplayed INTEGER);
             INSERT INTO games VALUES (1, 'Palworld', 'palworld', 'wine', '', 1, 'palworld-1', 0.5, 1790159026);
             INSERT INTO games VALUES (2, 'Valheim', 'valheim', 'linux', '/g/Valheim', 1, 'valheim-1', 0, 0);
             INSERT INTO games VALUES (3, 'Dota 2', 'dota-2', 'steam', '', 1, 'dota-2-1', 9, 1);
             INSERT INTO games VALUES (4, 'Gone', 'gone', 'wine', '', 0, 'gone-1', 0, 0);",
        )
        .unwrap();
        std::fs::write(
            data.join("games/palworld-1.yml"),
            "game:\n  exe: /g/Palworld/Palworld.exe\n  prefix: /g/Prefixes/Palworld\n  args: -windowed \"-name=A B\"\n\
             system:\n  mangohud: true\n  env:\n    DXVK_HUD: fps\n    PROTON_NVIDIA_NVML: 1\n\
             wine:\n  overrides:\n    winmm: n,b\n  version: GE-Proton11-7\n",
        )
        .unwrap();
        std::fs::write(data.join("games/valheim-1.yml"), "game:\n  exe: valheim.x86_64\n  working_dir: /g/Valheim\n").unwrap();
        std::fs::write(data.join("games/dota-2-1.yml"), "game:\n  appid: 570\n").unwrap();
        home
    }

    #[test]
    fn reads_wine_and_linux_games_and_skips_the_rest() {
        let home = fixture();
        let found = scan(home.path()).unwrap();
        assert_eq!(found.iter().map(|f| f.source.source_id.as_str()).collect::<Vec<_>>(), ["palworld", "valheim"]);

        let pal = &found[0];
        assert_eq!(pal.game.kind, Kind::Proton);
        assert_eq!(pal.game.exe.as_deref(), Some(Path::new("/g/Palworld/Palworld.exe")));
        assert_eq!(pal.game.install_dir.as_deref(), Some(Path::new("/g/Palworld")));
        assert_eq!(pal.game.prefix.as_deref(), Some(Path::new("/g/Prefixes/Palworld")));
        assert_eq!(pal.game.runner.proton.as_deref(), Some("GE-Proton11-7"));
        assert_eq!(pal.game.runner.args, ["-windowed", "-name=A B"]);
        assert_eq!(pal.game.runner.env["PROTON_NVIDIA_NVML"], "1");
        assert_eq!(pal.game.runner.dll_overrides["winmm"], "n,b");
        assert_eq!(pal.game.runner.mangohud, Some(true));
        assert_eq!(pal.game.playtime, 1800);
        assert!(pal.source.data["coverart"].as_str().unwrap().ends_with("coverart/palworld.jpg"));

        let val = &found[1];
        assert_eq!(val.game.kind, Kind::Native);
        assert_eq!(val.game.exe.as_deref(), Some(Path::new("/g/Valheim/valheim.x86_64")));
        assert_eq!((val.game.prefix.as_ref(), val.game.last_played), (None, None));
    }
}
