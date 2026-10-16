//! Manual add: from an exe or a folder, guess the game's name, folder, main
//! executable and kind. The user confirms everything in the add wizard (4e);
//! this only has to make the common case one click.

use serde::Serialize;
use somelauncher_proto::game::Kind;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Guess {
    pub name: String,
    pub kind: Kind,
    pub install_dir: PathBuf,
    pub exe: Option<PathBuf>,
    /// Every plausible executable, best first
    pub candidates: Vec<PathBuf>,
}

/// Executables that come with games but never are the game.
const NOT_THE_GAME: &[&str] = &[
    "crash",
    "unins",
    "setup",
    "install",
    "redist",
    "dxwebsetup",
    "dotnet",
    "prereq",
    "helper",
    "createdump",
    "upload_",
    "browser",
    "easyanticheat",
    "beservice",
    "dos_stub",
    "cefsharp",
    "notification",
    "uninstall",
    "vcredist",
];
/// Folders that hold no game executables, or ones we never want.
const SKIP_DIRS: &[&str] = &[
    "_commonredist",
    "redist",
    "__installer",
    "drive_c",
    "pfx",
    "monobleedingedge",
    "node_modules",
    "prefixes",
];
/// Folder names that say nothing about the game: the name comes from above them.
const GENERIC_DIRS: &[&str] = &[
    "bin", "bin64", "bin32", "binaries", "win64", "win32", "wingdk", "x64", "x86", "game",
    "client", "linux", "windows",
];
/// Release tags in folder names.
const TAGS: &[&str] = &[
    "steamrip.com",
    "steamrip",
    "repack",
    "fitgirl",
    "dodi",
    "elamigos",
    "gog",
    "dlsite",
    "linux64",
    "win64",
    "public",
];

const MAX_DEPTH: usize = 4;
const MAX_ENTRIES: usize = 4000;

fn normalized(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn is_version(token: &str) -> bool {
    let t = token.to_lowercase();
    if matches!(t.as_str(), "build" | "version" | "ver") {
        return true;
    }
    let t = t
        .strip_prefix("ver")
        .or_else(|| t.strip_prefix('v'))
        .unwrap_or(&t);
    let digits = t.chars().filter(char::is_ascii_digit).count();
    let only = t.chars().all(|c| c.is_ascii_digit() || c == '.');
    only && digits > 0 && (t.contains('.') || digits >= 5)
}

/// A readable game name from a folder name: `Muse Dash v6.7.0` → `Muse Dash`,
/// `How-to-Fish-SteamRIP.com` → `How to Fish`, `Stardew.Valley` → `Stardew Valley`.
pub fn clean_name(folder: &str) -> String {
    // Drop (…) […] {…}
    let mut s = String::new();
    let mut depth = 0;
    for c in folder.chars() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = (depth - 1).max(0),
            c if depth == 0 => s.push(c),
            _ => {}
        }
    }
    let mut words = Vec::new();
    'tokens: for token in s.split([' ', '_', '-']).filter(|t| !t.is_empty()) {
        if TAGS.contains(&token.to_lowercase().as_str()) {
            continue;
        }
        if is_version(token) {
            break;
        }
        for part in token.split('.').filter(|p| !p.is_empty()) {
            if TAGS.contains(&part.to_lowercase().as_str()) {
                continue;
            }
            if is_version(part) {
                break 'tokens;
            }
            words.push(part.to_string());
        }
    }
    let name = words.join(" ");
    if name.is_empty() {
        folder.to_string()
    } else {
        name
    }
}

/// A shell or batch script: a launcher at best, often a build or setup script.
pub fn is_script(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_lowercase().as_str(), "sh" | "bat"))
}

fn is_windows_exe(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_lowercase().as_str(), "exe" | "bat" | "lnk"))
}

fn is_linux_exe(p: &Path) -> bool {
    use std::io::Read;
    use std::os::unix::fs::PermissionsExt;
    match p
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .as_deref()
    {
        Some("x86_64" | "x86" | "appimage" | "sh") => true,
        None => {
            let executable = std::fs::metadata(p)
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0);
            let mut magic = [0u8; 4];
            executable
                && std::fs::File::open(p)
                    .and_then(|mut f| f.read_exact(&mut magic))
                    .is_ok()
                && magic == *b"\x7fELF"
        }
        _ => false,
    }
}

fn is_game_exe(p: &Path) -> bool {
    let stem = p
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_lowercase();
    (is_windows_exe(p) || is_linux_exe(p)) && !NOT_THE_GAME.iter().any(|n| stem.contains(n))
}

/// Executables under `dir`, `depth` levels down at most.
fn executables(dir: &Path) -> Vec<(PathBuf, usize)> {
    let mut out = Vec::new();
    let mut queue = vec![(dir.to_path_buf(), 1)];
    let mut seen = 0;
    while let Some((d, depth)) = queue.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            seen += 1;
            if seen > MAX_ENTRIES {
                return out;
            }
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_lowercase();
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                if depth < MAX_DEPTH
                    && !name.starts_with('.')
                    && !name.ends_with("_data")
                    && !SKIP_DIRS.contains(&name.as_str())
                {
                    queue.push((p, depth + 1));
                }
            } else if is_game_exe(&p) {
                out.push((p, depth));
            }
        }
    }
    out
}

