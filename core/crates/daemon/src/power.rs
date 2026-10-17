//! Power while gaming (5a, 5b). When the first game starts: the power profile
//! goes to performance (balanced on battery), and on a laptop on AC the
//! battery's charge limit is turned on (UPower's EnableChargeThreshold, the
//! same switch SomeShell's battery panel uses: on most laptops the battery then
//! holds and the laptop runs from the charger). When the last game ends both
//! go back, but only what we changed and only if nobody changed it since.
//! What to undo is kept in the `powerRestore` setting, so a daemon that died
//! mid-game still restores on its next start. While a game runs, plugging in
//! or out is followed (UPower's OnBattery signal, through `gdbus monitor`).
//!
//! Everything goes through `busctl`/`gdbus` on the system bus, so
//! `DBUS_SYSTEM_BUS_ADDRESS` points it at a fake UPower for testing
//! (`scripts/fake-upower.py`). A desktop without a battery, or without
//! power-profiles-daemon, simply skips the part it lacks.

use crate::rpc::State;
use anyhow::{Context as _, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

const UPOWER: &str = "org.freedesktop.UPower";
const UPOWER_PATH: &str = "/org/freedesktop/UPower";
const DEVICE: &str = "org.freedesktop.UPower.Device";
/// power-profiles-daemon's names, the current one first
const PROFILES: [(&str, &str); 2] = [
    ("org.freedesktop.UPower.PowerProfiles", "/org/freedesktop/UPower/PowerProfiles"),
    ("net.hadess.PowerProfiles", "/net/hadess/PowerProfiles"),
];
/// UPower device types
const TYPE_BATTERY: u64 = 2;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// power-profiles-daemon's profiles; empty when it is not running
    pub profiles: Vec<String>,
    pub active: Option<String>,
    pub on_battery: bool,
    /// The laptop battery; None on a desktop
    pub battery: Option<Battery>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Battery {
    pub path: String,
    pub limit_supported: bool,
    pub limit_enabled: bool,
    /// 0/0: the firmware picks the levels (conservation mode)
    pub limit_start: u64,
    pub limit_end: u64,
}

/// The `gaming` setting: what starting a game may change
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Prefs {
    pub power_profile: bool,
    pub charge_limit: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { power_profile: true, charge_limit: true }
    }
}

/// What we changed and must undo (the `powerRestore` setting)
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Restore {
    /// The profile before the game, and the one we set
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<(String, String)>,
    /// The battery whose charge limit we turned on
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<String>,
}

impl Restore {
    fn is_empty(&self) -> bool {
        self.profile.is_none() && self.limit.is_none()
    }
}

/// What a game gets: performance, or balanced on battery; None when there is no such profile
pub fn target(status: &Status) -> Option<&'static str> {
    let want = if status.on_battery { "balanced" } else { "performance" };
    status.profiles.iter().any(|p| p == want).then_some(want)
}

/// The changes to make for a running game, given what is already recorded:
/// (a profile to set, a battery whose limit to turn on). Called again when the
/// charger comes or goes, so a profile we set follows it, and one the user
/// picked during the game is left alone.
pub fn plan(status: &Status, prefs: Prefs, done: &Restore) -> (Option<&'static str>, Option<String>) {
    let mut profile = None;
    if prefs.power_profile
        && let (Some(want), Some(active)) = (target(status), status.active.as_deref())
        && active != want
    {
        match &done.profile {
            // Ours still: follow the charger
            Some((_, set)) if set == active => profile = Some(want),
            Some(_) => {}
            None => profile = Some(want),
        }
    }
    let mut limit = None;
    if prefs.charge_limit
        && !status.on_battery
        && done.limit.is_none()
        && let Some(b) = &status.battery
        && b.limit_supported
        && !b.limit_enabled
    {
        limit = Some(b.path.clone());
    }
    (profile, limit)
}

