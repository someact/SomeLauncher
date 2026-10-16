//! Method dispatch. Every method takes the request's params and returns a JSON
//! result or a protocol error; events go to every client through `State::emit`.

use crate::{games, import};
use rusqlite::Connection;
use serde_json::{Value, json};
use somelauncher_proto::game::{Game, GamePatch, NewGame, Source, SourceKind};
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

    pub async fn handle(&self, req: Request) -> Reply {
        match self.call(&req.method, &req.params).await {
            Ok(result) => Reply::ok(req.id, result),
            Err(e) => {
                tracing::debug!(method = req.method, error = e.message, "request failed");
                Reply::err(req.id, e)
            }
        }
    }

    async fn call(&self, method: &str, params: &Value) -> Result<Value, Error> {
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
            "library.import" => self.import(params).await,
            "shutdown" => {
                let _ = self.shutdown.send(true);
                Ok(Value::Null)
            }
            _ => Err(Error::new("unknown_method", format!("no method {method}"))),
        }
    }

    /// Runs the importers; one failing does not stop the others.
    async fn import(&self, params: &Value) -> Result<Value, Error> {
        let all = ["steam"];
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
