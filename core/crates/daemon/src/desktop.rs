//! Optional `.desktop` entries per game, so app launchers can start a game
//! directly (`somelauncher play <id>`). The file is the state: a game has an
//! entry when its file exists.

use anyhow::Result;
use somelauncher_proto::game::Game;
use std::path::{Path, PathBuf};

pub fn path(dir: &Path, id: i64) -> PathBuf {
    dir.join(format!("somelauncher-game-{id}.desktop"))
}

/// The `somelauncher` CLI: next to the daemon when installed together, else on PATH.
pub fn cli() -> String {
    let sibling = std::env::current_exe().ok().and_then(|e| Some(e.parent()?.join("somelauncher")));
    match sibling.filter(|p| p.is_file()) {
        Some(p) => p.to_string_lossy().into_owned(),
        None => "somelauncher".into(),
    }
}

/// A desktop entry string value: no line breaks.
fn value(s: &str) -> String {
    s.replace(['\n', '\r'], " ")
}

/// An Exec argument: quoted when it needs it, with the spec's escapes.
fn arg(s: &str) -> String {
    if s.chars().all(|c| c.is_ascii_alphanumeric() || "/-_.".contains(c)) {
        return s.into();
    }
    let escaped: String = s.chars().flat_map(|c| if "\"`$\\".contains(c) { vec!['\\', c] } else { vec![c] }).collect();
    format!("\"{}\"", escaped.replace('%', "%%"))
}

/// An icon file the sources know (Prism's instance icon, Lutris' art), else the generic one.
fn icon(game: &Game) -> String {
    let local = game.sources.iter().flat_map(|s| [&s.data["icon"], &s.data["coverart"]]).filter_map(|v| v.as_str()).find(|p| Path::new(p).is_file());
    local.map(String::from).unwrap_or_else(|| "applications-games".into())
}

pub fn write(dir: &Path, game: &Game, cli: &str) -> Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let file = path(dir, game.id);
    let text = format!(
        "[Desktop Entry]\nType=Application\nName={}\nComment=Play with SomeLauncher\nExec={} play {}\nIcon={}\nCategories=Game;\nTerminal=false\nX-SomeLauncher-Game={}\n",
        value(&game.name),
        arg(cli),
        game.id,
        value(&icon(game)),
        game.id
    );
    std::fs::write(&file, text)?;
    Ok(file)
}

/// Rewrites the entry when the game has one (after a rename).
pub fn refresh(dir: &Path, game: &Game, cli: &str) -> Result<()> {
    if path(dir, game.id).exists() {
        write(dir, game, cli)?;
    }
    Ok(())
}

pub fn remove(dir: &Path, id: i64) -> Result<()> {
    match std::fs::remove_file(path(dir, id)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use somelauncher_proto::game::{Kind, NewGame};

    fn game(name: &str) -> Game {
        Game::new(7, NewGame::new(name, Kind::Proton))
    }

    #[test]
    fn writes_refreshes_and_removes_entries() {
        let dir = tempfile::tempdir().unwrap();
        let mut g = game("How to Fish");
        refresh(dir.path(), &g, "somelauncher").unwrap();
        assert!(!path(dir.path(), 7).exists(), "refresh does not create");

        let file = write(dir.path(), &g, "/opt/Some Launcher/somelauncher").unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("Name=How to Fish\n"));
        assert!(text.contains("Exec=\"/opt/Some Launcher/somelauncher\" play 7\n"));
        assert!(text.contains("Icon=applications-games\n"));

        g.name = "How to Fish\nCo-op".into();
        refresh(dir.path(), &g, "somelauncher").unwrap();
        assert!(std::fs::read_to_string(&file).unwrap().contains("Name=How to Fish Co-op\nComment"));
        remove(dir.path(), 7).unwrap();
        remove(dir.path(), 7).unwrap();
        assert!(!file.exists());
    }
}
