//! Steam: installed games from every library folder's `appmanifest_*.acf`,
//! playtime from `userdata/*/config/localconfig.vdf`, Steam's own Proton
//! choice from `config/config.vdf` (shown read-only), and owned games that are
//! not installed through the Web API when a key is set.

use super::Found;
use super::vdf::{self, Vdf};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use somelauncher_proto::game::{Kind, NewGame, Source, SourceKind};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

/// Redistributables and tools that ship as apps but are not games.
const NOT_GAMES: &[&str] = &["228980"];

/// Every Steam install in this home (native and flatpak), each once.
pub fn roots(home: &Path) -> Vec<PathBuf> {
    let candidates = [
        ".steam/root",
        ".steam/steam",
        ".local/share/Steam",
        ".var/app/com.valvesoftware.Steam/.local/share/Steam",
        ".var/app/com.valvesoftware.Steam/data/Steam",
    ];
    let mut out: Vec<PathBuf> = Vec::new();
    for c in candidates {
        if let Ok(p) = home.join(c).canonicalize() {
            if p.join("steamapps").is_dir() && !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}

fn read_vdf(path: &Path) -> Result<Vdf> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    vdf::parse(&text).with_context(|| format!("parsing {}", path.display()))
}

/// The library folders of one Steam install, the install itself first.
pub fn libraries(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![root.to_path_buf()];
    if let Ok(v) = read_vdf(&root.join("steamapps/libraryfolders.vdf")) {
        for (_, folder) in v.get("libraryfolders").map(Vdf::entries).unwrap_or_default() {
            if let Some(p) = folder.str_at(&["path"]) {
                let p = PathBuf::from(p).canonicalize().unwrap_or_else(|_| p.into());
                if !out.contains(&p) {
                    out.push(p);
                }
            }
        }
    }
    out
}

#[derive(Debug, Default, Clone, Copy)]
struct Played {
    minutes: i64,
    last: i64,
}

/// Playtime per appid, over every Steam account that used this install.
fn playtime(root: &Path) -> HashMap<String, Played> {
    let mut out: HashMap<String, Played> = HashMap::new();
    let Ok(users) = std::fs::read_dir(root.join("userdata")) else { return out };
    for user in users.flatten() {
        let Ok(v) = read_vdf(&user.path().join("config/localconfig.vdf")) else { continue };
        let apps = v.at(&["UserLocalConfigStore", "Software", "Valve", "Steam", "apps"]);
        for (appid, app) in apps.map(Vdf::entries).unwrap_or_default() {
            let num = |k| app.str_at(&[k]).and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
            let p = out.entry(appid.clone()).or_default();
            p.minutes = p.minutes.max(num("Playtime"));
            p.last = p.last.max(num("LastPlayed"));
        }
    }
    out
}

/// Steam's per-game compatibility tool choice (`appid` → tool name).
fn compat_tools(root: &Path) -> HashMap<String, String> {
    let Ok(v) = read_vdf(&root.join("config/config.vdf")) else { return HashMap::new() };
    let map = v.at(&["InstallConfigStore", "Software", "Valve", "Steam", "CompatToolMapping"]);
    map.map(Vdf::entries)
        .unwrap_or_default()
        .iter()
        .filter_map(|(id, e)| Some((id.clone(), e.str_at(&["name"]).filter(|n| !n.is_empty())?.to_string())))
        .collect()
}

/// The SteamID64 of the account that logged in last, for the Web API.
pub fn last_user(root: &Path) -> Option<String> {
    let v = read_vdf(&root.join("config/loginusers.vdf")).ok()?;
    let users = v.get("users")?.entries();
    let recent = users.iter().find(|(_, u)| u.str_at(&["MostRecent"]) == Some("1"));
    let newest = || users.iter().max_by_key(|(_, u)| u.str_at(&["Timestamp"]).and_then(|t| t.parse::<i64>().ok()).unwrap_or(0));
    recent.or_else(newest).map(|(id, _)| id.clone())
}

fn is_tool(appid: &str, name: &str, dir: &Path) -> bool {
    NOT_GAMES.contains(&appid)
        || dir.join("toolmanifest.vdf").exists()
        || name.starts_with("Proton ")
        || name.starts_with("Steam Linux Runtime")
}

fn found(appid: &str, name: &str, install_dir: Option<PathBuf>, played: Played, data: Value) -> Found {
    let mut game = NewGame::new(name, Kind::Steam);
    game.install_dir = install_dir;
    game.playtime = played.minutes * 60;
    game.last_played = (played.last > 0).then_some(played.last);
    Found { source: Source { source: SourceKind::Steam, source_id: appid.into(), data }, game, follow_paths: true }
}

/// Installed games across every Steam install and library folder, by appid.
pub fn installed(home: &Path) -> BTreeMap<String, Found> {
    let mut out = BTreeMap::new();
    for root in roots(home) {
        let played = playtime(&root);
        let tools = compat_tools(&root);
        let flatpak = root.to_string_lossy().contains("com.valvesoftware.Steam");
        for lib in libraries(&root) {
            let apps = lib.join("steamapps");
            let Ok(entries) = std::fs::read_dir(&apps) else { continue };
            for entry in entries.flatten() {
                let file = entry.file_name();
                let file = file.to_string_lossy();
                if !(file.starts_with("appmanifest_") && file.ends_with(".acf")) {
                    continue;
                }
                let manifest = match read_vdf(&entry.path()) {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!("{e:#}");
                        continue;
                    }
                };
                let s = |k| manifest.str_at(&["AppState", k]).unwrap_or_default();
                let (appid, name) = (s("appid"), s("name"));
                let dir = apps.join("common").join(s("installdir"));
                if appid.is_empty() || name.is_empty() || is_tool(appid, name, &dir) || out.contains_key(appid) {
                    continue;
                }
                let mut p = played.get(appid).copied().unwrap_or_default();
                p.last = p.last.max(s("LastPlayed").parse().unwrap_or(0));
                let data = json!({
                    "installed": true,
                    "buildid": s("buildid"),
                    "stateFlags": s("StateFlags").parse::<i64>().unwrap_or(0),
                    "library": lib,
                    "proton": tools.get(appid),
                    "flatpak": flatpak,
                });
                out.insert(appid.to_string(), found(appid, name, Some(dir), p, data));
            }
        }
    }
    out
}

