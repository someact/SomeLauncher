//! Twintail Launcher: Genshin Impact and Zenless Zone Zero installs from its
//! `storage.db`, with their prefix and runner, so nothing is downloaded again.
//! Other games Twintail manages are left alone (SomeLauncher runs only these).

use super::Found;
use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};
use serde_json::json;
use somelauncher_proto::game::{Kind, NewGame, Source, SourceKind};
use std::path::{Path, PathBuf};

/// Twintail's game id (its manifest file name) → our name and the game's exe.
const GAMES: &[(&str, &str, &str)] = &[
    ("hk4e_global", "Genshin Impact", "GenshinImpact.exe"),
    ("hk4e_cn", "Genshin Impact", "YuanShen.exe"),
    ("nap_global", "Zenless Zone Zero", "ZenlessZoneZero.exe"),
    ("nap_cn", "Zenless Zone Zero", "ZenlessZoneZero.exe"),
];

fn databases(home: &Path) -> Vec<PathBuf> {
    [".local/share/twintaillauncher", ".var/app/app.twintaillauncher.ttl/data/twintaillauncher"]
        .iter()
        .map(|d| home.join(d).join("storage.db"))
        .filter(|p| p.is_file())
        .collect()
}

/// `KEY=value` pairs separated by `;` or new lines.
fn env(raw: &str) -> impl Iterator<Item = (String, String)> + '_ {
    raw.split([';', '\n']).filter_map(|kv| {
        let (k, v) = kv.trim().split_once('=')?;
        (!k.trim().is_empty()).then(|| (k.trim().to_string(), v.trim().to_string()))
    })
}

fn scan_db(db: &Path, out: &mut Vec<Found>) -> Result<()> {
    let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .with_context(|| format!("opening {}", db.display()))?;
    let mut stmt = conn.prepare(
        "SELECT i.id, m.filename, i.version, i.directory, i.runner_path, i.runner_prefix_path, i.env_vars, i.launch_args,
                i.graphics_api, i.use_mangohud, i.use_gamemode, i.use_xxmi, i.use_fps_unlock, i.fps_value, i.audio_langs,
                i.region_code, i.last_played_time, i.total_playtime, i.game_icon, i.game_background, i.use_gamescope, i.gamescope_args
         FROM install i JOIN manifest m ON m.id = i.manifest_id",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let text = |i: usize| -> rusqlite::Result<String> { Ok(r.get::<_, Option<String>>(i)?.unwrap_or_default()) };
        let flag = |i: usize| -> rusqlite::Result<bool> { Ok(r.get::<_, Option<bool>>(i)?.unwrap_or(false)) };
        let id = text(0)?;
        let biz = text(1)?.trim_end_matches(".json").to_string();
        let Some(&(_, name, exe)) = GAMES.iter().find(|(b, ..)| *b == biz) else { continue };
        let dir = PathBuf::from(text(3)?);
        if id.is_empty() || !dir.is_dir() || out.iter().any(|f| f.source.source_id == id) {
            continue;
        }

        let mut game = NewGame::new(name, Kind::Hoyo);
        game.exe = Some(dir.join(exe));
        game.install_dir = Some(dir);
        // Twintail keeps Proton's compatdata layout: the wine prefix is its `pfx`
        let compat = PathBuf::from(text(5)?);
        game.prefix = (!compat.as_os_str().is_empty()).then(|| if compat.join("pfx").is_dir() { compat.join("pfx") } else { compat.clone() });
        game.runner.proton = Some(text(4)?).filter(|p| !p.is_empty());
        game.runner.env = env(&text(6)?).collect();
        game.runner.args = shlex::split(&text(7)?).unwrap_or_default();
        game.runner.args.extend(shlex::split(&text(8)?).unwrap_or_default());
        game.runner.mangohud = flag(9)?.then_some(true);
        game.runner.gamemode = flag(10)?.then_some(true);
        if flag(20)? {
            let args = shlex::split(&text(21)?).unwrap_or_default();
            game.runner.gamescope = Some(somelauncher_proto::game::Gamescope { enabled: true, args, ..Default::default() });
        }
        game.playtime = r.get::<_, Option<i64>>(17)?.unwrap_or(0);
        game.last_played = text(16)?.parse::<i64>().ok().filter(|t| *t > 0);
        let data = json!({
            "biz": biz,
            "version": text(2)?,
            "region": text(15)?,
            "voice": text(14)?.split(',').map(str::trim).filter(|s| !s.is_empty()).collect::<Vec<_>>(),
            "compatData": compat,
            "xxmi": flag(11)?,
            "fpsUnlock": flag(12)?,
            "fps": text(13)?,
            "icon": text(18)?,
            "background": text(19)?,
        });
        out.push(Found { source: Source { source: SourceKind::Twintail, source_id: id, data }, game, follow_paths: false });
    }
    Ok(())
}

