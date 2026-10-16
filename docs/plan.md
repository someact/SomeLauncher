# SomeLauncher: plan

The task tracker. `[x]` done, `[~]` done differently (with a note), `[ ]` open. Each task is one commit, `Phase <n><sub>: <task>`.
The original idea is `SomeLauncher.md`; the decisions below came from the planning interview on 2026-10-16.

## Goals

- One launcher for every game on this PC: Hoyo gacha games, Steam games, non-Steam Windows games, native games, Minecraft.
- Clean and simple: once a game is set up, one click plays it. Setup may take a few steps, but play never does.
- Still customizable: Proton version, launch arguments, env, DLL overrides, gamescope, MangoHud and gamemode per game.
- Looks like SomeShell: glassmorphism with real compositor blur, matugen colors, spring motion.
- A launcher is only a launcher: it uses almost nothing while a game runs (video and animations pause, the UI can close).
- Works on any Arch + Hyprland PC, desktop or laptop. Hardware is detected at runtime, never assumed. Goes public later.

## Decisions

| Topic | Decision |
| :--- | :--- |
| Stack | Quickshell QML UI (`qs -c somelauncher`, a normal window) + Rust core |
| UI ↔ core | The Rust daemon `somelauncherd` owns the library, launches, downloads and playtime; the UI talks to it over a unix socket with JSON lines. Closing the UI never stops a game or a download |
| SomeShell | Standalone, but uses SomeShell when it is there: its colors and glass settings, game mode, island states, an overlay card and IPC |
| Hoyo (Genshin, ZZZ) | Native: HoYoPlay API + Sophon download, update, preload and repair. Checks for updates at launcher start and again before Play |
| XXMI | Managed: XXMI Libs + GIMI/ZZMI from GitHub releases, a per-game toggle, a mod folder list. No GameBanana in v1 |
| Steam games | Imported from the Steam library and run with `steam -applaunch <id>` (Steam keeps Proton, cloud saves, invites) |
| Steam-dependent non-Steam games | umu-run + Proton with a "Needs Steam" option that automates `LUTRIS_SETUP.md` (see 2c) |
| Minecraft | Prism is the backend now (instances, login, loaders), our UI on top. A native launcher comes near the end (Phase 10) |
| Achievements | Steam Web API (your key + SteamID) and gbe_fork/Goldberg `achievements.json`. No Hoyo achievements |
| Artwork and metadata | SteamGridDB (hero, grid, logo, icon) + Steam store (description, genres, tags) + HoYoPlay for Hoyo. The user confirms each match |
| Import | Steam library, Lutris games, Twintail installs, Prism instances |
| Laptop and PC | Power profile, battery bypass while gaming, per-machine launch defaults, gamemoderun |
| Build check | Steam games: the installed `buildid` against the latest public build (SteamDB has no API, so a PICS source such as `api.steamcmd.net`). Other games: the build the user records |
| Repo | `github.com/someact/SomeLauncher` (public). Commit and tick this plan after each task, push when a phase is done |

## Architecture

```
qs -c somelauncher  (QML, glass UI, QtMultimedia for animated Hoyo art)
      │  unix socket, JSON lines  ($XDG_RUNTIME_DIR/somelauncher.sock)
      ▼
somelauncherd  (Rust, systemd --user service or started on demand by the UI)
  ├ library     sqlite in ~/.local/share/somelauncher/library.db
  ├ importers   steam · lutris · twintail · prism
  ├ runners     umu/proton · steam · native · prism · hoyo
  ├ session     systemd-run --user --scope per game → process tree, playtime, exit
  ├ profiles    power profile, battery bypass, monitor defaults, PRIME
  ├ metadata    SteamGridDB · Steam store · HoYoPlay  (cache in ~/.cache/somelauncher)
  ├ hoyo        HoYoPlay API, Sophon chunks, diff update, repair, XXMI
  └ achievements  Steam Web API · gbe_fork
somelauncher  (Rust CLI on the same socket: `somelauncher play <id>`, desktop entries, the island)
```

