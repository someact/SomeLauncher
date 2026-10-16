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
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SourceKind::Steam => "steam",
            SourceKind::Lutris => "lutris",
            SourceKind::Twintail => "twintail",
            SourceKind::Prism => "prism",
            SourceKind::Manual => "manual",
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
    /// Options added by later phases (wrappers, scripts) that this build does not know yet
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

fn is_false(b: &bool) -> bool {
    !*b
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
    /// Automatic labels (genres, `co-op`, `needs steam`, …)
    pub tags: Vec<String>,
    /// User-facing library rows
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
    pub tags: Option<Vec<String>>,
    pub categories: Option<Vec<String>>,
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
        if let Some(v) = self.tags {
            g.tags = v;
        }
        if let Some(v) = self.categories {
            g.categories = v;
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
    fn runner_keeps_unknown_options_and_skips_empty_ones() {
        let r: Runner = serde_json::from_value(json!({ "proton": "GE-Proton10-20", "gamescope": { "w": 1920 } })).unwrap();
        assert_eq!(r.proton.as_deref(), Some("GE-Proton10-20"));
        assert_eq!(serde_json::to_value(&r).unwrap(), json!({ "proton": "GE-Proton10-20", "gamescope": { "w": 1920 } }));
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