/// Genshin and ZZZ installs whose folder still exists, native Twintail first.
pub fn scan(home: &Path) -> Result<Vec<Found>> {
    let mut out = Vec::new();
    for db in databases(home) {
        scan_db(&db, &mut out)?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_hoyo_installs_that_exist() {
        let home = tempfile::tempdir().unwrap();
        let tt = home.path().join(".local/share/twintaillauncher");
        let genshin = home.path().join("Games/Genshin Impact");
        let prefix = tt.join("compatibility/prefixes/hk4e_global/g1");
        std::fs::create_dir_all(&genshin).unwrap();
        std::fs::create_dir_all(prefix.join("pfx")).unwrap();
        let db = Connection::open(tt.join("storage.db")).unwrap();
        db.execute_batch(
            "CREATE TABLE manifest (id TEXT PRIMARY KEY, filename TEXT);
             CREATE TABLE install (id TEXT PRIMARY KEY, manifest_id TEXT, version TEXT, directory TEXT, runner_path TEXT,
                runner_prefix_path TEXT, env_vars TEXT, launch_args TEXT, graphics_api TEXT, use_mangohud bool, use_gamemode bool,
                use_xxmi bool, use_fps_unlock bool, fps_value TEXT, audio_langs TEXT, region_code TEXT, last_played_time TEXT,
                total_playtime INTEGER, game_icon TEXT, game_background TEXT, use_gamescope bool, gamescope_args TEXT);
             INSERT INTO manifest VALUES ('m1', 'hk4e_global.json'), ('m2', 'nap_global.json'), ('m3', 'hkrpg_global.json');",
        )
        .unwrap();
        let insert = "INSERT INTO install VALUES (?1, ?2, '7.1.0', ?3, '/runners/proton-cachyos', ?4, 'DXVK_HUD=fps; A=b', '-popupwindow',
                      '-use-d3d12', 1, 0, 0, 0, '60', 'en-us,ja-jp', 'glb_official', '1792086960', 126738, '', '', 1, '-f --mangoapp')";
        let genshin_s = genshin.to_str().unwrap();
        db.execute(insert, ["g1", "m1", genshin_s, prefix.to_str().unwrap()]).unwrap();
        db.execute(insert, ["z1", "m2", "/nowhere/ZZZ", ""]).unwrap();
        db.execute(insert, ["s1", "m3", genshin_s, ""]).unwrap();

        let found = scan(home.path()).unwrap();
        assert_eq!(found.len(), 1, "a missing folder and a game we do not run are skipped");
        let g = &found[0];
        assert_eq!((g.game.name.as_str(), g.game.kind), ("Genshin Impact", Kind::Hoyo));
        assert_eq!(g.game.exe.as_deref(), Some(genshin.join("GenshinImpact.exe").as_path()));
        assert_eq!(g.game.prefix.as_deref(), Some(prefix.join("pfx").as_path()));
        assert_eq!(g.game.runner.proton.as_deref(), Some("/runners/proton-cachyos"));
        assert_eq!(g.game.runner.args, ["-popupwindow", "-use-d3d12"]);
        assert_eq!(g.game.runner.env["DXVK_HUD"], "fps");
        assert_eq!(g.game.runner.env["A"], "b");
        assert_eq!((g.game.playtime, g.game.last_played), (126738, Some(1792086960)));
        assert_eq!(g.source.data["voice"], json!(["en-us", "ja-jp"]));
        assert_eq!(g.source.data["version"], "7.1.0");
        assert_eq!((g.game.runner.mangohud, g.game.runner.gamemode), (Some(true), None));
        assert_eq!(g.game.runner.gamescope.as_ref().unwrap().args, ["-f", "--mangoapp"]);
    }
}
