//! Method dispatch. Every method takes the request's params and returns a JSON
//! result or a protocol error; events go to every client through `State::emit`.

use crate::{desktop, games, import, launch, proton};
use anyhow::Context as _;
use rusqlite::Connection;
use serde_json::{Value, json};
use somelauncher_proto::game::{Game, GamePatch, NewGame, Source, SourceKind};
use somelauncher_proto::{Error, Event, PROTOCOL, Reply, Request, paths};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::{broadcast, watch};

pub struct State {
    pub(crate) db: Mutex<Connection>,
    db_path: PathBuf,
    /// Where importers look for other launchers' files
    pub(crate) home: PathBuf,
    pub(crate) http: reqwest::Client,
    pub(crate) jobs: crate::jobs::Jobs,
    /// Running (or starting) games
    pub(crate) running: Mutex<std::collections::HashMap<i64, crate::session::Running>>,
    /// Gaming mode: the power profile and charge limit while games run
    pub(crate) power: tokio::sync::Mutex<crate::power::Power>,
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
            power: Default::default(),
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
                "monitor": crate::monitor::current(),
                "db": self.db_path,
            })),
            "power.status" => Ok(self.power_status().await),
            "hoyo.games" => self.hoyo_games(params).await,
            "hoyo.content" => self.hoyo_content(params).await,
            "hoyo.adopt" => self.hoyo_adopt(params),
            "hoyo.plan" => self.hoyo_plan(params).await,
            "hoyo.install" => self.hoyo_install(params).await,
            "hoyo.updatePlan" => self.hoyo_update_plan(params).await,
            "hoyo.update" => self.hoyo_update(params).await,
            "hoyo.preload" => self.hoyo_preload(params).await,
            "hoyo.repair" => self.hoyo_repair(params).await,
            "hoyo.check" => Ok(json!(self.hoyo_check(params["force"].as_bool().unwrap_or(true)).await.map_err(|e| Error::new("network", format!("{e:#}")))?)),
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
                crate::meta::tags::retag(&db, id).map_err(internal)?;
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
                crate::meta::tags::retag(&db, id).map_err(internal)?;
                let game = self.game(&db, id)?;
                desktop::refresh(&paths::applications_dir(), &game, &desktop::cli()).map_err(internal)?;
                self.emit("library.changed", json!({ "ids": [id] }));
                to_json(game)
            }
            "games.remove" => {
                let id = id_param(params)?;
                if !games::remove(&self.db.lock().unwrap(), id).map_err(internal)? {
                    return Err(not_found(id));
                }
                desktop::remove(&paths::applications_dir(), id).map_err(internal)?;
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
                desktop::remove(&paths::applications_dir(), from).map_err(internal)?;
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
            "games.stop" => self.stop(id_param(params)?).await,
            "games.running" => Ok(self.running_games()),
            "games.logs" => {
                let id = id_param(params)?;
                let logs = crate::session::logs(id).into_iter().map(|(path, time, size)| json!({ "path": path, "time": time, "size": size }));
                Ok(Value::Array(logs.collect()))
            }
            "games.desktop" => {
                let id = id_param(params)?;
                let game = self.game(&self.db.lock().unwrap(), id)?;
                let dir = paths::applications_dir();
                match params.get("enabled").and_then(Value::as_bool) {
                    Some(true) => drop(desktop::write(&dir, &game, &desktop::cli()).map_err(internal)?),
                    Some(false) => desktop::remove(&dir, id).map_err(internal)?,
                    None => {}
                }
                let file = desktop::path(&dir, id);
                Ok(json!({ "enabled": file.exists(), "path": file }))
            }
            "games.duplicates" => to_json(games::duplicates(&self.db.lock().unwrap()).map_err(internal)?),
            "games.guess" => {
                let path = PathBuf::from(str_param(params, "path")?);
                let guess = crate::guess::guess(&path).ok_or_else(|| Error::new("not_found", format!("{} does not exist", path.display())))?;
                to_json(guess)
            }
            "meta.search" => {
                let game = match params.get("id") {
                    Some(_) => Some(self.game(&self.db.lock().unwrap(), id_param(params)?)?),
                    None => None,
                };
                let query = params.get("query").and_then(Value::as_str).map(String::from);
                let query = query.or_else(|| game.as_ref().map(|g| g.name.clone())).ok_or_else(|| Error::new("bad_params", "give `id` or `query`"))?;
                let key = self.setting_str("steamGridDbKey").map_err(internal)?;
                let found = crate::meta::search(&self.http, key.as_deref(), &query, game.as_ref()).await;
                to_json(found.map_err(|e| Error::new("network", format!("{e:#}")))?)
            }
            "meta.match" => {
                let id = id_param(params)?;
                let (source, source_id) = (str_param(params, "source")?, str_param(params, "sourceId")?);
                self.game(&self.db.lock().unwrap(), id)?;
                let key = self.setting_str("steamGridDbKey").map_err(internal)?;
                let meta = crate::meta::resolve(&self.http, key.as_deref(), source, source_id).await;
                let meta = meta.map_err(|e| Error::new("network", format!("{e:#}")))?;
                let db = self.db.lock().unwrap();
                games::set_meta(&db, id, Some(&meta)).map_err(internal)?;
                self.emit("library.changed", json!({ "ids": [id] }));
                let game = self.game(&db, id)?;
                drop(db);
                // Details, tags and art follow with `library.changed`
                self.matched(id);
                to_json(game)
            }
            "meta.unmatch" => {
                let id = id_param(params)?;
                let db = self.db.lock().unwrap();
                self.game(&db, id)?;
                games::set_meta(&db, id, None).map_err(internal)?;
                crate::meta::tags::retag(&db, id).map_err(internal)?;
                self.emit("library.changed", json!({ "ids": [id] }));
                Ok(Value::Null)
            }
            "meta.refresh" => {
                let force = params.get("force").and_then(Value::as_bool).unwrap_or(false);
                match params.get("id") {
                    Some(_) => {
                        let id = id_param(params)?;
                        self.game(&self.db.lock().unwrap(), id)?;
                        self.refresh_meta(id, force).await.map_err(|e| Error::new("network", format!("{e:#}")))?;
                        to_json(self.game(&self.db.lock().unwrap(), id)?)
                    }
                    None => Ok(json!({ "id": self.refresh_all_meta(force).map_err(|e| Error::new("busy", format!("{e:#}")))? })),
                }
            }
            "builds.check" => match params.get("id") {
                Some(_) => {
                    let id = id_param(params)?;
                    self.game(&self.db.lock().unwrap(), id)?;
                    self.check_build(id).await.map_err(|e| Error::new("network", format!("{e:#}")))?;
                    to_json(self.game(&self.db.lock().unwrap(), id)?)
                }
                None => Ok(json!({ "id": self.check_all_builds().map_err(|e| Error::new("busy", format!("{e:#}")))? })),
            },
            "art.fetch" => match params.get("id") {
                Some(_) => {
                    let id = id_param(params)?;
                    self.game(&self.db.lock().unwrap(), id)?;
                    let force = params.get("force").and_then(Value::as_bool).unwrap_or(false);
                    self.refresh_art(id, force).await.map_err(|e| Error::new("network", format!("{e:#}")))?;
                    to_json(self.game(&self.db.lock().unwrap(), id)?)
                }
                None => {
                    let force = params.get("force").and_then(Value::as_bool).unwrap_or(false);
                    let id = self.fetch_all_art(force, false).map_err(|e| Error::new("busy", format!("{e:#}")))?;
                    Ok(json!({ "id": id }))
                }
            },
            "art.set" => {
                let id = id_param(params)?;
                let kind = art_kind(params)?;
                self.game(&self.db.lock().unwrap(), id)?;
                let origin = params.get("origin").and_then(Value::as_str);
                self.set_art(id, kind, origin).await.map_err(|e| Error::new("bad_params", format!("{e:#}")))?;
                to_json(self.game(&self.db.lock().unwrap(), id)?)
            }
            "art.options" => {
                let game = self.game(&self.db.lock().unwrap(), id_param(params)?)?;
                self.art_options(&game, art_kind(params)?).await.map_err(internal)
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
    async fn import(self: &Arc<Self>, params: &Value) -> Result<Value, Error> {
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
        // Hoyo games found (Twintail's): their versions from their own files
        summary.updated.extend(self.adopt_all().into_iter().filter(|id| !summary.added.contains(id)));
        // Steam games need no confirmation: their app id is the match
        let matched = crate::meta::match_steam_games(&self.db.lock().unwrap()).map_err(internal)?;
        let new_matches = !matched.is_empty();
        summary.updated.extend(matched.into_iter().filter(|id| !summary.added.contains(id)));
        for id in summary.ids() {
            crate::meta::tags::retag(&self.db.lock().unwrap(), id).map_err(internal)?;
        }
        // Details and art for new games follow in the background; a running job is fine
        if new_matches || !summary.added.is_empty() {
            let _ = self.refresh_all_meta(false);
            let _ = self.fetch_all_art(false, false);
            let _ = self.check_all_builds();
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

    pub(crate) fn launch_context(&self) -> anyhow::Result<launch::Context> {
        let game_dirs: Vec<PathBuf> =
            crate::db::setting(&self.db.lock().unwrap(), "gameDirs")?.and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
        Ok(launch::Context {
            protons: proton::discover(&self.home),
            default_proton: self.setting_str("defaultProton")?,
            umu: launch::find_umu(&self.home, self.setting_str("umuRun")?.as_deref()),
            prefix_root: launch::prefix_root(self.setting_str("prefixDir")?.as_deref(), &game_dirs),
            defaults: crate::db::setting(&self.db.lock().unwrap(), "defaults")?
                .map(serde_json::from_value)
                .transpose()
                .context("the defaults setting")?
                .unwrap_or_default(),
            offload: crate::gpu::offload(Path::new("/sys/class/drm")),
            tools: launch::WRAPPERS.iter().filter(|t| launch::which(t).is_some()).map(|t| t.to_string()).collect(),
            monitor: crate::monitor::current(),
        })
    }

    pub(crate) fn setting_str(&self, key: &str) -> anyhow::Result<Option<String>> {
        Ok(crate::db::setting(&self.db.lock().unwrap(), key)?.and_then(|v| v.as_str().map(String::from)))
    }

    pub(crate) fn game(&self, db: &Connection, id: i64) -> Result<Game, Error> {
        games::get(db, id).map_err(internal)?.ok_or_else(|| not_found(id))
    }
}

fn art_kind(params: &Value) -> Result<&'static str, Error> {
    let kind = str_param(params, "kind")?;
    crate::meta::art::KINDS.into_iter().find(|k| *k == kind).ok_or_else(|| Error::new("bad_params", format!("no art kind {kind}")))
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

pub(crate) fn internal(e: anyhow::Error) -> Error {
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
