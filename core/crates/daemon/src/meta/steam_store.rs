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
