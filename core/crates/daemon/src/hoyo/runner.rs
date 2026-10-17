//! How Genshin Impact and ZZZ run on Linux (6f), as Twintail sets them up
//! (its game manifests, `extra.compat_overrides`, read 2026-10-17): Proton's
//! own options instead of protonfixes, Xalia off (it pops a gamepad overlay
//! over the game's login), ZZZ with its game folder as a drive and its
//! graphics API chosen by an argument, and the oldest Proton builds Genshin
//! still runs on. A game's own runner env and args win over these.

use anyhow::{Result, bail};

/// Env for every Hoyo game, plus each game's own
pub fn env(biz: &str) -> Vec<(&'static str, &'static str)> {
    // protonfixes would apply fixes meant for other games; Proton's options are set directly
    let mut env = vec![("PROTONFIXES_DISABLE", "1"), ("PROTON_USE_XALIA", "0")];
    if super::api::game_of(biz) == "nap" {
        env.push(("PROTON_SET_GAME_DRIVE", "1"));
    }
    env
}

/// ZZZ's graphics API: `-use-d3d11` (Twintail's default) or `-use-d3d12`
pub const GRAPHICS_APIS: &[(&str, &str)] = &[("dx11", "-use-d3d11"), ("dx12", "-use-d3d12")];

/// Arguments a game gets unless its own say otherwise: ZZZ's graphics API
/// (`runner.graphicsApi`, else DirectX 11)
pub fn args(biz: &str, own_args: &[String], graphics_api: Option<&str>) -> Vec<String> {
    if super::api::game_of(biz) != "nap" || own_args.iter().any(|a| a.starts_with("-use-d3d")) {
        return Vec::new();
    }
    let flag = GRAPHICS_APIS.iter().find(|(k, _)| Some(*k) == graphics_api).map(|(_, f)| *f).unwrap_or("-use-d3d11");
    vec![flag.to_string()]
}

/// The oldest Proton of each family a game runs on (Twintail's `min_runner_versions`)
fn minimums(biz: &str) -> &'static [&'static str] {
    match super::api::game_of(biz) {
        "hk4e" => &["10.26-proton-ge", "10.0-20251222-proton-cachyos", "10.0-33-proton-em"],
        _ => &[],
    }
}

/// A Proton build's family and version numbers, from its folder or Twintail's
/// name: `GE-Proton10-26` and `10.26-proton-ge` are both (ge, [10, 26])
pub fn proton_version(name: &str) -> Option<(&'static str, Vec<u64>)> {
    let lower = name.to_lowercase().replace("x86_64", "").replace("x86-64", "");
    let family = if lower.contains("cachyos") {
        "cachyos"
    } else if lower.contains("-em") || lower.starts_with("em-") || lower.contains("proton-em") {
        "em"
    } else if lower.contains("ge-proton") || lower.contains("proton-ge") {
        "ge"
    } else {
        return None;
    };
    let nums: Vec<u64> = lower.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect();
    (!nums.is_empty()).then_some((family, nums))
}

/// Refuses a Proton build older than the game needs (unknown builds pass)
pub fn check_proton(biz: &str, game: &str, proton: &str) -> Result<()> {
    let Some((family, have)) = proton_version(proton) else { return Ok(()) };
    for min in minimums(biz) {
        if let Some((f, need)) = proton_version(min)
            && f == family
            && have < need
        {
            bail!("{game} needs {family} Proton {} or newer; {proton} is older (pick another in its settings)", min.split("-proton").next().unwrap_or(min));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proton_versions_from_both_namings() {
        assert_eq!(proton_version("GE-Proton10-26"), Some(("ge", vec![10, 26])));
        assert_eq!(proton_version("GE-Proton11-7-x86_64"), Some(("ge", vec![11, 7])));
        assert_eq!(proton_version("10.26-proton-ge"), Some(("ge", vec![10, 26])));
        assert_eq!(proton_version("cachyos-11.0-20260703-slr"), Some(("cachyos", vec![11, 0, 20260703])));
        assert_eq!(proton_version("11.0-20260703-proton-cachyos"), Some(("cachyos", vec![11, 0, 20260703])));
        assert_eq!(proton_version("10.0-33-proton-em"), Some(("em", vec![10, 0, 33])));
        assert_eq!(proton_version("UMU-Proton-10.0-4"), None);
    }

    #[test]
    fn genshin_refuses_old_proton_and_zzz_gets_its_api() {
        assert!(check_proton("hk4e_global", "Genshin Impact", "GE-Proton10-20").is_err());
        assert!(check_proton("hk4e_global", "Genshin Impact", "GE-Proton10-26").is_ok());
        assert!(check_proton("hk4e_global", "Genshin Impact", "cachyos-10.0-20251101-slr").is_err());
        assert!(check_proton("hk4e_global", "Genshin Impact", "UMU-Proton-9.0").is_ok());
        assert!(check_proton("nap_global", "Zenless Zone Zero", "GE-Proton9-1").is_ok());
        assert_eq!(args("nap_global", &[], None), ["-use-d3d11"]);
        assert_eq!(args("nap_global", &[], Some("dx12")), ["-use-d3d12"]);
        assert!(args("nap_global", &["-use-d3d12".into()], Some("dx11")).is_empty(), "the game's own flag wins");
        assert!(args("hk4e_global", &[], Some("dx12")).is_empty());
        assert!(env("nap_global").contains(&("PROTON_SET_GAME_DRIVE", "1")));
    }
}
