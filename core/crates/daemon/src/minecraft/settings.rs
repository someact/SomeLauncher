//! An instance's settings in Prism's `instance.cfg` (Qt's QSettings INI): the
//! few the Minecraft page edits, each group behind its `Override*` switch as
//! in Prism; when a group is not overridden Prism's own defaults
//! (`prismlauncher.cfg`) apply. Only the keys we know are written, line by
//! line, so everything else in the file stays as Prism left it.

use anyhow::{Context as _, Result, bail};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

/// The keys the page reads and writes, by group (the first is the group's switch)
pub const GROUPS: &[&[&str]] = &[
    &["OverrideMemory", "MinMemAlloc", "MaxMemAlloc", "PermGen"],
    &["OverrideJavaLocation", "JavaPath", "AutomaticJava"],
    &["OverrideJavaArgs", "JvmArgs"],
    &["OverrideWindow", "LaunchMaximized", "MinecraftWinWidth", "MinecraftWinHeight"],
    &["OverridePerformance", "EnableFeralGamemode", "EnableMangoHud", "UseDiscreteGpu", "UseZink"],
    &["OverrideCommands", "WrapperCommand", "PreLaunchCommand", "PostExitCommand"],
];

fn known(key: &str) -> bool {
    GROUPS.iter().any(|g| g.contains(&key))
}

/// A QSettings value as JSON: `true`/`false`, whole numbers, else the string.
fn typed(v: &str) -> Value {
    match v {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        _ => v.parse::<i64>().map(Value::from).unwrap_or_else(|_| Value::from(v)),
    }
}

/// The known keys a cfg file sets.
pub fn read(cfg: &Path) -> Map<String, Value> {
    let text = std::fs::read_to_string(cfg).unwrap_or_default();
    crate::import::prism::ini(&text).into_iter().filter(|(k, _)| known(k)).map(|(k, v)| (k.clone(), typed(&v))).collect()
}

/// Prism's defaults for the same keys (its `prismlauncher.cfg`); `AutomaticJava` is its `AutomaticJavaSwitch` there.
pub fn defaults(root: &Path) -> Map<String, Value> {
    let text = std::fs::read_to_string(root.join("prismlauncher.cfg")).unwrap_or_default();
    let ini = crate::import::prism::ini(&text);
    let mut out: Map<String, Value> = ini.iter().filter(|(k, _)| known(k)).map(|(k, v)| (k.clone(), typed(v))).collect();
    if let Some(v) = ini.get("AutomaticJavaSwitch") {
        out.insert("AutomaticJava".into(), typed(v));
    }
    out
}

/// A string as QSettings writes it: quoted when it holds `,` `;` `=` `"` (a bare
/// `,` would make it a list) or starts or ends with a space.
fn escape(s: &str) -> String {
    let mut out = String::new();
    let mut quote = s.starts_with(' ') || s.ends_with(' ');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => {
                quote = true;
                out.push_str("\\\"");
            }
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            ',' | ';' | '=' => {
                quote = true;
                out.push(c);
            }
            c => out.push(c),
        }
    }
    if quote { format!("\"{out}\"") } else { out }
}

fn to_ini(v: &Value) -> Result<Option<String>> {
    Ok(match v {
        Value::Null => None,
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.as_i64().context("a whole number")?.to_string()),
        Value::String(s) => Some(escape(s)),
        _ => bail!("not a setting value: {v}"),
    })
}

/// Sets (or with `null` removes) known keys in the `[General]` section,
/// keeping every other line; written to a new file and moved over the old one.
pub fn write(cfg: &Path, changes: &Map<String, Value>) -> Result<()> {
    for k in changes.keys() {
        if !known(k) {
            bail!("{k} is not a setting the launcher changes");
        }
    }
    let text = std::fs::read_to_string(cfg).with_context(|| format!("reading {}", cfg.display()))?;
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    let start = match lines.iter().position(|l| l.trim() == "[General]") {
        Some(i) => i,
        None => {
            lines.insert(0, "[General]".into());
            0
        }
    };
    let mut end = lines[start + 1..].iter().position(|l| l.trim_start().starts_with('[')).map_or(lines.len(), |i| start + 1 + i);
    // New keys go after the section's last setting, before blank lines
    while end > start + 1 && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    for (key, value) in changes {
        let at = (start + 1..end).find(|&i| lines[i].split_once('=').is_some_and(|(k, _)| k.trim() == key));
        match (at, to_ini(value)?) {
            (Some(i), Some(v)) => lines[i] = format!("{key}={v}"),
            (Some(i), None) => {
                lines.remove(i);
                end -= 1;
            }
            (None, Some(v)) => {
                lines.insert(end, format!("{key}={v}"));
                end += 1;
            }
            (None, None) => {}
        }
    }
    let tmp = cfg.with_extension("cfg.somelauncher");
    std::fs::write(&tmp, lines.join("\n") + "\n")?;
    std::fs::rename(&tmp, cfg)?;
    Ok(())
}

