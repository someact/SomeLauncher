//! The game model as it goes over the wire. The daemon stores it in
//! `library.db` (`games` + `game_sources`); every field is camelCase in JSON.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// How a game is run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Through Steam itself (`steam -applaunch`)
    Steam,
    /// A Windows game through umu-run + Proton
    Proton,
    /// A Linux executable or script
    Native,
    /// Genshin Impact, Zenless Zone Zero (HoYoPlay + Sophon)
    Hoyo,
    /// A Prism instance
    Minecraft,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Steam => "steam",
            Kind::Proton => "proton",
            Kind::Native => "native",
            Kind::Hoyo => "hoyo",
            Kind::Minecraft => "minecraft",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        serde_json::from_value(Value::String(s.into())).ok()
    }
}

/// Where a game entry came from. One game can have several (merged duplicates, 1g).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Steam,
    Lutris,
    Twintail,
    Prism,
    /// Added by hand or found by a folder scan
    Manual,
    /// A Genshin Impact or Zenless Zone Zero install SomeLauncher manages (Phase 6):
    /// source id is the install folder, `data` `{biz, version, voices, installing?}`
    Hoyo,
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SourceKind::Steam => "steam",
            SourceKind::Lutris => "lutris",
            SourceKind::Twintail => "twintail",
            SourceKind::Prism => "prism",
            SourceKind::Manual => "manual",
            SourceKind::Hoyo => "hoyo",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        serde_json::from_value(Value::String(s.into())).ok()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub source: SourceKind,
    /// The id in that source: Steam appid, Lutris slug, instance folder, …
    pub source_id: String,
    /// What the importer read, kept for display (Steam's own Proton choice, the build id, …)
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub data: Value,
}

