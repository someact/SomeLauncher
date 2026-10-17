//! The HoYoPlay launcher API (6a): the games with their art, the current
//! branch (version, the versions a diff update starts from, a preload), the
//! launcher backgrounds (some are videos), and the news, events and HoYoLAB
//! links. Global (`*_global`) and CN (`*_cn`) are separate launchers with
//! their own host and id; a game's biz says which one it belongs to.
//! Answers are kept for a while ([`TTL`]): the launcher asks often, they change rarely.

use anyhow::{Context as _, Result, bail};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The games SomeLauncher installs and runs; others in the API are skipped
pub const SUPPORTED: &[&str] = &["hk4e", "nap"];
const TTL: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Region {
    Global,
    Cn,
}

impl Region {
    pub fn of(biz: &str) -> Region {
        if biz.ends_with("_cn") { Region::Cn } else { Region::Global }
    }
    fn host(self) -> &'static str {
        match self {
            Region::Global => "https://sg-hyp-api.hoyoverse.com/hyp/hyp-connect/api",
            Region::Cn => "https://hyp-api.mihoyo.com/hyp/hyp-connect/api",
        }
    }
    fn launcher(self) -> &'static str {
        match self {
            Region::Global => "VYTpXlbWo8",
            Region::Cn => "jGHBHlcOq1",
        }
    }
    /// Sophon's host (getBuild, getPatchBuild)
    pub fn sophon(self) -> &'static str {
        match self {
            Region::Global => "https://sg-public-api.hoyoverse.com/downloader/sophon_chunk/api",
            Region::Cn => "https://downloader-api.mihoyo.com/downloader/sophon_chunk/api",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Branch {
    pub package_id: String,
    pub branch: String,
    pub password: String,
    /// The version (`7.1.0`)
    pub tag: String,
    /// Versions a diff update can start from
    pub diff_tags: Vec<String>,
    /// Voice packs offered (`en-us`, `ja-jp`, …)
    pub voices: Vec<String>,
    /// The resource categories a full install has (`game`, and ZZZ's numbered ones)
    pub resources: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Background {
    pub image: String,
    /// A webm loop, played over `image` once it loads
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video: Option<String>,
    /// A transparent picture laid over the video
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HoyoGame {
    pub biz: String,
    pub id: String,
    pub name: String,
    pub region: Region,
    pub logo: String,
    pub icon: String,
    /// The game's own wide art (the launcher's default background)
    pub background: String,
    /// The launcher's backgrounds for this version, the first is the current one
    pub backgrounds: Vec<Background>,
    pub main: Option<Branch>,
    /// The next version, while it can be preloaded
    pub preload: Option<Branch>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Post {
    /// `info`, `activity` (events) or `announce`
    pub kind: String,
    pub title: String,
    pub link: String,
    pub date: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    pub title: String,
    pub link: String,
    pub icon: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Content {
    pub banners: Vec<Link>,
    pub posts: Vec<Post>,
    /// The game's HoYoLAB page (the first social link that goes there)
    pub hoyolab: Option<String>,
    pub social: Vec<Link>,
}

fn str_at(v: &Value, path: &[&str]) -> String {
    path.iter().fold(v, |v, k| &v[*k]).as_str().unwrap_or_default().to_string()
}
fn opt(s: String) -> Option<String> {
    (!s.is_empty()).then_some(s)
}

/// The game of a biz (`hk4e_global` → `hk4e`)
pub fn game_of(biz: &str) -> &str {
    biz.split('_').next().unwrap_or(biz)
}

async fn get(http: &reqwest::Client, region: Region, method: &str, query: &[(&str, &str)]) -> Result<Value> {
    let url = format!("{}/{method}", region.host());
    let mut q = vec![("launcher_id", region.launcher())];
    q.extend_from_slice(query);
    let v: Value = http.get(&url).query(&q).send().await?.error_for_status()?.json().await.with_context(|| format!("HoYoPlay {method}"))?;
    if v["retcode"].as_i64() != Some(0) {
        bail!("HoYoPlay {method}: {}", v["message"].as_str().unwrap_or("failed"));
    }
    Ok(v["data"].clone())
}

pub fn parse_branch(b: &Value) -> Option<Branch> {
    let tag = b["tag"].as_str()?.to_string();
    Some(Branch {
        package_id: str_at(b, &["package_id"]),
        branch: str_at(b, &["branch"]),
        password: str_at(b, &["password"]),
        tag,
        diff_tags: b["diff_tags"].as_array().into_iter().flatten().filter_map(|t| t.as_str().map(String::from)).collect(),
        voices: categories(b, "CATEGORY_TYPE_AUDIO"),
        resources: categories(b, "CATEGORY_TYPE_RESOURCE"),
    })
}

/// A branch's categories of one type that a full install takes
fn categories(b: &Value, kind: &str) -> Vec<String> {
    b["categories"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| c["type"] == kind && c["scenarios"].as_array().is_none_or(|s| s.iter().any(|s| s == "CATEGORY_SCENARIO_FULL")))
        .filter_map(|c| c["matching_field"].as_str().map(String::from))
        .collect()
}

pub fn parse_backgrounds(info: &Value) -> Vec<Background> {
    info["backgrounds"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|b| {
            let image = opt(str_at(b, &["background", "url"]))?;
            Some(Background { image, video: opt(str_at(b, &["video", "url"])), theme: opt(str_at(b, &["theme", "url"])) })
        })
        .collect()
}

pub fn parse_content(c: &Value) -> Content {
    let banners = c["banners"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|b| Link { title: String::new(), link: str_at(b, &["image", "link"]), icon: str_at(b, &["image", "url"]) })
        .collect();
    let posts = c["posts"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|p| Post {
            kind: str_at(p, &["type"]).trim_start_matches("POST_TYPE_").to_lowercase(),
            title: str_at(p, &["title"]),
            link: str_at(p, &["link"]),
            date: str_at(p, &["date"]),
        })
        .collect();
    let mut social = Vec::new();
    for s in c["social_media_list"].as_array().into_iter().flatten() {
        let icon = str_at(s, &["icon", "url"]);
        for l in s["links"].as_array().into_iter().flatten() {
            social.push(Link { title: str_at(l, &["title"]), link: str_at(l, &["link"]), icon: icon.clone() });
        }
    }
    let hoyolab = social.iter().find(|l| l.link.contains("hoyolab.com") || l.link.contains("miyoushe.com")).map(|l| l.link.clone());
    Content { banners, posts, hoyolab, social }
}

// --- cached fetches

type Cache<T> = Mutex<HashMap<String, (Instant, T)>>;
static GAMES: std::sync::LazyLock<Cache<Vec<HoyoGame>>> = std::sync::LazyLock::new(Default::default);
static CONTENT: std::sync::LazyLock<Cache<Content>> = std::sync::LazyLock::new(Default::default);

fn cached<T: Clone>(cache: &Cache<T>, key: &str, fresh: bool) -> Option<T> {
    let c = cache.lock().unwrap();
    c.get(key).filter(|(at, _)| !fresh || at.elapsed() < TTL).map(|(_, v)| v.clone())
}

/// The supported games of a region, with their branches and backgrounds
pub async fn games(http: &reqwest::Client, region: Region, language: &str, force: bool) -> Result<Vec<HoyoGame>> {
    let key = format!("{region:?}/{language}");
    if !force && let Some(g) = cached(&GAMES, &key, true) {
        return Ok(g);
    }
    let fetched = fetch_games(http, region, language).await;
    match fetched {
        Ok(g) => {
            GAMES.lock().unwrap().insert(key, (Instant::now(), g.clone()));
            Ok(g)
        }
        // Offline: what we had, however old
        Err(e) => cached(&GAMES, &key, false).ok_or(e),
    }
}

async fn fetch_games(http: &reqwest::Client, region: Region, language: &str) -> Result<Vec<HoyoGame>> {
    let lang = [("language", language)];
    let (list, branches, info) = tokio::try_join!(
        get(http, region, "getGames", &lang),
        get(http, region, "getGameBranches", &[]),
        get(http, region, "getAllGameBasicInfo", &lang),
    )?;
    let mut out = Vec::new();
    for g in list["games"].as_array().into_iter().flatten() {
        let biz = str_at(g, &["biz"]);
        if !SUPPORTED.contains(&game_of(&biz)) {
            continue;
        }
        let id = str_at(g, &["id"]);
        let branch = branches["game_branches"].as_array().into_iter().flatten().find(|b| b["game"]["id"] == id.as_str());
        let backgrounds = info["game_info_list"].as_array().into_iter().flatten().find(|i| i["game"]["id"] == id.as_str()).map(parse_backgrounds).unwrap_or_default();
        let d = &g["display"];
        out.push(HoyoGame {
            name: str_at(d, &["name"]),
            logo: str_at(d, &["logo", "url"]),
            icon: str_at(d, &["icon", "url"]),
            background: str_at(d, &["background", "url"]),
            backgrounds,
            main: branch.and_then(|b| parse_branch(&b["main"])),
            preload: branch.and_then(|b| parse_branch(&b["pre_download"])),
            region,
            biz,
            id,
        });
    }
    Ok(out)
}

/// One game's news, events, banners and links
pub async fn content(http: &reqwest::Client, biz: &str, language: &str, force: bool) -> Result<Content> {
    let key = format!("{biz}/{language}");
    if !force && let Some(c) = cached(&CONTENT, &key, true) {
        return Ok(c);
    }
    let region = Region::of(biz);
    let game = games(http, region, language, false).await?.into_iter().find(|g| g.biz == biz).with_context(|| format!("HoYoPlay has no game {biz}"))?;
    match get(http, region, "getGameContent", &[("language", language), ("game_id", &game.id)]).await {
        Ok(v) => {
            let mut c = parse_content(&v["content"]);
            c.hoyolab = c.hoyolab.or_else(|| community(biz).map(String::from));
            CONTENT.lock().unwrap().insert(key, (Instant::now(), c.clone()));
            Ok(c)
        }
        Err(e) => cached(&CONTENT, &key, false).ok_or(e),
    }
}

/// The game's community page, for games whose launcher content has no link to it
pub fn community(biz: &str) -> Option<&'static str> {
    Some(match biz {
        "hk4e_global" => "https://www.hoyolab.com/circles/2",
        "nap_global" => "https://www.hoyolab.com/circles/8",
        "hk4e_cn" => "https://www.miyoushe.com/ys/",
        "nap_cn" => "https://www.miyoushe.com/zzz/",
        _ => return None,
    })
}

/// The API's language for a locale (`th_TH.UTF-8` → `th-th`), English when it has none
pub fn language(locale: &str) -> String {
    const KNOWN: &[&str] = &["en-us", "zh-cn", "zh-tw", "ja-jp", "ko-kr", "de-de", "es-es", "fr-fr", "id-id", "it-it", "pt-pt", "ru-ru", "th-th", "tr-tr", "vi-vn"];
    let l = locale.split('.').next().unwrap_or("").replace('_', "-").to_lowercase();
    if KNOWN.contains(&l.as_str()) {
        return l;
    }
    let lang = l.split('-').next().unwrap_or("");
    KNOWN.iter().find(|k| k.starts_with(&format!("{lang}-"))).map(|k| k.to_string()).unwrap_or_else(|| "en-us".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn branches_with_their_voice_packs() {
        let b = json!({"package_id": "p", "branch": "main", "password": "x", "tag": "7.1.0", "diff_tags": ["7.0.0"],
            "categories": [{"matching_field": "game", "type": "CATEGORY_TYPE_RESOURCE"}, {"matching_field": "en-us", "type": "CATEGORY_TYPE_AUDIO"}]});
        let b = parse_branch(&b).unwrap();
        assert_eq!((b.tag.as_str(), b.diff_tags, b.voices), ("7.1.0", vec!["7.0.0".to_string()], vec!["en-us".to_string()]));
        assert_eq!(b.resources, vec!["game".to_string()]);
        assert_eq!(parse_branch(&Value::Null), None);
    }

    #[test]
    fn backgrounds_keep_their_video_and_theme() {
        let info = json!({"backgrounds": [
            {"background": {"url": "a.webp"}, "video": {"url": "v.webm"}, "theme": {"url": "t.webp"}},
            {"background": {"url": "b.webp"}, "video": {"url": ""}, "theme": {"url": ""}}]});
        let b = parse_backgrounds(&info);
        assert_eq!(b[0].video.as_deref(), Some("v.webm"));
        assert_eq!((b[1].image.as_str(), b[1].video.as_ref()), ("b.webp", None));
    }

    #[test]
    fn content_finds_the_hoyolab_link() {
        let c = json!({"banners": [{"image": {"url": "i.jpg", "link": "l"}}],
            "posts": [{"type": "POST_TYPE_ACTIVITY", "title": "Event", "link": "e", "date": "10/01"}],
            "social_media_list": [{"icon": {"url": "x.png"}, "links": [{"title": "HoYoLAB Community", "link": "https://www.hoyolab.com/home"}]}]});
        let c = parse_content(&c);
        assert_eq!((c.posts[0].kind.as_str(), c.hoyolab.as_deref()), ("activity", Some("https://www.hoyolab.com/home")));
        assert_eq!(c.banners[0].icon, "i.jpg");
    }

    #[test]
    fn languages_from_locales() {
        assert_eq!(language("th_TH.UTF-8"), "th-th");
        assert_eq!(language("de_AT.UTF-8"), "de-de");
        assert_eq!(language("C.UTF-8"), "en-us");
        assert_eq!(Region::of("nap_cn"), Region::Cn);
        assert_eq!(game_of("hk4e_global"), "hk4e");
    }
}
