//! Method dispatch. Every method takes the request's params and returns a JSON
//! result or a protocol error; events go to every client through `State::emit`.

use crate::{games, import, launch, needs_steam, proton, steam_client};
use anyhow::Context as _;
use rusqlite::Connection;
use serde_json::{Value, json};
use somelauncher_proto::game::{Game, GamePatch, Kind, NewGame, Source, SourceKind};
use somelauncher_proto::{Error, Event, PROTOCOL, Reply, Request};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::{broadcast, watch};

pub struct State {
    db: Mutex<Connection>,
    db_path: PathBuf,
    /// Where importers look for other launchers' files
    home: PathBuf,
    http: reqwest::Client,
    pub(crate) jobs: crate::jobs::Jobs,
    /// Running games: id → their systemd unit
    running: Mutex<std::collections::HashMap<i64, String>>,
    events: broadcast::Sender<Event>,
    shutdown: watch::Sender<bool>,
}

impl State {
    pub fn new(db: Connection, db_path: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            db: Mutex::new(db),
            db_path,
            home: std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default(),
            http: reqwest::Client::builder()
                .user_agent(concat!("SomeLauncher/", env!("CARGO_PKG_VERSION")))
                .timeout(std::time::Duration::from_secs(20))
                .build()
                .expect("the TLS backend loads"),
            jobs: Default::default(),
            running: Default::default(),
            events: broadcast::channel(256).0,
            shutdown: watch::channel(false).0,
        })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }
    pub fn shutdown_signal(&self) -> watch::Receiver<bool> {
        self.shutdown.subscribe()
    }
    pub fn emit(&self, event: &str, data: Value) {
        // No receivers is fine: nobody is connected
        let _ = self.events.send(Event { event: event.into(), data });
    }

    pub fn hello(&self) -> Event {
        Event { event: "hello".into(), data: json!({ "protocol": PROTOCOL, "version": env!("CARGO_PKG_VERSION") }) }
    }

    pub async fn handle(self: &Arc<Self>, req: Request) -> Reply {
        match self.call(&req.method, &req.params).await {
            Ok(result) => Reply::ok(req.id, result),
            Err(e) => {
                tracing::debug!(method = req.method, error = e.message, "request failed");
                Reply::err(req.id, e)
            }
        }
    }

    async fn call(self: &Arc<Self>, method: &str, params: &Value) -> Result<Value, Error> {
        match method {
            "ping" => Ok(json!({ "pong": true })),
            "status" => Ok(json!({
                "version": env!("CARGO_PKG_VERSION"),
                "protocol": PROTOCOL,
                "pid": std::process::id(),
                "db": self.db_path,
            })),
            "settings.get" => {
                let db = self.db.lock().unwrap();
                match params.get("key").and_then(Value::as_str) {
                    Some(key) => Ok(crate::db::setting(&db, key).map_err(internal)?.unwrap_or(Value::Null)),
                    None => Ok(Value::Object(crate::db::settings(&db).map_err(internal)?)),
                }
            }
            "settings.set" => {
                let key = str_param(params, "key")?;
                let value = params.get("value").cloned().unwrap_or(Value::Null);
                crate::db::set_setting(&self.db.lock().unwrap(), key, &value).map_err(internal)?;
                self.emit("settings.changed", json!({ "key": key, "value": value }));
                Ok(Value::Null)
            }
            "games.list" => {
                let hidden = params.get("includeHidden").and_then(Value::as_bool).unwrap_or(false);
                to_json(games::list(&self.db.lock().unwrap(), hidden).map_err(internal)?)
            }
            "games.get" => to_json(self.game(&self.db.lock().unwrap(), id_param(params)?)?),
            "games.add" => {
                let new: NewGame = parse(params)?;
                if new.name.trim().is_empty() {
                    return Err(Error::new("bad_params", "the name is empty"));
                }
                let db = self.db.lock().unwrap();
                let id = games::insert(&db, &new).map_err(internal)?;
                games::set_source(&db, id, &Source { source: SourceKind::Manual, source_id: id.to_string(), data: Value::Null })
                    .map_err(internal)?;
                let game = self.game(&db, id)?;
                self.emit("library.changed", json!({ "ids": [id] }));
                to_json(game)
            }
            "games.update" => {
                let id = id_param(params)?;
                let mut fields = params.as_object().cloned().unwrap_or_default();
                fields.remove("id");
                let patch: GamePatch = parse(&Value::Object(fields))?;
                let db = self.db.lock().unwrap();
                let mut game = self.game(&db, id)?;
                patch.apply(&mut game);
                games::save(&db, &game).map_err(internal)?;
                self.emit("library.changed", json!({ "ids": [id] }));
                to_json(game)
            }
            "games.remove" => {
                let id = id_param(params)?;
                if !games::remove(&self.db.lock().unwrap(), id).map_err(internal)? {
                    return Err(not_found(id));
                }
                self.emit("library.changed", json!({ "ids": [id], "removed": true }));
                Ok(Value::Null)
            }
            "games.merge" => {
                let (from, into) = (id_param(params)?, params.get("into").and_then(Value::as_i64));
                let into = into.ok_or_else(|| Error::new("bad_params", "missing integer `into`"))?;
                if from == into {
                    return Err(Error::new("bad_params", "a game cannot be merged into itself"));
                }
                let db = self.db.lock().unwrap();
                self.game(&db, from)?;
                self.game(&db, into)?;
                let game = games::merge(&db, from, into).map_err(internal)?;
                self.emit("library.changed", json!({ "ids": [into] }));
                self.emit("library.changed", json!({ "ids": [from], "removed": true }));
                to_json(game)
            }
            "games.plan" => {
                let game = self.game(&self.db.lock().unwrap(), id_param(params)?)?;
                let ctx = self.launch_context().map_err(internal)?;
                to_json(launch::plan(&game, &ctx).map_err(|e| Error::new("launch_failed", format!("{e:#}")))?)
            }
            "games.launch" => self.launch(id_param(params)?).await,
            "games.duplicates" => to_json(games::duplicates(&self.db.lock().unwrap()).map_err(internal)?),
            "games.guess" => {
                let path = PathBuf::from(str_param(params, "path")?);
                let guess = crate::guess::guess(&path).ok_or_else(|| Error::new("not_found", format!("{} does not exist", path.display())))?;
                to_json(guess)
            }
            "library.import" => self.import(params).await,
            "library.scan" => to_json(self.scan().map_err(internal)?),
            "proton.list" => {
                let list = proton::discover(&self.home);
                let default = proton::default(&list, self.setting_str("defaultProton").map_err(internal)?.as_deref());
                Ok(json!({ "builds": list, "default": default.map(|p| p.name) }))
            }
            "proton.available" => {
                let source = str_param(params, "source")?;
                to_json(proton::releases(&self.http, source, &proton::discover(&self.home)).await.map_err(internal)?)
            }
            "proton.install" => {
                let (source, tag) = (str_param(params, "source")?, str_param(params, "tag")?);
                let releases = proton::releases(&self.http, source, &proton::discover(&self.home)).await.map_err(internal)?;
                let release = releases.into_iter().find(|r| r.tag == tag).ok_or_else(|| Error::new("not_found", format!("no release {tag}")))?;
                let id = format!("proton:{tag}");
                let (http, dest) = (self.http.clone(), proton::install_dir(&self.home));
                crate::jobs::spawn(self, &id, &format!("Proton {tag}"), move |p| proton::install(http, release, dest, p))
                    .map_err(|e| Error::new("busy", format!("{e:#}")))?;
                Ok(json!({ "id": id }))
            }
            "proton.remove" => {
                let name = str_param(params, "name")?;
                let list = proton::discover(&self.home);
                let p = proton::resolve(&list, name).ok_or_else(|| Error::new("not_found", format!("no Proton {name}")))?;
                // Only builds we could have installed; Steam's and the system's belong to them
                let ours = proton::install_dir(&self.home).canonicalize().is_ok_and(|d| p.dir.parent() == Some(d.as_path()));
                if !ours {
                    return Err(Error::new("bad_params", format!("{} is not in compatibilitytools.d", p.name)));
                }
                std::fs::remove_dir_all(&p.dir).map_err(|e| internal(e.into()))?;
                Ok(Value::Null)
            }
            "downloads.list" => to_json(self.jobs.list()),
            "downloads.cancel" => {
                let id = str_param(params, "id")?;
                if !self.jobs.cancel(id) {
                    return Err(Error::new("not_found", format!("no download {id}")));
                }
                self.emit("download.done", json!({ "id": id, "error": "cancelled" }));
                Ok(Value::Null)
            }
            "shutdown" => {
                let _ = self.shutdown.send(true);
                Ok(Value::Null)
            }
            _ => Err(Error::new("unknown_method", format!("no method {method}"))),
        }
    }

    /// Runs the importers; one failing does not stop the others.
    async fn import(&self, params: &Value) -> Result<Value, Error> {
        let all = ["steam", "lutris", "twintail", "prism"];
        let sources: Vec<String> = match params.get("sources") {
            Some(v) => parse(v)?,
            None => all.iter().map(|s| s.to_string()).collect(),
        };
        if let Some(bad) = sources.iter().find(|s| !all.contains(&s.as_str())) {
            return Err(Error::new("bad_params", format!("no importer {bad}")));
        }
        let mut summary = import::Summary::default();
        let mut errors = serde_json::Map::new();
        for source in &sources {
            let result = match source.as_str() {
                "steam" => self.import_steam(&mut summary).await,
                "lutris" => import::lutris::scan(&self.home).and_then(|f| self.apply(f, &mut summary)),
                "twintail" => import::twintail::scan(&self.home).and_then(|f| self.apply(f, &mut summary)),
                "prism" => import::prism::scan(&self.home).and_then(|f| self.apply(f, &mut summary)),
                _ => unreachable!(),
            };
            if let Err(e) = result {
                tracing::warn!(source, "import failed: {e:#}");
                errors.insert(source.clone(), json!(format!("{e:#}")));
            }
        }
        let ids = summary.ids();
        if !ids.is_empty() {
            self.emit("library.changed", json!({ "ids": ids }));
        }
        tracing::info!(added = summary.added.len(), updated = summary.updated.len(), "imported {sources:?}");
        Ok(json!({ "added": summary.added, "updated": summary.updated, "errors": errors }))
    }

    fn apply(&self, found: Vec<import::Found>, summary: &mut import::Summary) -> anyhow::Result<()> {
        let s = import::apply(&mut self.db.lock().unwrap(), found)?;
        summary.added.extend(s.added);
        summary.updated.extend(s.updated);
        Ok(())
    }

    async fn import_steam(&self, summary: &mut import::Summary) -> anyhow::Result<()> {
        let setting = |key| -> anyhow::Result<Option<String>> {
            Ok(crate::db::setting(&self.db.lock().unwrap(), key)?.and_then(|v| v.as_str().map(String::from)))
        };
        let (key, steam_id) = (setting("steamApiKey")?, setting("steamId")?);
        let (found, _) = import::steam::scan(&self.home, &self.http, key.as_deref(), steam_id.as_deref()).await?;
        let seen = found.iter().map(|f| f.source.source_id.clone()).collect();
        let mut db = self.db.lock().unwrap();
        let s = import::apply(&mut db, found)?;
        // Not installed any more (and, with the Web API, maybe not owned either): keep the game, drop its paths
        let gone = import::sweep(&mut db, SourceKind::Steam, &seen, |g, data| {
            g.install_dir = None;
            if let Some(d) = data.as_object_mut() {
                d.insert("installed".into(), json!(false));
            }
        })?;
        summary.added.extend(s.added);
        summary.updated.extend(s.updated.into_iter().chain(gone));
        Ok(())
    }

    /// Folders under the `gameDirs` setting that hold a game the library does not have yet.
    fn scan(&self) -> anyhow::Result<Vec<crate::guess::Guess>> {
        let (dirs, ignored, known) = {
            let db = self.db.lock().unwrap();
            let list = |key| -> anyhow::Result<Vec<PathBuf>> {
                Ok(crate::db::setting(&db, key)?.and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default())
            };
            let known: Vec<PathBuf> =
                games::list(&db, true)?.into_iter().flat_map(|g| [g.install_dir, g.exe, g.prefix]).flatten().collect();
            (list("gameDirs")?, list("scanIgnored")?, known)
        };
        let mut out = Vec::new();
        for dir in dirs {
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            let mut folders: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
            folders.sort();
            for folder in folders {
                let name = folder.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
                // A folder a game lives in, or one inside a game's folder, is taken
                let taken = known.iter().any(|k| k.starts_with(&folder) || folder.starts_with(k));
                if taken || name.starts_with('.') || name == "prefixes" || ignored.contains(&folder) {
                    continue;
                }
                // Only scripts (build, setup) is most likely not a game
                let guess = crate::guess::guess(&folder);
                if let Some(g) = guess.filter(|g| g.exe.as_deref().is_some_and(|e| !crate::guess::is_script(e))) {
                    out.push(g);
                }
            }
        }
        Ok(out)
    }

    fn launch_context(&self) -> anyhow::Result<launch::Context> {
        let game_dirs: Vec<PathBuf> =
            crate::db::setting(&self.db.lock().unwrap(), "gameDirs")?.and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
        Ok(launch::Context {
            protons: proton::discover(&self.home),
            default_proton: self.setting_str("defaultProton")?,
            umu: launch::find_umu(&self.home, self.setting_str("umuRun")?.as_deref()),
            prefix_root: launch::prefix_root(self.setting_str("prefixDir")?.as_deref(), &game_dirs),
        })
    }

    async fn launch(self: &Arc<Self>, id: i64) -> Result<Value, Error> {
        let failed = |e: anyhow::Error| Error::new("launch_failed", format!("{e:#}"));
        let mut game = self.game(&self.db.lock().unwrap(), id)?;
        if self.running.lock().unwrap().contains_key(&id) {
            return Err(Error::new("busy", format!("{} is already running", game.name)));
        }
        let plan = launch::plan(&game, &self.launch_context().map_err(internal)?).map_err(failed)?;
        if game.kind == Kind::Steam {
            return self.launch_steam(game, plan).await;
        }
        if let Some(prefix) = &plan.new_prefix {
            std::fs::create_dir_all(prefix).map_err(|e| failed(anyhow::anyhow!("creating {}: {e}", prefix.display())))?;
            game.prefix = Some(prefix.clone());
            games::save(&self.db.lock().unwrap(), &game).map_err(internal)?;
            self.emit("library.changed", json!({ "ids": [id] }));
        }
        if let Some(exe) = &plan.make_executable {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(exe).map(|m| m.permissions().mode()).unwrap_or(0o644);
            std::fs::set_permissions(exe, std::fs::Permissions::from_mode(mode | 0o100))
                .map_err(|e| failed(anyhow::anyhow!("making {} executable: {e}", exe.display())))?;
            tracing::info!(id, "made {} executable", exe.display());
        }
        if game.runner.needs_steam && matches!(game.kind, Kind::Proton | Kind::Hoyo) {
            self.prepare_needs_steam(&game, &plan).await.map_err(failed)?;
        }
        let log_dir = somelauncher_proto::paths::state_dir().join("logs").join(id.to_string());
        std::fs::create_dir_all(&log_dir).map_err(|e| internal(e.into()))?;
        let log_path = log_dir.join(format!("{}.log", games::now()));
        let log = std::fs::File::create(&log_path).map_err(|e| internal(e.into()))?;
        if let Some(cmd) = &plan.pre_launch {
            launch::run_hook(cmd, &plan, &log).await.map_err(failed)?;
        }
        let unit = launch::unit_name(id);
        let mut child = launch::spawn(&plan, &unit, &game.name, &log).map_err(failed)?;
        self.running.lock().unwrap().insert(id, unit.clone());
        tracing::info!(id, unit, argv = ?plan.argv, "launched {}", game.name);
        self.emit("game.state", json!({ "id": id, "state": "running", "unit": unit }));

        let state = self.clone();
        tokio::spawn(async move {
            let status = child.wait().await;
            if let Some(cmd) = &plan.post_exit
                && let Err(e) = launch::run_hook(cmd, &plan, &log).await
            {
                tracing::warn!(id, "{e:#}");
            }
            state.running.lock().unwrap().remove(&id);
            tracing::info!(id, ?status, "{} exited", game.name);
            state.emit("game.state", json!({ "id": id, "state": "stopped" }));
        });
        Ok(json!({ "unit": unit, "log": log_path }))
    }

    /// A Steam game: `steam -applaunch`, then followed from Steam's process log
    /// (start) to its reaper process (exit). Steam owns the launch, so no scope or log of ours.
    async fn launch_steam(self: &Arc<Self>, game: Game, plan: launch::Plan) -> Result<Value, Error> {
        let failed = |e: anyhow::Error| Error::new("launch_failed", format!("{e:#}"));
        let id = game.id;
        let (appid, flatpak) = launch::steam_app(&game).ok_or_else(|| failed(anyhow::anyhow!("no Steam app id")))?;
        if steam_client::find_reaper(&appid).is_some() {
            return Err(Error::new("busy", format!("{} is already running in Steam", game.name)));
        }
        let root = import::steam::roots(&self.home)
            .into_iter()
            .find(|r| r.to_string_lossy().contains("com.valvesoftware.Steam") == flatpak)
            .ok_or_else(|| failed(anyhow::anyhow!("Steam is not installed")))?;
        let offset = steam_client::log_offset(&steam_client::process_log(&root));
        let skip = if flatpak { 3 } else { 1 };
        steam_client::run(&self.home, flatpak, &plan.argv[skip..]).map_err(failed)?;
        let unit = format!("steam:{appid}");
        self.running.lock().unwrap().insert(id, unit.clone());
        self.emit("game.state", json!({ "id": id, "state": "starting", "step": "steam" }));
        tracing::info!(id, appid, "asked Steam to start {}", game.name);

        let state = self.clone();
        tokio::spawn(async move {
            let result = async {
                let reaper = steam_client::wait_started(&root, &appid, offset).await?;
                state.emit("game.state", json!({ "id": id, "state": "running", "unit": format!("steam:{appid}") }));
                tracing::info!(id, reaper, "{} is running", game.name);
                steam_client::wait_pid(reaper).await
            }
            .await;
            state.running.lock().unwrap().remove(&id);
            match result {
                Ok(()) => {
                    tracing::info!(id, "{} exited", game.name);
                    state.emit("game.state", json!({ "id": id, "state": "stopped" }));
                }
                Err(e) => {
                    tracing::warn!(id, "{}: {e:#}", game.name);
                    state.emit("game.state", json!({ "id": id, "state": "stopped", "error": format!("{e:#}") }));
                }
            }
        });
        Ok(json!({ "unit": unit }))
    }

    /// LUTRIS_SETUP.md, automated: Steam running and logged in, its DLLs in the prefix, `steam_appid.txt`.
    async fn prepare_needs_steam(&self, game: &Game, plan: &launch::Plan) -> anyhow::Result<()> {
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

    pub(crate) fn setting_str(&self, key: &str) -> anyhow::Result<Option<String>> {
        Ok(crate::db::setting(&self.db.lock().unwrap(), key)?.and_then(|v| v.as_str().map(String::from)))
    }

    fn game(&self, db: &Connection, id: i64) -> Result<Game, Error> {
        games::get(db, id).map_err(internal)?.ok_or_else(|| not_found(id))
    }
}