/// Per-game runner options. Empty fields mean "use the global default".
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Runner {
    /// Proton / wine build name or path (`GE-Proton10-20`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proton: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// `WINEDLLOVERRIDES` entries: dll → `n,b`
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub dll_overrides: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<PathBuf>,
    /// umu's `GAMEID` (protonfixes), e.g. `umu-1145360`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game_id: Option<String>,
    /// Start Steam first and set up the prefix for Steam-dependent games (2c)
    #[serde(skip_serializing_if = "is_false")]
    pub needs_steam: bool,
    /// The app id written to `steam_appid.txt` for Needs Steam games; OnlineFix's `FakeAppId`, else 480, when unset
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steam_app_id: Option<String>,
    /// MangoHud overlay; None: the global default
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mangohud: Option<bool>,
    /// gamemoderun; None: the global default
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gamemode: Option<bool>,
    /// gamescope settings, replacing the global ones whole; None: the global default
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gamescope: Option<Gamescope>,
    /// Run on the discrete GPU of a hybrid laptop; None: the global default (on when hybrid)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prime: Option<bool>,
    /// A frame rate cap; None: the global default
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fps_cap: Option<FpsCap>,
    /// A shell command run before the game starts (with the game's env); a failure stops the launch
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_launch: Option<String>,
    /// A shell command run after the game has exited
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_exit: Option<String>,
    /// Options added by later phases (wrappers, scripts) that this build does not know yet
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Gamescope {
    pub enabled: bool,
    /// Output size (`-W`/`-H`); the monitor's when unset
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    /// The game's own resolution (`-w`/`-h`), lower than the output to upscale
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game_width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game_height: Option<u32>,
    /// `-r`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh: Option<u32>,
    /// `-F fsr`
    #[serde(skip_serializing_if = "is_false")]
    pub fsr: bool,
    /// `--hdr-enabled`
    #[serde(skip_serializing_if = "is_false")]
    pub hdr: bool,
    /// `-f`
    #[serde(skip_serializing_if = "is_false")]
    pub fullscreen: bool,
    /// Anything else, passed as is
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
}

/// A frame rate cap: `false` (none), `"monitor"` (the refresh rate of the
/// monitor the game starts on) or a number of frames per second.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FpsCap {
    Off(bool),
    Monitor(MonitorWord),
    Fps(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MonitorWord {
    Monitor,
}

impl FpsCap {
    /// The cap in frames per second, given the monitor's refresh rate
    pub fn fps(self, refresh: Option<u32>) -> Option<u32> {
        match self {
            FpsCap::Off(_) => None,
            FpsCap::Monitor(_) => refresh,
            FpsCap::Fps(0) => None,
            FpsCap::Fps(n) => Some(n),
        }
    }
}

/// Global launch defaults (the `defaults` setting); a game's own runner fields win.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Defaults {
    pub mangohud: bool,
    /// On unless turned off: gamemoderun is what SomeShell's game mode follows (5d)
    pub gamemode: bool,
    pub gamescope: Gamescope,
    /// None: on when the machine is a hybrid-GPU laptop
    pub prime: Option<bool>,
    /// None: no cap
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fps_cap: Option<FpsCap>,
}

impl Default for Defaults {
    fn default() -> Self {
        Self { mangohud: false, gamemode: true, gamescope: Gamescope::default(), prime: None, fps_cap: None }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    pub id: i64,
    pub name: String,
    pub kind: Kind,
    pub install_dir: Option<PathBuf>,
    pub exe: Option<PathBuf>,
    /// The wine prefix (proton and hoyo games)
    pub prefix: Option<PathBuf>,
    pub runner: Runner,
    /// Automatic labels (genres, Steam tags, `co-op`, `needs steam`, …), recomputed; not editable
    pub tags: Vec<String>,
    /// Library rows the user set; when empty the library uses `meta.categories`
    pub categories: Vec<String>,
    pub favorite: bool,
    pub hidden: bool,
    /// Seconds
    pub playtime: i64,
    /// Unix seconds
    pub last_played: Option<i64>,
    /// Unix seconds
    pub added: i64,
    pub sources: Vec<Source>,
    /// What the game was matched to for art and metadata; None until matched
    #[serde(default)]
    pub meta: Option<Meta>,
    /// Cached art files, sized for this screen
    #[serde(default)]
    pub art: Art,
    #[serde(default)]
    pub build: Build,
}

/// Which build of the game is installed and whether a newer one is out.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Build {
    /// What the user noted (games that are not from Steam: `v6.7.0`, a release date)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recorded: Option<String>,
    /// The latest public build (Steam games)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest: Option<String>,
    /// Unix seconds of the latest build
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_time: Option<i64>,
    /// Unix seconds of the last check
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked: Option<i64>,
    /// The installed build (Steam's manifest); worked out on load, not stored
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed: Option<String>,
    /// Worked out on load, not stored
    #[serde(default)]
    pub update_available: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Art {
    /// Wide background (the game page, the library's hero row)
    pub hero: Option<PathBuf>,
    /// Portrait cover (library cards)
    pub grid: Option<PathBuf>,
    /// The title, transparent (over the hero)
    pub logo: Option<PathBuf>,
    /// Square icon (lists, desktop entries)
    pub icon: Option<PathBuf>,
}

/// A metadata match (Phase 3) and what it brought.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    /// `steam` or `steamgriddb`: where the user picked it
    pub source: String,
    pub source_id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steam_appid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sgdb_id: Option<String>,
    /// Store details (Steam), filled by `meta.refresh`
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub genres: Vec<String>,
    /// Steam's user tags, most used first (SteamSpy)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steam_tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub developers: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub publishers: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub release: String,
    /// Steam's feature list (`Online Co-op`, `Steam Achievements`, …)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<String>,
    /// Library rows for the game when the user has set none
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<String>,
    /// Unix seconds of the last details fetch; None: never
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fetched: Option<i64>,
}

impl Game {
    /// A game as `games.add` stores it, before it was played or imported.
    pub fn new(id: i64, n: NewGame) -> Self {
        Self {
            id,
            name: n.name,
            kind: n.kind,
            install_dir: n.install_dir,
            exe: n.exe,
            prefix: n.prefix,
            runner: n.runner,
            tags: n.tags,
            categories: n.categories,
            favorite: false,
            hidden: false,
            playtime: n.playtime,
            last_played: n.last_played,
            added: 0,
            sources: Vec::new(),
            meta: None,
            art: Art::default(),
            build: Build::default(),
        }
    }
}

/// A game before it has an id: what `games.add` and the importers hand in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewGame {
    pub name: String,
    pub kind: Kind,
    #[serde(default)]
    pub install_dir: Option<PathBuf>,
    #[serde(default)]
    pub exe: Option<PathBuf>,
    #[serde(default)]
    pub prefix: Option<PathBuf>,
    #[serde(default)]
    pub runner: Runner,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    /// Seconds already played elsewhere (Steam, Lutris, Prism counters)
    #[serde(default)]
    pub playtime: i64,
    #[serde(default)]
    pub last_played: Option<i64>,
}

