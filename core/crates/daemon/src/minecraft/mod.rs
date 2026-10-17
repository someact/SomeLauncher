//! Minecraft through Prism Launcher (Phase 8): an instance is a library game of
//! kind `minecraft` with a `prism` source (import/prism.rs). Prism keeps owning
//! its files; we read them, change the few things the Minecraft page edits, and
//! hand Prism what it does best (new instances, accounts, launching).

use crate::rpc::State;
use anyhow::{Context as _, Result};
use serde_json::Value;
use somelauncher_proto::Error;
use somelauncher_proto::game::{Game, Kind, SourceKind};
use std::path::{Path, PathBuf};

const FLATPAK_ID: &str = "org.prismlauncher.PrismLauncher";

/// Where a library game's instance lives and which Prism owns it.
#[derive(Debug, Clone)]
pub struct Instance {
    /// Prism's instance id: its folder's name (`prismlauncher -l <id>`)
    pub id: String,
    /// The instance folder (`instance.cfg`, `mmc-pack.json`)
    pub dir: PathBuf,
    /// The game folder inside it (`minecraft` or `.minecraft`)
    pub game_dir: PathBuf,
    pub flatpak: bool,
}

impl Instance {
    pub fn of(game: &Game) -> Option<Instance> {
        if game.kind != Kind::Minecraft {
            return None;
        }
        let src = game.sources.iter().find(|s| s.source == SourceKind::Prism)?;
        let dir = game.install_dir.clone()?;
        Some(Instance {
            id: src.source_id.clone(),
            game_dir: crate::import::prism::game_dir(&dir),
            flatpak: src.data["flatpak"].as_bool().unwrap_or(false),
            dir,
        })
    }
}

/// The command that runs Prism: native first (or flatpak when the instance is the flatpak's).
pub fn prism_argv(home: &Path, flatpak: bool) -> Result<Vec<String>> {
    let native = crate::launch::which("prismlauncher");
    let has_flatpak = home.join(".var/app").join(FLATPAK_ID).is_dir();
    if native.is_some() && !(flatpak && has_flatpak) {
        return Ok(vec!["prismlauncher".into()]);
    }
    if has_flatpak {
        return Ok(["flatpak", "run", FLATPAK_ID].map(String::from).to_vec());
    }
    anyhow::bail!("Prism Launcher is not installed")
}

/// Runs Prism with `args` in its own scope, not waited for: when Prism was not
/// running this process becomes Prism, which must outlive the daemon.
pub fn run_prism(home: &Path, flatpak: bool, args: &[String]) -> Result<()> {
    let mut argv = prism_argv(home, flatpak)?;
    argv.extend(args.iter().cloned());
    let unit = format!("--unit=somelauncher-prism-{}-{}", crate::games::now(), std::process::id());
    let mut cmd = tokio::process::Command::new("systemd-run");
    cmd.args(["--user", "--scope", "--collect", "--quiet", "--expand-environment=no", &unit, "--"]).args(&argv);
    cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    cmd.spawn().context("running Prism Launcher")?;
    Ok(())
}

impl State {
    fn instance(&self, params: &Value) -> Result<(Game, Instance), Error> {
        let id = params["id"].as_i64().ok_or_else(|| Error::new("bad_params", "missing integer `id`"))?;
        let game = self.game(&self.db.lock().unwrap(), id)?;
        let inst = Instance::of(&game).ok_or_else(|| Error::new("not_found", format!("{} is not a Prism instance", game.name)))?;
        Ok((game, inst))
    }

    /// `minecraft.prism {id?}`: Prism's window, on that instance when given
    pub(crate) fn minecraft_prism(&self, params: &Value) -> Result<Value, Error> {
        let (flatpak, args) = match params.get("id").filter(|v| !v.is_null()) {
            Some(_) => {
                let (_, inst) = self.instance(params)?;
                (inst.flatpak, vec!["--show".to_string(), inst.id])
            }
            None => (false, vec![]),
        };
        run_prism(&self.home, flatpak, &args).map_err(|e| Error::new("launch_failed", format!("{e:#}")))?;
        Ok(Value::Null)
    }
}
