//! Steam achievements (7a). Two sources, the same list:
//! - Steam's own cache on this PC, no key needed: `appcache/stats/UserGameStatsSchema_<appid>.bin`
//!   (every achievement: names and descriptions in each language, icon files)
//!   and `UserGameStats_<account>_<appid>.bin` (a bit per unlocked achievement,
//!   with its unlock time). Steam writes them for games played on this PC.
//! - The Web API with the user's key: `GetSchemaForGame` and
//!   `GetPlayerAchievements` (the profile's game details must be public); it
//!   also knows games never played here.

use super::Achievement;
use crate::import::vdf::{self, Vdf};
use anyhow::{Context as _, Result, bail};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const CDN: &str = "https://cdn.akamai.steamstatic.com/steamcommunity/public/images/apps";
/// SteamID64 = this + the 32-bit account id
const STEAMID64_BASE: u64 = 76561197960265728;

pub fn account_id(steam_id64: &str) -> Option<u32> {
    steam_id64.parse::<u64>().ok()?.checked_sub(STEAMID64_BASE).and_then(|a| u32::try_from(a).ok())
}

/// The Steam client's language (`registry.vdf`), for the cache's texts and the Web API
pub fn language(home: &Path) -> String {
    [".steam/registry.vdf", ".var/app/com.valvesoftware.Steam/.steam/registry.vdf"]
        .iter()
        .filter_map(|p| std::fs::read_to_string(home.join(p)).ok())
        .filter_map(|t| vdf::parse(&t).ok())
        .find_map(|v| v.str_at(&["Registry", "HKCU", "Software", "Valve", "Steam", "language"]).map(String::from))
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| "english".into())
}

fn stats_dir(root: &Path) -> PathBuf {
    root.join("appcache/stats")
}

/// A text in the language, else English, else any
fn text(v: Option<&Vdf>, lang: &str) -> String {
    let Some(v) = v else { return String::new() };
    if let Some(s) = v.str() {
        return s.to_string();
    }
    v.str_at(&[lang])
        .or_else(|| v.str_at(&["english"]))
        .or_else(|| v.entries().iter().find(|(k, _)| k != "token").and_then(|(_, v)| v.str()))
        .unwrap_or_default()
        .to_string()
}

/// The achievements in Steam's cache, with what is unlocked; None without a schema file
pub fn local(root: &Path, account: Option<u32>, appid: &str, lang: &str) -> Result<Option<Vec<Achievement>>> {
    let dir = stats_dir(root);
    let schema = dir.join(format!("UserGameStatsSchema_{appid}.bin"));
    let Ok(bytes) = std::fs::read(&schema) else { return Ok(None) };
    let schema = vdf::parse_binary(&bytes).with_context(|| format!("reading {}", schema.display()))?;
    let user = account
        .and_then(|a| std::fs::read(dir.join(format!("UserGameStats_{a}_{appid}.bin"))).ok())
        .map(|b| vdf::parse_binary(&b))
        .transpose()
        .context("reading Steam's achievement progress")?;
    Ok(Some(from_cache(&schema, user.as_ref(), appid, lang)))
}

pub fn from_cache(schema: &Vdf, user: Option<&Vdf>, appid: &str, lang: &str) -> Vec<Achievement> {
    let stats = schema.at(&[appid, "stats"]).or_else(|| schema.entries().first().and_then(|(_, v)| v.get("stats")));
    let Some(stats) = stats else { return Vec::new() };
    let num = |s: &str| s.parse::<i64>().unwrap_or(i64::MAX);
    let mut stat_ids: Vec<&(String, Vdf)> = stats.entries().iter().collect();
    stat_ids.sort_by_key(|(k, _)| num(k));
    let mut out = Vec::new();
    for (stat_id, stat) in stat_ids {
        // `ACHIEVEMENTS`, or 4 in older schemas
        let kind = stat.str_at(&["type"]).unwrap_or_default();
        if kind != "ACHIEVEMENTS" && kind != "4" {
            continue;
        }
        let progress = user.and_then(|u| u.at(&["cache", stat_id]));
        let data = progress.and_then(|p| p.str_at(&["data"])).and_then(|d| d.parse::<i64>().ok()).unwrap_or(0) as u32;
        let mut bits: Vec<&(String, Vdf)> = stat.get("bits").map(|b| b.entries().iter().collect()).unwrap_or_default();
        bits.sort_by_key(|(k, _)| num(k));
        for (bit, a) in bits {
            let Ok(n) = bit.parse::<u32>() else { continue };
            let Some(api_name) = a.str_at(&["name"]) else { continue };
            let display = a.get("display");
            let icon = |k: &str| display.and_then(|d| d.str_at(&[k])).filter(|i| !i.is_empty()).map(|i| format!("{CDN}/{appid}/{i}"));
            let unlocked = n < 32 && data & (1 << n) != 0;
            let at = progress.and_then(|p| p.str_at(&["AchievementTimes", bit])).and_then(|t| t.parse::<i64>().ok()).filter(|t| *t > 0);
            out.push(Achievement {
                api_name: api_name.to_string(),
                name: text(display.and_then(|d| d.get("name")), lang),
                description: text(display.and_then(|d| d.get("desc")), lang),
                hidden: display.and_then(|d| d.str_at(&["hidden"])) == Some("1"),
                icon: icon("icon"),
                icon_gray: icon("icon_gray"),
                // An unlock without a time is still an unlock
                unlocked_at: if unlocked { Some(at.unwrap_or(0)) } else { None },
            });
        }
    }
    out
}

