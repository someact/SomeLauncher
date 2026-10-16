//! The unix socket: one task per client. Each client gets `hello` first, then
//! replies to its own requests and every broadcast event, in one ordered stream.

use crate::rpc::State;
use anyhow::{Context, Result};
use somelauncher_proto::{Error, Message, Reply, Request};
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{broadcast, mpsc};

/// Binds the socket, or returns None when a live daemon already owns it.
/// A socket file nobody answers on is left over from a crash and is replaced.
pub async fn bind(path: &Path) -> Result<Option<UnixListener>> {
    if path.exists() {
        if UnixStream::connect(path).await.is_ok() {
            return Ok(None);
        }
        std::fs::remove_file(path).with_context(|| format!("removing stale {}", path.display()))?;
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let listener = UnixListener::bind(path).with_context(|| format!("binding {}", path.display()))?;
    // Only this user may talk to the daemon
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(Some(listener))
}

pub async fn serve(listener: UnixListener, state: Arc<State>) -> Result<()> {
    let mut shutdown = state.shutdown_signal();
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                tokio::spawn(client(stream, state.clone()));
            }
            _ = shutdown.changed() => break,
            _ = term.recv() => break,
            _ = tokio::signal::ctrl_c() => break,
        }
    }
    Ok(())
}

async fn client(stream: UnixStream, state: Arc<State>) {
    let (read, mut write) = stream.into_split();
    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
    let mut events = state.subscribe();
    let _ = tx.send(Message::Event(state.hello()));

    // Writer: replies and events in the order they were produced
    let writer = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let mut line = serde_json::to_string(&msg).expect("messages serialize");
            line.push('\n');
            if write.write_all(line.as_bytes()).await.is_err() {
                break;
            }
        }
    });

    let mut lines = BufReader::new(read).lines();
    loop {
        tokio::select! {
            line = lines.next_line() => {
                let Ok(Some(line)) = line else { break };
                if line.trim().is_empty() {
                    continue;
                }
                let reply = match serde_json::from_str::<Request>(&line) {
                    Ok(req) => state.handle(req).await,
                    Err(e) => Reply::err(0, Error::new("bad_request", e.to_string())),
                };
                if tx.send(Message::Reply(reply)).is_err() {
                    break;
                }
            }
            event = events.recv() => match event {
                Ok(ev) => { let _ = tx.send(Message::Event(ev)); }
                Err(broadcast::error::RecvError::Lagged(n)) => tracing::warn!(n, "client lagged; events dropped"),
                Err(broadcast::error::RecvError::Closed) => break,
            },
        }
    }
    drop(tx);
    let _ = writer.await;
}
