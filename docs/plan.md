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

## Phase 4: Core UI ✅
- [~] 4a. Home: "Continue playing" (last played), favorites, the Hoyo games' update state; games not played for 30 days drop off Home (configurable) but stay in Library — a banner for the last played game (a click on a card shows that one, a double click plays), `home.recentDays` in config.json; favorites never drop off; the Hoyo cards show the installed version and `build.updateAvailable`, which the Hoyo update check (6e) fills
- [x] 4b. Library (Netflix layout): a hero row for recent/favorite, a row per category, a Steam badge on Steam and Needs-Steam games, search and filter — games without a category get a row per kind, uninstalled ones a last row; search (name, tag, category; typing anywhere starts it) and the filter chips show a grid. A click picks a card until 4c opens the game page
- [x] 4c. Game page (PS5 layout): full hero art, logo, a big Play button, a settings button, playtime, last played, achievement progress, build/update badge; the shared element moves from card to page — opens over the current page (`core/Nav.qml`) from a Library card or Home's banner (its info button); a double click on a card still plays; achievements show once Phase 7 fills `game.achievements` (`{unlocked, total}`)
- [x] 4d. Game settings sheet: Simple (runner, Proton, Needs Steam, gamescope, MangoHud) and Advanced (env, args, DLL overrides, prefix, scripts, logs). Nothing more than one click deep for the common options — also GameMode in Simple; Advanced adds name, categories, recorded build, the match (`components/MatchPicker.qml`, shared with 4e), art from a file, umu game id, Steam app id, discrete GPU, the launch command, desktop entry, hidden and removal. Every change saves at once
- [x] 4e. Add game wizard: 3 steps (pick file → confirm name and art → runner); import dialog on the first run — step 1 also offers what `library.scan` finds in the game folders (dismissable into `scanIgnored`); the game is added (and matched) only at the end, then its page opens. The first-run dialog picks the importers and an optional game folder (`ui.welcomed` in config.json). `+` in Library and Home's empty state open the wizard
- [~] 4f. Settings: game folders, API keys (SteamGridDB, Steam), defaults, profiles, appearance — "profiles" is the launch defaults card (this machine's profile: Proton, MangoHud, GameMode, gamescope, discrete GPU, umu-run); the power profile and battery options join it in Phase 5. Also: import again, fetch art, check updates, prefix folder, dismissed scan folders, Proton builds (install GE/CachyOS with progress and cancel, remove ours with a confirming second click), running jobs, the daemon (restart) and SomeShell detection
- [x] 4g. Motion pass: springs, page transitions, card hover, reduced motion option; everything pauses while a game runs — pages slide in from the side of the rail they come from, cards rise on hover and stagger in when a row is new, the game page grows out of its card (4c), sheets and dialogs spring; `Motion.paused` (one of our games running, or SomeShell's game mode) turns reduced motion on, which skips the decorative moves

## Phase 5: Profiles and SomeShell ✅
- [x] 5a. Power profile: `powerprofilesctl` to performance on launch (balanced on battery), restore the previous one on exit — through power-profiles-daemon's D-Bus API (`busctl`) rather than the CLI; follows the charger mid-game, leaves a profile the user picked, restores after a daemon crash (`powerRestore`). Tested on this PC and on `scripts/fake-upower.py`
- [x] 5b. Battery bypass while gaming on AC (UPower `EnableChargeThreshold`, as SomeShell's `services/Battery.qml`), restored on exit; laptop only — tested on a fake UPower laptop (`scripts/fake-upower.py`); still to try once on the real laptop, since the driver decides what the threshold does. Both switches are in Settings → Launch defaults
- [x] 5c. Per-machine defaults: the gamescope size/refresh and FPS cap come from the monitor the window is on (`hyprctl monitors -j`) — Hyprland's focused monitor at launch (the launcher's when Play is clicked there); `fpsCap` (`false`, `"monitor"`, a number) as a default and per game, through gamescope, else MangoHud, else DXVK/VKD3D
- [x] 5d. gamemoderun by default (SomeShell's `GameClients` then turns its game mode on); the launcher pauses video and animations, optionally closes or minimizes its window — "minimize" hides the window (Quickshell, so any compositor, nothing renders) and it comes back when the last game ends; Settings → Appearance → When a game starts (`ui.onPlay`: stay, hide, close). Motion pauses since 4g; video arrives with 6g
- [~] 5e. SomeShell integration when present: island state for download progress and "Playing X" (IPC on the shell), an overlay card (recent games, playtime), `qs -c somelauncher ipc call ...` targets — not IPC on the shell: SomeShell reads somelauncherd's socket itself (`services/Launcher.qml`, SomeShell 13i), so the island and the card work while the launcher window is hidden or closed. Island: download progress, else "Playing X" with the session time (outside game mode, where the island is a dot); overlay: a Games card (the running game, session and total playtime, recent games). The launcher side is the `launcher` IPC target (open, toggle, hideWindow, page, game, add, play, state), which SomeShell's clicks call

## Phase 6: Hoyo games (Genshin Impact, Zenless Zone Zero) ✅
- [x] 6a. HoYoPlay API client: games, branches (+ preload), background art (image and video), news and events, HoYoLAB links; global server first, CN later — `hoyo.games`, `hoyo.content` (kept 10 minutes, the last answer when offline); CN uses its own host and launcher id and answers, but nothing CN is installed or played yet. HoYoPlay is also the first automatic art source for Hoyo games
- [x] 6b. Adopt existing installs (from 1d, or pick a folder): read the version from the game files — `hoyo.adopt`; the version is Unity's bundle version in `globalgamemanagers` (Twintail leaves `config.ini` stale: 7.0.0 there, 7.1.0 installed), voice packs from `Persistent/audio_lang_*`, region from the exe (ZZZ: `version_info`). Twintail's installs are adopted after each import; the add wizard adopts a picked Genshin/ZZZ folder in one step. Checked on both real installs
- [x] 6c. Sophon install: manifest, chunk download with resume, verify, extract, voice packs chosen by the user — `hoyo.plan` and `hoyo.install`; a full install is every FULL resource category (ZZZ has over a hundred) plus the chosen voices. A real ZZZ install was started, cancelled at 1.4 GB and resumed from there (~70 MB/s), and the ignored test `sophon_real` installs two real categories with a resume and md5-checks every file (`SOPHON_TEST_MAX` picks bigger ones). Not tried: a whole install to the end (85 GB). The install UI comes with the Online page (6g)
- [x] 6d. Update: diff update when available, full otherwise; preload; repair (verify all files) — `hoyo.updatePlan`, `hoyo.update`, `hoyo.preload`, `hoyo.repair`. Diff patches are HDiff data (new files too: a diff from nothing) applied with `hpatchz` (HDiffPatch, fetched once). The ignored test `update_real` installs real ZZZ categories at the previous version, then preloads + diff-updates them, full-updates them and repairs a damaged file, md5-checking all against the current version (~1 min). Not tried on a whole game: both installs here are current
- [x] 6e. Check for updates at launcher start and before Play, and block Play on a version mismatch with an "Update" button — `hoyo.check` fills `build.latest` (daemon start, UI open); `games.launch` checks again and refuses with `update_required` (offline it plays). The Play button follows: Installing/Updating/Preloading/Repairing with progress, Resume install, Update to <version>. Checked with a faked old version in the test library and a throwaway ZZZ install (progress, cancel, resume)
- [x] 6f. Launch: runner and wine/Proton settings Twintail uses for each game, plus anything the game needs on Linux (taken from Twintail's and the anime-games launchers' current setup when this phase starts) — from Twintail's game manifests (2026-10-16): `PROTONFIXES_DISABLE=1`, `PROTON_USE_XALIA=0` (noxalia), ZZZ `PROTON_SET_GAME_DRIVE=1` (gamedrive) and its graphics API (`-use-d3d11` default, DX12 in its settings), Genshin refuses Proton older than GE 10-26 / CachyOS 10.0-20251222 / EM 10.0-33 (`hoyo/runner.rs`). Not done: Twintail's FPS unlocker and telemetry host blocking (needs root for /etc/hosts). Not launched for real here
- [x] 6g. Online page: full-screen art (animated when the API has video), logo, version, Play and settings, playtime, events and news, HoYoLAB link, a right dock to switch between Genshin and ZZZ — laid out like HoYoPlay: the background with its webm loop and theme picture on one crop (`components/HoyoBackdrop.qml`; files cached by the daemon, `hoyo.media`, a 14 MB loop is fetched once), news bottom left (banners, Events/Notices/Info), version, playtime and Play bottom right, the dock on the right edge. The loop plays only while the window is focused, no game page or settings cover it and motion is not reduced (~7 % CPU playing, 0 unfocused); Settings → Appearance turns it off. Also the install UI 6c left for here (`components/HoyoInstall.qml`: folder, voice packs with sizes, download/disk/free, or adopt an existing folder), cancel and progress on the page, a preload button while HoYoPlay offers one (not seen live: none offered now), and offline the library's Hoyo games with their cached art. Checked: both games, a real ZZZ install started from the dialog, cancelled and shown as Resume install, offline in a network-less namespace
- [x] 6h. XXMI: download and update XXMI Libs + GIMI/ZZMI, inject at launch, per-game toggle, mod folder list (enable/disable, preview image, open folder). Note: mods break HoYoverse's terms; the toggle is off by default — as Twintail does it (`hoyo/xxmi.rs`): its MIT `3dmloader.exe` (reads `<package>\d3dx.ini` from its working folder, waits for the game, injects) runs through umu-run in the game's prefix a second after the game, inside the same scope and wrappers; a loader left waiting is stopped when the game command ends. Packages from the projects' latest GitHub releases (`xxmi.status`/`xxmi.update`), mods in a Mods tab of the game's settings (`components/HoyoMods.qml`: switch, versions and update, mods with preview and switch, open folder, copy Twintail's mods). Found on the way: `systemd-run` read `$` in game command lines as its variables; every scope now has `--expand-environment=no`. Checked: install and update of the real packages, the loader under umu-run opening `zzmi\d3dx.ini` (Wine's file trace), the cleanup in a real scope, mod toggles, Twintail's Venti mod copied. Not tried: injection into a running game (not launched here)

## Phase 7: Achievements ✅
- [~] 7a. Steam Web API: schema (names, icons) + player achievements for owned Steam games — Steam's own cache on this PC first, no key needed (`appcache/stats/*.bin`, binary KeyValues: the list in Steam's language, icons, unlock bits and times); the Web API with a key (also games never played here), the cache on its errors. `achievements` table, `{unlocked, total}` on each game, `achievements.list`/`refresh`, `achievement.unlocked` for new unlocks; read at start, after each session, and by request. Checked on this PC's cache (12/12, 8/49, 0/91); the Web API is not tried (no key set here)
- [x] 7b. gbe_fork/Goldberg: unlocks from `GSE Saves/<appid>/achievements.json` (and the older Goldberg path) inside the prefix, or the game's local save when configured; names and icons from the game's own `steam_settings/achievements.json` + `achievement_images/`, the Steam schema only as a fallback. Test game: Muse Dash (gbe_fork experimental loader, `steamclient_loader` ini) — done so (`achievements/gbe.rs`), plus a game whose `steam_settings` has no list takes Steam's for its app id with the emulator's unlocks. Checked on Muse Dash's real `steam_settings` (39, Japanese/Korean texts, local icons) with a save written into a scratch prefix (0/39 → 1/39, `achievement.unlocked` sent); its real save does not exist yet (not played with this setup)
- [x] 7c. Progress on the game page and in the library card; a list with locked/unlocked and unlock time — the card's trophy pill (filled once all are unlocked); the game page's Achievements tile opens `components/AchievementsSheet.qml` (progress bar, All/Unlocked/Locked, unlocked newest first with dates, locked in the source's order with gray icons, hidden ones secret until hovered, a refresh button, where the list came from)
- [~] 7d. (added 2026-10-17) Unlock popup for non-Steam games: the daemon watches the running game's achievement file (inotify, no polling) and emits `achievement.unlocked`; a PS5-style toast shows it, through SomeShell when it runs, else our own small layer-shell popup over the game (not inside gamescope). Steam games keep Steam's own popup — done with our own popup only (`components/AchievementToast.qml`: overlay layer, top of the focused monitor, click-through, queued, only while the game runs); SomeShell showing it instead is left for SomeShell's repo (it already reads our socket, so it would listen to `achievement.unlocked`). The watch (`achievements/live.rs`) starts with each session (also one found again after a restart), fills the list first, follows the save folder's appearance, and stops with the game. Not shown with Settings → When a game starts → Close (the UI quits). Checked live with a stand-in game in Muse Dash's folder: one unlock, then two queued

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
