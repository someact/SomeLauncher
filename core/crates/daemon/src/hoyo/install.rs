//! Installing a Hoyo game with Sophon (6c): what it would take (`hoyo.plan`),
//! and the install itself (`hoyo.install`), a background job. The game is in
//! the library from the start, marked `installing` in its `hoyo` source, so a
//! stopped install (cancelled, or the daemon went away) shows as one to resume;
//! installing again picks up from the chunk log.

use super::api::{self, Branch, HoyoGame};
use super::{files, name_of, sophon};
use crate::rpc::{State, internal};
use anyhow::{Context as _, Result};
use serde_json::{Value, json};
use somelauncher_proto::Error;
use somelauncher_proto::game::{Kind, NewGame, Source, SourceKind};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The voice pack folder names the games use, for HoYoPlay's codes
fn voice_name(code: &str) -> Option<&'static str> {
    ["Chinese", "English(US)", "Japanese", "Korean"].into_iter().find(|n| files::voice_code(n) == Some(code))
}

/// The categories to fetch: every resource of a full install and the chosen voice packs
pub fn pick(categories: &[sophon::Category], branch: &Branch, voices: &[String]) -> Vec<sophon::Category> {
    categories
        .iter()
        .filter(|c| branch.resources.contains(&c.field) || voices.contains(&c.field))
        .cloned()
        .collect()
}

impl State {
    async fn hoyo_game(&self, biz: &str) -> Result<HoyoGame, Error> {
        let games = api::games(&self.http, api::Region::of(biz), &self.hoyo_language(), false).await.map_err(|e| Error::new("network", format!("{e:#}")))?;
        games.into_iter().find(|g| g.biz == biz).ok_or_else(|| Error::new("not_found", format!("HoYoPlay has no game {biz}")))
    }

    /// Where a new install goes: `<first game folder>/<name>`, else our data folder
    fn hoyo_default_dir(&self, biz: &str) -> PathBuf {
        let dirs: Vec<PathBuf> =
            crate::db::setting(&self.db.lock().unwrap(), "gameDirs").ok().flatten().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
        dirs.first().cloned().unwrap_or_else(|| somelauncher_proto::paths::data_dir().join("games")).join(name_of(biz))
    }

    /// The voice packs to install when none are given: the locale's language if offered, else English
    fn default_voices(&self, branch: &Branch) -> Vec<String> {
        let lang = self.hoyo_language();
        let pick = if branch.voices.contains(&lang) { lang } else { "en-us".to_string() };
        branch.voices.iter().filter(|v| **v == pick).cloned().collect()
    }

    /// `hoyo.plan {biz, voices?, dir?}`: sizes before installing
    pub(crate) async fn hoyo_plan(&self, params: &Value) -> Result<Value, Error> {
        let biz = params["biz"].as_str().ok_or_else(|| Error::new("bad_params", "missing string `biz`"))?;
        let game = self.hoyo_game(biz).await?;
        let branch = game.main.ok_or_else(|| Error::new("not_found", "no version to install"))?;
        let categories = sophon::build(&self.http, game.region, &branch).await.map_err(|e| Error::new("network", format!("{e:#}")))?;
        let voices: Vec<String> = match params.get("voices") {
            Some(v) => serde_json::from_value(v.clone()).map_err(|e| Error::new("bad_params", e.to_string()))?,
            None => self.default_voices(&branch),
        };
        let picked = pick(&categories, &branch, &voices);
        let dir = params["dir"].as_str().map(PathBuf::from).unwrap_or_else(|| self.hoyo_default_dir(biz));
        let voice_sizes: Vec<Value> = categories
            .iter()
            .filter(|c| branch.voices.contains(&c.field))
            .map(|c| json!({ "voice": c.field, "downloadSize": c.download_size, "size": c.size }))
            .collect();
        Ok(json!({
            "biz": biz,
            "version": branch.tag,
            "dir": dir,
            "voices": voices,
            "voiceSizes": voice_sizes,
            "downloadSize": picked.iter().map(|c| c.download_size).sum::<u64>(),
            "size": picked.iter().map(|c| c.size).sum::<u64>(),
            "free": sophon::free_space(&dir),
        }))
    }

