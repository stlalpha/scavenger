use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand};

use scavenger::config::{load_config, load_ai_config};
use scavenger::ctl;

fn default_config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("scavenger")
        .join("config.toml")
}

#[derive(Parser)]
#[command(name = "scavenger", about = "Continuous web intelligence terminal")]
struct Cli {
    #[arg(long, global = true, default_value_os_t = default_config_path())]
    config: PathBuf,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Start the background daemon
    Daemon,
    /// Control the running daemon
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

fn main() {
    let cli = Cli::parse();

    match cli.command {
        None => {
            let config = match load_config(&cli.config) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error: {e}");
                    process::exit(1);
                }
            };
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
        Some(Command::Daemon) => {
            let config = match load_config(&cli.config) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error: {e}");
                    process::exit(1);
                }
            };
            let ai_config = load_ai_config(&cli.config).ok();
            let daemon = scavenger::daemon::Daemon::new(config, ai_config, Some(cli.config));
            let rt = tokio::runtime::Runtime::new().expect("failed to create runtime");
            if let Err(e) = rt.block_on(daemon.run()) {
                eprintln!("Daemon error: {e}");
                process::exit(1);
            }
        }
        Some(Command::Ctl { action }) => {
            let config = match load_config(&cli.config) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error: {e}");
                    process::exit(1);
                }
            };
            match action {
                CtlAction::Status => ctl::cmd_status(&config),
                CtlAction::Stop => ctl::cmd_stop(&config),
                CtlAction::Poll { profile } => ctl::cmd_poll(&config, &profile),
                CtlAction::ListProfiles => ctl::cmd_list_profiles(&config),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parse_no_subcommand() {
        let cli = Cli::try_parse_from(["scavenger"]).unwrap();
        assert!(cli.command.is_none());
    }

    #[test]
    fn parse_daemon() {
        let cli = Cli::try_parse_from(["scavenger", "daemon"]).unwrap();
        assert!(matches!(cli.command, Some(Command::Daemon)));
    }

    #[test]
    fn parse_ctl_status() {
        let cli = Cli::try_parse_from(["scavenger", "ctl", "status"]).unwrap();
        assert!(matches!(cli.command, Some(Command::Ctl { action: CtlAction::Status })));
    }

    #[test]
    fn parse_ctl_poll() {
        let cli = Cli::try_parse_from(["scavenger", "ctl", "poll", "bikes"]).unwrap();
        match cli.command {
            Some(Command::Ctl { action: CtlAction::Poll { profile } }) => assert_eq!(profile, "bikes"),
            _ => panic!("expected ctl poll"),
        }
    }

    #[test]
    fn parse_custom_config() {
        let cli = Cli::try_parse_from(["scavenger", "--config", "/tmp/test.toml"]).unwrap();
        assert_eq!(cli.config, PathBuf::from("/tmp/test.toml"));
    }
}
