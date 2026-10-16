//! The Steam store: search and app details (no key needed), and the art every
//! Steam app has on Steam's CDN.

use anyhow::{Context, Result};
use serde::Deserialize;

const CDN: &str = "https://shared.steamstatic.com/store_item_assets/steam/apps";

/// Art on Steam's CDN by app id: `library_hero.jpg`, `library_600x900_2x.jpg`, `logo.png`, `header.jpg`.
pub fn art_url(appid: &str, file: &str) -> String {
    format!("{CDN}/{appid}/{file}")
}

#[derive(Debug, Clone, Deserialize)]
pub struct Hit {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub tiny_image: Option<String>,
}

pub async fn search(http: &reqwest::Client, term: &str) -> Result<Vec<Hit>> {
    #[derive(Deserialize)]
    struct Body {
        #[serde(default)]
        items: Vec<Hit>,
    }
    let body: Body = http
        .get("https://store.steampowered.com/api/storesearch/")
        .query(&[("term", term), ("l", "english"), ("cc", "US")])
        .send()
        .await?
        .error_for_status()
        .context("Steam store search")?
        .json()
        .await?;
    Ok(body.items)
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Details {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub short_description: String,
    #[serde(default)]
    pub genres: Vec<Described>,
    #[serde(default)]
    pub categories: Vec<Described>,
    #[serde(default)]
    pub developers: Vec<String>,
    #[serde(default)]
    pub publishers: Vec<String>,
    #[serde(default)]
    pub release_date: Option<Release>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Described {
    pub description: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Release {
    #[serde(default)]
    pub date: String,
}

/// Steam's user tags for an app, most used first (SteamSpy; 1 request a second at most).
pub async fn user_tags(http: &reqwest::Client, appid: &str) -> Result<Vec<String>> {
    #[derive(Deserialize)]
    struct Body {
        #[serde(default)]
        tags: serde_json::Value,
    }
    let body: Body = http
        .get("https://steamspy.com/api.php")
        .query(&[("request", "appdetails"), ("appid", appid)])
        .send()
        .await?
        .error_for_status()
        .context("SteamSpy")?
        .json()
        .await?;
    // `{"Horror": 812, …}`, or `[]` when it has none
    let mut tags: Vec<(String, i64)> =
        body.tags.as_object().into_iter().flatten().map(|(k, v)| (k.clone(), v.as_i64().unwrap_or(0))).collect();
    tags.sort_by_key(|t| std::cmp::Reverse(t.1));
    Ok(tags.into_iter().map(|t| t.0).collect())
}

/// App details, or None when Steam has none (delisted, region locked).
pub async fn details(http: &reqwest::Client, appid: &str) -> Result<Option<Details>> {
    #[derive(Deserialize)]
    struct Entry {
        success: bool,
        #[serde(default)]
        data: Option<Details>,
    }
    let body: std::collections::HashMap<String, Entry> = http
        .get("https://store.steampowered.com/api/appdetails")
        .query(&[("appids", appid), ("l", "english")])
        .send()
        .await?
        .error_for_status()
        .context("Steam app details")?
        .json()
        .await?;
    Ok(body.into_values().next().filter(|e| e.success).and_then(|e| e.data))
}
