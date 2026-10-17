//! Minecraft through Prism Launcher (Phase 8): an instance is a library game of
//! kind `minecraft` with a `prism` source (import/prism.rs). Prism keeps owning
//! its files; we read them, change the few things the Minecraft page edits, and
//! hand Prism what it does best (new instances, accounts, launching).

pub mod nbt;

use crate::rpc::{State, internal};
use anyhow::{Context as _, Result};
use serde_json::{Value, json};
use somelauncher_proto::Error;
use somelauncher_proto::game::{Game, Kind, SourceKind};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const FLATPAK_ID: &str = "org.prismlauncher.PrismLauncher";

/// Where a library game's instance lives and which Prism owns it.
#[derive(Debug, Clone)]
pub struct Instance {
    /// Prism's instance id: its folder's name (`prismlauncher -l <id>`)
    pub id: String,
    /// The instance folder (`instance.cfg`, `mmc-pack.json`)
    pub dir: PathBuf,
    /// The game folder inside it (`minecraft` or `.minecraft`)
    pub game_dir: PathBuf,
    pub flatpak: bool,
}

impl Instance {
    pub fn of(game: &Game) -> Option<Instance> {
        if game.kind != Kind::Minecraft {
            return None;
        }
        let src = game.sources.iter().find(|s| s.source == SourceKind::Prism)?;
        let dir = game.install_dir.clone()?;
        Some(Instance {
            id: src.source_id.clone(),
            game_dir: crate::import::prism::game_dir(&dir),
            flatpak: src.data["flatpak"].as_bool().unwrap_or(false),
            dir,
        })
    }
}

/// Prism's data folder for an instance (`accounts.json` lives there), else the first one installed.
fn prism_root(home: &Path, game: Option<&Game>) -> Option<PathBuf> {
    let from_game = game
        .and_then(|g| g.sources.iter().find(|s| s.source == SourceKind::Prism))
        .and_then(|s| s.data["root"].as_str())
        .map(PathBuf::from);
    from_game.or_else(|| crate::import::prism::installs(home).into_iter().next().map(|(root, _)| root))
}

fn mtime(p: &Path) -> Option<i64> {
    let t = std::fs::metadata(p).ok()?.modified().ok()?;
    Some(t.duration_since(UNIX_EPOCH).ok()?.as_secs() as i64)
}

/// Prism's accounts: `[{name, type, active}]` (`type`: `msa` or `offline`). Never the tokens.
pub fn accounts(root: &Path) -> Vec<Value> {
    let Ok(text) = std::fs::read_to_string(root.join("accounts.json")) else { return vec![] };
    let Ok(v) = serde_json::from_str::<Value>(&text) else { return vec![] };
    v["accounts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| {
            let name = a["profile"]["name"].as_str().filter(|n| !n.is_empty())?;
            Some(json!({ "name": name, "type": a["type"].as_str().unwrap_or("").to_lowercase(), "active": a["active"].as_bool() == Some(true) }))
        })
        .collect()
}

