# somelauncherd protocol

The UI and the CLI talk to `somelauncherd` over a unix socket, `$XDG_RUNTIME_DIR/somelauncher.sock`. `SOMELAUNCHER_SOCKET` overrides the path, which tests use. The socket is mode 0600.
Each message is one JSON object on one line. The types live in `core/crates/proto`.

## Messages

Client → daemon, a request:
```json
{"id": 1, "method": "settings.get", "params": {"key": "gameDirs"}}
```
`params` may be left out. `id` is chosen by the client and comes back in the reply.

Daemon → client, a reply (exactly one per request, in request order):
```json
{"id": 1, "result": ["/games"]}
{"id": 1, "error": {"code": "bad_params", "message": "missing string `key`"}}
```
A line that is not a request gets an error reply with `id` 0.

Daemon → client, an event (no `id`):
```json
{"event": "settings.changed", "data": {"key": "gameDirs", "value": ["/games"]}}
```
Every client gets every event. Besides the UI and the CLI, SomeShell is a client (its `services/Launcher.qml`: what runs, downloads, playtime), so events and their fields are part of the contract. Replies and events share one ordered stream, so a client tells them apart by the `id` field.

On connect the daemon first sends `hello`:
```json
{"event": "hello", "data": {"protocol": 1, "version": "0.1.0"}}
```
`protocol` is bumped on any incompatible change. A client that does not know the number must not send anything else.

## Error codes
| code | meaning |
| :--- | :--- |
| `bad_request` | the line is not a request |
| `unknown_method` | no such method |
| `bad_params` | a param is missing or has the wrong type |
| `not_running` | the game is not running |
| `launch_failed` | the game could not be started (the message says why) |
| `busy` | the same job already runs |
| `not_found` | no game (or other object) with that id |
| `network` | a web service failed or refused (the message says which) |
| `no_space` | the disk has too little free space for it (the message says how much) |
| `internal` | the daemon failed (the message says why; the log has more) |

