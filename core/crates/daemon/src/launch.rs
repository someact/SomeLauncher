//! How a game is started. `plan` turns a game and the global settings into a
//! command line, env and working dir (pure, so `games.plan` can show it and
//! tests can check it); `spawn` starts that plan in its own systemd scope.

use crate::proton::{self, Proton};
use anyhow::{Context as _, Result, anyhow, bail};
use serde::Serialize;
use somelauncher_proto::game::{Defaults, Game, Kind};
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
    pub defaults: Defaults,
    /// PRIME offload on a hybrid-GPU laptop
    pub offload: Option<crate::gpu::Offload>,
    /// Wrapper programs found on PATH (`gamescope`, `mangohud`, `gamemoderun`)
    pub tools: std::collections::BTreeSet<String>,
    /// The monitor the game starts on: gamescope's sizes and the FPS cap follow it
    pub monitor: Option<crate::monitor::Monitor>,
    /// Where XXMI lives (hoyo/xxmi.rs), for Hoyo games with mods on
    pub xxmi: PathBuf,
    pub home: PathBuf,
    /// The Prism account Minecraft instances play with (`minecraftAccount`); Prism's active one when None
    pub minecraft_account: Option<String>,
}

/// The wrapper programs `tools` is filled with.
pub const WRAPPERS: &[&str] = &["gamescope", "mangohud", "gamemoderun"];

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub argv: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: Option<PathBuf>,
    /// A prefix chosen for a game that had none; the game keeps it from now on
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_prefix: Option<PathBuf>,
    /// A Linux program without the exec bit, given it before the start
    #[serde(skip_serializing_if = "Option::is_none")]
    pub make_executable: Option<PathBuf>,
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
    let mut make_executable = None;
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
            // Genshin and ZZZ: Twintail's setup for them (hoyo/runner.rs)
            if let Some(biz) = crate::hoyo::biz_of(game) {
                crate::hoyo::runner::check_proton(&biz, &game.name, &proton.name)?;
                env.extend(crate::hoyo::runner::env(&biz).into_iter().map(|(k, v)| (k.to_string(), v.to_string())));
            }
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
            let (argv, chmod) = native_argv(exe)?;
            make_executable = chmod.then(|| exe.clone());
            argv
        }
        Kind::Steam => {
            let (appid, flatpak) = steam_app(game).ok_or_else(|| anyhow!("{} has no Steam app id", game.name))?;
            let mut argv: Vec<String> = if flatpak { vec!["flatpak".into(), "run".into(), "com.valvesoftware.Steam".into()] } else { vec!["steam".into()] };
            argv.extend(["-applaunch".into(), appid]);
            argv.extend(r.args.iter().cloned());
            // Steam starts the game itself, with its own Proton and launch options
            return Ok(Plan {
                argv,
                env,
                cwd: None,
                new_prefix: None,
                make_executable: None,
                pre_launch: r.pre_launch.clone(),
                post_exit: r.post_exit.clone(),
            });
        }
        // Prism starts the game with its own Java and settings
        Kind::Minecraft => return crate::minecraft::plan(game, &ctx.home, ctx.minecraft_account.as_deref()),
    };
    if let Some(biz) = crate::hoyo::biz_of(game) {
        argv.extend(crate::hoyo::runner::args(&biz, &r.args, r.extra.get("graphicsApi").and_then(|v| v.as_str())));
    }
    argv.extend(r.args.iter().cloned());
    // XXMI: its loader beside the game, inside the same wrappers
    if game.kind == Kind::Hoyo && r.extra.get("xxmi").and_then(|v| v.as_bool()) == Some(true) {
        let biz = crate::hoyo::biz_of(game).unwrap_or_default();
        let package = crate::hoyo::xxmi::importer_of(&biz).ok_or_else(|| anyhow!("{} has no XXMI package", game.name))?;
        if !crate::hoyo::xxmi::ready(&ctx.xxmi, package) {
            bail!("XXMI is not installed yet: update it in the game's settings (Mods), or turn mods off");
        }
        let umu = argv[0].clone();
        argv = crate::hoyo::xxmi::wrap(&ctx.xxmi, package, &umu, argv, &mut env);
    }
    let argv = wrap(game, ctx, argv, &mut env)?;
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
    Ok(Plan { argv, env, cwd, new_prefix, make_executable, pre_launch: r.pre_launch.clone(), post_exit: r.post_exit.clone() })
}

