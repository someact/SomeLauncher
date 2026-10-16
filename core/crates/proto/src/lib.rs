//! The wire format between `somelauncherd` and its clients (the QML UI, the CLI).
//! One JSON object per line over a unix socket; see `docs/protocol.md`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

pub mod game;

/// Bumped on any incompatible change; the daemon sends it in `hello`.
pub const PROTOCOL: u32 = 1;

/// Client → daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    pub method: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub params: Value,
}

/// Daemon → client. A reply carries the request's `id` and either `result` or
/// `error`; an event carries `event` and `data` and no `id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Message {
    Reply(Reply),
    Event(Event),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reply {
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<Error>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub event: String,
    #[serde(default)]
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Error {
    pub code: String,
    pub message: String,
}

impl Error {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into() }
    }
}

impl Reply {
    pub fn ok(id: u64, result: Value) -> Self {
        Self { id, result: Some(result), error: None }
    }
    pub fn err(id: u64, error: Error) -> Self {
        Self { id, result: None, error: Some(error) }
    }
}

/// XDG locations shared by the daemon and the CLI. Each honours its XDG
/// variable, so tests can point everything at a temporary directory.
pub mod paths {
    use super::PathBuf;

    fn xdg(var: &str, fallback: &str) -> PathBuf {
        match std::env::var_os(var) {
            Some(v) if !v.is_empty() => PathBuf::from(v),
            _ => PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(fallback),
        }
    }

    pub fn socket() -> PathBuf {
        if let Some(p) = std::env::var_os("SOMELAUNCHER_SOCKET") {
            return p.into();
        }
        let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        dir.join("somelauncher.sock")
    }
    pub fn config_dir() -> PathBuf {
        xdg("XDG_CONFIG_HOME", ".config").join("somelauncher")
    }
    pub fn data_dir() -> PathBuf {
        xdg("XDG_DATA_HOME", ".local/share").join("somelauncher")
    }
    pub fn state_dir() -> PathBuf {
        xdg("XDG_STATE_HOME", ".local/state").join("somelauncher")
    }
    pub fn cache_dir() -> PathBuf {
        xdg("XDG_CACHE_HOME", ".cache").join("somelauncher")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reply_and_event_are_told_apart() {
        let r: Message = serde_json::from_str(r#"{"id":3,"result":{"pong":true}}"#).unwrap();
        assert!(matches!(r, Message::Reply(Reply { id: 3, .. })));
        let e: Message = serde_json::from_str(r#"{"event":"hello","data":{"protocol":1}}"#).unwrap();
        assert!(matches!(e, Message::Event(Event { ref event, .. }) if event == "hello"));
    }

    #[test]
    fn request_params_default_to_null() {
        let r: Request = serde_json::from_str(r#"{"id":1,"method":"ping"}"#).unwrap();
        assert_eq!(r.params, Value::Null);
        assert_eq!(serde_json::to_value(&r).unwrap(), json!({"id":1,"method":"ping"}));
    }
}