/// Higher is likelier the game: shallow, named like the folder, not a script, big.
fn score(exe: &Path, depth: usize, folder: &str) -> (i64, u64) {
    let stem = normalized(exe.file_stem().and_then(|s| s.to_str()).unwrap_or_default());
    let folder = normalized(folder);
    let mut s = 100 - depth as i64 * 10;
    if stem.len() >= 4 && (folder.contains(&stem) || stem.contains(&folder)) {
        s += 50;
    }
    if is_script(exe) {
        s -= 40;
    }
    if exe
        .components()
        .any(|c| c.as_os_str().eq_ignore_ascii_case("engine"))
    {
        s -= 30;
    }
    (s, std::fs::metadata(exe).map(|m| m.len()).unwrap_or(0))
}

/// The folder that names the game an exe belongs to: its parent, above generic folders.
fn game_dir(exe: &Path) -> PathBuf {
    let mut dir = exe.parent().unwrap_or(Path::new("/")).to_path_buf();
    while let (Some(name), Some(parent)) = (dir.file_name().and_then(|n| n.to_str()), dir.parent())
    {
        if !GENERIC_DIRS.contains(&name.to_lowercase().as_str()) {
            break;
        }
        dir = parent.to_path_buf();
    }
    dir
}

pub fn guess(path: &Path) -> Option<Guess> {
    let (dir, exe, candidates) = if path.is_dir() {
        let folder = path.file_name()?.to_string_lossy().into_owned();
        let mut found: Vec<_> = executables(path)
            .into_iter()
            .map(|(p, d)| (score(&p, d, &clean_name(&folder)), p))
            .collect();
        found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        let candidates: Vec<PathBuf> = found.into_iter().map(|(_, p)| p).collect();
        (path.to_path_buf(), candidates.first().cloned(), candidates)
    } else if path.is_file() {
        (
            game_dir(path),
            Some(path.to_path_buf()),
            vec![path.to_path_buf()],
        )
    } else {
        return None;
    };
    let kind = match &exe {
        Some(e) if is_windows_exe(e) => Kind::Proton,
        Some(_) => Kind::Native,
        None => Kind::Proton,
    };
    let name = clean_name(&dir.file_name()?.to_string_lossy());
    Some(Guess {
        name,
        kind,
        install_dir: dir,
        exe,
        candidates,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_lose_versions_and_release_tags() {
        for (folder, name) in [
            ("Muse Dash v6.7.0", "Muse Dash"),
            ("How-to-Fish-SteamRIP.com", "How to Fish"),
            ("Stardew.Valley", "Stardew Valley"),
            ("Minecraft.Dungeons.II", "Minecraft Dungeons II"),
            ("GS 0.2.4.1(Windows)", "GS"),
            ("TrueFacials_Pro_0.599", "TrueFacials Pro"),
            ("Summer Heat-Ver1.02-DLsite", "Summer Heat"),
            (
                "Project.Zomboid.Build.25485521.Linux64.public",
                "Project Zomboid",
            ),
            ("Gaea-2.3.1.0", "Gaea"),
            ("Left 4 Dead 2", "Left 4 Dead 2"),
            ("Cyberpunk 2077", "Cyberpunk 2077"),
            ("Valheim", "Valheim"),
            ("Terraria Version 1.4", "Terraria"),
        ] {
            assert_eq!(clean_name(folder), name, "{folder}");
        }
    }

    fn touch(path: &Path, bytes: usize) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, vec![0u8; bytes]).unwrap();
    }

    #[test]
    fn picks_the_main_exe_of_a_folder() {
        let t = tempfile::tempdir().unwrap();
        let ue = t.path().join("Palworld");
        touch(&ue.join("Palworld.exe"), 10);
        touch(
            &ue.join("Engine/Binaries/Win64/CrashReportClient.exe"),
            1000,
        );
        touch(
            &ue.join("Pal/Binaries/Win64/Palworld-Win64-Shipping.exe"),
            1000,
        );
        let g = guess(&ue).unwrap();
        assert_eq!((g.name.as_str(), g.kind), ("Palworld", Kind::Proton));
        assert_eq!(g.exe, Some(ue.join("Palworld.exe")));
        assert_eq!(
            g.candidates.len(),
            2,
            "the crash reporter is not a candidate"
        );

        let unity = t.path().join("Muse Dash v6.7.0");
        touch(&unity.join("Muse Dash/UnityCrashHandler64.exe"), 10);
        touch(&unity.join("Muse Dash/MuseDash.exe"), 10);
        touch(
            &unity.join("Muse Dash/MuseDash_Data/Plugins/ZFGameBrowser.exe"),
            10,
        );
        assert_eq!(
            guess(&unity).unwrap().exe,
            Some(unity.join("Muse Dash/MuseDash.exe"))
        );

        let native = t.path().join("Valheim");
        touch(&native.join("valheim.x86_64"), 10);
        touch(&native.join("start_server.sh"), 10);
        let g = guess(&native).unwrap();
        assert_eq!(
            (g.kind, g.exe),
            (Kind::Native, Some(native.join("valheim.x86_64")))
        );

        let tool = t.path().join("gbe_fork");
        touch(&tool.join("build_win_premake.bat"), 10);
        assert!(is_script(guess(&tool).unwrap().exe.as_deref().unwrap()));
    }

    #[test]
    fn an_exe_is_named_after_the_folder_above_generic_ones() {
        let t = tempfile::tempdir().unwrap();
        let exe = t.path().join("Hollow_Knight/bin/x64/hollow_knight.exe");
        touch(&exe, 1);
        let g = guess(&exe).unwrap();
        assert_eq!(
            (g.name.as_str(), g.install_dir),
            ("Hollow Knight", t.path().join("Hollow_Knight"))
        );
        assert!(guess(&t.path().join("missing")).is_none());
    }
}