fn id_param(params: &Value) -> Result<i64, Error> {
    params.get("id").and_then(Value::as_i64).ok_or_else(|| Error::new("bad_params", "missing integer `id`"))
}

fn parse<T: serde::de::DeserializeOwned>(params: &Value) -> Result<T, Error> {
    serde_json::from_value(params.clone()).map_err(|e| Error::new("bad_params", e.to_string()))
}

fn to_json(v: impl serde::Serialize) -> Result<Value, Error> {
    serde_json::to_value(v).map_err(|e| internal(e.into()))
}

fn not_found(id: i64) -> Error {
    Error::new("not_found", format!("no game {id}"))
}

fn str_param<'a>(params: &'a Value, key: &str) -> Result<&'a str, Error> {
    params.get(key).and_then(Value::as_str).ok_or_else(|| Error::new("bad_params", format!("missing string `{key}`")))
}

fn internal(e: anyhow::Error) -> Error {
    tracing::error!("{e:#}");
    Error::new("internal", format!("{e:#}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> Arc<State> {
        State::new(games::test_db(), ":memory:".into())
    }
    fn req(method: &str, params: Value) -> Request {
        Request { id: 7, method: method.into(), params }
    }

    #[tokio::test]
    async fn settings_set_emits_and_get_reads_back() {
        let s = state();
        let mut events = s.subscribe();
        let r = s.handle(req("settings.set", json!({ "key": "steamApiKey", "value": "abc" }))).await;
        assert!(r.error.is_none());
        assert_eq!(events.try_recv().unwrap().event, "settings.changed");
        let r = s.handle(req("settings.get", json!({ "key": "steamApiKey" }))).await;
        assert_eq!(r.result, Some(json!("abc")));
    }

    #[tokio::test]
    async fn unknown_methods_and_bad_params_are_errors() {
        let s = state();
        assert_eq!(s.handle(req("nope", Value::Null)).await.error.unwrap().code, "unknown_method");
        assert_eq!(s.handle(req("settings.set", json!({}))).await.error.unwrap().code, "bad_params");
    }

    #[tokio::test]
    async fn games_add_update_remove() {
        let s = state();
        let mut events = s.subscribe();
        let r = s.handle(req("games.add", json!({ "name": "How to Fish", "kind": "proton", "exe": "/g/fish.exe" }))).await;
        let game = r.result.unwrap();
        let id = game["id"].as_i64().unwrap();
        assert_eq!(game["sources"][0]["source"], "manual");
        assert_eq!(events.try_recv().unwrap().event, "library.changed");

        let r = s.handle(req("games.update", json!({ "id": id, "favorite": true, "exe": null, "runner": { "needsSteam": true } }))).await;
        let game = r.result.unwrap();
        assert_eq!((game["favorite"].clone(), game["exe"].clone()), (json!(true), Value::Null));
        assert_eq!(game["runner"], json!({ "needsSteam": true }));
        assert_eq!(s.handle(req("games.list", Value::Null)).await.result.unwrap().as_array().unwrap().len(), 1);

        let bad = s.handle(req("games.update", json!({ "id": id, "playtime": 1 }))).await;
        assert_eq!(bad.error.unwrap().code, "bad_params");
        assert!(s.handle(req("games.remove", json!({ "id": id }))).await.error.is_none());
        assert_eq!(s.handle(req("games.get", json!({ "id": id }))).await.error.unwrap().code, "not_found");
    }
}
