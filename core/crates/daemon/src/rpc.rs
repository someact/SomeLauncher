//! Method dispatch. Every method takes the request's params and returns a JSON
//! result or a protocol error; events go to every client through `State::emit`.

use crate::games;
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
    events: broadcast::Sender<Event>,
    shutdown: watch::Sender<bool>,
}

impl State {
    pub fn new(db: Connection, db_path: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            db: Mutex::new(db),
            db_path,
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

    pub fn handle(&self, req: Request) -> Reply {
        match self.call(&req.method, &req.params) {
            Ok(result) => Reply::ok(req.id, result),
            Err(e) => {
                tracing::debug!(method = req.method, error = e.message, "request failed");
                Reply::err(req.id, e)
            }
        }
    }

    fn call(&self, method: &str, params: &Value) -> Result<Value, Error> {
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
            "shutdown" => {
                let _ = self.shutdown.send(true);
                Ok(Value::Null)
            }
            _ => Err(Error::new("unknown_method", format!("no method {method}"))),
        }
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

    #[test]
    fn settings_set_emits_and_get_reads_back() {
        let s = state();
        let mut events = s.subscribe();
        let r = s.handle(req("settings.set", json!({ "key": "steamApiKey", "value": "abc" })));
        assert!(r.error.is_none());
        assert_eq!(events.try_recv().unwrap().event, "settings.changed");
        let r = s.handle(req("settings.get", json!({ "key": "steamApiKey" })));
        assert_eq!(r.result, Some(json!("abc")));
    }

    #[test]
    fn unknown_methods_and_bad_params_are_errors() {
        let s = state();
        assert_eq!(s.handle(req("nope", Value::Null)).error.unwrap().code, "unknown_method");
        assert_eq!(s.handle(req("settings.set", json!({}))).error.unwrap().code, "bad_params");
    }

    #[test]
    fn games_add_update_remove() {
        let s = state();
        let mut events = s.subscribe();
        let r = s.handle(req("games.add", json!({ "name": "How to Fish", "kind": "proton", "exe": "/g/fish.exe" })));
        let game = r.result.unwrap();
        let id = game["id"].as_i64().unwrap();
        assert_eq!(game["sources"][0]["source"], "manual");
        assert_eq!(events.try_recv().unwrap().event, "library.changed");

        let r = s.handle(req("games.update", json!({ "id": id, "favorite": true, "exe": null, "runner": { "needsSteam": true } })));
        let game = r.result.unwrap();
        assert_eq!((game["favorite"].clone(), game["exe"].clone()), (json!(true), Value::Null));
        assert_eq!(game["runner"], json!({ "needsSteam": true }));
        assert_eq!(s.handle(req("games.list", Value::Null)).result.unwrap().as_array().unwrap().len(), 1);

        let bad = s.handle(req("games.update", json!({ "id": id, "playtime": 1 })));
        assert_eq!(bad.error.unwrap().code, "bad_params");
        assert!(s.handle(req("games.remove", json!({ "id": id }))).error.is_none());
        assert_eq!(s.handle(req("games.get", json!({ "id": id }))).error.unwrap().code, "not_found");
    }
}
