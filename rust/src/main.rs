use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "scavenger", about = "Marketplace monitoring daemon + TUI")]
struct Cli {
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
    /// Trigger an immediate poll for a profile
    Poll {
        /// Profile name to poll
        profile: String,
    },
    /// List configured profiles
    ListProfiles,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        None => {
            todo!("launch TUI")
        }
        Some(Command::Daemon) => {
            todo!("start daemon")
        }
        Some(Command::Ctl { action }) => match action {
            CtlAction::Status => todo!("ctl status"),
            CtlAction::Stop => todo!("ctl stop"),
            CtlAction::Poll { profile: _ } => todo!("ctl poll"),
            CtlAction::ListProfiles => todo!("ctl list-profiles"),
        },
    }
}