    /// `hoyo.install {biz, dir?, voices?}` (or `{id}` to resume): starts the job
    pub(crate) async fn hoyo_install(self: &Arc<Self>, params: &Value) -> Result<Value, Error> {
        // Resuming: the game's own install record
        let resume = match params["id"].as_i64() {
            Some(id) => {
                let g = self.game(&self.db.lock().unwrap(), id)?;
                let src = g.sources.iter().find(|s| s.source == SourceKind::Hoyo).cloned();
                let installing = src.as_ref().map(|s| s.data["installing"].clone()).filter(|v| !v.is_null());
                let (Some(src), Some(inst), Some(dir)) = (src, installing, g.install_dir.clone()) else {
                    return Err(Error::new("not_found", format!("{} has no install to resume", g.name)));
                };
                Some((src.source_id, dir, serde_json::from_value::<Vec<String>>(inst["voices"].clone()).unwrap_or_default()))
            }
            None => None,
        };
        let (biz, dir, voices) = match resume {
            Some(r) => r,
            None => {
                let biz = params["biz"].as_str().ok_or_else(|| Error::new("bad_params", "missing string `biz`"))?.to_string();
                let dir = params["dir"].as_str().map(PathBuf::from).unwrap_or_else(|| self.hoyo_default_dir(&biz));
                let voices = params.get("voices").map(|v| serde_json::from_value(v.clone())).transpose().map_err(|e| Error::new("bad_params", e.to_string()))?;
                (biz, dir, voices.unwrap_or_default())
            }
        };
        let game = self.hoyo_game(&biz).await?;
        let branch = game.main.clone().ok_or_else(|| Error::new("not_found", "no version to install"))?;
        let voices = if voices.is_empty() { self.default_voices(&branch) } else { voices };
        let exe = super::EXES.iter().find(|(b, _)| *b == biz).map(|(_, e)| dir.join(e)).ok_or_else(|| Error::new("bad_params", format!("unknown game {biz}")))?;
        // A folder that already holds the game is adopted, not installed over
        if params["id"].is_null() && exe.is_file() {
            return Err(Error::new("busy", format!("{} is already installed in {}; adopt it instead", name_of(&biz), dir.display())));
        }

        let categories = sophon::build(&self.http, game.region, &branch).await.map_err(|e| Error::new("network", format!("{e:#}")))?;
        let picked = pick(&categories, &branch, &voices);
        // A new install that cannot fit fails here, before anything is made
        let need: u64 = picked.iter().map(|c| c.size).sum();
        if params["id"].is_null()
            && let Some(free) = sophon::free_space(&dir)
            && free < need
        {
            return Err(Error::new("no_space", format!("{} needs {} GB, {} has {} GB free", name_of(&biz), need / 1_000_000_000 + 1, dir.display(), free / 1_000_000_000)));
        }

        let id = self.installing_game(&biz, &dir, &exe, &branch.tag, &voices).map_err(internal)?;
        self.emit("library.changed", json!({ "ids": [id] }));
        let label = format!("Installing {} {}", name_of(&biz), branch.tag);
        let job = format!("hoyo-{id}");
        let state = self.clone();
        let http = self.http.clone();
        crate::jobs::spawn(self, &job, &label, move |progress| async move {
            sophon::install(&http, &picked, &dir, &format!("install-{}.log", branch.tag), &progress).await?;
            finish(&dir, &biz, &branch.tag, &voices)?;
            let install = files::detect(&dir).context("the installed game is not where it should be")?;
            state.adopt(&install, Some(id))?;
            state.emit("library.changed", json!({ "ids": [id] }));
            Ok(())
        })
        .map_err(|e| Error::new("busy", format!("{e:#}")))?;
        Ok(json!({ "id": job, "game": id }))
    }