/// The worlds in `saves/`, last played first: `{folder, name, icon, lastPlayed, mode, hardcore, version}`
/// from each `level.dat` (its file time and the folder's name when it cannot be read).
pub fn worlds(game_dir: &Path) -> Vec<Value> {
    let mut out: Vec<Value> = std::fs::read_dir(game_dir.join("saves"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("level.dat").is_file())
        .map(|p| {
            let folder = p.file_name().unwrap_or_default().to_string_lossy().into_owned();
            let level = std::fs::read(p.join("level.dat")).ok().and_then(|b| nbt::read(&b).ok());
            let data = level.as_ref().map(|l| &l["Data"]).unwrap_or(&Value::Null);
            let last = data["LastPlayed"].as_i64().map(|ms| ms / 1000).or_else(|| mtime(&p.join("level.dat")));
            let mode = match data["GameType"].as_i64() {
                Some(0) => "survival",
                Some(1) => "creative",
                Some(2) => "adventure",
                Some(3) => "spectator",
                _ => "",
            };
            json!({
                "folder": folder,
                "name": data["LevelName"].as_str().filter(|n| !n.is_empty()).unwrap_or(&folder),
                "icon": Some(p.join("icon.png")).filter(|i| i.is_file()),
                "lastPlayed": last,
                "mode": mode,
                "hardcore": data["hardcore"].as_i64() == Some(1),
                "version": data["Version"]["Name"].as_str(),
            })
        })
        .collect();
    out.sort_by_key(|w| std::cmp::Reverse(w["lastPlayed"].as_i64().unwrap_or(0)));
    out
}

/// The multiplayer list (`servers.dat`): `{name, address, icon}`, `icon` a PNG data URL.
pub fn servers(game_dir: &Path) -> Vec<Value> {
    let Some(v) = std::fs::read(game_dir.join("servers.dat")).ok().and_then(|b| nbt::read(&b).ok()) else { return vec![] };
    v["servers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| {
            let address = s["ip"].as_str().filter(|a| !a.is_empty())?;
            let icon = s["icon"].as_str().filter(|i| !i.is_empty()).map(|i| format!("data:image/png;base64,{i}"));
            Some(json!({ "name": s["name"].as_str().unwrap_or(address), "address": address, "icon": icon }))
        })
        .collect()
}

/// Screenshots newest first, `{path, time}`, at most `max`.
pub fn screenshots(game_dir: &Path, max: usize) -> Vec<Value> {
    let mut shots: Vec<(PathBuf, i64)> = std::fs::read_dir(game_dir.join("screenshots"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("png") || x.eq_ignore_ascii_case("jpg")))
        .filter_map(|p| Some((p.clone(), mtime(&p)?)))
        .collect();
    shots.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| b.0.cmp(&a.0)));
    shots.into_iter().take(max).map(|(path, time)| json!({ "path": path, "time": time })).collect()
}

/// The game folder's own folders the Files tab opens, when they exist
const FOLDERS: [&str; 8] = ["mods", "resourcepacks", "shaderpacks", "saves", "screenshots", "config", "logs", "crash-reports"];

/// How many files a folder holds whose name passes `keep`.
fn count(dir: &Path, keep: impl Fn(&str) -> bool) -> usize {
    std::fs::read_dir(dir).into_iter().flatten().flatten().filter(|e| keep(&e.file_name().to_string_lossy())).count()
}

fn is_mod(name: &str) -> bool {
    name.ends_with(".jar") || name.ends_with(".zip")
}

/// A modpack Prism manages the instance for (`ManagedPack*` in `instance.cfg`): `{type, name, version}`.
fn managed_pack(dir: &Path) -> Option<Value> {
    let cfg = crate::import::prism::ini(&std::fs::read_to_string(dir.join("instance.cfg")).ok()?);
    if cfg.get("ManagedPack").map(String::as_str) != Some("true") {
        return None;
    }
    let get = |k: &str| cfg.get(k).filter(|v| !v.is_empty()).cloned();
    Some(json!({ "type": get("ManagedPackType"), "name": get("ManagedPackName"), "version": get("ManagedPackVersionName") }))
}

/// A running Minecraft of this instance: a `java` process working in its game folder
/// (Prism starts the game there), however it was started.
pub fn find_java(game_dir: &Path) -> Option<i32> {
    let want = std::fs::canonicalize(game_dir).ok()?;
    std::fs::read_dir("/proc").ok()?.flatten().find_map(|e| {
        let pid: i32 = e.file_name().to_str()?.parse().ok()?;
        let exe = std::fs::read_link(e.path().join("exe")).ok()?;
        let name = exe.file_name()?.to_str()?;
        if !name.starts_with("java") {
            return None;
        }
        (std::fs::read_link(e.path().join("cwd")).ok()? == want).then_some(pid)
    })
}