- Config: `~/.config/somelauncher/config.json` (UI and global defaults). Per-game settings live in the db and can be exported as TOML.
- Each game runs in its own `systemd-run --user --scope`, so Proton's whole process tree is tracked and playtime ends when the scope is empty. Steam games are tracked through Steam's `reaper SteamLaunch AppId=<id>` process.
- Without the daemon, the CLI starts it (as the UI does), so `somelauncher play` and desktop entries always work.

## Phase 0: Foundations ✅
Dev note (2026-10-16): unix socket paths are limited to 107 bytes, so tests point `SOMELAUNCHER_SOCKET` at a short path.
- [x] 0a. `git init`, `.gitignore`, layout: `core/` (Rust workspace: `somelauncherd`, `somelauncher` CLI, shared `proto` crate), `ui/` (Quickshell config), `docs/`, `install.sh`
- [x] 0b. Protocol: request/response + events (`library.changed`, `game.state`, `download.progress`), versioned, documented in `docs/protocol.md`
- [x] 0c. Daemon skeleton: socket, sqlite + migrations, config, logging to `~/.local/state/somelauncher/`
- [x] 0d. UI skeleton: window, left nav (logo slot left empty, Home, Online, Library, Minecraft, Settings), socket client singleton, reconnect
- [x] 0e. Design system: Theme (SomeShell's matugen colors when present, else our own matugen template or defaults), Glass, springs, buttons and cards. Copy the needed SomeShell components rather than importing them; note the source commit — no own matugen template yet: without SomeShell the fallback palette is used until 9a installs one
- [x] 0f. Dev commands documented in CLAUDE.md (build, run the daemon in the foreground, run the UI from the repo, tests)

## Phase 1: Library ✅
- [x] 1a. Game model: id, kind (`steam`, `proton`, `native`, `hoyo`, `minecraft`), paths, runner settings, tags, categories, favorite, hidden, playtime, last played
- [x] 1b. Steam import: `libraryfolders.vdf` + `appmanifest_*.acf` (installed), owned games through the Web API when a key is set; Steam's own Proton choice is shown read-only
- [x] 1c. Lutris import: `games/*.yml` + `pga.db` → exe, prefix, wine version, env, DLL overrides, args
- [x] 1d. Twintail import: find Genshin/ZZZ install folders, prefixes and runner versions, so nothing is downloaded again
- [x] 1e. Prism import: instances dir, `instance.cfg`, `mmc-pack.json`, `instgroups.json`
- [~] 1f. Manual add: pick an exe or folder, guess the name from the folder, then confirm art (3a) and runner; a library scan for new folders under chosen game dirs — daemon side: `games.guess` (name, folder, main exe, kind) + `games.add`, and `library.scan`; the wizard itself is 4e and art confirmation 3a
- [~] 1g. Duplicates (the same game from two sources) are merged, keeping one entry — automatic when it is the same install (same exe, or same folder and kind); the same name from different installs (e.g. a Steam copy and a Lutris copy) is two real copies, so `games.duplicates` lists them and `games.merge` merges on the user's say

## Phase 2: Runners and launch ✅
- [x] 2a. Proton discovery: `compatibilitytools.d` (user and system), Steam's Proton, umu's `UMU-Proton`; the default is the newest GE-Proton; download GE-Proton/cachyos from GitHub releases in settings
- [x] 2b. umu-run runner: prefix per game (default `<games>/Prefixes/<name>`), `GAMEID`/`UMU_ID`, env, args, DLL overrides, working dir, pre/post scripts
- [x] 2c. "Needs Steam" option (OnlineFix/gbe_fork co-op): start Steam and wait for login, `UMU_USE_STEAM=1`, copy (not link) `steamclient(64).dll`, `GameOverlayRenderer64.dll`, `Steam.dll`, `steam.exe` into the prefix, write `steam_appid.txt`, preset overrides (`winmm`, `OnlineFix64`, `SteamOverlay64`, `StubDRM64`, `steam_api64` = `n,b`). Test game: How to Fish (`<games>/How to Fish`, OnlineFix)
- [x] 2d. Steam runner: `steam -applaunch <id> <args>`, detect start and exit through the reaper process
- [x] 2e. Native runner (Linux executables, scripts)
- [x] 2f. Wrappers per game with global defaults: gamescope (size, refresh, FSR, HDR), MangoHud, gamemoderun, PRIME offload on hybrid GPUs
- [x] 2g. Session: systemd scope, playtime saved even after a crash, a stop button (kills the scope), last 3 logs per game
- [~] 2h. CLI `somelauncher play|list|stop` and `.desktop` entries per game (optional) — the CLI's no-daemon fallback starts the daemon (as the UI does) instead of launching games itself, so launching has one code path

## Phase 3: Metadata and artwork ✅
- [x] 3a. Match by name (SteamGridDB search + Steam store search), show the top results with covers, and the user confirms or picks another (Lutris-style)
- [x] 3b. Art set per game: hero, grid (portrait), logo, icon; the user can replace any image with a local file
- [x] 3c. Auto tags and categories from Steam genres and tags (+ our own: `co-op`, `needs steam`, `gacha`, `modded`)
- [x] 3d. Image cache sized for the screen (as SomeShell does with wallpapers) so Qt never holds a 4K decode
- [x] 3e. Build check: Steam `buildid` against the latest public one; a badge when an update is out

## Phase 4: Core UI
- [~] 4a. Home: "Continue playing" (last played), favorites, the Hoyo games' update state; games not played for 30 days drop off Home (configurable) but stay in Library — a banner for the last played game (a click on a card shows that one, a double click plays), `home.recentDays` in config.json; favorites never drop off; the Hoyo cards show the installed version and `build.updateAvailable`, which the Hoyo update check (6e) fills
- [x] 4b. Library (Netflix layout): a hero row for recent/favorite, a row per category, a Steam badge on Steam and Needs-Steam games, search and filter — games without a category get a row per kind, uninstalled ones a last row; search (name, tag, category; typing anywhere starts it) and the filter chips show a grid. A click picks a card until 4c opens the game page
- [ ] 4c. Game page (PS5 layout): full hero art, logo, a big Play button, a settings button, playtime, last played, achievement progress, build/update badge; the shared element moves from card to page
- [ ] 4d. Game settings sheet: Simple (runner, Proton, Needs Steam, gamescope, MangoHud) and Advanced (env, args, DLL overrides, prefix, scripts, logs). Nothing more than one click deep for the common options
- [ ] 4e. Add game wizard: 3 steps (pick file → confirm name and art → runner); import dialog on the first run
- [ ] 4f. Settings: game folders, API keys (SteamGridDB, Steam), defaults, profiles, appearance
- [ ] 4g. Motion pass: springs, page transitions, card hover, reduced motion option; everything pauses while a game runs

## Phase 5: Profiles and SomeShell
- [ ] 5a. Power profile: `powerprofilesctl` to performance on launch (balanced on battery), restore the previous one on exit
- [ ] 5b. Battery bypass while gaming on AC (UPower `EnableChargeThreshold`, as SomeShell's `services/Battery.qml`), restored on exit; laptop only
- [ ] 5c. Per-machine defaults: the gamescope size/refresh and FPS cap come from the monitor the window is on (`hyprctl monitors -j`)
- [ ] 5d. gamemoderun by default (SomeShell's `GameClients` then turns its game mode on); the launcher pauses video and animations, optionally closes or minimizes its window
- [ ] 5e. SomeShell integration when present: island state for download progress and "Playing X" (IPC on the shell), an overlay card (recent games, playtime), `qs -c somelauncher ipc call ...` targets

## Phase 6: Hoyo games (Genshin Impact, Zenless Zone Zero)
- [ ] 6a. HoYoPlay API client: games, branches (+ preload), background art (image and video), news and events, HoYoLAB links; global server first, CN later
- [ ] 6b. Adopt existing installs (from 1d, or pick a folder): read the version from the game files
- [ ] 6c. Sophon install: manifest, chunk download with resume, verify, extract, voice packs chosen by the user
- [ ] 6d. Update: diff update when available, full otherwise; preload; repair (verify all files)
- [ ] 6e. Check for updates at launcher start and before Play, and block Play on a version mismatch with an "Update" button
- [ ] 6f. Launch: runner and wine/Proton settings Twintail uses for each game, plus anything the game needs on Linux (taken from Twintail's and the anime-games launchers' current setup when this phase starts)
- [ ] 6g. Online page: full-screen art (animated when the API has video), logo, version, Play and settings, playtime, events and news, HoYoLAB link, a right dock to switch between Genshin and ZZZ
- [ ] 6h. XXMI: download and update XXMI Libs + GIMI/ZZMI, inject at launch, per-game toggle, mod folder list (enable/disable, preview image, open folder). Note: mods break HoYoverse's terms; the toggle is off by default

## Phase 7: Achievements
- [ ] 7a. Steam Web API: schema (names, icons) + player achievements for owned Steam games
- [ ] 7b. gbe_fork/Goldberg: unlocks from `GSE Saves/<appid>/achievements.json` (and the older Goldberg path) inside the prefix, or the game's local save when configured; names and icons from the game's own `steam_settings/achievements.json` + `achievement_images/`, the Steam schema only as a fallback. Test game: Muse Dash (gbe_fork experimental loader, `steamclient_loader` ini)
- [ ] 7c. Progress on the game page and in the library card; a list with locked/unlocked and unlock time

## Phase 8: Minecraft (Prism backend)
- [ ] 8a. UX proposal for review (below), then build it after feedback
- [ ] 8b. Instances page: groups from `instgroups.json`, cards with icon, version and loader
- [ ] 8c. Instance page: Play (`prismlauncher -l <instance>`), playtime (Prism's own counters + ours), worlds, screenshots
- [ ] 8d. Mods tab: list `mods/` with enable/disable (`.disabled`), open folder
- [ ] 8e. Settings tab: RAM, Java, JVM args, window size (written to `instance.cfg`)
- [ ] 8f. New instance and account handling open Prism for now

### Minecraft UX proposal (for your review)
- The page is two panes. Left is a narrow column of instance cards (icon + name + small `1.21.1 · Fabric` chip), grouped and collapsible. Right is the selected instance.
- The instance header matches the game page: a wide glass banner (a world screenshot or a picked image) with the name, version and loader chips, and a big Play button. The account sits top right.
- Under it are four tabs, kept short: **Overview** (playtime, last played, worlds with last-played dates, recent screenshots), **Mods** (search box, toggle per mod, count, open folder), **Settings** (RAM slider, Java picker, a few switches; "Advanced" expands JVM args) and **Files** (open the folders: mods, resource packs, shaders, saves).
- "+" at the top of the left column is the new-instance flow, three steps: version → loader → name. Prism does it until Phase 10.
- Prism's strength (everything is configurable) is kept by putting the rare options behind Advanced, not by removing them.

## Phase 9: Polish and release
- [ ] 9a. `install.sh` (deps: quickshell, umu-launcher, gamescope, gamemode, mangohud; build the Rust core; desktop entry; user service), same style as SomeShell's
- [ ] 9b. Memory and CPU check while idle and while a game runs (the UI closed or paused)
- [ ] 9c. README (features + install only), screenshots
- [ ] 9d. Public repo

## Phase 10: Native Minecraft (later)
- [ ] 10a. Microsoft/Xbox login (device code), token storage in the keyring
- [ ] 10b. Version manifests, assets, libraries, Java download (Adoptium)
- [ ] 10c. Loaders: Fabric, Quilt, NeoForge, Forge
- [ ] 10d. Modrinth (and CurseForge with a key) search for mods and modpacks
- [ ] 10e. Migrate Prism instances, then drop the Prism dependency

## Open questions
- Hoyo CN servers: needed, or global only?
- A controller / couch mode (gamescope session, big-screen UI)? Not planned yet.
- The logo: left empty until you have one.
