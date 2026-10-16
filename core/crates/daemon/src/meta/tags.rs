//! Automatic tags and categories. Tags are all automatic and recomputed when
//! what they come from changes; categories (library rows) belong to the user,
//! the automatic ones only fill in while the user has set none.

use somelauncher_proto::game::{Game, Kind, Meta};

/// How many of Steam's user tags a game gets.
const STEAM_TAGS: usize = 6;
/// Genres that say little about what a game is.
const NOT_A_ROW: &[&str] = &["indie", "early access", "free to play", "massively multiplayer", "casual"];

fn is_coop(meta: &Meta) -> bool {
    meta.features.iter().chain(&meta.steam_tags).any(|f| f.to_lowercase().contains("co-op"))
}

/// The game's tags: genres, Steam's top user tags, and our own
/// (`co-op`, `multiplayer`, `needs steam`, `gacha`, `modded`).
pub fn compute(game: &Game) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    let mut add = |t: &str| {
        let t = t.trim().to_lowercase();
        if !t.is_empty() && !tags.contains(&t) {
            tags.push(t);
        }
    };
    if let Some(m) = &game.meta {
        if is_coop(m) {
            add("co-op");
        }
        if m.features.iter().any(|f| f == "Multi-player") {
            add("multiplayer");
        }
    }
    if game.runner.needs_steam {
        add("needs steam");
        add("co-op");
    }
    if game.kind == Kind::Hoyo {
        add("gacha");
    }
    let modded = game.sources.iter().any(|s| s.data["xxmi"] == true || s.data["loader"].is_string());
    if modded {
        add("modded");
    }
    if let Some(m) = &game.meta {
        m.genres.iter().for_each(|g| add(g));
        m.steam_tags.iter().take(STEAM_TAGS).for_each(|t| add(t));
    }
    tags
}

/// Library rows for a matched game: `Co-op` when it is one, then its main genre.
pub fn categories(meta: &Meta) -> Vec<String> {
    let mut out = Vec::new();
    if is_coop(meta) {
        out.push("Co-op".to_string());
    }
    if let Some(g) = meta.genres.iter().find(|g| !NOT_A_ROW.contains(&g.to_lowercase().as_str())) {
        out.push(g.clone());
    }
    out
}

/// Recomputes a game's tags; true when they changed.
pub fn retag(conn: &rusqlite::Connection, id: i64) -> anyhow::Result<bool> {
    let Some(mut game) = crate::games::get(conn, id)? else { return Ok(false) };
    let tags = compute(&game);
    if tags == game.tags {
        return Ok(false);
    }
    game.tags = tags;
    crate::games::save(conn, &game)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use somelauncher_proto::game::{NewGame, Source, SourceKind};

    fn meta() -> Meta {
        Meta {
            source: "steam".into(),
            source_id: "3241660".into(),
            name: "R.E.P.O.".into(),
            genres: vec!["Action".into(), "Early Access".into()],
            steam_tags: ["Horror", "Online Co-Op", "Multiplayer", "Comedy", "Co-op", "Physics", "3D"].map(String::from).to_vec(),
            features: vec!["Single-player".into(), "Multi-player".into(), "Online Co-op".into()],
            ..Default::default()
        }
    }

    #[test]
    fn tags_come_from_steam_and_from_the_game() {
        let mut g = Game::new(1, NewGame::new("R.E.P.O.", Kind::Steam));
        g.meta = Some(meta());
        assert_eq!(compute(&g), ["co-op", "multiplayer", "action", "early access", "horror", "online co-op", "comedy", "physics"]);

        let mut fish = Game::new(2, NewGame::new("How to Fish", Kind::Proton));
        fish.runner.needs_steam = true;
        assert_eq!(compute(&fish), ["needs steam", "co-op"]);

        let mut zzz = Game::new(3, NewGame::new("Zenless Zone Zero", Kind::Hoyo));
        zzz.sources.push(Source { source: SourceKind::Twintail, source_id: "z".into(), data: serde_json::json!({ "xxmi": true }) });
        assert_eq!(compute(&zzz), ["gacha", "modded"]);
    }

    #[test]
    fn categories_skip_vague_genres() {
        assert_eq!(categories(&meta()), ["Co-op", "Action"]);
        let m = Meta { genres: vec!["Indie".into(), "RPG".into()], ..Default::default() };
        assert_eq!(categories(&m), ["RPG"]);
    }
}