/// What `games.launch` runs for an instance: `prismlauncher -l <id>`, with the
/// chosen account (`-a`, when Prism still has it) and a world or server to join.
pub fn plan(game: &Game, home: &Path, account: Option<&str>) -> Result<crate::launch::Plan> {
    let inst = Instance::of(game).with_context(|| format!("{} is not a Prism instance", game.name))?;
    let mut argv = prism_argv(home, inst.flatpak)?;
    argv.extend(["-l".to_string(), inst.id]);
    if let Some(name) = account
        && let Some(root) = prism_root(home, Some(game))
        && accounts(&root).iter().any(|a| a["name"] == name)
    {
        argv.extend(["-a".to_string(), name.to_string()]);
    }
    argv.extend(game.runner.args.iter().cloned());
    Ok(crate::launch::Plan {
        argv,
        env: Default::default(),
        cwd: None,
        new_prefix: None,
        make_executable: None,
        pre_launch: game.runner.pre_launch.clone(),
        post_exit: game.runner.post_exit.clone(),
    })
}

/// The command that runs Prism: native first (or flatpak when the instance is the flatpak's).
pub fn prism_argv(home: &Path, flatpak: bool) -> Result<Vec<String>> {
    let native = crate::launch::which("prismlauncher");
    let has_flatpak = home.join(".var/app").join(FLATPAK_ID).is_dir();
    if native.is_some() && !(flatpak && has_flatpak) {
        return Ok(vec!["prismlauncher".into()]);
    }
    if has_flatpak {
        return Ok(["flatpak", "run", FLATPAK_ID].map(String::from).to_vec());
    }
    anyhow::bail!("Prism Launcher is not installed")
}

/// Runs Prism with `args` in its own scope, not waited for: when Prism was not
/// running this process becomes Prism, which must outlive the daemon.
pub fn run_prism(home: &Path, flatpak: bool, args: &[String]) -> Result<()> {
    let mut argv = prism_argv(home, flatpak)?;
    argv.extend(args.iter().cloned());
    spawn_scoped(&argv)
}

/// A Prism command line (from `prism_argv`) in its own scope, not waited for.
pub fn spawn_scoped(argv: &[String]) -> Result<()> {
    let unit = format!("--unit=somelauncher-prism-{}-{}", crate::games::now(), std::process::id());
    let mut cmd = tokio::process::Command::new("systemd-run");
    cmd.args(["--user", "--scope", "--collect", "--quiet", "--expand-environment=no", &unit, "--"]).args(argv);
    cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    // systemd-run stays the parent until Prism quits, and tokio reaps it then
    cmd.spawn().context("running Prism Launcher")?;
    Ok(())
}

/// How long Prism may take to start the game (it may fetch libraries and assets first)
const START_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);

/// Waits until the instance's Minecraft runs and returns its pid. Woken by the
/// game folder's `logs/` (the game writes there as it boots), so nothing polls.
pub async fn wait_started(game_dir: &Path) -> Result<i32> {
    let logs = game_dir.join("logs");
    std::fs::create_dir_all(&logs).with_context(|| format!("creating {}", logs.display()))?;
    let mask = inotify::WatchMask::CREATE | inotify::WatchMask::MODIFY | inotify::WatchMask::MOVED_TO;
    let started = crate::watch::until(&logs, mask, Some(START_TIMEOUT), || find_java(game_dir).is_some()).await?;
    if !started {
        anyhow::bail!("Prism did not start the game within {} minutes", START_TIMEOUT.as_secs() / 60);
    }
    find_java(game_dir).context("the game exited right away")
}

impl State {
    fn instance(&self, params: &Value) -> Result<(Game, Instance), Error> {
        let id = params["id"].as_i64().ok_or_else(|| Error::new("bad_params", "missing integer `id`"))?;
        let game = self.game(&self.db.lock().unwrap(), id)?;
        let inst = Instance::of(&game).ok_or_else(|| Error::new("not_found", format!("{} is not a Prism instance", game.name)))?;
        Ok((game, inst))
    }