/// The list from the Web API; Ok(None) when the game has no achievements
pub async fn api(http: &reqwest::Client, key: &str, steam_id: &str, appid: &str, lang: &str) -> Result<Option<Vec<Achievement>>> {
    let schema: serde_json::Value = http
        .get("https://api.steampowered.com/ISteamUserStats/GetSchemaForGame/v2/")
        .query(&[("key", key), ("appid", appid), ("l", lang)])
        .send()
        .await?
        .error_for_status()
        .context("Steam Web API GetSchemaForGame (is the key right?)")?
        .json()
        .await?;
    let list = schema["game"]["availableGameStats"]["achievements"].as_array().cloned().unwrap_or_default();
    if list.is_empty() {
        return Ok(None);
    }
    let player: serde_json::Value = http
        .get("https://api.steampowered.com/ISteamUserStats/GetPlayerAchievements/v1/")
        .query(&[("key", key), ("steamid", steam_id), ("appid", appid)])
        .send()
        .await?
        .json()
        .await
        .context("Steam Web API GetPlayerAchievements")?;
    let stats = &player["playerstats"];
    if stats["success"] != true {
        bail!("Steam Web API: {} (the profile's game details must be public)", stats["error"].as_str().unwrap_or("no achievements for this account"));
    }
    let unlocked: HashMap<&str, i64> = stats["achievements"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|a| a["achieved"].as_i64() == Some(1))
        .filter_map(|a| Some((a["apiname"].as_str()?, a["unlocktime"].as_i64().unwrap_or(0))))
        .collect();
    let s = |v: &serde_json::Value| v.as_str().filter(|s| !s.is_empty()).map(String::from);
    Ok(Some(
        list.iter()
            .filter_map(|a| {
                let api_name = a["name"].as_str()?.to_string();
                Some(Achievement {
                    name: s(&a["displayName"]).unwrap_or_else(|| api_name.clone()),
                    description: s(&a["description"]).unwrap_or_default(),
                    hidden: a["hidden"].as_i64() == Some(1),
                    icon: s(&a["icon"]),
                    icon_gray: s(&a["icongray"]),
                    unlocked_at: unlocked.get(api_name.as_str()).copied(),
                    api_name,
                })
            })
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cache_gives_names_icons_and_unlocks() {
        let schema = vdf::parse(
            r#""480" { "stats" {
                "1" { "type" "ACHIEVEMENTS" "bits" {
                    "0" { "name" "WIN" "display" { "name" { "english" "Winner" "thai" "ผู้ชนะ" "token" "T" } "desc" { "english" "Win once" } "hidden" "0" "icon" "a.jpg" "icon_gray" "b.jpg" } }
                    "1" { "name" "SECRET" "display" { "name" { "english" "Secret" } "desc" { "english" "?" } "hidden" "1" } }
                    "2" { "name" "LOST" "display" { "name" { "english" "Lost" } } }
                } }
                "2" { "type" "INT" }
            } }"#,
        )
        .unwrap();
        let user = vdf::parse(r#""cache" { "1" { "data" "5" "AchievementTimes" { "0" "1700000000" } } }"#).unwrap();
        let a = from_cache(&schema, Some(&user), "480", "thai");
        assert_eq!(a.len(), 3);
        assert_eq!((a[0].name.as_str(), a[0].unlocked_at), ("ผู้ชนะ", Some(1700000000)));
        assert_eq!(a[0].icon.as_deref(), Some("https://cdn.akamai.steamstatic.com/steamcommunity/public/images/apps/480/a.jpg"));
        assert_eq!((a[1].hidden, a[1].unlocked_at), (true, None));
        assert_eq!(a[2].unlocked_at, Some(0), "bit 2 is set, without a time");
        assert!(from_cache(&schema, None, "480", "english").iter().all(|a| a.unlocked_at.is_none()));
        assert_eq!(account_id("76561199521830285"), Some(1561564557));
    }
}
