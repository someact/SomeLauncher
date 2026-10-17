//! The monitor a game starts on (5c): Hyprland's focused monitor, which is the
//! launcher's own when Play is clicked there. Its size and refresh fill the
//! gamescope options a game leaves empty, and `fpsCap: "monitor"` caps at its
//! refresh. Outside Hyprland (or without `hyprctl`) there is none, and
//! gamescope picks its own defaults.

use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Monitor {
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// Hz, rounded (Hyprland says 99.999 for a 100 Hz mode)
    pub refresh: u32,
}

/// From `hyprctl monitors -j`: the focused one, else the first
pub fn parse(json: &str) -> Option<Monitor> {
    let list: Vec<Value> = serde_json::from_str(json).ok()?;
    let m = list.iter().find(|m| m["focused"] == Value::Bool(true)).or(list.first())?;
    Some(Monitor {
        name: m["name"].as_str().unwrap_or_default().into(),
        width: m["width"].as_u64()? as u32,
        height: m["height"].as_u64()? as u32,
        refresh: m["refreshRate"].as_f64().map(|r| r.round() as u32).filter(|r| *r > 0)?,
    })
}

pub fn current() -> Option<Monitor> {
    std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")?;
    let out = std::process::Command::new("hyprctl").args(["monitors", "-j"]).output().ok()?;
    parse(&String::from_utf8_lossy(&out.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_focused_monitor_with_its_refresh_rounded() {
        let json = r#"[{"name":"eDP-1","width":2560,"height":1600,"refreshRate":165.0,"focused":false},
                       {"name":"DP-1","width":1920,"height":1080,"refreshRate":99.999,"focused":true}]"#;
        assert_eq!(parse(json), Some(Monitor { name: "DP-1".into(), width: 1920, height: 1080, refresh: 100 }));
        assert_eq!(parse("[]"), None);
    }
}
