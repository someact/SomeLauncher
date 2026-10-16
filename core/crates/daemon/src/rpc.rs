//! Method dispatch. Every method takes the request's params and returns a JSON
//! result or a protocol error; events go to every client through `State::emit`.

use rusqlite::Connection;
use serde_json::{Value, json};
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
            "shutdown" => {
                let _ = self.shutdown.send(true);
                Ok(Value::Null)
            }
            _ => Err(Error::new("unknown_method", format!("no method {method}"))),
        }
    }
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
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&mut conn).unwrap();
        State::new(conn, ":memory:".into())
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
}
