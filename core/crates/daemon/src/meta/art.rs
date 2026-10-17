//! Art per game: `hero` (wide background), `grid` (portrait cover), `logo`
//! (transparent title), `icon` (square). The db keeps where each came from
//! (`origin`: a URL or a file) and whether the user chose it; the cache keeps
//! the file, so a cleared cache is rebuilt from the origins.
//! Automatic origins, best first: SteamGridDB, Steam's CDN, then what the
//! importers found (Lutris' cover, Prism's and Twintail's icons, Twintail's background).
//! Hoyo games get HoYoPlay's art (background, logo, icon) before all of these.

use super::{sgdb, steam_store};
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use somelauncher_proto::game::{Art, Game, SourceKind};
use std::path::{Path, PathBuf};

pub const KINDS: [&str; 4] = ["hero", "grid", "logo", "icon"];

fn sgdb_kind(kind: &str) -> &'static str {
    match kind {
        "hero" => "heroes",
        "grid" => "grids",
        "logo" => "logos",
        _ => "icons",
    }
}

/// Steam's own small icon for an app, from its local library cache.
fn steam_icon(home: &Path, appid: &str) -> Option<PathBuf> {
    crate::import::steam::roots(home).into_iter().find_map(|root| {
        let dir = root.join("appcache/librarycache").join(appid);
        std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|e| e == "jpg") && p.is_file())
    })
}

/// Every automatic origin for one kind, best first.
pub async fn origins(http: &reqwest::Client, key: Option<&str>, home: &Path, game: &Game, kind: &str) -> Vec<String> {
    // Hoyo games: HoYoPlay's own art for the current version first
    let mut out = crate::hoyo::art_origins(http, game, kind).await;
    let meta = game.meta.as_ref();
    if let (Some(key), Some(id)) = (key, meta.and_then(|m| m.sgdb_id.as_deref())) {
        match sgdb::images(http, key, sgdb_kind(kind), id).await {
            Ok(images) => out.extend(images.into_iter().take(5).map(|i| i.url)),
            Err(e) => tracing::debug!(game = game.id, "SteamGridDB {kind}: {e:#}"),
        }
    }
    let appid = meta.and_then(|m| m.steam_appid.clone()).or_else(|| game.sources.iter().find(|s| s.source == SourceKind::Steam).map(|s| s.source_id.clone()));
    if let Some(appid) = &appid {
        match kind {
            "hero" => out.push(steam_store::art_url(appid, "library_hero.jpg")),
            "grid" => out.push(steam_store::art_url(appid, "library_600x900_2x.jpg")),
            "logo" => out.push(steam_store::art_url(appid, "logo.png")),
            _ => out.extend(steam_icon(home, appid).map(|p| p.to_string_lossy().into_owned())),
        }
    }
    let local: &[&str] = match kind {
        "hero" => &["background"],
        "grid" => &["coverart"],
        "icon" => &["icon"],
        _ => &[],
    };
    for s in &game.sources {
        for key in local {
            if let Some(v) = s.data[*key].as_str().filter(|v| !v.is_empty()) {
                out.push(v.to_string());
            }
        }
    }
    out.dedup();
    out
}

/// The bytes behind an origin, checked to be an image.
pub async fn load(http: &reqwest::Client, origin: &str) -> Result<Vec<u8>> {
    let bytes = if origin.starts_with("http://") || origin.starts_with("https://") {
        let resp = http.get(origin).send().await?.error_for_status().with_context(|| format!("downloading {origin}"))?;
        resp.bytes().await?.to_vec()
    } else {
        tokio::fs::read(origin).await.with_context(|| format!("reading {origin}"))?
    };
    if image_ext(&bytes).is_none() {
        bail!("{origin} is not an image");
    }
    Ok(bytes)
}

/// The file extension for image bytes, from their magic.
pub fn image_ext(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("jpg")
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

/// The widest connected monitor's width in pixels, from `/sys/class/drm` (1920 when unknown).
pub fn screen_width(drm: &Path) -> u32 {
    let widths = std::fs::read_dir(drm).into_iter().flatten().flatten().filter_map(|e| {
        let p = e.path();
        let connected = std::fs::read_to_string(p.join("status")).is_ok_and(|s| s.trim() == "connected");
        let mode = std::fs::read_to_string(p.join("modes")).ok()?;
        connected.then(|| mode.lines().next()?.split('x').next()?.parse::<u32>().ok()).flatten()
    });
    widths.max().unwrap_or(1920)
}

/// The box each kind is fitted into: heroes span the screen, the rest are shown small.
pub fn target(kind: &str, screen: u32) -> (u32, u32) {
    match kind {
        "hero" => (screen, screen),
        "grid" => (600, 900),
        "logo" => (1280, 640),
        _ => (256, 256),
    }
}

/// Fits an image into `(w, h)` (never larger than it is): JPEG, or PNG when it has
/// transparency. A JPEG or PNG that already fits is kept as it is.
pub fn fit(bytes: &[u8], (w, h): (u32, u32)) -> Result<(Vec<u8>, &'static str)> {
    use image::{GenericImageView, ImageFormat};
    let ext = image_ext(bytes).context("not an image")?;
    let img = image::load_from_memory(bytes).context("decoding the image")?;
    let (iw, ih) = img.dimensions();
    if iw <= w && ih <= h && ext != "webp" {
        return Ok((bytes.to_vec(), ext));
    }
    let img = if iw > w || ih > h { img.resize(w, h, image::imageops::FilterType::Lanczos3) } else { img };
    let mut out = std::io::Cursor::new(Vec::new());
    if img.color().has_alpha() {
        img.write_to(&mut out, ImageFormat::Png)?;
        Ok((out.into_inner(), "png"))
    } else {
        let rgb = image::DynamicImage::ImageRgb8(img.to_rgb8());
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90).encode_image(&rgb)?;
        Ok((out.into_inner(), "jpg"))
    }
}

