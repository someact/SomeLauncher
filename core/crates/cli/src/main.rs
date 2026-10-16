//! `somelauncher`: the command line client of somelauncherd.
//!   somelauncher call <method> [json params]   any protocol method, prints the result
//!   somelauncher status | ping

use anyhow::{Context, Result, bail};
use serde_json::Value;
use somelauncher_proto::{Message, Request, paths};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (method, params) = match args.first().map(String::as_str) {
        Some("call") => {
            let method = args.get(1).context("usage: somelauncher call <method> [json params]")?;
            let params = match args.get(2) {
                Some(p) => serde_json::from_str(p).context("params must be JSON")?,
                None => Value::Null,
            };
            (method.clone(), params)
        }
        Some(m @ ("status" | "ping")) => (m.to_string(), Value::Null),
        Some("--version") => {
            println!("somelauncher {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        _ => bail!("usage: somelauncher <status|ping|call <method> [json params]>"),
    };

    let socket = paths::socket();
    let mut stream = UnixStream::connect(&socket)
        .with_context(|| format!("somelauncherd is not running ({})", socket.display()))?;
    let req = Request { id: 1, method, params };
    writeln!(stream, "{}", serde_json::to_string(&req)?)?;

    for line in BufReader::new(stream).lines() {
        if let Message::Reply(reply) = serde_json::from_str(&line?)? {
            if reply.id != req.id {
                continue;
            }
            if let Some(err) = reply.error {
                bail!("{}: {}", err.code, err.message);
            }
            println!("{}", serde_json::to_string_pretty(&reply.result.unwrap_or(Value::Null))?);
            return Ok(());
        }
    }
    bail!("somelauncherd closed the connection")
}
