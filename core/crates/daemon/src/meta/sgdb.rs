//! SteamGridDB (needs the user's API key, `steamGridDbKey`): search, Steam app
//! id lookups, and art (grids, heroes, logos, icons).

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde::de::DeserializeOwned;

const API: &str = "https://www.steamgriddb.com/api/v2";

async fn get<T: DeserializeOwned>(http: &reqwest::Client, key: &str, path: &str) -> Result<T> {
    #[derive(Deserialize)]
    struct Body<T> {
        success: bool,
        data: Option<T>,
        #[serde(default)]
        errors: Vec<String>,
    }
    let resp = http.get(format!("{API}{path}")).bearer_auth(key).send().await.context("SteamGridDB")?;
    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        bail!("SteamGridDB refused the API key");
    }
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        bail!("not on SteamGridDB");
    }
    let body: Body<T> = resp.error_for_status().context("SteamGridDB")?.json().await?;
    match (body.success, body.data) {
        (true, Some(d)) => Ok(d),
        _ => bail!("SteamGridDB: {}", body.errors.join(", ")),
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SgdbGame {
    pub id: u64,
    pub name: String,
    /// Unix seconds
    #[serde(default)]
    pub release_date: Option<i64>,
}

pub async fn search(http: &reqwest::Client, key: &str, term: &str) -> Result<Vec<SgdbGame>> {
    let term: String = url_path(term);
    get(http, key, &format!("/search/autocomplete/{term}")).await
}

pub async fn game(http: &reqwest::Client, key: &str, id: &str) -> Result<SgdbGame> {
    get(http, key, &format!("/games/id/{id}")).await
}

/// The SteamGridDB game for a Steam app.
pub async fn by_steam(http: &reqwest::Client, key: &str, appid: &str) -> Result<SgdbGame> {
    get(http, key, &format!("/games/steam/{appid}")).await
}

#[derive(Debug, Clone, Deserialize)]
pub struct Image {
    pub url: String,
    pub thumb: String,
}

/// Art of one kind (`grids`, `heroes`, `logos`, `icons`), best first. Grids are portrait (600x900).
pub async fn images(http: &reqwest::Client, key: &str, kind: &str, game: &str) -> Result<Vec<Image>> {
    let query = match kind {
        "grids" => "?dimensions=600x900&types=static&nsfw=false&humor=false",
        _ => "?types=static&nsfw=false&humor=false",
    };
    get(http, key, &format!("/{kind}/game/{game}{query}")).await
}

/// Percent-encodes a path segment.
fn url_path(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn path_segments_are_encoded() {
        assert_eq!(super::url_path("R.E.P.O. / co-op?"), "R.E.P.O.%20%2F%20co-op%3F");
    }
}