fn art_dir(game: i64) -> PathBuf {
    somelauncher_proto::paths::cache_dir().join("art").join(game.to_string())
}

/// Removes a kind's cached files.
pub fn remove_files(game: i64, kind: &str) {
    let Ok(entries) = std::fs::read_dir(art_dir(game)) else { return };
    for f in entries.flatten().map(|e| e.path()) {
        if f.file_name().is_some_and(|n| n.to_string_lossy().starts_with(&format!("{kind}-"))) {
            let _ = std::fs::remove_file(f);
        }
    }
}

/// Writes an image, fitted for `screen`, into the cache as `<kind>-<hash>.<ext>`
/// (a new name per origin and size, so the UI never shows a stale cached
/// decode) and removes the old one. Decoding is slow: call it off the runtime.
pub fn store(game: i64, kind: &str, origin: &str, bytes: &[u8], screen: u32) -> Result<PathBuf> {
    let size = target(kind, screen);
    let (bytes, ext) = fit(bytes, size)?;
    let bytes = &bytes;
    let dir = art_dir(game);
    std::fs::create_dir_all(&dir)?;
    let hash = hex::encode(&Sha256::digest(format!("{origin} {}x{}", size.0, size.1).as_bytes())[..4]);
    let file = dir.join(format!("{kind}-{hash}.{ext}"));
    for old in std::fs::read_dir(&dir)?.flatten().map(|e| e.path()) {
        let name = old.file_name().unwrap_or_default().to_string_lossy().into_owned();
        if name.starts_with(&format!("{kind}-")) && old != file {
            let _ = std::fs::remove_file(old);
        }
    }
    std::fs::write(&file, bytes)?;
    Ok(file)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub origin: String,
    pub custom: bool,
    pub file: Option<PathBuf>,
}

pub fn row(conn: &Connection, game: i64, kind: &str) -> Result<Option<Row>> {
    Ok(conn
        .query_row("SELECT origin, custom, file FROM art WHERE game_id = ?1 AND kind = ?2", params![game, kind], |r| {
            Ok(Row { origin: r.get(0)?, custom: r.get(1)?, file: r.get::<_, Option<String>>(2)?.map(PathBuf::from) })
        })
        .optional()?)
}

pub fn set_row(conn: &Connection, game: i64, kind: &str, row: Option<&Row>) -> Result<()> {
    match row {
        Some(r) => conn.execute(
            "INSERT INTO art (game_id, kind, origin, custom, file) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(game_id, kind) DO UPDATE SET origin = excluded.origin, custom = excluded.custom, file = excluded.file",
            params![game, kind, r.origin, r.custom, r.file.as_ref().map(|f| f.to_string_lossy().into_owned())],
        )?,
        None => conn.execute("DELETE FROM art WHERE game_id = ?1 AND kind = ?2", params![game, kind])?,
    };
    Ok(())
}

/// The art files a game has (only ones that exist).
pub fn of(conn: &Connection, game: i64) -> Result<Art> {
    let mut art = Art::default();
    for kind in KINDS {
        let file = row(conn, game, kind)?.and_then(|r| r.file).filter(|f| f.is_file());
        match kind {
            "hero" => art.hero = file,
            "grid" => art.grid = file,
            "logo" => art.logo = file,
            _ => art.icon = file,
        }
    }
    Ok(art)
}

/// What fetching one kind should do, given what the db has.
#[derive(Debug, PartialEq)]
pub enum Need {
    /// Up to date
    Nothing,
    /// The user's choice, whose file is gone: load it again
    Reload(String),
    /// Find the best automatic one
    Find,
}

