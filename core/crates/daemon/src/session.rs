//! Running games. A session row is written when a game starts, touched once a
//! minute while it runs and closed when it ends, adding its time to the game's
//! playtime; after a crash of the daemon the time up to the last touch is kept.
//! A scope game ends when its systemd scope is empty (`cgroup.events`, inotify),
//! a Steam game when its reaper exits (pidfd): Proton's whole process tree
//! counts, not only the process we started.

use crate::rpc::{State, internal};
use crate::{games, import, launch, needs_steam, steam_client, watch};
use anyhow::{Context as _, Result, anyhow};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use somelauncher_proto::Error;
use somelauncher_proto::game::{Game, Kind};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

const HEARTBEAT: Duration = Duration::from_secs(60);
/// Between asking a game to quit and killing it
const STOP_GRACE: Duration = Duration::from_secs(10);
const LOGS_KEPT: usize = 3;

/// How a running game is followed and stopped.
#[derive(Debug, Clone)]
pub enum Track {
    /// A `systemd-run --user --scope` unit
    Scope(String),
    /// A Steam game, by app id
    Steam(String),
}

impl Track {
    fn unit(&self) -> String {
        match self {
            Track::Scope(u) => u.clone(),
            Track::Steam(appid) => format!("steam:{appid}"),
        }
    }
    fn parse(unit: &str) -> Track {
        match unit.strip_prefix("steam:") {
            Some(appid) => Track::Steam(appid.into()),
            None => Track::Scope(unit.into()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Running {
    pub track: Track,
    /// None while Steam has not started the game yet
    pub since: Option<i64>,
}

// --- the sessions table

fn open(conn: &Connection, game: i64, unit: &str, at: i64) -> Result<i64> {
    conn.execute("INSERT INTO sessions (game_id, unit, started, seen) VALUES (?1, ?2, ?3, ?3)", params![game, unit, at])?;
    let id = conn.last_insert_rowid();
    conn.execute("UPDATE games SET last_played = ?2 WHERE id = ?1", params![game, at])?;
    Ok(id)
}

fn touch(conn: &Connection, session: i64, at: i64) -> Result<()> {
    conn.execute("UPDATE sessions SET seen = ?2 WHERE id = ?1", params![session, at])?;
    Ok(())
}

/// Ends a session at `at` and adds its length to the game's playtime.
fn close(conn: &Connection, session: i64, at: i64) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    let (game, started): (i64, i64) = tx.query_row("SELECT game_id, started FROM sessions WHERE id = ?1", [session], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let at = at.max(started);
    tx.execute("UPDATE sessions SET ended = ?2, seen = ?2 WHERE id = ?1", params![session, at])?;
    tx.execute("UPDATE games SET playtime = playtime + ?2, last_played = ?3 WHERE id = ?1", params![game, at - started, at])?;
    tx.commit()?;
    Ok(())
}

/// Sessions not closed: (session, game, unit, last seen).
fn unfinished(conn: &Connection) -> Result<Vec<(i64, i64, String, i64)>> {
    let mut stmt = conn.prepare("SELECT id, game_id, unit, seen FROM sessions WHERE ended IS NULL")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

// --- following processes

fn populated(cgroup: &Path) -> bool {
    std::fs::read_to_string(cgroup.join("cgroup.events")).is_ok_and(|s| s.lines().any(|l| l == "populated 1"))
}

/// Waits until a cgroup has no process left (or is gone).
async fn wait_cgroup(cgroup: PathBuf) -> Result<()> {
    let events = cgroup.join("cgroup.events");
    if !events.exists() {
        return Ok(());
    }
    watch::until(&events, inotify::WatchMask::MODIFY, None, || !populated(&cgroup)).await?;
    Ok(())
}

/// The cgroup of a running scope unit, asked of systemd once.
async fn unit_cgroup(unit: &str) -> Option<PathBuf> {
    let out = tokio::process::Command::new("systemctl").args(["--user", "show", "-p", "ControlGroup", "--value", unit]).output().await.ok()?;
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!path.is_empty()).then(|| PathBuf::from(format!("/sys/fs/cgroup{path}")))
}

/// The cgroup a freshly spawned `systemd-run --scope` moved itself into. It
/// does so before it starts the game, within milliseconds; None when the
/// process is gone first or no scope appears (no systemd).
async fn spawned_cgroup(pid: u32, unit: &str) -> Option<PathBuf> {
    for _ in 0..250 {
        let line = std::fs::read_to_string(format!("/proc/{pid}/cgroup")).ok()?;
        if let Some(path) = line.lines().find_map(|l| l.strip_prefix("0::")).filter(|p| p.ends_with(unit)) {
            return Some(PathBuf::from(format!("/sys/fs/cgroup{path}")));
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    None
}

/// Every process below `root` (children, grandchildren, …), from /proc.
fn descendants(root: i32) -> Vec<i32> {
    let mut parent = std::collections::HashMap::new();
    for e in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|s| s.parse::<i32>().ok()) else { continue };
        let Ok(stat) = std::fs::read_to_string(e.path().join("stat")) else { continue };
        // `pid (comm) state ppid …`; comm may hold spaces and parens, so read after the last ')'
        let ppid = stat.rsplit_once(')').and_then(|(_, rest)| rest.split_whitespace().nth(1)?.parse::<i32>().ok());
        if let Some(ppid) = ppid {
            parent.insert(pid, ppid);
        }
    }
    let mut out = Vec::new();
    let mut frontier = vec![root];
    while let Some(p) = frontier.pop() {
        for (&child, &pp) in &parent {
            if pp == p && !out.contains(&child) {
                out.push(child);
                frontier.push(child);
            }
        }
    }
    out
}

fn signal(pids: &[i32], sig: i32) {
    for &pid in pids {
        // SAFETY: kill only sends a signal
        unsafe { libc::kill(pid, sig) };
    }
}

// --- logs

fn log_dir(game: i64) -> PathBuf {
    somelauncher_proto::paths::state_dir().join("logs").join(game.to_string())
}

/// A game's logs, newest first: (path, unix time, size).
pub fn logs(game: i64) -> Vec<(PathBuf, i64, u64)> {
    let mut out: Vec<(PathBuf, i64, u64)> = std::fs::read_dir(log_dir(game))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let time = p.file_stem()?.to_str()?.parse().ok()?;
            Some((p.clone(), time, e.metadata().ok()?.len()))
        })
        .collect();
    out.sort_by_key(|a| std::cmp::Reverse(a.1));
    out
}

/// A new log for a game, keeping the newest `LOGS_KEPT` with it.
fn new_log(game: i64) -> Result<(PathBuf, std::fs::File)> {
    let dir = log_dir(game);
    std::fs::create_dir_all(&dir)?;
    for (old, ..) in logs(game).into_iter().skip(LOGS_KEPT - 1) {
        let _ = std::fs::remove_file(old);
    }
    let path = dir.join(format!("{}.log", games::now()));
    let file = std::fs::File::create(&path)?;
    Ok((path, file))
}

fn failed(e: anyhow::Error) -> Error {
    Error::new("launch_failed", format!("{e:#}"))
}

impl State {
    pub(crate) async fn launch(self: &Arc<Self>, id: i64) -> Result<Value, Error> {
        let mut game = self.game(&self.db.lock().unwrap(), id)?;
        if self.running.lock().unwrap().contains_key(&id) {
            return Err(Error::new("busy", format!("{} is already running", game.name)));
        }
        let plan = launch::plan(&game, &self.launch_context().map_err(internal)?).map_err(failed)?;
        if game.kind == Kind::Steam {
            return self.launch_steam(game, plan).await;
        }
        if let Some(prefix) = &plan.new_prefix {
            std::fs::create_dir_all(prefix).map_err(|e| failed(anyhow!("creating {}: {e}", prefix.display())))?;
            game.prefix = Some(prefix.clone());
            games::save(&self.db.lock().unwrap(), &game).map_err(internal)?;
            self.emit("library.changed", json!({ "ids": [id] }));
        }
        if let Some(exe) = &plan.make_executable {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(exe).map(|m| m.permissions().mode()).unwrap_or(0o644);
            std::fs::set_permissions(exe, std::fs::Permissions::from_mode(mode | 0o100))
                .map_err(|e| failed(anyhow!("making {} executable: {e}", exe.display())))?;
            tracing::info!(id, "made {} executable", exe.display());
        }
        // Claimed before anything slow, so a second click is `busy`
        let unit = launch::unit_name(id);
        self.running.lock().unwrap().insert(id, Running { track: Track::Scope(unit.clone()), since: None });
        let started = self.start_scope(&game, &plan, &unit).await;
        if started.is_err() {
            self.running.lock().unwrap().remove(&id);
            self.emit("game.state", json!({ "id": id, "state": "stopped" }));
            self.games_changed();
        }
        started
    }

    async fn start_scope(self: &Arc<Self>, game: &Game, plan: &launch::Plan, unit: &str) -> Result<Value, Error> {
        let id = game.id;
        if game.runner.needs_steam && matches!(game.kind, Kind::Proton | Kind::Hoyo) {
            self.prepare_needs_steam(game, plan).await.map_err(failed)?;
        }
        let (log_path, log) = new_log(id).map_err(internal)?;
        if let Some(cmd) = &plan.pre_launch {
            launch::run_hook(cmd, plan, &log).await.map_err(failed)?;
        }
        let mut child = launch::spawn(plan, unit, &game.name, &log).map_err(failed)?;
        tracing::info!(id, unit, argv = ?plan.argv, "launched {}", game.name);
        let pid = child.id().unwrap_or(0);
        let unit_owned = unit.to_string();
        let wait = async move {
            if let Some(cgroup) = spawned_cgroup(pid, &unit_owned).await {
                wait_cgroup(cgroup).await?;
            }
            child.wait().await?;
            Ok(())
        };
        let (plan, name) = (plan.clone(), game.name.clone());
        let after = async move {
            if let Some(cmd) = &plan.post_exit
                && let Err(e) = launch::run_hook(cmd, &plan, &log).await
            {
                tracing::warn!("{name}: {e:#}");
            }
        };
        self.follow(id, Track::Scope(unit.into()), wait, after).map_err(internal)?;
        Ok(json!({ "unit": unit, "log": log_path }))
    }

    /// A Steam game: `steam -applaunch`, then followed from Steam's process log
    /// (start) to its reaper process (exit). Steam owns the launch, so no scope or log of ours.
    async fn launch_steam(self: &Arc<Self>, game: Game, plan: launch::Plan) -> Result<Value, Error> {
        let id = game.id;
        let (appid, flatpak) = launch::steam_app(&game).ok_or_else(|| failed(anyhow!("no Steam app id")))?;
        if steam_client::find_reaper(&appid).is_some() {
            return Err(Error::new("busy", format!("{} is already running in Steam", game.name)));
        }
        let root = import::steam::roots(&self.home)
            .into_iter()
            .find(|r| r.to_string_lossy().contains("com.valvesoftware.Steam") == flatpak)
            .ok_or_else(|| failed(anyhow!("Steam is not installed")))?;
        let offset = steam_client::log_offset(&steam_client::process_log(&root));
        let skip = if flatpak { 3 } else { 1 };
        steam_client::run(&self.home, flatpak, &plan.argv[skip..]).map_err(failed)?;
        let track = Track::Steam(appid.clone());
        let unit = track.unit();
        self.running.lock().unwrap().insert(id, Running { track: track.clone(), since: None });
        self.emit("game.state", json!({ "id": id, "state": "starting", "step": "steam" }));
        self.games_changed();
        tracing::info!(id, appid, "asked Steam to start {}", game.name);

        let state = self.clone();
        tokio::spawn(async move {
            match steam_client::wait_started(&root, &appid, offset).await {
                Ok(reaper) => {
                    tracing::info!(id, reaper, "{} is running", game.name);
                    if let Err(e) = state.follow(id, track, steam_client::wait_pid(reaper), async {}) {
                        tracing::error!(id, "{e:#}");
                    }
                }
                Err(e) => {
                    tracing::warn!(id, "{}: {e:#}", game.name);
                    state.running.lock().unwrap().remove(&id);
                    state.emit("game.state", json!({ "id": id, "state": "stopped", "error": format!("{e:#}") }));
                    state.games_changed();
                }
            }
        });
        Ok(json!({ "unit": unit }))
    }

    /// Opens a session and follows the game until `wait` returns, touching the
    /// session every minute; then closes it, runs `after` and tells everyone.
    fn follow(
        self: &Arc<Self>,
        id: i64,
        track: Track,
        wait: impl Future<Output = Result<()>> + Send + 'static,
        after: impl Future<Output = ()> + Send + 'static,
    ) -> Result<()> {
        let now = games::now();
        let session = open(&self.db.lock().unwrap(), id, &track.unit(), now)?;
        self.resume(id, session, now, track, wait, after);
        Ok(())
    }

    /// Follows a game whose session is already open (a new one, or one found again after a restart).
    fn resume(
        self: &Arc<Self>,
        id: i64,
        session: i64,
        since: i64,
        track: Track,
        wait: impl Future<Output = Result<()>> + Send + 'static,
        after: impl Future<Output = ()> + Send + 'static,
    ) {
        let unit = track.unit();
        self.running.lock().unwrap().insert(id, Running { track, since: Some(since) });
        self.emit("game.state", json!({ "id": id, "state": "running", "unit": unit, "since": since }));
        self.games_changed();
        let state = self.clone();
        tokio::spawn(async move {
            let mut beat = tokio::time::interval_at(tokio::time::Instant::now() + HEARTBEAT, HEARTBEAT);
            tokio::pin!(wait);
            let result = loop {
                tokio::select! {
                    r = &mut wait => break r,
                    _ = beat.tick() => {
                        if let Err(e) = touch(&state.db.lock().unwrap(), session, games::now()) {
                            tracing::warn!(id, "session heartbeat: {e:#}");
                        }
                    }
                }
            };
            if let Err(e) = &result {
                tracing::warn!(id, "following the game: {e:#}");
            }
            if let Err(e) = close(&state.db.lock().unwrap(), session, games::now()) {
                tracing::error!(id, "closing the session: {e:#}");
            }
            after.await;
            state.running.lock().unwrap().remove(&id);
            tracing::info!(id, "game {id} ended");
            state.emit("game.state", json!({ "id": id, "state": "stopped" }));
            state.games_changed();
            state.emit("library.changed", json!({ "ids": [id] }));
        });
    }

    /// Asks the game to quit (SIGTERM to its whole tree), and kills what is left after a grace period.
    pub(crate) async fn stop(self: &Arc<Self>, id: i64) -> Result<Value, Error> {
        let track = self.running.lock().unwrap().get(&id).map(|r| r.track.clone());
        let Some(track) = track else { return Err(Error::new("not_running", format!("game {id} is not running"))) };
        match track {
            Track::Scope(unit) => {
                let kill = move |sig: &'static str| {
                    let unit = unit.clone();
                    async move {
                        tokio::process::Command::new("systemctl").args(["--user", "kill", "--signal", sig, &unit]).output().await
                    }
                };
                kill("SIGTERM").await.map_err(|e| internal(e.into()))?;
                let state = self.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(STOP_GRACE).await;
                    if state.running.lock().unwrap().contains_key(&id) {
                        let _ = kill("SIGKILL").await;
                    }
                });
            }
            Track::Steam(appid) => {
                let reaper = steam_client::find_reaper(&appid).ok_or_else(|| Error::new("not_running", "Steam has not started it yet"))?;
                signal(&descendants(reaper), libc::SIGTERM);
                let state = self.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(STOP_GRACE).await;
                    if state.running.lock().unwrap().contains_key(&id) {
                        let mut pids = descendants(reaper);
                        pids.push(reaper);
                        signal(&pids, libc::SIGKILL);
                    }
                });
            }
        }
        tracing::info!(id, "stopping game {id}");
        Ok(Value::Null)
    }

    pub(crate) fn running_games(&self) -> Value {
        let running = self.running.lock().unwrap();
        let mut list: Vec<Value> =
            running.iter().map(|(id, r)| json!({ "id": id, "unit": r.track.unit(), "since": r.since })).collect();
        list.sort_by_key(|v| v["id"].as_i64());
        Value::Array(list)
    }

    /// After a daemon start: games still running are followed again, the
    /// others' sessions are closed at their last heartbeat.
    pub async fn recover(self: &Arc<Self>) -> Result<()> {
        let open_sessions = unfinished(&self.db.lock().unwrap())?;
        for (session, game, unit, seen) in open_sessions {
            let started: i64 = self.db.lock().unwrap().query_row("SELECT started FROM sessions WHERE id = ?1", [session], |r| r.get(0))?;
            let track = Track::parse(&unit);
            let alive: Option<std::pin::Pin<Box<dyn Future<Output = Result<()>> + Send>>> = match &track {
                Track::Scope(u) => match unit_cgroup(u).await.filter(|c| populated(c)) {
                    Some(cg) => Some(Box::pin(wait_cgroup(cg))),
                    None => None,
                },
                Track::Steam(appid) => steam_client::find_reaper(appid).map(|pid| Box::pin(steam_client::wait_pid(pid)) as _),
            };
            match alive {
                Some(wait) => {
                    tracing::info!(game, unit, "game {game} still runs; following it again");
                    self.resume(game, session, started, track, wait, async {});
                }
                None => {
                    tracing::info!(game, unit, "closing the session of game {game} at its last heartbeat");
                    close(&self.db.lock().unwrap(), session, seen)?;
                }
            }
        }
        Ok(())
    }

    /// LUTRIS_SETUP.md, automated: Steam running and logged in, its DLLs in the prefix, `steam_appid.txt`.
    async fn prepare_needs_steam(&self, game: &Game, plan: &launch::Plan) -> Result<()> {
        if !steam_client::status(&self.home).logged_in {
            self.emit("game.state", json!({ "id": game.id, "state": "starting", "step": "steam" }));
            steam_client::ensure_running(&self.home).await?;
        }
        let prefix = plan.env.get("WINEPREFIX").map(PathBuf::from).context("the plan has no prefix")?;
        let copied = needs_steam::prepare_prefix(&needs_steam::steam_root(&self.home)?, &prefix)?;
        if !copied.is_empty() {
            tracing::info!(id = game.id, ?copied, "copied Steam files into {}", prefix.display());
        }
        needs_steam::write_app_id(game)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use somelauncher_proto::game::NewGame;

    #[test]
    fn closing_a_session_adds_its_time() {
        let conn = games::test_db();
        let mut new = NewGame::new("Valheim", Kind::Native);
        new.playtime = 100;
        let g = games::insert(&conn, &new).unwrap();
        let s = open(&conn, g, "somelauncher-game-1-1.scope", 1000).unwrap();
        touch(&conn, s, 1060).unwrap();
        assert_eq!(unfinished(&conn).unwrap(), [(s, g, "somelauncher-game-1-1.scope".to_string(), 1060)]);
        close(&conn, s, 1090).unwrap();
        let game = games::get(&conn, g).unwrap().unwrap();
        assert_eq!((game.playtime, game.last_played), (190, Some(1090)));
        assert!(unfinished(&conn).unwrap().is_empty());
    }

    #[test]
    fn tracks_round_trip_through_their_unit() {
        assert!(matches!(Track::parse("steam:480"), Track::Steam(a) if a == "480"));
        assert!(matches!(Track::parse("somelauncher-game-1-2.scope"), Track::Scope(u) if u == "somelauncher-game-1-2.scope"));
        assert_eq!(Track::Steam("480".into()).unit(), "steam:480");
    }

    #[test]
    fn descendants_finds_the_whole_tree() {
        let mut child = std::process::Command::new("sh").args(["-c", "sleep 2 & sleep 2; wait"]).spawn().unwrap();
        std::thread::sleep(Duration::from_millis(100));
        let below = descendants(child.id() as i32);
        assert_eq!(below.len(), 2, "both sleeps");
        signal(&below, libc::SIGKILL);
        child.wait().unwrap();
    }
}