/// Wraps the command in the game's wrappers (or the defaults):
/// `gamemoderun gamescope <opts> [--mangoapp] -- [mangohud] <command>`, and adds PRIME offload env.
/// gamescope's empty sizes and refresh are the monitor's. An FPS cap goes to
/// gamescope (`--framerate-limit`), else MangoHud (`fps_limit`, its overlay
/// hidden unless it is on), else DXVK/VKD3D for wine games.
/// A wrapper turned on for this game but missing is an error; one on by default but missing is skipped.
fn wrap(game: &Game, ctx: &Context, command: Vec<String>, env: &mut BTreeMap<String, String>) -> Result<Vec<String>> {
    let (r, d) = (&game.runner, &ctx.defaults);
    let available = |tool: &str, own: bool| -> Result<bool> {
        if ctx.tools.contains(tool) {
            return Ok(true);
        }
        if own {
            bail!("{tool} is not installed");
        }
        tracing::warn!("{tool} is on by default but not installed; skipped");
        Ok(false)
    };
    let mangohud = r.mangohud.unwrap_or(d.mangohud) && available("mangohud", r.mangohud == Some(true))?;
    let gamemode = r.gamemode.unwrap_or(d.gamemode) && available("gamemoderun", r.gamemode == Some(true))?;
    let gs = r.gamescope.as_ref().unwrap_or(&d.gamescope);
    let gamescope = gs.enabled && available("gamescope", r.gamescope.as_ref().is_some_and(|g| g.enabled))?;
    let monitor = ctx.monitor.as_ref();
    let cap = r.fps_cap.or(d.fps_cap).and_then(|c| c.fps(monitor.map(|m| m.refresh)));
    // Without gamescope, MangoHud can still cap (hidden), when it is installed
    let mangohud_cap = cap.is_some() && !gamescope && !mangohud && ctx.tools.contains("mangohud");
    if let Some(offload) = ctx.offload
        && r.prime.or(d.prime).unwrap_or(true)
    {
        env.extend(offload.env().iter().map(|(k, v)| (k.to_string(), v.to_string())));
    }

    let mut argv = Vec::new();
    if gamemode {
        argv.push("gamemoderun".to_string());
    }
    if gamescope {
        argv.push("gamescope".into());
        let mut opt = |flag: &str, v: Option<u32>| {
            if let Some(v) = v {
                argv.extend([flag.to_string(), v.to_string()]);
            }
        };
        opt("-W", gs.width.or(monitor.map(|m| m.width)));
        opt("-H", gs.height.or(monitor.map(|m| m.height)));
        opt("-w", gs.game_width);
        opt("-h", gs.game_height);
        opt("-r", gs.refresh.or(monitor.map(|m| m.refresh)));
        opt("--framerate-limit", cap);
        if gs.fsr {
            argv.extend(["-F".into(), "fsr".into()]);
        }
        if gs.hdr {
            argv.push("--hdr-enabled".into());
        }
        if gs.fullscreen {
            argv.push("-f".into());
        }
        argv.extend(gs.args.iter().cloned());
        // Inside gamescope its own overlay replaces the mangohud wrapper
        if mangohud {
            argv.push("--mangoapp".into());
        }
        argv.push("--".into());
    } else if mangohud || mangohud_cap {
        argv.push("mangohud".into());
        if let Some(fps) = cap {
            let config = if mangohud { format!("fps_limit={fps}") } else { format!("no_display,fps_limit={fps}") };
            env.insert("MANGOHUD_CONFIG".into(), config);
        }
    } else if let Some(fps) = cap
        && matches!(game.kind, Kind::Proton | Kind::Hoyo)
    {
        env.insert("DXVK_FRAME_RATE".into(), fps.to_string());
        env.insert("VKD3D_FRAME_RATE".into(), fps.to_string());
    } else if cap.is_some() {
        tracing::warn!("{}: no way to cap the frame rate (install MangoHud or turn gamescope on)", game.name);
    }
    argv.extend(command);
    Ok(argv)
}

