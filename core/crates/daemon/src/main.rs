//! somelauncherd: owns the library, launches, downloads and playtime.
//! Clients (the QML UI, the `somelauncher` CLI) talk to it over a unix socket
//! with JSON lines (docs/protocol.md). Closing a client never stops its work.

mod db;
mod games;
mod import;
mod rpc;
mod server;

use anyhow::{Context, Result};
use somelauncher_proto::paths;
use std::io::IsTerminal;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[tokio::main]
async fn main() -> Result<()> {
    if std::env::args().any(|a| a == "--version") {
        println!("somelauncherd {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    init_logging()?;

    let socket = paths::socket();
    let Some(listener) = server::bind(&socket).await? else {
        tracing::info!("another somelauncherd already answers on {}", socket.display());
        return Ok(());
    };

    let db_path = paths::data_dir().join("library.db");
    let db = db::open(&db_path).with_context(|| format!("opening {}", db_path.display()))?;
    tracing::info!(db = %db_path.display(), socket = %socket.display(), "started");

    let state = rpc::State::new(db, db_path);
    let result = server::serve(listener, state).await;
    let _ = std::fs::remove_file(&socket);
    tracing::info!("stopped");
    result
}

/// Always to `$XDG_STATE_HOME/somelauncher/daemon.log`, and to stderr when it is a terminal.
fn init_logging() -> Result<()> {
    let dir = paths::state_dir();
    std::fs::create_dir_all(&dir)?;
    let file = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("daemon.log"))?;
    let filter = || EnvFilter::try_from_env("SOMELAUNCHER_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let stderr = std::io::stderr().is_terminal().then(|| fmt::layer().with_writer(std::io::stderr).with_filter(filter()));
    tracing_subscriber::registry()
        .with(fmt::layer().with_ansi(false).with_writer(std::sync::Mutex::new(file)).with_filter(filter()))
        .with(stderr)
        .init();
    Ok(())
}
