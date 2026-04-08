use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

const CDP_PORT: u16 = 9222;
const CHROME_DATA: &str = "/tmp/scavenger-chrome";

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

pub fn is_chrome_running() -> bool {
    chrome_pid().is_some()
}

pub fn start_chrome(headless: bool) -> Result<(), String> {
    if is_chrome_running() {
        eprintln!("\x1b[2mChrome CDP :{CDP_PORT} already up\x1b[0m");
        return Ok(());
    }

    let chrome = find_chrome().ok_or("Chrome not found. Install Google Chrome.")?;

    std::fs::create_dir_all(CHROME_DATA).ok();

    let mut args = vec![
        format!("--remote-debugging-port={CDP_PORT}"),
        format!("--user-data-dir={CHROME_DATA}"),
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

    Command::new(&chrome)
        .args(&args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to start Chrome: {e}"))?;

    // Wait up to 5s for Chrome to start listening
    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(250));
        if let Some(pid) = chrome_pid() {
            eprintln!("\x1b[32mChrome ready  pid={pid}\x1b[0m");
            return Ok(());
        }
    }

    Err("Chrome failed to start within 5 seconds".to_string())
}

pub fn stop_chrome() {
    if let Some(pid) = chrome_pid() {
        eprintln!("\x1b[1mStopping Chrome (pid {pid})...\x1b[0m");
        unsafe {
            libc::kill(pid as i32, libc::SIGTERM);
        }
        eprintln!("\x1b[32mChrome stopped\x1b[0m");
    } else {
        eprintln!("\x1b[2mChrome not running\x1b[0m");
    }
}

pub fn ensure_chrome() -> Result<(), String> {
    start_chrome(true)
}