## Methods
| method | params | result |
| :--- | :--- | :--- |
| `ping` | — | `{"pong": true}` |
| `status` | — | `{version, protocol, pid, db, monitor}`; `monitor` is where games start (`{name, width, height, refresh}`, Hyprland's focused one) or `null` |
| `settings.get` | `{key?}` | the value (`null` when unset), or every setting as an object without `key` |
| `settings.set` | `{key, value}` | `null`; a `null` value removes the key. Emits `settings.changed` |
| `games.list` | `{includeHidden?}` | every game (below), by name; hidden ones only with `includeHidden: true` |
| `games.get` | `{id}` | the game |
| `games.add` | a new game: `{name, kind, installDir?, exe?, prefix?, runner?, tags?, categories?, playtime?, lastPlayed?}` | the game, with a `manual` source. Emits `library.changed` |
| `games.update` | `{id, …fields}`: any of `name kind installDir exe prefix runner categories recordedBuild favorite hidden` | the game. A path set to `null` is cleared; `runner` is replaced whole. Emits `library.changed` |
| `games.remove` | `{id}` | `null`. Emits `library.changed` with `removed: true` |
| `games.plan` | `{id}` | what launching would run, without running it: `{argv, env, cwd, newPrefix?, preLaunch?, postExit?}` (`env` is only what we add to the daemon's) |
| `games.launch` | `{id}` | `{unit, log}` once the game is started in its own systemd scope. Errors: `launch_failed` (why, e.g. no Proton or umu-run), `busy` (already running). Emits `game.state` |
| `games.stop` | `{id}` | `null`: SIGTERM to the game's whole process tree (its scope, or everything under a Steam game's reaper), SIGKILL 10 s later if it still runs. `not_running` when it is not |
| `games.running` | — | running and starting games: `[{id, unit, since}]` (`since` is `null` while Steam has not started it yet); what a client that just connected needs |
| `games.logs` | `{id}` | the game's last 3 logs, newest first: `[{path, time, size}]` |
| `games.desktop` | `{id, enabled?}` | `{enabled, path}`; `enabled: true` writes `$XDG_DATA_HOME/applications/somelauncher-game-<id>.desktop` (`Exec=somelauncher play <id>`), `false` removes it, left out only reports. The entry follows renames and goes with the game |
| `games.duplicates` | — | groups of games with the same name (ignoring case and punctuation) that are separate entries: copies from different installs, for the user to merge or keep |
| `games.merge` | `{id, into}` | the merged game: `id`'s sources move to `into`, empty fields are filled, tags and categories joined, playtimes added; `id` is deleted. Emits `library.changed` for both |
| `games.guess` | `{path}`: an exe or a folder | `{name, kind, installDir, exe, candidates}`: a guess for the add wizard, nothing is added. Genshin Impact and ZZZ installs come back as `kind: "hoyo"`, for `hoyo.adopt`. `candidates` are every plausible executable, best first. `not_found` when the path does not exist |
| `meta.search` | `{id?, query?}`: a game (its name is the query) and/or a query | candidates `[{source, id, name, year?, cover?, thumb?, reason?}]`, best first: the ids the game names itself first (`reason`: `Steam library`, `OnlineFix.ini`, `steam_appid.txt`), then the Steam store's and SteamGridDB's (with `steamGridDbKey`) hits by name. `cover` is a portrait image URL to confirm with, `thumb` a fallback. Error `network` |
| `meta.match` | `{id, source, sourceId}`: a candidate | the game with its `meta` set. The other source's id is filled in when found (a Steam app's SteamGridDB id, a SteamGridDB game's Steam app by its exact name). Emits `library.changed` |
| `meta.unmatch` | `{id}` | `null`; the game has no match again |
| `meta.refresh` | `{id?, force?}` | with `id`: the game, its store details and tags fetched (when missing, or again with `force`); without: `{id}` of a background job for every matched game (a request a second, SteamSpy's limit) |
| `builds.check` | `{id?}` | with `id`: the game, its latest build looked up; without: `{id}` of a background job for every installed Steam game. Also done when the daemon starts (unless the last check is under 12 hours old) and after an import that added games |
| `art.fetch` | `{id?, force?}` | with `id`: the game, its art brought up to date (`force` looks for better automatic art too); without: `{id}` of a background job doing every game (`download.*` events, `done`/`total` in games). Art the user chose is never replaced |
| `art.set` | `{id, kind, origin?}`: `kind` is `hero`, `grid`, `logo` or `icon`; `origin` a file or URL | the game with that art; without `origin` the kind goes back to automatic |
| `art.options` | `{id, kind}` | the automatic origins for that kind, best first, to pick another with `art.set` |
| `library.import` | `{sources?}`: a list of `steam`, `lutris`, `twintail`, `prism`; all of them when left out | `{added, updated, errors}`: game ids, and a message per importer that failed (the others still run). Emits `library.changed` |
| `library.scan` | — | a `games.guess` result for each folder directly under `gameDirs` that holds an executable and no known game, minus `scanIgnored`. Nothing is added |
| `proton.list` | — | `{builds, default}`: every Proton build `{name, internal, dir, source}` (`source`: `user`, `system`, `steam`, `lutris`, `twintail`) and the default's `name` |
| `proton.available` | `{source}`: `ge` or `cachyos` | the newest releases with an x86_64 build: `{source, tag, published, url, checksumUrl, size, installed}` |
| `proton.install` | `{source, tag}` | `{id}` at once; the download runs in the background (`download.*` events), is checked against its sha512 and unpacked into `compatibilitytools.d`. Error `busy` when it already runs |
| `proton.remove` | `{name}` | `null`; only builds in our `compatibilitytools.d` |
| `downloads.list` | — | running jobs `{id, label, done, total}` |
| `downloads.cancel` | `{id}` | `null`; the job stops and its partial files are removed (a Hoyo install keeps what it fetched, to resume). Emits `download.done` with `error: "cancelled"` |
| `power.status` | — | `{profiles, active, onBattery, battery, gaming, prefs}`: power-profiles-daemon's profiles (empty without it) and the active one, the laptop battery (`null` on a desktop: `{path, limitSupported, limitEnabled, limitStart, limitEnd}`), whether gaming mode is on (see Gaming mode), and the `gaming` setting with its defaults |
| `hoyo.games` | `{region?, force?}`: `global` (default) or `cn` | the HoYoPlay games we support (Genshin Impact, Zenless Zone Zero): `{biz, id, name, region, logo, icon, background, backgrounds, main, preload, games}`. `main`/`preload` are branches `{packageId, branch, password, tag, diffTags, voices}` (`preload` only while there is one); `backgrounds` the launcher's `{image, video?, theme?}`, the current first (a `video` is a webm loop, `theme` a transparent picture over it); `games` the library ids of this game. Kept 10 minutes; `force` asks again. Error `network` |
| `hoyo.content` | `{biz, force?}` | `{banners, posts, hoyolab, social}`: banners `{link, icon}` (`icon` is the image), posts `{kind, title, link, date}` (`kind`: `info`, `activity`, `announce`), the game's HoYoLAB (or Miyoushe) page, social links `{title, link, icon}`. Error `network` |
| `hoyo.adopt` | `{path}` (a game folder or its exe) or `{id}` (a library game's folder) | the game: an install on disk becomes ours to update and run. Which game and region come from the exe (ZZZ's `version_info` for the region), the version from `<Game>_Data/globalgamemanagers` (Unity's bundle version; `config.ini` only as a fallback, other launchers leave it stale), the voice packs from `Persistent/audio_lang_*`. A library game in that folder keeps its entry and gets a `hoyo` source; otherwise one is added. Done for every Hoyo game after an import too. Error `not_found` |
| `hoyo.plan` | `{biz, voices?, dir?}` | what an install takes: `{biz, version, dir, voices, voiceSizes, downloadSize, size, free}` (bytes; `voiceSizes` lists every voice pack `{voice, downloadSize, size}`; voices default to the locale's language when offered, else English). Error `network` |
| `hoyo.install` | `{biz, dir?, voices?}`, or `{id}` to resume one | `{id, game}`: the job (`download.*` events, `done`/`total` in downloaded bytes) and the library game, made at once with `installing: {version, voices}` in its `hoyo` source until the job ends. `dir` defaults to `<first game folder>/<name>`. Every resource category of a full install plus the voice packs, through Sophon: manifests and chunks are checked against their md5, chunks written in place, and a log of finished chunks (`<dir>/.somelauncher/`) lets a cancelled or broken install go on with `{id}`; cancelling keeps what was fetched. At the end `config.ini` and the game's voice pack list are written and the game is adopted. Errors `no_space` (checked before anything is made), `busy` (the folder already holds the game: adopt it), `network` |
| `hoyo.updatePlan` | `{id, force?}` | `{id, installed, latest, upToDate, update?, preload?}`: `update`/`preload` are `{method, downloadSize}` (`diff`, with its size, when every category has patches from the installed version; `full` otherwise, size `null` until compared); `preload` also `{version, done}` while HoYoPlay offers one. Errors `not_found` (not an installed Hoyo game), `network` |
| `hoyo.update` | `{id}` | `{id}` of the job (`hoyo-<game id>`): a diff update (each changed file's HDiff patch cut from its bundle by a range request, applied with `hpatchz`, md5-checked, then swapped in; preloaded patches are taken from disk) or a full one (files whose md5 changed between the installed and new manifests fetched again); files the new version drops are deleted; then the version is read again. Resumes from a log. `hpatchz` is the one on PATH, else HDiffPatch v5.1.3 fetched once (checksum pinned) into `$XDG_DATA_HOME/somelauncher/tools`. Errors `busy` (it runs, or another job on it), `bad_params` (up to date) |
| `hoyo.preload` | `{id}` | `{id}` of the job: the next version's patches fetched into `<dir>/.somelauncher/preload-<version>/`; the game is not touched until its update |
| `hoyo.repair` | `{id}` | `{id}` of the job: every file md5-checked against the installed version's manifest (progress through the check), then the bad ones fetched again and swapped in |
| `shutdown` | — | `null`, then the daemon exits |

## Events
| event | data |
| :--- | :--- |
| `hello` | `{protocol, version}`, sent only on connect |
| `settings.changed` | `{key, value}` |
| `download.progress` | `{id, label, done, total}` in bytes, a few times a second at most |
| `download.done` | `{id, label, error?}` |
| `game.state` | `{id, state, unit?, step?}`: `starting` (with `step: "steam"` while waiting for Steam), `running` (with its systemd unit, or `steam:<appid>`, and `since`) or `stopped` (with `error` when it failed to start) |
| `library.changed` | `{ids, removed?}`: these games changed (or were removed); fetch them again |
| `power.changed` | `{gaming}`: gaming mode went on (the first game started) or off (the last one ended) |

## Game
`core/crates/proto/src/game.rs`. Times are unix seconds, `playtime` is seconds.
```json
{
  "id": 4, "name": "How to Fish", "kind": "proton",
  "installDir": "/games/How to Fish", "exe": "/games/How to Fish/HowToFish.exe", "prefix": "/games/Prefixes/how-to-fish",
  "runner": {"proton": "GE-Proton10-20", "args": [], "env": {}, "dllOverrides": {"winmm": "n,b"}, "workingDir": null, "gameId": "umu-0", "needsSteam": true},
  "tags": ["co-op", "needs steam", "simulation"], "categories": [],
  "favorite": false, "hidden": false, "playtime": 5400, "lastPlayed": 1792137600, "added": 1792051200,
  "sources": [{"source": "lutris", "sourceId": "how-to-fish", "data": {}}],
  "meta": {"source": "steam", "sourceId": "4001890", "name": "How to Fish", "steamAppid": "4001890", "sgdbId": "5432101"}
}
```
- `kind`: `steam` (run through Steam), `proton` (umu-run), `native`, `hoyo`, `minecraft` (a Prism instance).
- `runner`: empty fields mean the global default and are left out of the JSON. Keys this daemon does not know are kept as they are.
- `art`: `{hero, grid, logo, icon}`, cached files (`$XDG_CACHE_HOME/somelauncher/art/<id>/<kind>-<hash>.<ext>`, a new name whenever the art changes) or `null`. Each is sized for this machine so Qt never holds a 4K decode: heroes at most the widest connected monitor's width (from `/sys/class/drm`), grids 600x900, logos 1280x640, icons 256x256, never enlarged; JPEG, or PNG when transparent. When the widest monitor changes, the next daemon start makes them all again from their origins. Automatic art comes, best first, from HoYoPlay (Hoyo games: the current launcher background, the logo and icon), SteamGridDB, Steam's CDN (by the Steam app id), then the importers' files (Lutris covers, Prism and Twintail icons, Twintail backgrounds); a new match fetches it again.
- `meta`: the metadata match (`null` until matched). The user confirms each from `meta.search`; Steam games match themselves on import. After an import, details and art for new games are fetched by background jobs. A match brings, in the background, `description`, `genres`, `features` (Steam's feature list), `steamTags` (Steam's user tags from SteamSpy), `developers`, `publishers`, `release`, and `categories`: the automatic library rows (`Co-op` for co-op games, then the main genre).
- `tags`: automatic, recomputed whenever what they come from changes: `co-op`, `multiplayer`, `needs steam` (runner), `gacha` (hoyo), `modded` (XXMI on, or a Minecraft mod loader), the genres and Steam's top 6 user tags; lowercase.
- `build`: `{recorded?, latest?, latestTime?, checked?, installed?, updateAvailable}`. Hoyo games: `installed` is the version read from the files (`hoyo` source). Steam games: `installed` is the build in Steam's manifest, `latest` the latest public build (api.steamcmd.net, a mirror of Steam's PICS), and `updateAvailable` is true when they differ or Steam itself has an update waiting. Other games: `recorded` is the build the user notes (`recordedBuild` in `games.update`).
- `categories`: the user's library rows; while empty the library uses `meta.categories`.
- `sources`: where the game was imported from (`steam`, `lutris`, `twintail`, `prism`, `manual`, and `hoyo` for an install we manage: id its folder, `{biz, version, voices, installing?}`), with what that source said in `data`. A merged game (the same game from two sources) has several.

## Settings keys
Daemon settings live in `library.db` and are changed only through `settings.set`. UI preferences (looks, motion) are not stored here: they live in `~/.config/somelauncher/config.json`, which the UI owns.

| key | value |
| :--- | :--- |
| `steamGridDbKey` | SteamGridDB API key (steamgriddb.com → Preferences → API); without it only the Steam store is searched |
| `steamApiKey` | Steam Web API key; with it `library.import` adds owned games that are not installed |
| `defaults` | launch defaults for every game: `{mangohud, gamemode, gamescope, prime, fpsCap}` (see Launching); a game's own runner fields win |
| `gameDirs` | folders whose subfolders `library.scan` looks at |
| `scanIgnored` | folders `library.scan` no longer suggests (the user dismissed them) |
| `defaultProton` | the Proton build games use when they name none (any name `proton.list` knows, or a path); the newest GE-Proton when unset |
| `prefixDir` | where new wine prefixes go (`<prefixDir>/<game name>`); `Prefixes` in the first `gameDirs` folder when unset |
| `umuRun` | the umu-run to use; else `umu-run` on PATH, else the copy Lutris ships |
| `artScreenWidth` | the screen width art was last sized for (the daemon keeps it) |
| `buildsChecked` | when the last update check ran (the daemon keeps it) |
| `gaming` | what gaming mode may change: `{powerProfile, chargeLimit}`, both `true` when unset |
| `powerRestore` | what gaming mode changed and must undo (the daemon keeps it): `{profile: [before, set], limit: <battery path>}` |
| `language` | the language for HoYoPlay's texts (`en-us`, `th-th`, …); the locale's when unset |
| `steamId` | SteamID64 for the Web API; the account that logged in last when unset |

## Launching
Every game runs in its own `systemd-run --user --scope` (unit `somelauncher-game-<id>-<time>.scope`), its output in `$XDG_STATE_HOME/somelauncher/logs/<id>/<time>.log` (the last 3 are kept).

Sessions: a game counts as running until its scope has no process left (`cgroup.events`), so a launcher that starts the game and exits does not end it; a Steam game until its reaper exits. Each session is a row in `sessions`, touched once a minute; when it ends its length is added to `playtime` and `lastPlayed` is set (`library.changed`). When the daemon starts it follows games that still run again, and closes the sessions of the others at their last touch (a crash loses at most a minute). Games keep running when the daemon or the UI stops.
- `proton` and `hoyo` games: `umu-run <exe> <args>` with `WINEPREFIX` (the game's prefix; a game without one gets `<prefixDir>/<name>`, created and saved on first launch), `PROTONPATH` (its Proton, else the default), `GAMEID` (`runner.gameId`, else `umu-0`) and `WINEDLLOVERRIDES` from `runner.dllOverrides`. The working dir is `runner.workingDir`, else the exe's folder.
- `steam` games: `steam -applaunch <appid> <args>` (flatpak Steam through `flatpak run`), started in its own scope since it becomes Steam when Steam was not running. Steam keeps its own Proton and launch options; `runner.args` are passed on. The game counts as `running` when Steam's `logs/gameprocess_log.txt` says it started and its `reaper SteamLaunch AppId=<appid>` process exists (10 minutes at most, Steam may update it first), and `stopped` when that reaper exits (pidfd). `games.launch` returns `{unit: "steam:<appid>"}`; `busy` when the game already runs in Steam.
- `native` games: the exe with its args. Without the exec bit, a script runs through its `#!` interpreter (or `sh` for `.sh`), and a binary is given the bit first (`makeExecutable` in the plan). A Windows exe is refused with a hint to use Proton.
- Wrappers (`proton`, `hoyo`, `native`): `gamemoderun gamescope <opts> [--mangoapp] -- [mangohud] <command>`. Each comes from the game's `runner.mangohud`, `runner.gamemode`, `runner.gamescope` (`{enabled, width, height, gameWidth, gameHeight, refresh, fsr, hdr, fullscreen, args}` → `-W -H -w -h -r -F fsr --hdr-enabled -f`, replacing the default whole) or else the `defaults` setting. Empty `width`/`height`/`refresh` are the monitor's the game starts on (Hyprland's focused one, the launcher's when Play is clicked).
- FPS cap: `runner.fpsCap`, else `defaults.fpsCap`: `false`, `"monitor"` (its refresh) or a number. It goes to gamescope (`--framerate-limit`) when that runs, else MangoHud (`MANGOHUD_CONFIG=fps_limit=N`; with MangoHud off but installed it wraps the game hidden, `no_display`), else `DXVK_FRAME_RATE`/`VKD3D_FRAME_RATE` for `proton`/`hoyo` games. Inside gamescope MangoHud is its `--mangoapp`. `defaults.gamemode` is on unless set to `false` (SomeShell's game mode follows gamemoderun). A wrapper the game turns on but that is not installed fails the launch; one only on by default is skipped. Steam games get none (Steam's launch options apply).
- PRIME: on a hybrid-GPU laptop (display on the integrated GPU, a discrete one present) games get the offload env (NVIDIA: `__NV_PRIME_RENDER_OFFLOAD=1 __GLX_VENDOR_LIBRARY_NAME=nvidia __VK_LAYER_NV_optimus=NVIDIA_only`, Mesa: `DRI_PRIME=1`), unless `runner.prime` (or `defaults.prime`) is `false`.
- `runner.env` is applied last and wins, except that a `WINEDLLOVERRIDES` there is joined to ours.
- `runner.needsSteam` (`proton`/`hoyo` games whose co-op needs the real Steam client: OnlineFix, gbe_fork experimental): Steam is started (`steam -silent`, in its own scope) and awaited until logged in (`registry.vdf`, 3 minutes at most); `steamclient.dll`, `steamclient64.dll`, `GameOverlayRenderer64.dll`, `Steam.dll` and `steam.exe` (Steam's `SteamService.exe`) are copied, never linked, into the prefix's `Program Files (x86)/Steam`; `steam_appid.txt` is written next to the exe when missing (`runner.steamAppId`, else OnlineFix's `FakeAppId`, else 480); the env gets `UMU_USE_STEAM=1` and `n,b` overrides for `winmm`, `OnlineFix64`, `SteamOverlay64`, `StubDRM64`, `steam_api64` unless the game sets them.
- `runner.preLaunch` runs through `sh -c` before the game, with its env and working dir; if it fails the game does not start. `runner.postExit` runs after the game exits.

## Gaming mode
While at least one game runs or starts, the power profile is performance (balanced on battery) through power-profiles-daemon, and on a laptop on AC the battery's charge limit is on (UPower's `EnableChargeThreshold`: a start/stop threshold, or conservation mode, which runs the laptop from the charger). Plugging in or out during the game is followed (UPower's `OnBattery`, watched only while gaming). When the last game ends each goes back, only if it is still what we set (a profile the user picked during the game stays). What to undo is kept in `powerRestore`, so a daemon that died mid-game restores on its next start. A desktop without a battery or without power-profiles-daemon skips that part. All of it is on the system bus through `busctl`/`gdbus`; `DBUS_SYSTEM_BUS_ADDRESS` points it at `scripts/fake-upower.py` for testing without a laptop.

## Proton
A game's `runner.proton` (and `defaultProton`) may be a folder name (`GE-Proton11-7`), the internal name Steam uses (`proton-cachyos-11.0-20260703-slr-x86_64`) or a path; all resolve to the same build. Downloads go to the first Steam install's `compatibilitytools.d` (`~/.local/share/Steam/compatibilitytools.d` without Steam), where umu-run and Steam find them too.

## Imports
Re-importing is safe: a game seen before is updated (paths only where empty, playtime and last played to the larger value, the source's `data` replaced), and nothing the user edited is overwritten. A removed game is remembered and not imported again. An entry whose install is already in the library (the same exe, or the same folder and kind) is linked to that game as a second source instead of added; the same game installed twice stays two entries until `games.merge`.

- **steam**: every Steam install (native and flatpak) and library folder; tools (Proton, runtimes, redistributables) are skipped. Paths follow Steam: an uninstalled game keeps its entry with `installDir: null`. Source `data`: `{installed, buildid, stateFlags, library, proton, flatpak}`, where `proton` is Steam's own compatibility tool choice for the game (`null`: Steam's default).
- **lutris**: installed games in `pga.db` (native and flatpak Lutris) with their `games/<configpath>.yml`. `wine` games become `proton` games (exe, prefix, `wine.version` as `runner.proton`, overrides, env, args, working dir), `linux` games `native`; Lutris' Steam games and other runners are skipped. `system.mangohud`/`gamemode` go to `runner.mangohud`/`runner.gamemode`. Source id: the slug; `data`: `{runner, configpath, coverart, banner}` (Lutris' own art, when present).
- **twintail**: Genshin Impact and Zenless Zone Zero installs in Twintail's `storage.db` (native first, then flatpak) whose folder still exists, as `hoyo` games. `prefix` is the wine prefix inside Twintail's compatdata (`<compatdata>/pfx`), `runner.proton` the path of Twintail's runner, args include its graphics API flag. Source id: Twintail's install id; `data`: `{biz, version, region, voice, compatData, xxmi, fpsUnlock, fps, icon, background}`.
- **prism**: every Prism instance (native and flatpak) with an `instance.cfg`, as `minecraft` games; `installDir` is the instance folder, playtime is Prism's. Instance settings (RAM, Java) stay in Prism's files. Source id: the instance folder name (`prismlauncher -l <id>`); `data`: `{root, flatpak, group, minecraft, loader, loaderVersion, iconKey, icon, pack}` (`group` from `instgroups.json`, `icon` a custom icon file or `null` for Prism's built-in ones).
