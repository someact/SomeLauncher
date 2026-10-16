//! `somelauncher`: the command line client of somelauncherd.
//!   somelauncher list [--all]                  the library (hidden games with --all)
//!   somelauncher play <id|name>                launch a game
//!   somelauncher stop <id|name>                stop a running game
//!   somelauncher call <method> [json params]   any protocol method, prints the result
//!   somelauncher status | ping
//! The daemon is started when nobody answers on the socket (`SOMELAUNCHER_DAEMON`, else
//! `somelauncherd` next to this binary or on PATH), so desktop entries work on their own.

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use somelauncher_proto::{Message, Request, paths};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const USAGE: &str = "usage: somelauncher <list [--all] | play <game> | stop <game> | status | ping | call <method> [json params]>";

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("call") => {
            let method = args.get(1).context("usage: somelauncher call <method> [json params]")?;
            let params = match args.get(2) {
                Some(p) => serde_json::from_str(p).context("params must be JSON")?,
                None => Value::Null,
            };
            print_json(&call(method, params)?)
        }
        Some(m @ ("status" | "ping")) => print_json(&call(m, Value::Null)?),
        Some("list") => list(args.get(1).is_some_and(|a| a == "--all")),
        Some("play") => {
            let id = find(args.get(1).context("usage: somelauncher play <id or name>")?)?;
            let r = call("games.launch", json!({ "id": id }))?;
            println!("started {}", r["unit"].as_str().unwrap_or_default());
            Ok(())
        }
        Some("stop") => {
            let id = find(args.get(1).context("usage: somelauncher stop <id or name>")?)?;
            call("games.stop", json!({ "id": id }))?;
            Ok(())
        }
        Some("--version") => {
            println!("somelauncher {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => bail!(USAGE),
    }
}

fn print_json(v: &Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(v)?);
    Ok(())
}

fn daemon_binary() -> PathBuf {
    if let Some(p) = std::env::var_os("SOMELAUNCHER_DAEMON") {
        return p.into();
    }
    let sibling = std::env::current_exe().ok().and_then(|e| Some(e.parent()?.join("somelauncherd")));
    sibling.filter(|p| p.is_file()).unwrap_or_else(|| "somelauncherd".into())
}

/// Connects, starting the daemon when nobody answers.
fn connect() -> Result<UnixStream> {
    let socket = paths::socket();
    if let Ok(s) = UnixStream::connect(&socket) {
        return Ok(s);
    }
    let bin = daemon_binary();
    std::process::Command::new(&bin)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| format!("somelauncherd is not running and {} could not be started", bin.display()))?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match UnixStream::connect(&socket) {
            Ok(s) => return Ok(s),
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(e).with_context(|| format!("somelauncherd did not open {}", socket.display())),
        }
    }
}

fn call(method: &str, params: Value) -> Result<Value> {
    let mut stream = connect()?;
    let req = Request { id: 1, method: method.into(), params };
    writeln!(stream, "{}", serde_json::to_string(&req)?)?;
    for line in BufReader::new(stream).lines() {
        if let Message::Reply(reply) = serde_json::from_str(&line?)? {
            if reply.id != req.id {
                continue;
            }
            if let Some(err) = reply.error {
                bail!("{}", err.message);
            }
            return Ok(reply.result.unwrap_or(Value::Null));
        }
    }
    bail!("somelauncherd closed the connection")
}

/// A game by id, by exact name (ignoring case), or by the only name containing the text.
fn find(game: &str) -> Result<i64> {
    if let Ok(id) = game.parse() {
        return Ok(id);
    }
    let games = call("games.list", json!({ "includeHidden": true }))?;
    let games = games.as_array().cloned().unwrap_or_default();
    let name = |g: &Value| g["name"].as_str().unwrap_or_default().to_lowercase();
    let want = game.to_lowercase();
    if let Some(g) = games.iter().find(|g| name(g) == want) {
        return Ok(g["id"].as_i64().unwrap_or_default());
    }
    let matches: Vec<&Value> = games.iter().filter(|g| name(g).contains(&want)).collect();
    match matches.as_slice() {
        [g] => Ok(g["id"].as_i64().unwrap_or_default()),
        [] => bail!("no game named {game:?}"),
        many => {
            let names: Vec<&str> = many.iter().filter_map(|g| g["name"].as_str()).collect();
            bail!("{game:?} matches {}: say which", names.join(", "))
        }
    }
}

fn list(all: bool) -> Result<()> {
    let games = call("games.list", json!({ "includeHidden": all }))?;
    let running = call("games.running", Value::Null)?;
    let running: Vec<i64> = running.as_array().into_iter().flatten().filter_map(|r| r["id"].as_i64()).collect();
    let mut out = std::io::stdout().lock();
    for g in games.as_array().into_iter().flatten() {
        let id = g["id"].as_i64().unwrap_or_default();
        let hours = g["playtime"].as_i64().unwrap_or(0) as f64 / 3600.0;
        let mark = if running.contains(&id) { "▶" } else { " " };
        let line = writeln!(out, "{id:>4} {mark} {:<9} {:>7.1} h  {}", g["kind"].as_str().unwrap_or_default(), hours, g["name"].as_str().unwrap_or_default());
        // A closed pipe (`| head`) is not an error
        if line.is_err() {
            break;
        }
    }
    Ok(())
}