/// Java installs to pick from: Prism's own (`<root>/java/*`) and the system's
/// (`/usr/lib/jvm/*`), `{path, version, source}` by version, newest first.
pub fn javas(root: Option<&Path>, system: &Path) -> Vec<Value> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (dir, source) in [(root.map(|r| r.join("java")), "prism"), (Some(system.to_path_buf()), "system")] {
        let Some(dir) = dir else { continue };
        let mut homes: Vec<PathBuf> = std::fs::read_dir(&dir).into_iter().flatten().flatten().map(|e| e.path()).collect();
        homes.sort();
        for home in homes {
            // `default` and `default-runtime` link to one of the others
            if home.is_symlink() {
                continue;
            }
            let java = home.join("bin/java");
            let Ok(real) = std::fs::canonicalize(&java) else { continue };
            if !seen.insert(real) {
                continue;
            }
            let release = std::fs::read_to_string(home.join("release")).unwrap_or_default();
            // Arch's packages have no `release`: their folder says it (`java-21-openjdk`)
            let folder = home.file_name().unwrap_or_default().to_string_lossy().into_owned();
            let version = release
                .lines()
                .find_map(|l| l.strip_prefix("JAVA_VERSION="))
                .map(|v| v.trim_matches('"').to_string())
                .or_else(|| folder.split('-').find(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())).map(String::from));
            out.push(json!({ "path": java, "version": version, "source": source }));
        }
    }
    let major = |v: &Value| v["version"].as_str().and_then(|s| s.split('.').next()?.parse::<u32>().ok()).unwrap_or(0);
    out.sort_by_key(|v| std::cmp::Reverse(major(v)));
    out
}

/// This machine's memory in MB (the RAM slider's end).
pub fn ram_mb() -> Option<u64> {
    let info = std::fs::read_to_string("/proc/meminfo").ok()?;
    let kb: u64 = info.lines().find_map(|l| l.strip_prefix("MemTotal:"))?.trim().trim_end_matches("kB").trim().parse().ok()?;
    Some(kb / 1024)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_only_its_keys_and_keeps_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("instance.cfg");
        std::fs::write(&cfg, "[General]\nname=Vibe\nOverrideMemory=false\nMaxMemAlloc=4096\nnotes=\n\n[UI]\nmods_Page\\Columns=\"AAAA\"\n").unwrap();
        let changes: Map<String, Value> = serde_json::from_value(json!({
            "OverrideMemory": true, "MaxMemAlloc": 6144, "OverrideJavaArgs": true,
            "JvmArgs": "-XX:+UseZGC -Dfoo=bar", "WrapperCommand": "say \"hi\"", "PermGen": null
        }))
        .unwrap();
        write(&cfg, &changes).unwrap();
        let text = std::fs::read_to_string(&cfg).unwrap();
        assert!(text.starts_with("[General]\nname=Vibe\nOverrideMemory=true\nMaxMemAlloc=6144\nnotes=\n"), "{text}");
        assert!(text.contains("JvmArgs=\"-XX:+UseZGC -Dfoo=bar\"\n") && text.contains("WrapperCommand=\"say \\\"hi\\\"\"\n"), "{text}");
        assert!(text.ends_with("\n[UI]\nmods_Page\\Columns=\"AAAA\"\n"), "the other section is left alone: {text}");
        let back = read(&cfg);
        assert_eq!((&back["OverrideMemory"], &back["MaxMemAlloc"], &back["JvmArgs"]), (&json!(true), &json!(6144), &json!("-XX:+UseZGC -Dfoo=bar")));
        assert_eq!(back["WrapperCommand"], "say \"hi\"", "read back the way Prism reads it");
        assert!(!back.contains_key("name"), "only the known keys");

        write(&cfg, &serde_json::from_value(json!({ "MaxMemAlloc": null })).unwrap()).unwrap();
        assert!(!read(&cfg).contains_key("MaxMemAlloc"));
        assert!(write(&cfg, &serde_json::from_value(json!({ "name": "x" })).unwrap()).is_err(), "not ours to change");
    }

    #[test]
    fn finds_java_installs_and_prism_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("prism");
        let sys = dir.path().join("jvm");
        for (home, v) in [(root.join("java/java-runtime-delta"), "21.0.7"), (sys.join("java-25-openjdk"), "25.0.1"), (sys.join("java-17-openjdk"), "")] {
            std::fs::create_dir_all(home.join("bin")).unwrap();
            std::fs::write(home.join("bin/java"), "").unwrap();
            if !v.is_empty() {
                std::fs::write(home.join("release"), format!("JAVA_VERSION=\"{v}\"\n")).unwrap();
            }
        }
        std::os::unix::fs::symlink(sys.join("java-25-openjdk"), sys.join("default")).unwrap();
        let list = javas(Some(&root), &sys);
        assert_eq!(list.iter().map(|j| j["version"].as_str().unwrap()).collect::<Vec<_>>(), ["25.0.1", "21.0.7", "17"], "newest first, the default link once; no release file: the folder's");
        assert_eq!(list[1]["source"], "prism");

        std::fs::write(root.join("prismlauncher.cfg"), "[General]\nMaxMemAlloc=4096\nAutomaticJavaSwitch=true\nApplicationTheme=system\n").unwrap();
        let d = defaults(&root);
        assert_eq!((&d["MaxMemAlloc"], &d["AutomaticJava"]), (&json!(4096), &json!(true)));
        assert!(!d.contains_key("ApplicationTheme"));
    }
}
