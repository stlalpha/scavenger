use std::path::{Path, PathBuf};
use std::process;

use clap::{Parser, Subcommand};

use scavenger::chrome;
use scavenger::config::{load_config, load_ai_config};
use scavenger::ctl;

fn default_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".config")
        .join("scavenger")
        .join("config.toml")
}

#[derive(Parser)]
#[command(name = "scavenger", about = "SCAVENGER — continuous web intelligence terminal")]
struct Cli {
    #[arg(long, global = true, default_value_os_t = default_config_path())]
    config: PathBuf,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Start Chrome + daemon, then launch TUI
    Start,
    /// Stop daemon and Chrome
    Stop,
    /// Show status of Chrome, daemon, and DB
    Status,
    /// Tail the daemon log
    Log,
    /// Start Chrome visible for manual login
    Login,
    /// Stop everything, then start again
    Restart,
    /// Start the daemon process (usually called via `start`)
    Daemon,
    /// Wipe search data (listings, poll state, cached images). Profiles in
    /// config.toml, sops-encrypted secrets, and Chrome logins are kept.
    Reset {
        /// Delete listings + price history only; keep per-source poll
        /// timestamps so the next start doesn't re-poll everything at once
        #[arg(long)]
        listings_only: bool,
        /// Skip the confirmation prompt
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Control the running daemon directly
    Ctl {
        #[command(subcommand)]
        action: CtlAction,
    },
}

#[derive(Subcommand)]
enum CtlAction {
    Status,
    Stop,
    Poll { profile: String },
    Reload,
    ListProfiles,
}

fn load_or_die(path: &Path) -> scavenger::config::AppConfig {
    match load_config(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: {e}");
            process::exit(1);
        }
    }
}

fn daemon_log_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
        .join("scavenger")
        .join("daemon.log")
}

fn ensure_daemon(config_path: &Path) {
    let config = load_or_die(config_path);
    if ctl::daemon_alive(&config) {
        eprintln!("\x1b[2mDaemon already up\x1b[0m");
        return;
    }

    eprintln!("\x1b[1mStarting daemon...\x1b[0m");
    let log_path = daemon_log_path();
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    // Spawn daemon as a background process
    let exe = std::env::current_exe().expect("can't find own executable");
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .expect("can't open daemon log");
    let log_err = log_file.try_clone().expect("can't clone log file");

    std::process::Command::new(exe)
        .args(["--config", &config_path.to_string_lossy(), "daemon"])
        .stdout(log_file)
        .stderr(log_err)
        .stdin(std::process::Stdio::null())
        .spawn()
        .expect("failed to spawn daemon");

    // Wait up to 15s for daemon to come up
    for i in 0..30 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        let config = load_or_die(config_path);
        if ctl::daemon_alive(&config) {
            eprintln!("\x1b[32mDaemon ready\x1b[0m");
            return;
        }
        if i % 10 == 9 {
            eprint!(".");
        }
    }
    eprintln!("\x1b[31mDaemon not responding after 15s\x1b[0m");
    process::exit(1);
}

fn cmd_start(config_path: &Path) {
    if let Err(e) = chrome::ensure_chrome() {
        eprintln!("\x1b[31m{e}\x1b[0m");
        process::exit(1);
    }
    ensure_daemon(config_path);
    eprintln!();
    cmd_status(config_path);
}

fn cmd_stop(config_path: &Path) {
    let config = load_or_die(config_path);
    if ctl::daemon_alive(&config) {
        eprintln!("\x1b[1mStopping daemon...\x1b[0m");
        ctl::cmd_stop(&config);
        std::thread::sleep(std::time::Duration::from_secs(1));
        eprintln!("\x1b[32mDaemon stopped\x1b[0m");
    } else {
        eprintln!("\x1b[2mDaemon not running\x1b[0m");
    }
    chrome::stop_chrome();
}

fn cmd_status(config_path: &Path) {
    eprintln!("\x1b[1mscavenger status\x1b[0m");
    eprintln!();

    let chrome_status = chrome::chrome_status();
    if chrome_status.running {
        match chrome_status.pid {
            Some(pid) => eprintln!(
                "  \x1b[32mchrome   up\x1b[0m  \x1b[2m{} pid={pid}\x1b[0m",
                chrome_status.ownership.label()
            ),
            None => eprintln!(
                "  \x1b[32mchrome   up\x1b[0m  \x1b[2m{}\x1b[0m",
                chrome_status.ownership.label()
            ),
        }
    } else if chrome_status.ownership == chrome::ChromeOwnershipState::Stale {
        match chrome_status.pid {
            Some(pid) => eprintln!("  \x1b[31mchrome   down\x1b[0m  \x1b[2mstale pid={pid}\x1b[0m"),
            None => eprintln!("  \x1b[31mchrome   down\x1b[0m  \x1b[2mstale\x1b[0m"),
        }
    } else {
        eprintln!("  \x1b[31mchrome   down\x1b[0m  \x1b[2mmissing\x1b[0m");
    }

    let config = load_or_die(config_path);
    if ctl::daemon_alive(&config) {
        eprintln!("  \x1b[32mdaemon   up\x1b[0m");
    } else {
        eprintln!("  \x1b[31mdaemon   down\x1b[0m");
    }

    let db_path = config.db_path();
    if db_path.exists() {
        if let Ok(meta) = std::fs::metadata(&db_path) {
            let size_kb = meta.len() / 1024;
            eprintln!("  \x1b[2mdb       {size_kb}K  {}\x1b[0m", db_path.display());
        }
    }
}