    /// The library entry of an install in progress (made, or found by its folder)
    fn installing_game(&self, biz: &str, dir: &Path, exe: &Path, tag: &str, voices: &[String]) -> Result<i64> {
        let db = self.db.lock().unwrap();
        let existing = crate::games::list(&db, true)?.into_iter().find(|g| g.kind == Kind::Hoyo && g.install_dir.as_deref() == Some(dir)).map(|g| g.id);
        let id = match existing {
            Some(id) => id,
            None => {
                let mut n = NewGame::new(name_of(biz), Kind::Hoyo);
                n.install_dir = Some(dir.to_path_buf());
                n.exe = Some(exe.to_path_buf());
                crate::games::insert(&db, &n)?
            }
        };
        let data = json!({ "biz": biz, "version": null, "voices": [], "installing": { "version": tag, "voices": voices } });
        crate::games::set_source(&db, id, &Source { source: SourceKind::Hoyo, source_id: biz.into(), data })?;
        Ok(id)
    }
}

/// After the last chunk: what the official launcher leaves behind (`config.ini`,
/// the voice pack list the game reads), and our chunk log goes
pub fn finish(dir: &Path, biz: &str, tag: &str, voices: &[String]) -> Result<()> {
    std::fs::write(dir.join("config.ini"), format!("[General]\r\nchannel=1\r\nsub_channel=0\r\ngame_version={tag}\r\n"))?;
    let exe = super::EXES.iter().find(|(b, _)| *b == biz).map(|(_, e)| *e).unwrap_or_default();
    let data = dir.join(format!("{}_Data", exe.trim_end_matches(".exe")));
    let list = if api::game_of(biz) == "nap" { "audio_lang_launcher" } else { "audio_lang_14" };
    let names: String = voices.iter().filter_map(|v| voice_name(v)).map(|n| format!("{n}\r\n")).collect();
    std::fs::create_dir_all(data.join("Persistent"))?;
    std::fs::write(data.join("Persistent").join(list), names)?;
    let _ = std::fs::remove_file(dir.join(sophon::STATE_DIR).join(format!("install-{tag}.log")));
    let _ = std::fs::remove_dir(dir.join(sophon::STATE_DIR));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_install_with_the_chosen_voices() {
        let cat = |f: &str| sophon::Category {
            field: f.into(),
            manifest: String::new(),
            checksum: String::new(),
            manifest_url: String::new(),
            chunk_url: String::new(),
            compressed: true,
            download_size: 1,
            size: 1,
        };
        let branch = Branch {
            package_id: String::new(),
            branch: "main".into(),
            password: String::new(),
            tag: "3.2.0".into(),
            diff_tags: vec![],
            voices: vec!["en-us".into(), "ja-jp".into()],
            resources: vec!["game".into(), "10100".into()],
        };
        let all = [cat("game"), cat("10100"), cat("en-us"), cat("ja-jp"), cat("mini-en-us")];
        let picked: Vec<String> = pick(&all, &branch, &["ja-jp".into()]).into_iter().map(|c| c.field).collect();
        assert_eq!(picked, ["game", "10100", "ja-jp"]);
    }

    #[test]
    fn finishing_writes_the_launcher_files() {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join(sophon::STATE_DIR)).unwrap();
        finish(t.path(), "nap_global", "3.2.0", &["en-us".into(), "ja-jp".into()]).unwrap();
        let langs = std::fs::read_to_string(t.path().join("ZenlessZoneZero_Data/Persistent/audio_lang_launcher")).unwrap();
        assert_eq!(langs, "English(US)\r\nJapanese\r\n");
        assert!(std::fs::read_to_string(t.path().join("config.ini")).unwrap().contains("game_version=3.2.0"));
        assert!(!t.path().join(sophon::STATE_DIR).exists());
    }
}
