//! How a game is started. `plan` turns a game and the global settings into a
//! command line, env and working dir (pure, so `games.plan` can show it and
//! tests can check it); `spawn` starts that plan in its own systemd scope.

use crate::proton::{self, Proton};
use anyhow::{Context as _, Result, anyhow, bail};
use serde::Serialize;
use somelauncher_proto::game::{Game, Kind};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;

/// What a plan needs besides the game: discovered tools and global settings.
pub struct Context {
    pub protons: Vec<Proton>,
    pub default_proton: Option<String>,
    pub umu: Option<PathBuf>,
    /// Where new prefixes go (`<prefixDir>/<game name>`)
    pub prefix_root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub argv: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: Option<PathBuf>,
    /// A prefix chosen for a game that had none; the game keeps it from now on
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_prefix: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_launch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_exit: Option<String>,
}

pub fn which(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")?.to_str()?.split(':').map(|d| Path::new(d).join(name)).find(|p| p.is_file())
}

/// umu-run: the `umuRun` setting, else on PATH, else the copy Lutris ships.
pub fn find_umu(home: &Path, setting: Option<&str>) -> Option<PathBuf> {
    setting
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(|| which("umu-run"))
        .or_else(|| Some(home.join(".local/share/lutris/runtime/umu/umu-run")).filter(|p| p.is_file()))
}

/// The folder for new prefixes: the `prefixDir` setting, else `Prefixes` in the first game folder, else our data dir.
pub fn prefix_root(setting: Option<&str>, game_dirs: &[PathBuf]) -> PathBuf {
    match (setting, game_dirs.first()) {
        (Some(s), _) if !s.is_empty() => PathBuf::from(s),
        (_, Some(dir)) => dir.join("Prefixes"),
        _ => somelauncher_proto::paths::data_dir().join("prefixes"),
    }
}

/// A folder name from a game name: no slashes, no leading dot.
fn folder_name(name: &str) -> String {
    let s: String = name.chars().map(|c| if c == '/' || c.is_control() { '_' } else { c }).collect();
    let s = s.trim().trim_start_matches('.').to_string();
    if s.is_empty() { "game".into() } else { s }
}