#[derive(Debug, serde::Deserialize)]
struct Owned {
    appid: u64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    playtime_forever: i64,
    #[serde(default)]
    rtime_last_played: i64,
}

async fn owned(http: &reqwest::Client, key: &str, steam_id: &str) -> Result<Vec<Owned>> {
    #[derive(serde::Deserialize)]
    struct Body {
        response: Response,
    }
    #[derive(serde::Deserialize)]
    struct Response {
        #[serde(default)]
        games: Vec<Owned>,
    }
    let body: Body = http
        .get("https://api.steampowered.com/IPlayerService/GetOwnedGames/v1/")
        .query(&[("key", key), ("steamid", steam_id), ("include_appinfo", "1"), ("include_played_free_games", "1")])
        .send()
        .await?
        .error_for_status()
        .context("Steam Web API (is the key right and the profile's game details public?)")?
        .json()
        .await?;
    Ok(body.response.games)
}

/// Installed games, plus owned ones when `key` is set. Returns the entries and whether
/// the owned list was fetched (then a game missing from it is no longer owned).
pub async fn scan(home: &Path, http: &reqwest::Client, key: Option<&str>, steam_id: Option<&str>) -> Result<(Vec<Found>, bool)> {
    let mut games = installed(home);
    let Some(key) = key.filter(|k| !k.is_empty()) else { return Ok((games.into_values().collect(), false)) };
    let steam_id = match steam_id.filter(|s| !s.is_empty()) {
        Some(s) => s.to_string(),
        None => roots(home).iter().find_map(|r| last_user(r)).context("no Steam account found; set steamId")?,
    };
    for o in owned(http, key, &steam_id).await? {
        let appid = o.appid.to_string();
        let played = Played { minutes: o.playtime_forever, last: o.rtime_last_played };
        match games.get_mut(&appid) {
            Some(f) => {
                f.game.playtime = f.game.playtime.max(played.minutes * 60);
                f.game.last_played = f.game.last_played.max((played.last > 0).then_some(played.last));
            }
            None if !o.name.is_empty() => {
                games.insert(appid.clone(), found(&appid, &o.name, None, played, json!({ "installed": false })));
            }
            None => {}
        }
    }
    Ok((games.into_values().collect(), true))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn acf(appid: &str, name: &str, dir: &str) -> String {
        format!(
            "\"AppState\"\n{{\n\t\"appid\"\t\t\"{appid}\"\n\t\"name\"\t\t\"{name}\"\n\t\"installdir\"\t\t\"{dir}\"\n\t\"buildid\"\t\t\"42\"\n\t\"LastPlayed\"\t\t\"1700000000\"\n\t\"StateFlags\"\t\t\"4\"\n}}"
        )
    }

    /// A home with a Steam install, a second library folder, a tool and a playtime entry.
    fn fixture() -> tempfile::TempDir {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join(".local/share/Steam");
        let lib2 = home.path().join("games/SteamLibrary");
        std::fs::create_dir_all(lib2.join("steamapps")).unwrap();
        write(
            &root.join("steamapps/libraryfolders.vdf"),
            &format!(
                "\"libraryfolders\" {{ \"0\" {{ \"path\" \"{}\" }} \"1\" {{ \"path\" \"{}\" }} }}",
                root.display(),
                lib2.display()
            ),
        );
        write(&root.join("steamapps/appmanifest_480.acf"), &acf("480", "Spacewar", "Spacewar"));
        write(&lib2.join("steamapps/appmanifest_3241660.acf"), &acf("3241660", "R.E.P.O.", "REPO"));
        write(&lib2.join("steamapps/appmanifest_2180100.acf"), &acf("2180100", "Proton Hotfix", "Proton Hotfix"));
        write(&lib2.join("steamapps/common/Proton Hotfix/toolmanifest.vdf"), "");
        write(
            &root.join("userdata/1/config/localconfig.vdf"),
            r#""UserLocalConfigStore" { "Software" { "Valve" { "Steam" { "apps" { "3241660" { "LastPlayed" "1781372839" "Playtime" "264" } } } } } }"#,
        );
        write(
            &root.join("config/config.vdf"),
            r#""InstallConfigStore" { "Software" { "Valve" { "Steam" { "CompatToolMapping" { "3241660" { "name" "GE-Proton11-5" "config" "" } } } } } }"#,
        );
        write(
            &root.join("config/loginusers.vdf"),
            r#""users" { "7656" { "MostRecent" "0" "Timestamp" "1" } "7657" { "MostRecent" "1" "Timestamp" "0" } }"#,
        );
        home
    }

    #[test]
    fn finds_installed_games_in_every_library_but_not_tools() {
        let home = fixture();
        let games = installed(home.path());
        assert_eq!(games.keys().collect::<Vec<_>>(), ["3241660", "480"]);
        let repo = &games["3241660"];
        assert_eq!(repo.game.name, "R.E.P.O.");
        assert_eq!(repo.game.playtime, 264 * 60);
        assert_eq!(repo.game.last_played, Some(1781372839));
        assert!(repo.game.install_dir.as_ref().unwrap().ends_with("games/SteamLibrary/steamapps/common/REPO"));
        assert_eq!(repo.source.data["proton"], "GE-Proton11-5");
        assert_eq!(repo.source.data["buildid"], "42");
        assert_eq!(games["480"].source.data["proton"], Value::Null);
        assert_eq!(games["480"].game.last_played, Some(1700000000));
        assert_eq!(last_user(&roots(home.path())[0]).as_deref(), Some("7657"));
    }

    #[test]
    fn a_home_without_steam_finds_nothing() {
        let home = tempfile::tempdir().unwrap();
        assert!(roots(home.path()).is_empty());
        assert!(installed(home.path()).is_empty());
    }
}
