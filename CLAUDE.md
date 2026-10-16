# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

# SomeLauncher

An all-in-one game launcher for Hyprland (Arch/CachyOS) built to sit beside SomeShell (`/mnt/ssd_backup/Projects/SomeShell`, its own CLAUDE.md describes it). Aim: clean and simple, one click to play once a game is set up, but every runner option still configurable.
The original idea is `SomeLauncher.md`. Decisions, architecture and the task tracker are in `docs/plan.md`; read it before starting work. No code exists yet: Phase 0 sets up the layout, and adds build/run/test commands here when it lands.

## Architecture (planned, see docs/plan.md)
- `ui/`: Quickshell QML app (`qs -c somelauncher`), a normal window. Glass, Theme and motion follow SomeShell's components (copied, not imported, so the launcher runs without SomeShell). Uses SomeShell's matugen colors when they exist.
- `core/`: Rust workspace. `somelauncherd` owns the sqlite library, importers (Steam, Lutris, Twintail, Prism), runners (umu-run/Proton, `steam -applaunch`, native, Prism, Hoyo), downloads (HoYoPlay API + Sophon), playtime and achievements. The `somelauncher` CLI and the UI talk to it over a unix socket with JSON lines (`docs/protocol.md`). Closing the UI must never stop a game or a download.
- Every game runs in its own `systemd-run --user --scope` so Proton's whole process tree is tracked. Steam games are tracked through Steam's `reaper SteamLaunch AppId=<id>` process.
- Steam-dependent non-Steam games ("Needs Steam") automate the steps in `/mnt/ssd_backup/Gaming/Games/LUTRIS_SETUP.md`: `UMU_USE_STEAM=1`, real copies (not symlinks, which fail inside pressure-vessel) of the Steam DLLs in the prefix, `steam_appid.txt`, and `n,b` DLL overrides.

## SomeShell integration
Optional, detected at runtime: matugen colors and glass settings (`~/.config/someshell/`), game mode (gamemoderun → SomeShell's `GameClients` turns it on), island states and an overlay card over SomeShell IPC (`qs -c someshell ipc call ...`). SomeShell's QML, blur, animation and font gotchas in its CLAUDE.md apply here too (compositor blur via `BackgroundEffect`, `SpringAnimation` only in a `Behavior`, no `cache: false` on network `AnimatedImage`, never animate a font axis).

## Rules
- Don't hardcode this machine (paths, monitor, GPU, user names); the repo goes public after Phase 9. Laptop features (battery bypass, power profiles on battery) are detected, never assumed.
- A launcher is only a launcher: no polling where an event exists, panels and video load only while shown, everything pauses while a game runs.
- Work through `docs/plan.md` one phase at a time. Commit after each verified task with `Phase <n><sub>: <task>`, ticking it in the plan in the same commit (`[x]` done, `[~]` done differently with a note). New mid-phase requests go into the plan first, dated.
- `README.md` (once it exists) holds only features and install; how things work goes here.
