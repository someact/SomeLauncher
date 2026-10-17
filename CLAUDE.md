# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

# SomeLauncher

An all-in-one game launcher for Hyprland (Arch/CachyOS) built to sit beside SomeShell (`/mnt/ssd_backup/Projects/SomeShell`, its own CLAUDE.md describes it). Aim: clean and simple, one click to play once a game is set up, but every runner option still configurable.
The original idea is `SomeLauncher.md`. Decisions, architecture and the task tracker are in `docs/plan.md`; read it before starting work. Remote: `https://github.com/someact/SomeLauncher` (public).

## Commands
```sh
scripts/dev.sh build            # cargo build of core/ (debug)
scripts/dev.sh test             # cargo tests; one test: scripts/dev.sh test -p somelauncherd settings_set (builds the test binary only: `build` before running the daemon)
scripts/dev.sh daemon           # somelauncherd in the foreground, debug log on the terminal
scripts/dev.sh ui [page]        # the UI from ui/ against the debug daemon (home online library minecraft settings); SOMELAUNCHER_GAME=<id> also opens that game's page
scripts/dev.sh check [secs]     # load the UI and print QML warnings/errors; must print nothing
core/target/debug/somelauncher list | play <game> | stop <game> | status | call <method> '<json>'   # talk to the daemon (started when not running)
qs -p ui/shell.qml ipc call launcher open | toggle | hideWindow | page <id> | game <id> | add | play <id> | state   # the running UI (installed: qs -c somelauncher)
```
- The UI starts the daemon itself when nobody answers on the socket (`SOMELAUNCHER_DAEMON`, set by dev.sh, else `somelauncherd` on PATH), and starts it again once per outage. A running daemon from an older build keeps answering: `somelauncher call shutdown` before testing daemon changes through the UI.
- Isolated runs: point `XDG_DATA_HOME`/`XDG_STATE_HOME`/`XDG_CONFIG_HOME` at a temp dir and `SOMELAUNCHER_SOCKET` at a short path (unix socket paths max 107 bytes, the scratchpad is too long).
- Gaming mode (5a/5b) without a laptop: `scripts/fake-upower.py` on a private bus (`dbus-daemon --session --fork --print-address=1`), the daemon started with `DBUS_SYSTEM_BUS_ADDRESS` pointing at it; its header has the steps.
- `pkill -f <pattern>` from a tool shell kills that shell too (its own command line matches): kill by pid.
- The daemon logs to `~/.local/state/somelauncher/daemon.log` (`SOMELAUNCHER_LOG` is an env filter, default `info`).
- UI screenshots for review: `grim -g "$(hyprctl clients -j | jq -r '.[]|select(.title=="SomeLauncher")|"\(.at[0]),\(.at[1]) \(.size[0])x\(.size[1])"')" out.png`.

## Architecture
- `ui/`: Quickshell QML app (`qs -c somelauncher`), a transparent `FloatingWindow` that Hyprland's window blur frosts; `components/Glass.qml` tints it. `core/` singletons: `Theme` (same token names as SomeShell's, so components move between the two), `Motion`/`components/Spring.qml` (`Motion.reduced` also while a game runs: `Motion.paused`), `Config` (UI prefs, `~/.config/somelauncher/config.json`), `Shell` (SomeShell's colors, glass settings and game mode, all optional), `Daemon` (socket client: `Daemon.call(method, params, (result, error) => …)`, `onEvent(name, data)`), `Library` (the games and what runs, kept current from `library.changed`/`game.state`; rows are lists of ids, cards bind to `Library.byId[id]`; `Library.update` applies the daemon's reply at once), `Nav` (the open game page), `Pick` (the desktop's file and folder dialogs). Components copied from SomeShell name the source commit in their header. `pages/<Name>Page.qml` load one at a time; `pages/GamePage.qml` opens over them (`Nav.openGame(id, cardArt)`, it grows out of that Item and shrinks back).
- `core/`: Rust workspace: `proto` (wire types + XDG paths shared by all), `daemon` (`main` → `server` (socket, one task per client, broadcast events) → `rpc::State::call` (method dispatch) → `db` (sqlite, migrations by `PRAGMA user_version`)), `cli`. A new method is a match arm in `rpc.rs` plus a row in `docs/protocol.md`; schema changes are a new entry in `db::MIGRATIONS`, never an edit of an old one.
- Daemon settings (game folders, API keys) are in `library.db` via `settings.get/set`; UI looks are in `config.json`. Never mix the two.
- Planned (see docs/plan.md): `somelauncherd` owns the sqlite library, importers (Steam, Lutris, Twintail, Prism), runners (umu-run/Proton, `steam -applaunch`, native, Prism, Hoyo), downloads (HoYoPlay API + Sophon), playtime and achievements. The `somelauncher` CLI and the UI talk to it over a unix socket with JSON lines (`docs/protocol.md`). Closing the UI must never stop a game or a download.
- Every game runs in its own `systemd-run --user --scope` so Proton's whole process tree is tracked. Steam games are tracked through Steam's `reaper SteamLaunch AppId=<id>` process.
- Steam-dependent non-Steam games ("Needs Steam") automate the steps in `/mnt/ssd_backup/Gaming/Games/LUTRIS_SETUP.md`: `UMU_USE_STEAM=1`, real copies (not symlinks, which fail inside pressure-vessel) of the Steam DLLs in the prefix, `steam_appid.txt`, and `n,b` DLL overrides.

## QML gotchas
- Qt 6.12's QML compiler segfaults on optional calls (`f?.()`, `obj[k]?.()`): write `if (f) f()`.
- A `WheelHandler` swallows every wheel it gets, even with `event.accepted = false`, and an interactive Flickable eats vertical wheels: a sideways row inside a scrolling page is `interactive: false` with `WheelHandler { orientation: Qt.Horizontal }` (`components/GameRow.qml`).
- A `PressButton`'s children go into its inner Item, so `parent.radius`/`parent.text` inside one is not the button: use an id.
- `TextInput` lets Return/Enter go on to its parents (the game page plays on Enter): `components/Field.qml` swallows them; a raw TextInput must too.
- Items declared inside a ListView/Flickable go into its scrolling `contentItem`; overlays (arrows, scrims) go beside it in a wrapper Item.

## SomeShell integration
Optional, detected at runtime: matugen colors and glass settings (`~/.config/someshell/`), game mode (gamemoderun → SomeShell's `GameClients` turns it on), island states and an overlay card over SomeShell IPC (`qs -c someshell ipc call ...`). SomeShell's QML, blur, animation and font gotchas in its CLAUDE.md apply here too (compositor blur via `BackgroundEffect`, `SpringAnimation` only in a `Behavior`, no `cache: false` on network `AnimatedImage`, never animate a font axis).

## Rules
- Don't hardcode this machine (paths, monitor, GPU, user names); the repo goes public after Phase 9. Laptop features (battery bypass, power profiles on battery) are detected, never assumed.
- A launcher is only a launcher: no polling where an event exists, panels and video load only while shown, everything pauses while a game runs.
- Work through `docs/plan.md` one phase at a time. Commit after each verified task with `Phase <n><sub>: <task>`, ticking it in the plan in the same commit (`[x]` done, `[~]` done differently with a note). New mid-phase requests go into the plan first, dated.
- Push to `origin main` when a phase is done (the user's rule), not after every task.
- `README.md` (once it exists) holds only features and install; how things work goes here.