/// The undo, given the state now: (the profile to go back to, the battery whose limit to turn off)
pub fn undo(status: &Status, done: &Restore) -> (Option<String>, Option<String>) {
    let profile = done
        .profile
        .as_ref()
        .filter(|(was, set)| status.active.as_deref() == Some(set) && was != set && status.profiles.contains(was))
        .map(|(was, _)| was.clone());
    let limit = done
        .limit
        .as_ref()
        .filter(|path| status.battery.as_ref().is_some_and(|b| &b.path == *path && b.limit_enabled))
        .cloned();
    (profile, limit)
}

// --- the system bus, through busctl

async fn busctl(args: &[&str]) -> Result<String> {
    let out = tokio::process::Command::new("busctl").arg("--system").arg("--json=short").args(args).output().await.context("running busctl")?;
    if !out.status.success() {
        bail!("busctl {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `busctl get-property` prints one `{"type", "data"}` per property, a line each
fn values(out: &str) -> Vec<Value> {
    out.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).map(|v| v["data"].clone()).collect()
}

/// The profiles and the active one, from whichever name power-profiles-daemon has
async fn profiles() -> Option<(Vec<String>, String, &'static str, &'static str)> {
    for (name, path) in PROFILES {
        if let Ok(out) = busctl(&["get-property", name, path, name, "Profiles", "ActiveProfile"]).await
            && let [list, active] = values(&out).as_slice()
        {
            let list = list.as_array().into_iter().flatten().filter_map(|p| p["Profile"]["data"].as_str().map(String::from)).collect();
            return Some((list, active.as_str().unwrap_or_default().into(), name, path));
        }
    }
    None
}

pub async fn set_profile(profile: &str) -> Result<()> {
    let Some((_, _, name, path)) = profiles().await else { bail!("power-profiles-daemon is not running") };
    busctl(&["set-property", name, path, name, "ActiveProfile", "s", profile]).await.map(drop)
}

/// The first laptop battery (a power supply of the battery type; not a mouse or a UPS)
async fn battery() -> Option<Battery> {
    let out = busctl(&["call", UPOWER, UPOWER_PATH, UPOWER, "EnumerateDevices"]).await.ok()?;
    let devices: Value = serde_json::from_str(out.trim()).ok()?;
    for path in devices["data"][0].as_array().into_iter().flatten().filter_map(Value::as_str) {
        let Ok(out) = busctl(&["get-property", UPOWER, path, DEVICE, "Type", "PowerSupply", "IsPresent"]).await else { continue };
        let v = values(&out);
        if v.first().and_then(Value::as_u64) != Some(TYPE_BATTERY) || v.get(1) != Some(&Value::Bool(true)) || v.get(2) == Some(&Value::Bool(false)) {
            continue;
        }
        // Older UPower has no charge limit properties: then it is unsupported
        let limit = busctl(&["get-property", UPOWER, path, DEVICE, "ChargeThresholdSupported", "ChargeThresholdEnabled", "ChargeStartThreshold", "ChargeEndThreshold"])
            .await
            .map(|o| values(&o))
            .unwrap_or_default();
        return Some(Battery {
            path: path.into(),
            limit_supported: limit.first().and_then(Value::as_bool).unwrap_or(false),
            limit_enabled: limit.get(1).and_then(Value::as_bool).unwrap_or(false),
            limit_start: limit.get(2).and_then(Value::as_u64).unwrap_or(0),
            limit_end: limit.get(3).and_then(Value::as_u64).unwrap_or(0),
        });
    }
    None
}

pub async fn set_limit(path: &str, on: bool) -> Result<()> {
    busctl(&["call", UPOWER, path, DEVICE, "EnableChargeThreshold", "b", if on { "true" } else { "false" }]).await.map(drop)
}

pub async fn status() -> Status {
    let (profiles, active) = match profiles().await {
        Some((list, active, ..)) => (list, Some(active)),
        None => (Vec::new(), None),
    };
    let on_battery = busctl(&["get-property", UPOWER, UPOWER_PATH, UPOWER, "OnBattery"])
        .await
        .ok()
        .and_then(|o| values(&o).first().and_then(Value::as_bool))
        .unwrap_or(false);
    Status { profiles, active, on_battery, battery: battery().await }
}

/// Lines from `gdbus monitor` that say the charger came or went
fn charger_changed(line: &str) -> bool {
    line.contains("PropertiesChanged") && line.contains("OnBattery")
}

// --- following the games

/// Whether we are in gaming mode, and the charger watch while we are
#[derive(Default)]
pub struct Power {
    active: bool,
    watch: Option<tokio::task::JoinHandle<()>>,
}

impl State {
    fn power_prefs(&self) -> Prefs {
        crate::db::setting(&self.db.lock().unwrap(), "gaming").ok().flatten().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()
    }
    fn power_done(&self) -> Restore {
        crate::db::setting(&self.db.lock().unwrap(), "powerRestore").ok().flatten().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()
    }
    fn save_power_done(&self, done: &Restore) {
        let value = if done.is_empty() { Value::Null } else { serde_json::to_value(done).unwrap_or(Value::Null) };
        if let Err(e) = crate::db::set_setting(&self.db.lock().unwrap(), "powerRestore", &value) {
            tracing::error!("saving what to restore: {e:#}");
        }
    }

    /// Called whenever a game starts or ends: gaming mode on with the first, off after the last
    pub(crate) fn games_changed(self: &Arc<Self>) {
        let state = self.clone();
        tokio::spawn(async move { state.power_sync().await });
    }

    /// After a daemon start: something left to restore means a game was running when it stopped
    pub(crate) async fn power_recover(self: &Arc<Self>) {
        self.power.lock().await.active = !self.power_done().is_empty();
        self.power_sync().await;
    }

    async fn power_sync(self: &Arc<Self>) {
        let mut power = self.power.lock().await;
        let gaming = !self.running.lock().unwrap().is_empty();
        if gaming == power.active {
            return;
        }
        power.active = gaming;
        if gaming {
            self.power_apply().await;
            let state = self.clone();
            power.watch = Some(tokio::spawn(async move { state.watch_charger().await }));
        } else {
            if let Some(w) = power.watch.take() {
                w.abort();
            }
            self.power_restore().await;
        }
        self.emit("power.changed", json!({ "gaming": gaming }));
    }

    async fn power_apply(&self) {
        let status = status().await;
        let mut done = self.power_done();
        let (profile, limit) = plan(&status, self.power_prefs(), &done);
        if let Some(p) = profile {
            match set_profile(p).await {
                Ok(()) => {
                    let was = done.profile.take().map(|(was, _)| was).or(status.active.clone()).unwrap_or_default();
                    tracing::info!("power profile {was} → {p} while gaming");
                    done.profile = Some((was, p.into()));
                }
                Err(e) => tracing::warn!("setting the power profile: {e:#}"),
            }
        }
        if let Some(path) = limit {
            match set_limit(&path, true).await {
                Ok(()) => {
                    tracing::info!("charge limit on while gaming ({path})");
                    done.limit = Some(path);
                }
                Err(e) => tracing::warn!("turning the charge limit on: {e:#}"),
            }
        }
        self.save_power_done(&done);
    }

    async fn power_restore(&self) {
        let done = self.power_done();
        if done.is_empty() {
            return;
        }
        let status = status().await;
        let (profile, limit) = undo(&status, &done);
        if let Some(p) = profile {
            match set_profile(&p).await {
                Ok(()) => tracing::info!("power profile back to {p}"),
                Err(e) => tracing::warn!("restoring the power profile: {e:#}"),
            }
        }
        if let Some(path) = limit {
            match set_limit(&path, false).await {
                Ok(()) => tracing::info!("charge limit off again ({path})"),
                Err(e) => tracing::warn!("turning the charge limit off: {e:#}"),
            }
        }
        // Whatever could not be undone is left: retrying later would fight the user
        self.save_power_done(&Restore::default());
    }

    /// While gaming: the charger coming or going sets the profile again (and the limit on plugging in)
    async fn watch_charger(self: Arc<Self>) {
        use tokio::io::AsyncBufReadExt;
        let child = tokio::process::Command::new("gdbus")
            .args(["monitor", "--system", "--dest", UPOWER, "--object-path", UPOWER_PATH])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => return tracing::debug!("not following the charger: {e}"),
        };
        let Some(out) = child.stdout.take() else { return };
        let mut lines = tokio::io::BufReader::new(out).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if charger_changed(&line) {
                tracing::info!("the charger came or went while gaming");
                self.power_apply().await;
            }
        }
    }

    pub(crate) async fn power_status(&self) -> Value {
        let s = status().await;
        json!({
            "profiles": s.profiles,
            "active": s.active,
            "onBattery": s.on_battery,
            "battery": s.battery,
            "gaming": self.power.lock().await.active,
            "prefs": { "powerProfile": self.power_prefs().power_profile, "chargeLimit": self.power_prefs().charge_limit },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pc(active: &str) -> Status {
        Status { profiles: vec!["power-saver".into(), "balanced".into(), "performance".into()], active: Some(active.into()), on_battery: false, battery: None }
    }
    fn laptop(active: &str, on_battery: bool, limit: bool) -> Status {
        Status {
            on_battery,
            battery: Some(Battery { path: "/b0".into(), limit_supported: true, limit_enabled: limit, ..Default::default() }),
            ..pc(active)
        }
    }

    #[test]
    fn a_pc_gets_performance_and_no_limit() {
        assert_eq!(plan(&pc("balanced"), Prefs::default(), &Restore::default()), (Some("performance"), None));
        assert_eq!(plan(&pc("performance"), Prefs::default(), &Restore::default()), (None, None));
        let off = Prefs { power_profile: false, charge_limit: true };
        assert_eq!(plan(&pc("balanced"), off, &Restore::default()), (None, None));
    }

    #[test]
    fn a_laptop_on_battery_gets_balanced_and_on_ac_the_limit() {
        assert_eq!(plan(&laptop("power-saver", true, false), Prefs::default(), &Restore::default()), (Some("balanced"), None));
        assert_eq!(plan(&laptop("balanced", false, false), Prefs::default(), &Restore::default()), (Some("performance"), Some("/b0".into())));
        // The user already had it on: not ours to touch
        assert_eq!(plan(&laptop("performance", false, true), Prefs::default(), &Restore::default()), (None, None));
        let unsupported = Status { battery: Some(Battery { path: "/b0".into(), ..Default::default() }), ..laptop("performance", false, false) };
        assert_eq!(plan(&unsupported, Prefs::default(), &Restore::default()).1, None);
    }

    #[test]
    fn our_profile_follows_the_charger_but_a_users_choice_stays() {
        let ours = Restore { profile: Some(("balanced".into(), "performance".into())), limit: None };
        // Unplugged mid-game: performance (ours) → balanced
        assert_eq!(plan(&laptop("performance", true, false), Prefs::default(), &ours).0, Some("balanced"));
        // The user picked power-saver during the game: left alone
        assert_eq!(plan(&laptop("power-saver", true, false), Prefs::default(), &ours).0, None);
    }

    #[test]
    fn undo_only_reverts_what_is_still_ours() {
        let done = Restore { profile: Some(("balanced".into(), "performance".into())), limit: Some("/b0".into()) };
        assert_eq!(undo(&laptop("performance", false, true), &done), (Some("balanced".into()), Some("/b0".into())));
        // Changed by the user meanwhile, and the limit already off
        assert_eq!(undo(&laptop("power-saver", false, false), &done), (None, None));
    }

    #[test]
    fn reads_busctl_output() {
        let out = "{\"type\":\"u\",\"data\":2}\n{\"type\":\"b\",\"data\":true}\n";
        assert_eq!(values(out), vec![json!(2), json!(true)]);
        assert!(charger_changed("/org/freedesktop/UPower: org.freedesktop.DBus.Properties.PropertiesChanged ('org.freedesktop.UPower', {'OnBattery': <true>}, @as [])"));
        assert!(!charger_changed("/org/freedesktop/UPower: org.freedesktop.DBus.Properties.PropertiesChanged ('org.freedesktop.UPower', {'LidIsClosed': <true>}, @as [])"));
    }
}