impl NewGame {
    pub fn new(name: impl Into<String>, kind: Kind) -> Self {
        Self {
            name: name.into(),
            kind,
            install_dir: None,
            exe: None,
            prefix: None,
            runner: Runner::default(),
            tags: Vec::new(),
            categories: Vec::new(),
            playtime: 0,
            last_played: None,
        }
    }
}

/// `games.update`: only the fields present change. A path set to `null` is cleared.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GamePatch {
    pub name: Option<String>,
    pub kind: Option<Kind>,
    #[serde(default, deserialize_with = "present")]
    pub install_dir: Option<Option<PathBuf>>,
    #[serde(default, deserialize_with = "present")]
    pub exe: Option<Option<PathBuf>>,
    #[serde(default, deserialize_with = "present")]
    pub prefix: Option<Option<PathBuf>>,
    pub runner: Option<Runner>,
    pub categories: Option<Vec<String>>,
    /// The build the user notes; `null` clears it
    #[serde(default, deserialize_with = "present")]
    pub recorded_build: Option<Option<String>>,
    pub favorite: Option<bool>,
    pub hidden: Option<bool>,
}

/// Tells a missing field (outer None) from an explicit `null` (Some(None)).
fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
}

impl GamePatch {
    pub fn apply(self, g: &mut Game) {
        if let Some(v) = self.name {
            g.name = v;
        }
        if let Some(v) = self.kind {
            g.kind = v;
        }
        if let Some(v) = self.install_dir {
            g.install_dir = v;
        }
        if let Some(v) = self.exe {
            g.exe = v;
        }
        if let Some(v) = self.prefix {
            g.prefix = v;
        }
        if let Some(v) = self.runner {
            g.runner = v;
        }
        if let Some(v) = self.categories {
            g.categories = v;
        }
        if let Some(v) = self.recorded_build {
            g.build.recorded = v.filter(|b| !b.trim().is_empty());
        }
        if let Some(v) = self.favorite {
            g.favorite = v;
        }
        if let Some(v) = self.hidden {
            g.hidden = v;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn imported_wrapper_flags_become_typed_fields() {
        let r: Runner = serde_json::from_value(json!({ "mangohud": true, "gamescope": { "enabled": true, "width": 2560, "fsr": true } })).unwrap();
        assert_eq!((r.mangohud, r.gamemode), (Some(true), None));
        let gs = r.gamescope.unwrap();
        assert!(gs.enabled && gs.fsr && !gs.hdr);
        assert_eq!(gs.width, Some(2560));
        assert!(r.extra.is_empty());
    }

    #[test]
    fn runner_keeps_unknown_options_and_skips_empty_ones() {
        let r: Runner = serde_json::from_value(json!({ "proton": "GE-Proton10-20", "vkBasalt": { "cas": 0.5 } })).unwrap();
        assert_eq!(r.proton.as_deref(), Some("GE-Proton10-20"));
        assert_eq!(serde_json::to_value(&r).unwrap(), json!({ "proton": "GE-Proton10-20", "vkBasalt": { "cas": 0.5 } }));
        assert_eq!(serde_json::to_value(Runner::default()).unwrap(), json!({}));
    }

    #[test]
    fn patch_tells_null_from_missing() {
        let p: GamePatch = serde_json::from_value(json!({ "exe": null, "favorite": true })).unwrap();
        assert_eq!(p.exe, Some(None));
        assert_eq!(p.prefix, None);
        assert!(serde_json::from_value::<GamePatch>(json!({ "playtime": 5 })).is_err());
    }

    #[test]
    fn kinds_parse_from_their_names() {
        assert_eq!(Kind::parse("hoyo"), Some(Kind::Hoyo));
        assert_eq!(Kind::parse("Hoyo"), None);
        assert_eq!(SourceKind::parse(SourceKind::Lutris.as_str()), Some(SourceKind::Lutris));
    }
}
