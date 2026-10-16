# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

# SomeLauncher

An all-in-one game launcher for Hyprland (Arch/CachyOS) built to sit beside SomeShell (`/mnt/ssd_backup/Projects/SomeShell`, its own CLAUDE.md describes it). Aim: clean and simple, one click to play once a game is set up, but every runner option still configurable.
The original idea is `SomeLauncher.md`. Decisions, architecture and the task tracker are in `docs/plan.md`; read it before starting work. Remote: `https://github.com/someact/SomeLauncher` (public).

## Commands
```sh
scripts/dev.sh build            # cargo build of core/ (debug)
scripts/dev.sh test             # cargo tests; one test: scripts/dev.sh test -p somelauncherd settings_set
scripts/dev.sh daemon           # somelauncherd in the foreground, debug log on the terminal
scripts/dev.sh ui [page]        # the UI from ui/ against the debug daemon (home online library minecraft settings)
scripts/dev.sh check [secs]     # load the UI and print QML warnings/errors; must print nothing
core/target/debug/somelauncher list | play <game> | stop <game> | status | call <method> '<json>'   # talk to the daemon (started when not running)
```
- The UI starts the daemon itself when nobody answers on the socket (`SOMELAUNCHER_DAEMON`, set by dev.sh, else `somelauncherd` on PATH), and starts it again once per outage. A running daemon from an older build keeps answering: `somelauncher call shutdown` before testing daemon changes through the UI.
- Isolated runs: point `XDG_DATA_HOME`/`XDG_STATE_HOME`/`XDG_CONFIG_HOME` at a temp dir and `SOMELAUNCHER_SOCKET` at a short path (unix socket paths max 107 bytes, the scratchpad is too long).
- The daemon logs to `~/.local/state/somelauncher/daemon.log` (`SOMELAUNCHER_LOG` is an env filter, default `info`).
- UI screenshots for review: `grim -g "$(hyprctl clients -j | jq -r '.[]|select(.title=="SomeLauncher")|"\(.at[0]),\(.at[1]) \(.size[0])x\(.size[1])"')" out.png`.

## Architecture
- `ui/`: Quickshell QML app (`qs -c somelauncher`), a transparent `FloatingWindow` that Hyprland's window blur frosts; `components/Glass.qml` tints it. `core/` singletons: `Theme` (same token names as SomeShell's, so components move between the two), `Motion`/`components/Spring.qml`, `Config` (UI prefs, `~/.config/somelauncher/config.json`), `Shell` (SomeShell's colors, glass settings and game mode, all optional), `Daemon` (socket client: `Daemon.call(method, params, (result, error) => …)`, `onEvent(name, data)`). Components copied from SomeShell name the source commit in their header. `pages/<Name>Page.qml` load one at a time.
- `core/`: Rust workspace: `proto` (wire types + XDG paths shared by all), `daemon` (`main` → `server` (socket, one task per client, broadcast events) → `rpc::State::call` (method dispatch) → `db` (sqlite, migrations by `PRAGMA user_version`)), `cli`. A new method is a match arm in `rpc.rs` plus a row in `docs/protocol.md`; schema changes are a new entry in `db::MIGRATIONS`, never an edit of an old one.
- Daemon settings (game folders, API keys) are in `library.db` via `settings.get/set`; UI looks are in `config.json`. Never mix the two.
- Planned (see docs/plan.md): `somelauncherd` owns the sqlite library, importers (Steam, Lutris, Twintail, Prism), runners (umu-run/Proton, `steam -applaunch`, native, Prism, Hoyo), downloads (HoYoPlay API + Sophon), playtime and achievements. The `somelauncher` CLI and the UI talk to it over a unix socket with JSON lines (`docs/protocol.md`). Closing the UI must never stop a game or a download.
- Every game runs in its own `systemd-run --user --scope` so Proton's whole process tree is tracked. Steam games are tracked through Steam's `reaper SteamLaunch AppId=<id>` process.
- Steam-dependent non-Steam games ("Needs Steam") automate the steps in `/mnt/ssd_backup/Gaming/Games/LUTRIS_SETUP.md`: `UMU_USE_STEAM=1`, real copies (not symlinks, which fail inside pressure-vessel) of the Steam DLLs in the prefix, `steam_appid.txt`, and `n,b` DLL overrides.

## QML gotchas
- Qt 6.12's QML compiler segfaults on optional calls (`f?.()`, `obj[k]?.()`): write `if (f) f()`.

## SomeShell integration
Optional, detected at runtime: matugen colors and glass settings (`~/.config/someshell/`), game mode (gamemoderun → SomeShell's `GameClients` turns it on), island states and an overlay card over SomeShell IPC (`qs -c someshell ipc call ...`). SomeShell's QML, blur, animation and font gotchas in its CLAUDE.md apply here too (compositor blur via `BackgroundEffect`, `SpringAnimation` only in a `Behavior`, no `cache: false` on network `AnimatedImage`, never animate a font axis).

## Rules
- Don't hardcode this machine (paths, monitor, GPU, user names); the repo goes public after Phase 9. Laptop features (battery bypass, power profiles on battery) are detected, never assumed.
- A launcher is only a launcher: no polling where an event exists, panels and video load only while shown, everything pauses while a game runs.
- Work through `docs/plan.md` one phase at a time. Commit after each verified task with `Phase <n><sub>: <task>`, ticking it in the plan in the same commit (`[x]` done, `[~]` done differently with a note). New mid-phase requests go into the plan first, dated.
- Push to `origin main` when a phase is done (the user's rule), not after every task.
- `README.md` (once it exists) holds only features and install; how things work goes here.