fn path_str(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

pub fn plan(game: &Game, ctx: &Context) -> Result<Plan> {
    let r = &game.runner;
    let exe = || game.exe.as_ref().ok_or_else(|| anyhow!("{} has no executable set", game.name));
    let mut env = BTreeMap::new();
    let mut new_prefix = None;
    let mut argv = match game.kind {
        Kind::Proton | Kind::Hoyo => {
            let umu = ctx.umu.as_ref().ok_or_else(|| anyhow!("umu-run is not installed (install umu-launcher)"))?;
            let proton = match &r.proton {
                Some(name) => proton::resolve(&ctx.protons, name).ok_or_else(|| anyhow!("Proton {name} is not installed"))?,
                None => proton::default(&ctx.protons, ctx.default_proton.as_deref()).ok_or_else(|| anyhow!("no Proton build is installed"))?,
            };
            let prefix = match &game.prefix {
                Some(p) => p.clone(),
                None => {
                    let p = ctx.prefix_root.join(folder_name(&game.name));
                    new_prefix = Some(p.clone());
                    p
                }
            };
            env.insert("WINEPREFIX".into(), path_str(&prefix));
            env.insert("PROTONPATH".into(), path_str(&proton.dir));
            env.insert("GAMEID".into(), r.game_id.clone().unwrap_or_else(|| "umu-0".into()));
            let mut overrides: Vec<String> = r.dll_overrides.iter().map(|(dll, mode)| format!("{dll}={mode}")).collect();
            if r.needs_steam {
                // Keep Proton's lsteamclient so the game reaches the real Steam client
                env.insert("UMU_USE_STEAM".into(), "1".into());
                let own = |dll: &str| r.dll_overrides.keys().any(|k| k.eq_ignore_ascii_case(dll));
                overrides.extend(crate::needs_steam::OVERRIDES.iter().filter(|d| !own(d)).map(|d| format!("{d}=n,b")));
            }
            if !overrides.is_empty() {
                env.insert("WINEDLLOVERRIDES".into(), overrides.join(";"));
            }
            vec![path_str(umu), path_str(exe()?)]
        }
        Kind::Native => {
            let exe = exe()?;
            let is_script = exe.extension().is_some_and(|e| e == "sh");
            let runnable = std::fs::metadata(exe).is_ok_and(|m| std::os::unix::fs::PermissionsExt::mode(&m.permissions()) & 0o111 != 0);
            if is_script && !runnable { vec!["sh".into(), path_str(exe)] } else { vec![path_str(exe)] }
        }
        Kind::Steam => {
            let (appid, flatpak) = steam_app(game).ok_or_else(|| anyhow!("{} has no Steam app id", game.name))?;
            let mut argv: Vec<String> = if flatpak { vec!["flatpak".into(), "run".into(), "com.valvesoftware.Steam".into()] } else { vec!["steam".into()] };
            argv.extend(["-applaunch".into(), appid]);
            argv.extend(r.args.iter().cloned());
            // Steam starts the game itself, with its own Proton and launch options
            return Ok(Plan { argv, env, cwd: None, new_prefix: None, pre_launch: r.pre_launch.clone(), post_exit: r.post_exit.clone() });
        }
        Kind::Minecraft => bail!("Minecraft instances are launched through Prism (Phase 2e)"),
    };
    argv.extend(r.args.iter().cloned());
    // The game's own env wins; overrides it sets are joined with ours
    for (k, v) in &r.env {
        match (k.as_str(), env.get(k)) {
            ("WINEDLLOVERRIDES", Some(ours)) => {
                let joined = format!("{ours};{v}");
                env.insert(k.clone(), joined);
            }
            _ => {
                env.insert(k.clone(), v.clone());
            }
        }
    }
    let cwd = r.working_dir.clone().or_else(|| game.exe.as_ref().and_then(|e| e.parent().map(Path::to_path_buf)));
    Ok(Plan { argv, env, cwd, new_prefix, pre_launch: r.pre_launch.clone(), post_exit: r.post_exit.clone() })
}

/// A Steam game's app id and whether it belongs to flatpak Steam.
pub fn steam_app(game: &Game) -> Option<(String, bool)> {
    let s = game.sources.iter().find(|s| s.source == somelauncher_proto::game::SourceKind::Steam)?;
    Some((s.source_id.clone(), s.data["flatpak"].as_bool().unwrap_or(false)))
}

/// The systemd unit a game's scope gets.
pub fn unit_name(id: i64) -> String {
    format!("somelauncher-game-{id}-{}.scope", crate::games::now())
}

/// Runs a pre-launch or post-exit command with the game's env and working dir.
pub async fn run_hook(cmd: &str, plan: &Plan, log: &std::fs::File) -> Result<()> {
    let status = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .envs(&plan.env)
        .current_dir(plan.cwd.as_deref().unwrap_or(Path::new("/")))
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log.try_clone()?)
        .status()
        .await
        .with_context(|| format!("running `{cmd}`"))?;
    if !status.success() {
        bail!("`{cmd}` failed ({status})");
    }
    Ok(())
}

/// Starts the plan in `unit` (a transient `systemd-run --user --scope`, so the
/// whole process tree is tracked and can be stopped), output into `log`.
pub fn spawn(plan: &Plan, unit: &str, description: &str, log: &std::fs::File) -> Result<tokio::process::Child> {
    let (program, args): (String, Vec<String>) = match which("systemd-run") {
        Some(sr) => {
            let mut args = vec![
                "--user".into(),
                "--scope".into(),
                "--collect".into(),
                "--quiet".into(),
                format!("--unit={unit}"),
                format!("--description={description}"),
                "--".into(),
            ];
            args.extend(plan.argv.iter().cloned());
            (path_str(&sr), args)
        }
        None => (plan.argv[0].clone(), plan.argv[1..].to_vec()),
    };
    if let Some(cwd) = &plan.cwd
        && !cwd.is_dir()
    {
        bail!("the working folder {} does not exist", cwd.display());
    }
    let child = tokio::process::Command::new(program)
        .args(args)
        .envs(&plan.env)
        .current_dir(plan.cwd.as_deref().unwrap_or(Path::new("/")))
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log.try_clone()?)
        .spawn()
        .context("starting the game")?;
    Ok(child)
}

#[cfg(test)]
mod tests {
    use super::*;
    use somelauncher_proto::game::Runner;

    fn game(kind: Kind) -> Game {
        Game {
            id: 1,
            name: "How to Fish".into(),
            kind,
            install_dir: None,
            exe: Some("/g/How to Fish/How to Fish.exe".into()),
            prefix: None,
            runner: Runner::default(),
            tags: vec![],
            categories: vec![],
            favorite: false,
            hidden: false,
            playtime: 0,
            last_played: None,
            added: 0,
            sources: vec![],
        }
    }

