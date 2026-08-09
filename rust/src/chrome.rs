use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

const CDP_PORT: u16 = 9222;
const OWNER_FILE: &str = "owner.json";
const LEGACY_CHROME_DATA: &str = "/tmp/scavenger-chrome";

/// The Chrome profile holds marketplace logins (Facebook especially), so it
/// lives in the durable data dir — /tmp is wiped on reboot, which cost a
/// re-login every restart. Old profiles are migrated on first use.
pub fn chrome_data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join(".local/share")
        })
        .join("scavenger")
        .join("chrome")
}

/// One-time migration of a legacy /tmp profile (with its cookies/logins)
/// into the durable location. Best-effort: if the rename fails (e.g. the
/// old dir is gone or crosses filesystems), Chrome just starts fresh.
fn migrate_legacy_profile(dest: &PathBuf) {
    let legacy = PathBuf::from(LEGACY_CHROME_DATA);
    if !dest.exists() && legacy.is_dir() {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        match std::fs::rename(&legacy, dest) {
            Ok(()) => eprintln!(
                "Migrated Chrome profile (with logins) from {LEGACY_CHROME_DATA} to {}",
                dest.display()
            ),
            Err(e) => eprintln!(
                "Could not migrate legacy Chrome profile from {LEGACY_CHROME_DATA}: {e} — starting fresh"
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChromeOwnershipState {
    Owned,
    External,
    Stale,
    Missing,
}

impl ChromeOwnershipState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Owned => "owned",
            Self::External => "external",
            Self::Stale => "stale",
            Self::Missing => "missing",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChromeStatus {
    pub running: bool,
    pub pid: Option<u32>,
    pub ownership: ChromeOwnershipState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ChromeOwner {
    pid: u32,
    spawned_pid: u32,
    port: u16,
    chrome_path: String,
    user_data_dir: String,
}

fn find_chrome() -> Option<PathBuf> {
    let candidates = [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/usr/bin/google-chrome-stable",
        "/usr/bin/google-chrome",
        "/usr/bin/chromium-browser",
        "/usr/bin/chromium",
    ];
    for c in &candidates {
        let p = PathBuf::from(c);
        if p.exists() {
            return Some(p);
        }
    }
    // Try PATH
    if let Ok(output) = Command::new("which").arg("google-chrome").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Some(PathBuf::from(path));
            }
        }
    }
    None
}

fn chrome_pid() -> Option<u32> {
    let output = Command::new("lsof")
        .args(["-ti", &format!("tcp:{CDP_PORT}"), "-sTCP:LISTEN"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout);
    s.trim().lines().next()?.trim().parse().ok()
}

fn owner_path() -> PathBuf {
    chrome_data_dir().join(OWNER_FILE)
}

fn read_owner() -> Option<ChromeOwner> {
    let raw = std::fs::read_to_string(owner_path()).ok()?;
    serde_json::from_str(&raw).ok()
}

fn write_owner(owner: &ChromeOwner) -> Result<(), String> {
    std::fs::create_dir_all(chrome_data_dir())
        .map_err(|e| format!("Failed to create Chrome data dir: {e}"))?;
    let raw = serde_json::to_string_pretty(owner)
        .map_err(|e| format!("Failed to serialize Chrome ownership metadata: {e}"))?;
    std::fs::write(owner_path(), raw)
        .map_err(|e| format!("Failed to write Chrome ownership metadata: {e}"))
}

fn remove_owner() {
    let _ = std::fs::remove_file(owner_path());
}

fn process_command(pid: u32) -> Option<String> {
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let command = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if command.is_empty() {
        None
    } else {
        Some(command)
    }
}

/// Pull one `--flag=value` out of a `ps -o command=` line. Values may
/// contain spaces (the durable profile dir lives under "Application
/// Support"), and ps doesn't quote them — so the value runs until the
/// next ` --` flag or end of line. Whitespace-splitting can never match
/// such an argument.
fn extract_arg_value<'a>(command: &'a str, key: &str) -> Option<&'a str> {
    let start = command.find(key)? + key.len();
    let rest = &command[start..];
    let end = rest.find(" --").unwrap_or(rest.len());
    Some(rest[..end].trim())
}

fn command_matches_owner(command: &str, owner: &ChromeOwner) -> bool {
    let port_arg = format!("--remote-debugging-port={}", owner.port);

    owner.port == CDP_PORT
        && command.split_whitespace().any(|arg| arg == port_arg)
        && extract_arg_value(command, "--user-data-dir=") == Some(owner.user_data_dir.as_str())
}

fn classify_chrome(
    pid: Option<u32>,
    owner: Option<&ChromeOwner>,
    command: Option<&str>,
) -> ChromeStatus {
    match (pid, owner) {
        (Some(pid), Some(owner))
            if owner.pid == pid
                && command
                    .map(|command| command_matches_owner(command, owner))
                    .unwrap_or(false) =>
        {
            ChromeStatus {
                running: true,
                pid: Some(pid),
                ownership: ChromeOwnershipState::Owned,
            }
        }
        (Some(pid), _) => ChromeStatus {
            running: true,
            pid: Some(pid),
            ownership: ChromeOwnershipState::External,
        },
        (None, Some(owner)) => ChromeStatus {
            running: false,
            pid: Some(owner.pid),
            ownership: ChromeOwnershipState::Stale,
        },
        (None, None) => ChromeStatus {
            running: false,
            pid: None,
            ownership: ChromeOwnershipState::Missing,
        },
    }
}

pub fn chrome_status() -> ChromeStatus {
    let pid = chrome_pid();
    let owner = read_owner();
    let command = pid.and_then(process_command);
    classify_chrome(pid, owner.as_ref(), command.as_deref())
}

fn signal_pid(pid: u32, signal: i32) -> std::io::Result<()> {
    let rc = unsafe { libc::kill(pid as i32, signal) };
    if rc == -1 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn pid_exists(pid: u32) -> bool {
    match signal_pid(pid, 0) {
        Ok(()) => true,
        Err(e) => e.raw_os_error() != Some(libc::ESRCH),
    }
}

fn wait_for_pid_exit(pid: u32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !pid_exists(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    !pid_exists(pid)
}

fn terminate_started_chrome(pid: u32) -> Result<(), String> {
    match signal_pid(pid, libc::SIGTERM) {
        Ok(()) => {
            if wait_for_pid_exit(pid, Duration::from_secs(5)) {
                Ok(())
            } else {
                Err(format!("Chrome pid {pid} did not exit after SIGTERM"))
            }
        }
        Err(e) if e.raw_os_error() == Some(libc::ESRCH) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

pub fn is_chrome_running() -> bool {
    chrome_pid().is_some()
}

pub fn start_chrome(headless: bool) -> Result<(), String> {
    let status = chrome_status();
    if status.running {
        eprintln!(
            "\x1b[2mChrome CDP :{CDP_PORT} already up ({})\x1b[0m",
            status.ownership.label()
        );
        return Ok(());
    }
    if status.ownership == ChromeOwnershipState::Stale {
        remove_owner();
    }

    let chrome = find_chrome().ok_or("Chrome not found. Install Google Chrome.")?;

    let data_dir = chrome_data_dir();
    migrate_legacy_profile(&data_dir);
    std::fs::create_dir_all(&data_dir).ok();

    let mut args = vec![
        format!("--remote-debugging-port={CDP_PORT}"),
        format!("--user-data-dir={}", data_dir.display()),
        "--no-first-run".to_string(),
        "--disable-default-apps".to_string(),
    ];

    if headless {
        eprintln!("\x1b[1mStarting Chrome...\x1b[0m");
        args.extend([
            "--headless=new".to_string(),
            "--disable-blink-features=AutomationControlled".to_string(),
            "--disable-gpu".to_string(),
        ]);
    } else {
        eprintln!("\x1b[1mStarting Chrome (visible)...\x1b[0m");
    }

    let mut child = Command::new(&chrome)
        .args(&args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to start Chrome: {e}"))?;

    // Wait up to 5s for Chrome to start listening
    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(250));
        if let Some(pid) = chrome_pid() {
            let owner = ChromeOwner {
                pid,
                spawned_pid: child.id(),
                port: CDP_PORT,
                chrome_path: chrome.to_string_lossy().into_owned(),
                user_data_dir: chrome_data_dir().display().to_string(),
            };
            if let Err(e) = write_owner(&owner) {
                let cleanup = terminate_started_chrome(pid);
                remove_owner();
                if let Err(cleanup_err) = cleanup {
                    return Err(format!(
                        "{e}; also failed to stop newly started Chrome pid {pid}: {cleanup_err}"
                    ));
                }
                return Err(e);
            }
            eprintln!("\x1b[32mChrome ready  pid={pid}\x1b[0m");
            return Ok(());
        }
    }

    // Timed out: the process we spawned started but never bound the CDP
    // port. Reap it so it doesn't linger holding the profile lock and get
    // misclassified as an External Chrome on the next start.
    let _ = child.kill();
    let _ = child.wait();
    Err("Chrome failed to start within 5 seconds".to_string())
}

pub fn stop_chrome() {
    let status = chrome_status();
    match (status.running, status.pid, status.ownership) {
        (true, Some(pid), ChromeOwnershipState::Owned) => {
            eprintln!("\x1b[1mStopping Chrome (pid {pid})...\x1b[0m");
            match signal_pid(pid, libc::SIGTERM) {
                Ok(()) => {
                    if wait_for_pid_exit(pid, Duration::from_secs(5)) {
                        remove_owner();
                        eprintln!("\x1b[32mChrome stopped\x1b[0m");
                    } else {
                        eprintln!(
                            "\x1b[31mChrome pid {pid} did not exit; ownership metadata retained\x1b[0m"
                        );
                    }
                }
                Err(e) if e.raw_os_error() == Some(libc::ESRCH) => {
                    remove_owner();
                    eprintln!(
                        "\x1b[2mChrome already stopped; removed stale ownership metadata\x1b[0m"
                    );
                }
                Err(e) => {
                    eprintln!("\x1b[31mFailed to stop Chrome pid {pid}: {e}\x1b[0m");
                }
            }
        }
        (true, Some(pid), ownership) => {
            eprintln!(
                "\x1b[2mChrome CDP :{CDP_PORT} is {ownership} (pid {pid}); leaving it running\x1b[0m",
                ownership = ownership.label()
            );
        }
        (false, _, ChromeOwnershipState::Stale) => {
            remove_owner();
            eprintln!("\x1b[2mChrome not running; removed stale ownership metadata\x1b[0m");
        }
        _ => {
            eprintln!("\x1b[2mChrome not running\x1b[0m");
        }
    }
}

pub fn ensure_chrome() -> Result<(), String> {
    start_chrome(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner(pid: u32) -> ChromeOwner {
        ChromeOwner {
            pid,
            spawned_pid: pid,
            port: CDP_PORT,
            chrome_path: "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".to_string(),
            user_data_dir: chrome_data_dir().display().to_string(),
        }
    }

    fn owned_command() -> String {
        format!(
            "Google Chrome --remote-debugging-port={CDP_PORT} --user-data-dir={}",
            chrome_data_dir().display()
        )
    }

    #[test]
    fn command_matching_requires_port_and_user_data_dir() {
        let owner = owner(42);

        assert!(command_matches_owner(&owned_command(), &owner));
        assert!(!command_matches_owner(
            "Google Chrome --remote-debugging-port=9222",
            &owner
        ));
        assert!(!command_matches_owner(
            &format!("Google Chrome --user-data-dir={}", chrome_data_dir().display()),
            &owner
        ));
        assert!(!command_matches_owner(
            &format!(
                "Google Chrome --remote-debugging-port={CDP_PORT} --user-data-dir={}-other",
                chrome_data_dir().display()
            ),
            &owner
        ));
    }

    #[test]
    fn classifies_owned_chrome_when_pid_and_command_match() {
        let owner = owner(42);

        let status = classify_chrome(Some(42), Some(&owner), Some(&owned_command()));

        assert!(status.running);
        assert_eq!(status.pid, Some(42));
        assert_eq!(status.ownership, ChromeOwnershipState::Owned);
    }

    #[test]
    fn classifies_external_chrome_without_metadata() {
        let status = classify_chrome(Some(42), None, Some(&owned_command()));

        assert!(status.running);
        assert_eq!(status.pid, Some(42));
        assert_eq!(status.ownership, ChromeOwnershipState::External);
    }

    #[test]
    fn classifies_external_chrome_when_pid_differs_from_metadata() {
        let owner = owner(7);

        let status = classify_chrome(Some(42), Some(&owner), Some(&owned_command()));

        assert!(status.running);
        assert_eq!(status.pid, Some(42));
        assert_eq!(status.ownership, ChromeOwnershipState::External);
    }

    #[test]
    fn classifies_external_chrome_when_command_is_not_plausible() {
        let owner = owner(42);

        let status = classify_chrome(Some(42), Some(&owner), Some("Google Chrome"));

        assert!(status.running);
        assert_eq!(status.pid, Some(42));
        assert_eq!(status.ownership, ChromeOwnershipState::External);
    }

    #[test]
    fn classifies_external_chrome_when_command_is_missing() {
        let owner = owner(42);

        let status = classify_chrome(Some(42), Some(&owner), None);

        assert!(status.running);
        assert_eq!(status.pid, Some(42));
        assert_eq!(status.ownership, ChromeOwnershipState::External);
    }

    #[test]
    fn classifies_stale_metadata_when_no_cdp_pid_exists() {
        let owner = owner(42);

        let status = classify_chrome(None, Some(&owner), None);

        assert!(!status.running);
        assert_eq!(status.pid, Some(42));
        assert_eq!(status.ownership, ChromeOwnershipState::Stale);
    }

    #[test]
    fn classifies_missing_metadata_and_missing_cdp_pid() {
        let status = classify_chrome(None, None, None);

        assert!(!status.running);
        assert_eq!(status.pid, None);
        assert_eq!(status.ownership, ChromeOwnershipState::Missing);
    }
}