/// How a Linux program starts: directly when it can; a script without the exec
/// bit through its interpreter; a binary without it directly once it has the
/// bit (the bool asks for that: games unpacked from archives or NTFS lose it).
fn native_argv(exe: &Path) -> Result<(Vec<String>, bool)> {
    use std::io::Read;
    let mut head = Vec::with_capacity(256);
    std::fs::File::open(exe)
        .and_then(|f| f.take(256).read_to_end(&mut head))
        .with_context(|| format!("{} cannot be read", exe.display()))?;
    if head.starts_with(b"MZ") {
        bail!("{} is a Windows program: set its runner to Proton", exe.display());
    }
    let executable = std::fs::metadata(exe).is_ok_and(|m| std::os::unix::fs::PermissionsExt::mode(&m.permissions()) & 0o111 != 0);
    if executable {
        return Ok((vec![path_str(exe)], false));
    }
    if let Some(line) = head.strip_prefix(b"#!") {
        let line = String::from_utf8_lossy(line.split(|&b| b == b'\n').next().unwrap_or_default()).into_owned();
        let mut argv: Vec<String> = line.split_whitespace().map(String::from).collect();
        if !argv.is_empty() {
            argv.push(path_str(exe));
            return Ok((argv, false));
        }
    }
    if exe.extension().is_some_and(|e| e == "sh") {
        return Ok((vec!["sh".into(), path_str(exe)], false));
    }
    Ok((vec![path_str(exe)], true))
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
                // The command line as it is: systemd would read `$X` (and `$$`) in it as its own
                "--expand-environment=no".into(),
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

    fn game(kind: Kind) -> Game {
        let mut n = somelauncher_proto::game::NewGame::new("How to Fish", kind);
        n.exe = Some("/g/How to Fish/How to Fish.exe".into());
        Game::new(1, n)
    }

    fn ctx() -> Context {
        let p = |name: &str| Proton { name: name.into(), internal: name.into(), dir: format!("/c/{name}").into(), source: "user" };
        Context {
            protons: vec![p("GE-Proton10-34"), p("GE-Proton11-7"), p("cachyos-11.0-20260703-slr")],
            default_proton: None,
            umu: Some("/usr/bin/umu-run".into()),
            prefix_root: "/g/Prefixes".into(),
            // Without the default gamemoderun, so the tests see their own wrappers only
            defaults: Defaults { gamemode: false, ..Defaults::default() },
            offload: None,
            tools: WRAPPERS.iter().map(|t| t.to_string()).collect(),
            monitor: Some(crate::monitor::Monitor { name: "DP-1".into(), width: 2560, height: 1440, refresh: 144 }),
            xxmi: "/nonexistent/xxmi".into(),
            home: "/nonexistent/home".into(),
            minecraft_account: None,
        }
    }

    #[test]
    fn xxmi_runs_its_loader_inside_the_wrappers() {
        let mut g = game(Kind::Hoyo);
        g.name = "Genshin Impact".into();
        g.exe = Some("/g/Genshin Impact/GenshinImpact.exe".into());
        g.runner.proton = Some("GE-Proton11-7".into());
        g.runner.mangohud = Some(true);
        g.runner.extra.insert("xxmi".into(), serde_json::json!(true));
        assert!(plan(&g, &ctx()).unwrap_err().to_string().contains("XXMI is not installed"));

        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        std::fs::create_dir_all(root.join("gimi")).unwrap();
        for f in ["3dmloader.exe", "gimi/d3dx.ini", "gimi/d3d11.dll", "gimi/d3dcompiler_47.dll"] {
            std::fs::write(root.join(f), b"").unwrap();
        }
        let c = Context { xxmi: root.into(), ..ctx() };
        let p = plan(&g, &c).unwrap();
        assert_eq!(&p.argv[..3], ["mangohud", "sh", "-c"]);
        assert_eq!(&p.argv[5..], ["/usr/bin/umu-run", "/g/Genshin Impact/GenshinImpact.exe"]);
        assert_eq!(p.env["SOMELAUNCHER_XXMI_PACKAGE"], "gimi");
        g.runner.extra.insert("xxmi".into(), serde_json::json!(false));
        assert_eq!(plan(&g, &c).unwrap().argv[1], "/usr/bin/umu-run");
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
    fn wrappers_come_from_the_game_or_the_defaults() {
        use somelauncher_proto::game::Gamescope;
        let mut g = game(Kind::Proton);
        g.prefix = Some("/p".into());
        let mut c = ctx();
        c.defaults.gamemode = true;
        c.defaults.mangohud = true;
        let p = plan(&g, &c).unwrap();
        assert_eq!(p.argv[..3], ["gamemoderun", "mangohud", "/usr/bin/umu-run"]);

        g.runner.mangohud = Some(false);
        g.runner.gamescope = Some(Gamescope {
            enabled: true,
            width: Some(2560),
            height: Some(1440),
            game_width: Some(1920),
            game_height: Some(1080),
            refresh: Some(165),
            fsr: true,
            hdr: true,
            fullscreen: true,
            args: vec!["--adaptive-sync".into()],
        });
        let p = plan(&g, &c).unwrap();
        let expected = "gamemoderun gamescope -W 2560 -H 1440 -w 1920 -h 1080 -r 165 -F fsr --hdr-enabled -f --adaptive-sync -- /usr/bin/umu-run";
        assert_eq!(p.argv[..p.argv.len() - 1].join(" "), expected);
        g.runner.mangohud = None;
        assert!(plan(&g, &c).unwrap().argv.contains(&"--mangoapp".to_string()));

        // Missing tools: an error when the game asks, skipped when only the default does
        c.tools.clear();
        assert!(plan(&g, &c).unwrap_err().to_string().contains("gamescope is not installed"));
        g.runner.gamescope = None;
        assert_eq!(plan(&g, &c).unwrap().argv[0], "/usr/bin/umu-run");
    }

    #[test]
    fn prime_offload_only_on_hybrid_machines() {
        let mut g = game(Kind::Native);
        g.exe = Some("/bin/sh".into());
        let mut c = ctx();
        assert!(plan(&g, &c).unwrap().env.is_empty());
        c.offload = Some(crate::gpu::Offload::Nvidia);
        assert_eq!(plan(&g, &c).unwrap().env["__NV_PRIME_RENDER_OFFLOAD"], "1");
        g.runner.prime = Some(false);
        assert!(plan(&g, &c).unwrap().env.is_empty());
        g.runner.env.insert("__GLX_VENDOR_LIBRARY_NAME".into(), "mesa".into());
        g.runner.prime = None;
        assert_eq!(plan(&g, &c).unwrap().env["__GLX_VENDOR_LIBRARY_NAME"], "mesa", "the game's env wins");
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
    fn native_games_run_directly_or_through_their_interpreter() {
        use std::os::unix::fs::PermissionsExt;
        let t = tempfile::tempdir().unwrap();
        let file = |name: &str, body: &[u8], mode: u32| {
            let p = t.path().join(name);
            std::fs::write(&p, body).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
            p
        };
        let mut g = game(Kind::Native);
        let run = |g: &mut Game, exe: &Path| {
            g.exe = Some(exe.into());
            plan(g, &ctx())
        };

        let elf = file("valheim.x86_64", b"\x7fELF....", 0o755);
        g.runner.args = vec!["-console".into()];
        let p = run(&mut g, &elf).unwrap();
        assert_eq!(p.argv, [elf.to_str().unwrap(), "-console"]);
        assert!(p.env.is_empty() && p.make_executable.is_none());
        g.runner.args.clear();

        let bare = file("game.bin", b"\x7fELF....", 0o644);
        assert_eq!(run(&mut g, &bare).unwrap().make_executable, Some(bare.clone()));
        let shebang = file("start", b"#!/usr/bin/env bash -e\necho", 0o644);
        assert_eq!(run(&mut g, &shebang).unwrap().argv, ["/usr/bin/env", "bash", "-e", shebang.to_str().unwrap()]);
        let sh = file("run.sh", b"echo hi", 0o644);
        assert_eq!(run(&mut g, &sh).unwrap().argv, ["sh", sh.to_str().unwrap()]);
        let win = file("game.exe", b"MZ\x90\x00", 0o755);
        assert!(run(&mut g, &win).unwrap_err().to_string().contains("Proton"));
        assert!(run(&mut g, &t.path().join("missing")).is_err());
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

    #[test]
    fn gamescope_takes_the_monitors_size_and_refresh_when_empty() {
        let mut g = game(Kind::Proton);
        g.runner.gamescope = Some(somelauncher_proto::game::Gamescope { enabled: true, height: Some(1080), ..Default::default() });
        let argv = plan(&g, &ctx()).unwrap().argv;
        let opts = &argv[..argv.iter().position(|a| a == "--").unwrap()];
        assert_eq!(opts, ["gamescope", "-W", "2560", "-H", "1080", "-r", "144"]);
    }

    #[test]
    fn an_fps_cap_goes_to_gamescope_else_mangohud_else_dxvk() {
        use somelauncher_proto::game::{FpsCap, Gamescope, MonitorWord};
        let mut g = game(Kind::Proton);
        g.runner.fps_cap = Some(FpsCap::Monitor(MonitorWord::Monitor));
        g.runner.gamescope = Some(Gamescope { enabled: true, ..Default::default() });
        assert!(plan(&g, &ctx()).unwrap().argv.windows(2).any(|w| w == ["--framerate-limit", "144"]));

        g.runner.gamescope = None;
        let p = plan(&g, &ctx()).unwrap();
        assert_eq!(p.argv[0], "mangohud");
        assert_eq!(p.env["MANGOHUD_CONFIG"], "no_display,fps_limit=144");
        g.runner.mangohud = Some(true);
        assert_eq!(plan(&g, &ctx()).unwrap().env["MANGOHUD_CONFIG"], "fps_limit=144");

        g.runner.mangohud = None;
        g.runner.fps_cap = Some(FpsCap::Fps(60));
        let mut c = ctx();
        c.tools.remove("mangohud");
        let p = plan(&g, &c).unwrap();
        assert_eq!((p.env["DXVK_FRAME_RATE"].as_str(), p.env["VKD3D_FRAME_RATE"].as_str()), ("60", "60"));

        // A game can turn a default cap off
        c.defaults.fps_cap = Some(FpsCap::Fps(60));
        g.runner.fps_cap = Some(FpsCap::Off(false));
        assert!(!plan(&g, &c).unwrap().env.contains_key("DXVK_FRAME_RATE"));
    }

    #[test]
    fn fps_caps_read_as_false_monitor_or_a_number() {
        use somelauncher_proto::game::FpsCap;
        let read = |s: &str| serde_json::from_str::<FpsCap>(s).unwrap().fps(Some(100));
        assert_eq!((read("false"), read("\"monitor\""), read("75")), (None, Some(100), Some(75)));
    }

    #[test]
    fn gamemoderun_is_on_by_default() {
        let mut c = ctx();
        c.defaults = serde_json::from_str("{}").unwrap();
        assert_eq!(plan(&game(Kind::Proton), &c).unwrap().argv[0], "gamemoderun");
        c.tools.remove("gamemoderun");
        assert_ne!(plan(&game(Kind::Proton), &c).unwrap().argv[0], "gamemoderun");
    }
}