    fn ctx() -> Context {
        let p = |name: &str| Proton { name: name.into(), internal: name.into(), dir: format!("/c/{name}").into(), source: "user" };
        Context {
            protons: vec![p("GE-Proton10-34"), p("GE-Proton11-7"), p("cachyos-11.0-20260703-slr")],
            default_proton: None,
            umu: Some("/usr/bin/umu-run".into()),
            prefix_root: "/g/Prefixes".into(),
        }
    }

    #[test]
    fn proton_games_run_through_umu_with_their_settings() {
        let mut g = game(Kind::Proton);
        g.runner.args = vec!["-windowed".into()];
        g.runner.dll_overrides.insert("winmm".into(), "n,b".into());
        g.runner.env.insert("WINEDLLOVERRIDES".into(), "dxgi=n".into());
        g.runner.env.insert("DXVK_HUD".into(), "fps".into());
        let p = plan(&g, &ctx()).unwrap();
        assert_eq!(p.argv, ["/usr/bin/umu-run", "/g/How to Fish/How to Fish.exe", "-windowed"]);
        assert_eq!(p.env["PROTONPATH"], "/c/GE-Proton11-7", "the newest GE-Proton by default");
        assert_eq!(p.env["WINEPREFIX"], "/g/Prefixes/How to Fish");
        assert_eq!(p.env["GAMEID"], "umu-0");
        assert_eq!(p.env["WINEDLLOVERRIDES"], "winmm=n,b;dxgi=n");
        assert_eq!(p.env["DXVK_HUD"], "fps");
        assert_eq!(p.cwd.as_deref(), Some(Path::new("/g/How to Fish")));
        assert_eq!(p.new_prefix.as_deref(), Some(Path::new("/g/Prefixes/How to Fish")));

        g.prefix = Some("/p/fish".into());
        g.runner.proton = Some("cachyos-11.0-20260703-slr".into());
        g.runner.working_dir = Some("/w".into());
        let p = plan(&g, &ctx()).unwrap();
        assert_eq!((p.env["PROTONPATH"].as_str(), p.env["WINEPREFIX"].as_str(), p.new_prefix), ("/c/cachyos-11.0-20260703-slr", "/p/fish", None));
        assert_eq!(p.cwd.as_deref(), Some(Path::new("/w")));

        g.runner.proton = Some("GE-Proton9-1".into());
        assert!(plan(&g, &ctx()).unwrap_err().to_string().contains("not installed"));
    }

    #[test]
    fn needs_steam_keeps_lsteamclient_and_adds_the_loader_overrides() {
        let mut g = game(Kind::Proton);
        g.runner.needs_steam = true;
        g.runner.dll_overrides.insert("winmm".into(), "n".into());
        let p = plan(&g, &ctx()).unwrap();
        assert_eq!(p.env["UMU_USE_STEAM"], "1");
        assert_eq!(p.env["WINEDLLOVERRIDES"], "winmm=n;OnlineFix64=n,b;SteamOverlay64=n,b;StubDRM64=n,b;steam_api64=n,b");
    }

    #[test]
    fn steam_games_run_through_steam() {
        let mut g = game(Kind::Steam);
        assert!(plan(&g, &ctx()).is_err(), "no app id");
        g.sources.push(somelauncher_proto::game::Source {
            source: somelauncher_proto::game::SourceKind::Steam,
            source_id: "3241660".into(),
            data: serde_json::json!({ "flatpak": false }),
        });
        g.runner.args = vec!["-nolog".into()];
        let p = plan(&g, &ctx()).unwrap();
        assert_eq!(p.argv, ["steam", "-applaunch", "3241660", "-nolog"]);
        assert!(p.env.is_empty() && p.cwd.is_none());
    }

    #[test]
    fn native_games_run_directly() {
        let mut g = game(Kind::Native);
        g.exe = Some("/g/Valheim/valheim.x86_64".into());
        g.runner.args = vec!["-console".into()];
        let p = plan(&g, &ctx()).unwrap();
        assert_eq!(p.argv, ["/g/Valheim/valheim.x86_64", "-console"]);
        assert!(p.env.is_empty());
        g.exe = None;
        assert!(plan(&g, &ctx()).is_err());
    }

    #[test]
    fn prefix_root_and_folder_names() {
        assert_eq!(prefix_root(Some("/x"), &["/g".into()]), PathBuf::from("/x"));
        assert_eq!(prefix_root(None, &["/g".into()]), PathBuf::from("/g/Prefixes"));
        assert_eq!(folder_name("AC/DC .exe"), "AC_DC .exe");
        assert_eq!(folder_name("..."), "game");
    }
}
