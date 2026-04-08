use std::path::PathBuf;
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
    ListProfiles,
}

fn load_or_die(path: &PathBuf) -> scavenger::config::AppConfig {
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

fn ensure_daemon(config_path: &PathBuf) {
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

fn cmd_start(config_path: &PathBuf) {
    if let Err(e) = chrome::ensure_chrome() {
        eprintln!("\x1b[31m{e}\x1b[0m");
        process::exit(1);
    }
    ensure_daemon(config_path);
    eprintln!();
    cmd_status(config_path);
}

fn cmd_stop(config_path: &PathBuf) {
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

fn cmd_status(config_path: &PathBuf) {
    eprintln!("\x1b[1mscavenger status\x1b[0m");
    eprintln!();

    if chrome::is_chrome_running() {
        eprintln!("  \x1b[32mchrome   up\x1b[0m");
    } else {
        eprintln!("  \x1b[31mchrome   down\x1b[0m");
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
            match scavenger::tui::App::new(config) {
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
        Some(Command::Ctl { action }) => {
            let config = load_or_die(&cli.config);
            match action {
                CtlAction::Status => ctl::cmd_status(&config),
                CtlAction::Stop => ctl::cmd_stop(&config),
                CtlAction::Poll { profile } => ctl::cmd_poll(&config, &profile),
                CtlAction::ListProfiles => ctl::cmd_list_profiles(&config),
            }
        }
    }
}