fn cmd_log() {
    let log = daemon_log_path();
    if !log.exists() {
        eprintln!("\x1b[31mNo log at {}\x1b[0m", log.display());
        process::exit(1);
    }
    let status = std::process::Command::new("tail")
        .args(["-f", &log.to_string_lossy()])
        .status()
        .expect("failed to run tail");
    process::exit(status.code().unwrap_or(1));
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        None | Some(Command::Start) => {
            // Default: start everything and launch TUI
            cmd_start(&cli.config);
            eprintln!();
            let config = load_or_die(&cli.config);
            match scavenger::tui::App::new(config, cli.config.clone()) {
                Ok(mut app) => {
                    if let Err(e) = app.run() {
                        eprintln!("TUI error: {e}");
                        process::exit(1);
                    }
                }
                Err(e) => {
                    eprintln!("Error: {e}");
                    process::exit(1);
                }
            }
        }
        Some(Command::Stop) => cmd_stop(&cli.config),
        Some(Command::Status) => cmd_status(&cli.config),
        Some(Command::Log) => cmd_log(),
        Some(Command::Login) => {
            if let Err(e) = chrome::start_chrome(false) {
                eprintln!("\x1b[31m{e}\x1b[0m");
                process::exit(1);
            }
            eprintln!("Log in to your accounts in the Chrome window.");
            eprintln!("Press Ctrl+C when done.");
            loop { std::thread::sleep(std::time::Duration::from_secs(60)); }
        }
        Some(Command::Restart) => {
            cmd_stop(&cli.config);
            eprintln!();
            cmd_start(&cli.config);
        }
        Some(Command::Daemon) => {
            let config = load_or_die(&cli.config);
            let ai_config = load_ai_config(&cli.config).ok();
            let daemon = scavenger::daemon::Daemon::new(config, ai_config, Some(cli.config));
            let rt = tokio::runtime::Runtime::new().expect("failed to create runtime");
            if let Err(e) = rt.block_on(daemon.run()) {
                eprintln!("Daemon error: {e}");
                process::exit(1);
            }
        }
        Some(Command::Reset { listings_only, yes }) => {
            let config = load_or_die(&cli.config);
            if ctl::daemon_alive(&config) {
                eprintln!("\x1b[31mDaemon is running — stop it first: scavenger stop\x1b[0m");
                eprintln!("(the TUI must be closed too; both hold the database open)");
                process::exit(1);
            }
            let opts = scavenger::reset::ResetOptions { listings_only };
            let plan = scavenger::reset::plan(&config, &opts);
            let listings = plan
                .listing_count
                .map(|n| n.to_string())
                .unwrap_or_else(|| "?".into());
            if listings_only {
                eprintln!(
                    "Will delete {listings} listings (+ price history) from {}.",
                    plan.db_path.display()
                );
                eprintln!("Poll timestamps are kept — no all-at-once re-poll on next start.");
            } else {
                eprintln!(
                    "Will delete {} ({listings} listings, {} KB) and {} cached images in {}.",
                    plan.db_path.display(),
                    plan.db_bytes / 1024,
                    plan.image_count,
                    plan.image_cache_path.display()
                );
                eprintln!("All profiles will re-poll immediately on next daemon start.");
            }
            eprintln!("Profiles, sops secrets, and Chrome logins are untouched.");
            if !yes {
                eprint!("Proceed? [y/N] ");
                let mut answer = String::new();
                if std::io::stdin().read_line(&mut answer).is_err()
                    || !answer.trim().eq_ignore_ascii_case("y")
                {
                    eprintln!("Aborted.");
                    process::exit(1);
                }
            }
            match scavenger::reset::run(&config, &opts) {
                Ok(out) => {
                    if listings_only {
                        println!("Deleted {} listings; database compacted.", out.listings_deleted);
                    } else {
                        println!(
                            "Removed database ({} listings) and {} cached images. Fresh start on next run.",
                            out.listings_deleted, out.images_removed
                        );
                    }
                }
                Err(e) => {
                    eprintln!("\x1b[31mReset failed: {e}\x1b[0m");
                    process::exit(1);
                }
            }
        }
        Some(Command::Ctl { action }) => {
            let config = load_or_die(&cli.config);
            match action {
                CtlAction::Status => ctl::cmd_status(&config),
                CtlAction::Stop => ctl::cmd_stop(&config),
                CtlAction::Poll { profile } => ctl::cmd_poll(&config, &profile),
                CtlAction::Reload => ctl::cmd_reload(&config),
                CtlAction::ListProfiles => ctl::cmd_list_profiles(&config),
            }
        }
    }
}
