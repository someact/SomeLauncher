//! What an install folder says about itself (6b): which game and region
//! (the exe, ZZZ's `version_info`), the installed version and the voice packs.
//! The version is Unity's bundle version in `<Game>_Data/globalgamemanagers`
//! (`7.1.0_48052158_48145775` → `7.1.0`): the official launcher's `config.ini`
//! is not kept current by other launchers, so it is only a fallback.

use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Install {
    pub biz: String,
    pub dir: PathBuf,
    pub exe: PathBuf,
    pub version: Option<String>,
    /// Voice packs, as HoYoPlay names them (`en-us`, `ja-jp`, …)
    pub voices: Vec<String>,
}

/// The `<Game>_Data` folder of an exe
fn data_dir(exe: &Path) -> PathBuf {
    let stem = exe.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    exe.with_file_name(format!("{stem}_Data"))
}

/// A game folder, or its exe: which game it holds
pub fn detect(path: &Path) -> Option<Install> {
    let dir = if path.is_file() { path.parent()?.to_path_buf() } else { path.to_path_buf() };
    let (mut biz, exe) = super::EXES.iter().find_map(|(biz, exe)| {
        let p = dir.join(exe);
        p.is_file().then(|| (biz.to_string(), p))
    })?;
    // ZZZ's exe is the same in both regions: `version_info` starts with OSPROD (global) or CNPROD
    if biz.starts_with("nap_")
        && let Ok(info) = std::fs::read(dir.join("version_info"))
        && info.starts_with(b"CNPROD")
    {
        biz = "nap_cn".into();
    }
    let data = data_dir(&exe);
    let version = std::fs::read(data.join("globalgamemanagers"))
        .ok()
        .and_then(|b| bundle_version(&b))
        .or_else(|| config_version(&dir));
    let voices = voices(&data);
    Some(Install { biz, dir, exe, version, voices })
}

/// Unity's bundle version: a length-prefixed string after `01 01 01 01 01 00 00 00`
pub fn bundle_version(bytes: &[u8]) -> Option<String> {
    const MARK: &[u8] = &[1, 1, 1, 1, 1, 0, 0, 0];
    let mut at = 0;
    while let Some(i) = bytes[at..].windows(MARK.len()).position(|w| w == MARK) {
        let start = at + i + MARK.len();
        at = start;
        let Some(len) = bytes.get(start..start + 4).map(|l| u32::from_le_bytes(l.try_into().unwrap()) as usize) else { break };
        if !(5..=64).contains(&len) {
            continue;
        }
        let Some(s) = bytes.get(start + 4..start + 4 + len).and_then(|s| std::str::from_utf8(s).ok()) else { continue };
        let version = s.split('_').next().unwrap_or(s);
        let parts: Vec<&str> = version.split('.').collect();
        if parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())) {
            return Some(version.to_string());
        }
    }
    None
}

/// `game_version=` in the official launcher's `config.ini`
fn config_version(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("config.ini")).ok()?;
    text.lines().find_map(|l| l.trim().strip_prefix("game_version=")).map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// The voice packs a game lists in its Persistent folder
fn voices(data: &Path) -> Vec<String> {
    let persistent = data.join("Persistent");
    let Some(text) = ["audio_lang_14", "audio_lang_launcher"].iter().find_map(|f| std::fs::read_to_string(persistent.join(f)).ok()) else {
        return Vec::new();
    };
    let mut out: Vec<String> = text.lines().filter_map(|l| voice_code(l.trim())).map(String::from).collect();
    out.dedup();
    out
}

/// The games' folder names for voice packs → HoYoPlay's codes
pub fn voice_code(name: &str) -> Option<&'static str> {
    Some(match name {
        "Chinese" => "zh-cn",
        "English(US)" => "en-us",
        "Japanese" => "ja-jp",
        "Korean" => "ko-kr",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unity(version: &str) -> Vec<u8> {
        let mut b = b"junk\x01\x01\x01\x01\x01\x00\x00\x00\x02\x00\x00\x00ab".to_vec();
        b.extend_from_slice(&[1, 1, 1, 1, 1, 0, 0, 0]);
        b.extend_from_slice(&(version.len() as u32).to_le_bytes());
        b.extend_from_slice(version.as_bytes());
        b.extend_from_slice(b"\0\0\0more");
        b
    }

    #[test]
    fn the_bundle_version_without_its_build_numbers() {
        assert_eq!(bundle_version(&unity("7.1.0_48052158_48145775")).as_deref(), Some("7.1.0"));
        assert_eq!(bundle_version(&unity("3.2.0")).as_deref(), Some("3.2.0"));
        assert_eq!(bundle_version(&unity("not.a.version")), None);
    }

    #[test]
    fn detects_the_game_region_version_and_voices() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path();
        std::fs::write(dir.join("ZenlessZoneZero.exe"), b"MZ").unwrap();
        std::fs::write(dir.join("version_info"), b"CNPRODWin3.2.0").unwrap();
        let data = dir.join("ZenlessZoneZero_Data");
        std::fs::create_dir_all(data.join("Persistent")).unwrap();
        std::fs::write(data.join("globalgamemanagers"), unity("3.2.0")).unwrap();
        std::fs::write(data.join("Persistent/audio_lang_launcher"), "English(US)\nJapanese\n").unwrap();
        let i = detect(&dir.join("ZenlessZoneZero.exe")).unwrap();
        assert_eq!((i.biz.as_str(), i.version.as_deref(), i.voices), ("nap_cn", Some("3.2.0"), vec!["en-us".to_string(), "ja-jp".to_string()]));

        // Without Unity's file: the official config.ini
        let t2 = tempfile::tempdir().unwrap();
        std::fs::write(t2.path().join("GenshinImpact.exe"), b"MZ").unwrap();
        std::fs::write(t2.path().join("config.ini"), "[General]\ngame_version=7.0.0\n").unwrap();
        let g = detect(t2.path()).unwrap();
        assert_eq!((g.biz.as_str(), g.version.as_deref()), ("hk4e_global", Some("7.0.0")));
        assert_eq!(detect(&dir.join("nothing")), None);
    }
}