    /// `minecraft.instance {id}`: what the instance holds, read from its files each time
    pub(crate) fn minecraft_instance(&self, params: &Value) -> Result<Value, Error> {
        let (_, inst) = self.instance(params)?;
        let g = &inst.game_dir;
        Ok(json!({
            "dir": inst.dir,
            "gameDir": g,
            "worlds": worlds(g),
            "servers": servers(g),
            "screenshots": screenshots(g, 200),
            "mods": count(&g.join("mods"), is_mod),
            "modsDisabled": count(&g.join("mods"), |n| n.ends_with(".disabled")),
            "resourcePacks": count(&g.join("resourcepacks"), |n| !n.starts_with('.')),
            "shaderPacks": count(&g.join("shaderpacks"), |n| !n.starts_with('.') && !n.ends_with(".txt")),
            "pack": managed_pack(&inst.dir),
            "folders": FOLDERS.iter().filter(|f| g.join(f).is_dir()).collect::<Vec<_>>(),
        }))
    }

    /// `minecraft.accounts {id?}`: Prism's accounts and the one Play uses (`minecraftAccount`, else Prism's active one)
    pub(crate) fn minecraft_accounts(&self, params: &Value) -> Result<Value, Error> {
        let game = match params["id"].as_i64() {
            Some(id) => Some(self.game(&self.db.lock().unwrap(), id)?),
            None => None,
        };
        let list = prism_root(&self.home, game.as_ref()).map(|r| accounts(&r)).unwrap_or_default();
        let chosen = self.setting_str("minecraftAccount").map_err(internal)?.filter(|c| list.iter().any(|a| a["name"] == c.as_str()));
        let current = chosen.clone().or_else(|| list.iter().find(|a| a["active"] == true).and_then(|a| a["name"].as_str().map(String::from)));
        Ok(json!({ "accounts": list, "chosen": chosen, "current": current }))
    }

    /// `minecraft.prism {id?}`: Prism's window, on that instance when given
    pub(crate) fn minecraft_prism(&self, params: &Value) -> Result<Value, Error> {
        let (flatpak, args) = match params.get("id").filter(|v| !v.is_null()) {
            Some(_) => {
                let (_, inst) = self.instance(params)?;
                (inst.flatpak, vec!["--show".to_string(), inst.id])
            }
            None => (false, vec![]),
        };
        run_prism(&self.home, flatpak, &args).map_err(|e| Error::new("launch_failed", format!("{e:#}")))?;
        Ok(Value::Null)
    }
}

#[cfg(test)]
mod tests {
    use super::nbt::tests::{Field, compound, gzip};
    use super::*;
    use somelauncher_proto::game::{NewGame, Source};

