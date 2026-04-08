mod config;
mod db;
mod models;
mod tui;

use std::path::PathBuf;

fn main() {
    let config_path = PathBuf::from(
        std::env::var("SCAVENGER_CONFIG")
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
                format!("{home}/.config/scavenger/config.toml")
            }),
    );

    let config = match config::AppConfig::load(&config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to load config: {e}");
            std::process::exit(1);
        }
    };

    let mut app = match tui::App::new(config) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Failed to initialize: {e}");
            std::process::exit(1);
        }
    };

    if let Err(e) = app.run() {
        eprintln!("Runtime error: {e}");
        std::process::exit(1);
    }
}
