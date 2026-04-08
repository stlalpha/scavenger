mod config;
mod ctl;
mod models;

use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand};

use config::{default_config_path, load_config};

#[derive(Parser)]
#[command(name = "scavenger", about = "Continuous web intelligence terminal")]
struct Cli {
    /// Path to config file
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
    /// Show daemon status
    Status,
    /// Stop the daemon
    Stop,
    /// Trigger immediate poll for a profile
    Poll {
        /// Profile name or id
        profile: String,
    },
    /// List all configured profiles
    ListProfiles,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        None => {
            let app_config = match load_config(&cli.config) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error: {e}");
                    process::exit(1);
                }
            };
            let _ = app_config;
            todo!("TUI launch not yet implemented")
        }
        Some(Command::Daemon) => {
            if let Err(e) = ctl::cmd_start(&cli.config) {
                eprintln!("Error: {e}");
                process::exit(1);
            }
        }
        Some(Command::Ctl { action }) => {
            let app_config = match load_config(&cli.config) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error: {e}");
                    process::exit(1);
                }
            };
            match action {
                CtlAction::Status => ctl::cmd_status(&app_config),
                CtlAction::Stop => ctl::cmd_stop(&app_config),
                CtlAction::Poll { profile } => ctl::cmd_poll(&app_config, &profile),
                CtlAction::ListProfiles => ctl::cmd_list_profiles(&app_config),
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
        assert!(matches!(
            cli.command,
            Some(Command::Ctl {
                action: CtlAction::Status
            })
        ));
    }

    #[test]
    fn parse_ctl_stop() {
        let cli = Cli::try_parse_from(["scavenger", "ctl", "stop"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Ctl {
                action: CtlAction::Stop
            })
        ));
    }

    #[test]
    fn parse_ctl_poll() {
        let cli = Cli::try_parse_from(["scavenger", "ctl", "poll", "bikes"]).unwrap();
        match cli.command {
            Some(Command::Ctl {
                action: CtlAction::Poll { profile },
            }) => assert_eq!(profile, "bikes"),
            _ => panic!("expected ctl poll"),
        }
    }

    #[test]
    fn parse_ctl_list_profiles() {
        let cli = Cli::try_parse_from(["scavenger", "ctl", "list-profiles"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Ctl {
                action: CtlAction::ListProfiles
            })
        ));
    }

    #[test]
    fn parse_custom_config() {
        let cli = Cli::try_parse_from(["scavenger", "--config", "/tmp/test.toml"]).unwrap();
        assert_eq!(cli.config, PathBuf::from("/tmp/test.toml"));
        assert!(cli.command.is_none());
    }

    #[test]
    fn parse_config_before_subcommand() {
        let cli =
            Cli::try_parse_from(["scavenger", "--config", "/tmp/test.toml", "ctl", "status"])
                .unwrap();
        assert_eq!(cli.config, PathBuf::from("/tmp/test.toml"));
        assert!(matches!(
            cli.command,
            Some(Command::Ctl {
                action: CtlAction::Status
            })
        ));
    }
}
