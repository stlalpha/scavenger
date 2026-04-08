use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "scavenger", about = "Marketplace monitoring daemon + TUI")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the background daemon
    Daemon,
    /// Daemon control commands
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
            // Launch TUI (default)
            todo!("TUI not yet implemented")
        }
        Some(Commands::Daemon) => {
            todo!("daemon not yet implemented")
        }
        Some(Commands::Ctl { action }) => match action {
            CtlAction::Status => todo!("ctl status"),
            CtlAction::Stop => todo!("ctl stop"),
            CtlAction::Poll { profile: _ } => todo!("ctl poll"),
            CtlAction::ListProfiles => todo!("ctl list-profiles"),
        },
    }
}