pub fn need(row: Option<&Row>, force: bool) -> Need {
    match row {
        Some(r) if r.custom => {
            if r.file.as_ref().is_some_and(|f| f.is_file()) { Need::Nothing } else { Need::Reload(r.origin.clone()) }
        }
        Some(r) if !force && r.file.as_ref().is_some_and(|f| f.is_file()) => Need::Nothing,
        _ => Need::Find,
    }
}

/// `store` on a blocking thread.
pub async fn store_sized(game: i64, kind: &'static str, origin: String, bytes: Vec<u8>, screen: u32) -> Result<PathBuf> {
    tokio::task::spawn_blocking(move || store(game, kind, &origin, &bytes, screen)).await?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encoded(w: u32, h: u32, alpha: bool, format: image::ImageFormat) -> Vec<u8> {
        let img = if alpha {
            image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(w, h, image::Rgba([10, 20, 30, 128])))
        } else {
            image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(w, h, image::Rgb([10, 20, 30])))
        };
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, format).unwrap();
        out.into_inner()
    }

    fn dims(bytes: &[u8]) -> (u32, u32) {
        use image::GenericImageView;
        image::load_from_memory(bytes).unwrap().dimensions()
    }

    #[test]
    fn images_are_fitted_never_enlarged() {
        let hero = encoded(3840, 1240, false, image::ImageFormat::Jpeg);
        let (out, ext) = fit(&hero, target("hero", 1920)).unwrap();
        assert_eq!((dims(&out), ext), ((1920, 620), "jpg"));

        let small = encoded(300, 450, false, image::ImageFormat::Png);
        let (out, ext) = fit(&small, target("grid", 1920)).unwrap();
        assert_eq!((out, ext), (small, "png"), "kept as it is");

        let logo = encoded(2000, 800, true, image::ImageFormat::Png);
        let (out, ext) = fit(&logo, target("logo", 1920)).unwrap();
        assert_eq!((dims(&out), ext), ((1280, 512), "png"), "transparency kept");

        let webp = encoded(100, 100, false, image::ImageFormat::WebP);
        assert_eq!(fit(&webp, target("icon", 1920)).unwrap().1, "jpg", "webp is converted");
    }

    #[test]
    fn the_widest_connected_monitor() {
        let t = tempfile::tempdir().unwrap();
        for (name, status, mode) in [("card1-DP-1", "connected", "2560x1440\n1920x1080\n"), ("card1-HDMI-A-1", "connected", "1920x1080\n"), ("card1-DP-2", "disconnected", "")] {
            std::fs::create_dir_all(t.path().join(name)).unwrap();
            std::fs::write(t.path().join(name).join("status"), format!("{status}\n")).unwrap();
            std::fs::write(t.path().join(name).join("modes"), mode).unwrap();
        }
        assert_eq!(screen_width(t.path()), 2560);
        assert_eq!(screen_width(&t.path().join("none")), 1920);
    }

    #[test]
    fn image_types_by_magic() {
        assert_eq!(image_ext(&[0xff, 0xd8, 0xff, 0xe0]), Some("jpg"));
        assert_eq!(image_ext(b"\x89PNG\r\n\x1a\n...."), Some("png"));
        assert_eq!(image_ext(b"RIFF\0\0\0\0WEBPVP8 "), Some("webp"));
        assert_eq!(image_ext(b"<!DOCTYPE html>"), None);
    }

    #[test]
    fn custom_art_is_kept_and_reloaded() {
        let t = tempfile::tempdir().unwrap();
        let file = t.path().join("grid.jpg");
        std::fs::write(&file, "x").unwrap();
        let custom = Row { origin: "/mine.png".into(), custom: true, file: Some(file.clone()) };
        assert_eq!(need(Some(&custom), true), Need::Nothing, "force never replaces the user's choice");
        let gone = Row { file: Some(t.path().join("gone.jpg")), ..custom };
        assert_eq!(need(Some(&gone), false), Need::Reload("/mine.png".into()));
        let auto = Row { origin: "https://x".into(), custom: false, file: Some(file) };
        assert_eq!(need(Some(&auto), false), Need::Nothing);
        assert_eq!(need(Some(&auto), true), Need::Find);
        assert_eq!(need(None, false), Need::Find);
    }

    #[test]
    fn rows_round_trip() {
        let conn = crate::games::test_db();
        let id = crate::games::insert(&conn, &somelauncher_proto::game::NewGame::new("A", somelauncher_proto::game::Kind::Native)).unwrap();
        let r = Row { origin: "https://x/a.jpg".into(), custom: false, file: Some("/nowhere/a.jpg".into()) };
        set_row(&conn, id, "hero", Some(&r)).unwrap();
        assert_eq!(row(&conn, id, "hero").unwrap(), Some(r));
        assert_eq!(of(&conn, id).unwrap(), Art::default(), "a missing file is not shown");
        set_row(&conn, id, "hero", None).unwrap();
        assert_eq!(row(&conn, id, "hero").unwrap(), None);
    }
}