    fn instance(home: &Path) -> (Game, PathBuf) {
        let root = home.join(".local/share/PrismLauncher");
        let dir = root.join("instances/Vibe");
        let g = dir.join("minecraft");
        std::fs::create_dir_all(&g).unwrap();
        std::fs::write(root.join("accounts.json"), r#"{"accounts":[{"type":"MSA","active":true,"profile":{"name":"Steve"},"msa":{"token":"secret"}},{"type":"Offline","profile":{"name":"Alex"}}]}"#).unwrap();
        let mut new = NewGame::new("Vibe", Kind::Minecraft);
        new.install_dir = Some(dir.clone());
        let mut game = Game::new(1, new);
        game.sources.push(Source { source: SourceKind::Prism, source_id: "Vibe".into(), data: json!({ "root": root, "flatpak": false }) });
        (game, g)
    }

    #[test]
    fn reads_worlds_servers_screenshots_and_accounts() {
        let home = tempfile::tempdir().unwrap();
        let (_, g) = instance(home.path());
        for (folder, name, last, mode) in [("New World", "Home", 1_000_000i64, 0), ("Creative", "", 2_000_000, 1)] {
            std::fs::create_dir_all(g.join("saves").join(folder)).unwrap();
            let level = compound(&[(
                "Data",
                Field::Compound(vec![("LevelName", Field::Str(name)), ("LastPlayed", Field::Long(last)), ("GameType", Field::Int(mode)), ("hardcore", Field::Byte(0))]),
            )]);
            std::fs::write(g.join("saves").join(folder).join("level.dat"), gzip(&level)).unwrap();
        }
        std::fs::write(g.join("saves/Creative/icon.png"), "").unwrap();
        std::fs::create_dir_all(g.join("saves/not a world")).unwrap();
        let w = worlds(&g);
        assert_eq!(w.iter().map(|w| w["name"].as_str().unwrap()).collect::<Vec<_>>(), ["Creative", "Home"], "last played first; no LevelName: the folder");
        assert_eq!((&w[0]["mode"], &w[0]["lastPlayed"], &w[1]["mode"]), (&json!("creative"), &json!(2000), &json!("survival")));
        assert!(w[0]["icon"].as_str().unwrap().ends_with("Creative/icon.png") && w[1]["icon"].is_null());

        let servers_dat = compound(&[("servers", Field::List(vec![vec![("name", Field::Str("Vibe")), ("ip", Field::Str("mc.example.org")), ("icon", Field::Str("iVBO"))], vec![("name", Field::Str("empty"))]]))]);
        std::fs::write(g.join("servers.dat"), servers_dat).unwrap();
        assert_eq!(servers(&g), [json!({ "name": "Vibe", "address": "mc.example.org", "icon": "data:image/png;base64,iVBO" })]);

        std::fs::create_dir_all(g.join("screenshots")).unwrap();
        for n in ["a.png", "b.png", "notes.txt"] {
            std::fs::write(g.join("screenshots").join(n), "").unwrap();
        }
        assert_eq!(screenshots(&g, 10).len(), 2);
        assert_eq!(screenshots(&g, 1).len(), 1);

        let acc = accounts(&home.path().join(".local/share/PrismLauncher"));
        assert_eq!(acc, [json!({ "name": "Steve", "type": "msa", "active": true }), json!({ "name": "Alex", "type": "offline", "active": false })]);
        assert!(!serde_json::to_string(&acc).unwrap().contains("secret"), "no tokens");
    }

    #[test]
    fn plans_prism_with_a_known_account_only() {
        let home = tempfile::tempdir().unwrap();
        let (game, _) = instance(home.path());
        let Ok(p) = plan(&game, home.path(), Some("Alex")) else { return }; // no Prism on this machine
        assert_eq!(p.argv[1..], ["-l", "Vibe", "-a", "Alex"]);
        let p = plan(&game, home.path(), Some("Gone")).unwrap();
        assert_eq!(p.argv[1..], ["-l", "Vibe"], "an account Prism no longer has: its active one");
    }

    #[tokio::test]
    async fn finds_the_instance_java_once_the_game_logs() {
        let home = tempfile::tempdir().unwrap();
        let (_, g) = instance(home.path());
        assert_eq!(find_java(&g), None);
        // A stand-in Minecraft: a program named java, working in the game folder
        let java = home.path().join("java");
        std::fs::copy("/usr/bin/sleep", &java).unwrap();
        let g2 = g.clone();
        let started = tokio::spawn(async move { wait_started(&g2).await });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let mut child = std::process::Command::new(&java).arg("5").current_dir(&g).spawn().unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        std::fs::write(g.join("logs/latest.log"), "booting").unwrap();
        let pid = tokio::time::timeout(std::time::Duration::from_secs(5), started).await.unwrap().unwrap().unwrap();
        assert_eq!(pid, child.id() as i32);
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(find_java(&g), None);
    }
}
