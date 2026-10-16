//! The Steam client: whether it runs and is logged in (`registry.vdf`),
//! starting it, `steam -applaunch`, and following a Steam game from Steam's
//! `logs/gameprocess_log.txt` (start) to its `reaper SteamLaunch AppId=<id>`
//! process (exit, through a pidfd). Nothing here polls.

use crate::import::vdf;
use crate::watch;
use anyhow::{Context, Result, bail};
use inotify::WatchMask;
use std::os::fd::{FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::time::Duration;

const LOGIN_TIMEOUT: Duration = Duration::from_secs(180);
/// Steam may update a game before starting it
const START_TIMEOUT: Duration = Duration::from_secs(600);

fn registry(home: &Path) -> Option<PathBuf> {
    [".steam/registry.vdf", ".var/app/com.valvesoftware.Steam/.steam/registry.vdf"].iter().map(|p| home.join(p)).find(|p| p.is_file())
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Status {
    pub running: bool,
    pub logged_in: bool,
}

/// From `registry.vdf`, which Steam rewrites when it starts, logs in and quits.
pub fn status(home: &Path) -> Status {
    let Some(v) = registry(home).and_then(|p| vdf::parse(&std::fs::read_to_string(p).ok()?).ok()) else {
        return Status { running: false, logged_in: false };
    };
    let pid = v.str_at(&["Registry", "HKLM", "Software", "Valve", "Steam", "SteamPID"]).and_then(|p| p.parse::<u32>().ok()).unwrap_or(0);
    let running = pid != 0 && Path::new(&format!("/proc/{pid}")).exists();
    let user = v.str_at(&["Registry", "HKCU", "Software", "Valve", "Steam", "ActiveProcess", "ActiveUser"]).unwrap_or("0");
    Status { running, logged_in: running && user != "0" && !user.is_empty() }
}

/// Runs the `steam` command with `args` in its own scope: when Steam was not
/// running this process becomes Steam, which must outlive the game and the daemon.
pub fn run(home: &Path, flatpak: bool, args: &[String]) -> Result<()> {
    let flatpak = flatpak || (crate::launch::which("steam").is_none() && home.join(".var/app/com.valvesoftware.Steam").is_dir());
    let mut argv: Vec<String> = if flatpak {
        ["flatpak", "run", "com.valvesoftware.Steam"].map(String::from).to_vec()
    } else {
        crate::launch::which("steam").context("Steam is not installed")?;
        vec!["steam".into()]
    };
    argv.extend(args.iter().cloned());
    let unit = format!("--unit=somelauncher-steam-{}-{}", crate::games::now(), std::process::id());
    let mut cmd = tokio::process::Command::new("systemd-run");
    cmd.args(["--user", "--scope", "--collect", "--quiet", &unit, "--"]).args(&argv);
    cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    // Not waited for: systemd-run stays the parent until that process quits, and tokio reaps it then
    cmd.spawn().context("running steam")?;
    Ok(())
}

/// Makes sure Steam runs and is logged in, starting it when needed.
pub async fn ensure_running(home: &Path) -> Result<()> {
    if status(home).logged_in {
        return Ok(());
    }
    if !status(home).running {
        tracing::info!("starting Steam");
        run(home, false, &["-silent".into()])?;
    }
    let dir = registry(home).and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_else(|| home.join(".steam"));
    std::fs::create_dir_all(&dir)?;
    let mask = WatchMask::CLOSE_WRITE | WatchMask::MOVED_TO | WatchMask::CREATE;
    if !watch::until(&dir, mask, Some(LOGIN_TIMEOUT), || status(home).logged_in).await? {
        bail!("Steam did not log in within {} seconds", LOGIN_TIMEOUT.as_secs());
    }
    Ok(())
}

/// Steam's game process log in a Steam install.
pub fn process_log(steam_root: &Path) -> PathBuf {
    steam_root.join("logs/gameprocess_log.txt")
}

/// Where the log ends now (its file and size): lines after it are new.
#[derive(Debug, Clone, Copy)]
pub struct LogPos {
    ino: u64,
    len: u64,
}

pub fn log_offset(log: &Path) -> LogPos {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(log).map(|m| LogPos { ino: m.ino(), len: m.len() }).unwrap_or(LogPos { ino: 0, len: 0 })
}

/// Whether the log, past `pos`, says Steam started `appid`. A new file (Steam
/// rotates the log when it starts) or a shorter one is read from the start.
fn started_since(log: &Path, pos: LogPos, appid: &str) -> bool {
    let now = log_offset(log);
    let Ok(bytes) = std::fs::read(log) else { return false };
    let from = if now.ino != pos.ino || (bytes.len() as u64) < pos.len { 0 } else { pos.len as usize };
    let needle = format!("AppID {appid} adding PID");
    String::from_utf8_lossy(&bytes[from..]).contains(&needle)
}

/// Steam's `reaper SteamLaunch AppId=<appid>` process, the root of a running Steam game.
pub fn find_reaper(appid: &str) -> Option<i32> {
    let want = format!("AppId={appid}");
    std::fs::read_dir("/proc").ok()?.flatten().find_map(|e| {
        let pid: i32 = e.file_name().to_str()?.parse().ok()?;
        let cmdline = std::fs::read(e.path().join("cmdline")).ok()?;
        let mut args = cmdline.split(|&b| b == 0).map(|a| String::from_utf8_lossy(a).into_owned());
        let exe = args.next()?;
        let args: Vec<String> = args.collect();
        let is_reaper = exe.ends_with("/reaper") || exe == "reaper";
        (is_reaper && args.iter().any(|a| a == "SteamLaunch") && args.contains(&want)).then_some(pid)
    })
}

/// Waits until Steam has started `appid` and returns its reaper's pid.
pub async fn wait_started(steam_root: &Path, appid: &str, offset: LogPos) -> Result<i32> {
    let log = process_log(steam_root);
    let dir = log.parent().context("Steam has no logs folder")?.to_path_buf();
    std::fs::create_dir_all(&dir)?;
    let mask = WatchMask::MODIFY | WatchMask::CREATE | WatchMask::MOVED_TO;
    let started = watch::until(&dir, mask, Some(START_TIMEOUT), || started_since(&log, offset, appid) && find_reaper(appid).is_some()).await?;
    if !started {
        bail!("Steam did not start the game within {} minutes", START_TIMEOUT.as_secs() / 60);
    }
    find_reaper(appid).context("the game exited right away")
}

/// Waits for a process to exit, through a pidfd (no polling). A process that
/// is already gone returns at once.
pub async fn wait_pid(pid: i32) -> Result<()> {
    // SAFETY: pidfd_open takes a pid and flags and returns a new fd or -1
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    if fd < 0 {
        let err = std::io::Error::last_os_error();
        return if err.raw_os_error() == Some(libc::ESRCH) { Ok(()) } else { Err(err).context("pidfd_open") };
    }
    // SAFETY: the fd was just returned by the kernel and is owned by nobody else
    let fd = unsafe { OwnedFd::from_raw_fd(fd as i32) };
    // SAFETY: the AsyncFd owns the OwnedFd, so the fd stays open and the same for its lifetime
    let fd = unsafe { tokio::io::unix::AsyncFd::register_with_interest(fd, tokio::io::Interest::READABLE) }.map_err(|e| e.into_parts().1)?;
    let _ready = fd.readable().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_reads_the_registry() {
        let home = tempfile::tempdir().unwrap();
        assert_eq!(status(home.path()), Status { running: false, logged_in: false });
        std::fs::create_dir_all(home.path().join(".steam")).unwrap();
        let reg = |pid: u32, user: &str| {
            format!(
                r#""Registry" {{ "HKLM" {{ "Software" {{ "Valve" {{ "Steam" {{ "SteamPID" "{pid}" }} }} }} }}
                   "HKCU" {{ "Software" {{ "Valve" {{ "Steam" {{ "ActiveProcess" {{ "ActiveUser" "{user}" }} }} }} }} }} }}"#
            )
        };
        let me = std::process::id();
        std::fs::write(home.path().join(".steam/registry.vdf"), reg(me, "0")).unwrap();
        assert_eq!(status(home.path()), Status { running: true, logged_in: false });
        std::fs::write(home.path().join(".steam/registry.vdf"), reg(me, "1561564557")).unwrap();
        assert!(status(home.path()).logged_in);
    }

    #[test]
    fn the_log_tells_new_starts_from_old_ones() {
        let t = tempfile::tempdir().unwrap();
        let log = t.path().join("gameprocess_log.txt");
        std::fs::write(&log, "[2026-08-08 02:51:51] AppID 480 adding PID 1 as a tracked process\n").unwrap();
        let offset = log_offset(&log);
        assert!(!started_since(&log, offset, "480"), "an old start does not count");
        let rotated = t.path().join("new.txt");
        std::fs::write(&rotated, "[x] Client version: 1\n[x] AppID 480 adding PID 2 as a tracked process \"…\"\n").unwrap();
        std::fs::rename(&rotated, &log).unwrap();
        assert!(started_since(&log, offset, "480"), "a rotated log is read from the start");
        assert!(!started_since(&log, LogPos { ino: 0, len: 0 }, "4801"));
    }

    #[tokio::test]
    async fn finds_a_reaper_and_follows_it_to_its_exit() {
        // A process that looks like Steam's: argv0 `…/reaper`, then `SteamLaunch AppId=<id>`
        let mut child = std::process::Command::new("bash")
            .args(["-c", "exec -a /steam/ubuntu12_32/reaper sh -c 'sleep 0.3; true' SteamLaunch AppId=987654"])
            .spawn()
            .unwrap();
        let pid = child.id() as i32;
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(find_reaper("987654"), Some(pid));
        assert_eq!(find_reaper("98765"), None);
        let reaper = std::thread::spawn(move || child.wait());
        tokio::time::timeout(Duration::from_secs(5), wait_pid(pid)).await.unwrap().unwrap();
        reaper.join().unwrap().unwrap();
    }

    #[tokio::test]
    async fn wait_pid_returns_when_the_process_exits() {
        let mut child = std::process::Command::new("sleep").arg("0.2").spawn().unwrap();
        let pid = child.id() as i32;
        let reaper = std::thread::spawn(move || child.wait());
        tokio::time::timeout(Duration::from_secs(5), wait_pid(pid)).await.unwrap().unwrap();
        reaper.join().unwrap().unwrap();
        wait_pid(i32::MAX - 1).await.unwrap();
    }
}
