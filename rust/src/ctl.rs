use std::io::{BufRead, BufReader, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use serde_json::{json, Value};

use crate::config::{load_ai_config, load_config, AppConfig};

/// Send a JSON command to the daemon over its Unix socket and return the response.
fn send(socket_path: &Path, command: Value) -> Value {
    let stream = match UnixStream::connect(socket_path) {
        Ok(s) => s,
        Err(e) => {
            let msg = if e.kind() == std::io::ErrorKind::NotFound
                || e.kind() == std::io::ErrorKind::ConnectionRefused
            {
                "Daemon not running".to_string()
            } else {
                format!("Socket error: {e}")
            };
            return json!({"status": "error", "message": msg});
        }
    };

    if let Err(e) = stream.set_read_timeout(Some(Duration::from_secs(5))) {
        return json!({"status": "error", "message": format!("Socket error: {e}")});
    }
    if let Err(e) = stream.set_write_timeout(Some(Duration::from_secs(5))) {
        return json!({"status": "error", "message": format!("Socket error: {e}")});
    }

    let mut writer = stream.try_clone().unwrap();
    let payload = command.to_string() + "\n";
    if let Err(e) = writer.write_all(payload.as_bytes()) {
        return json!({"status": "error", "message": format!("Socket error: {e}")});
    }
    let _ = writer.shutdown(Shutdown::Write);

    let mut reader = BufReader::new(writer);
    let mut buf = String::new();
    match reader.read_line(&mut buf) {
        Ok(0) => json!({"status": "error", "message": "Empty response from daemon"}),
        Ok(n) if n > 65536 => json!({"status": "error", "message": "Response too large"}),
        Ok(_) => serde_json::from_str(buf.trim()).unwrap_or_else(|e| {
            json!({"status": "error", "message": format!("Invalid JSON: {e}")})
        }),
        Err(e) => json!({"status": "error", "message": format!("Read error: {e}")}),
    }
}

pub fn cmd_list_profiles(config: &AppConfig) {
    for p in &config.profiles {
        let state = if p.enabled { "enabled" } else { "disabled" };
        println!("  [{state}] {} ({})", p.name, p.id);
        println!(
            "           sources: {}  priority: {}",
            p.sources.join(", "),
            p.alert_priority
        );
    }
}

pub fn cmd_status(config: &AppConfig) {
    let resp = send(&config.socket_path(), json!({"command": "status"}));
    if resp["status"] == "ok" {
        println!("running");
    } else {
        eprintln!("error: {}", resp["message"].as_str().unwrap_or("unknown"));
    }
}

pub fn cmd_poll(config: &AppConfig, profile_name: &str) {
    let matching = config
        .profiles
        .iter()
        .find(|p| p.name == profile_name || p.id == profile_name);

    let Some(profile) = matching else {
        eprintln!("Error: profile not found: {profile_name}");
        std::process::exit(1);
    };

    let resp = send(
        &config.socket_path(),
        json!({"command": "poll", "profile_id": profile.id}),
    );
    if resp["status"] == "ok" {
        println!("ok");
    } else {
        eprintln!("error: {}", resp["message"].as_str().unwrap_or("unknown"));
    }
}

pub fn cmd_stop(config: &AppConfig) {
    let resp = send(&config.socket_path(), json!({"command": "shutdown"}));
    if resp["status"] == "ok" {
        println!("ok");
    } else {
        eprintln!("error: {}", resp["message"].as_str().unwrap_or("unknown"));
    }
}

pub fn cmd_start(config_path: &Path) -> Result<()> {
    let _config = load_config(config_path)?;
    let ai_config = load_ai_config(config_path)?;

    if ai_config.enabled {
        println!(
            "Starting SCAVENGER daemon (AI: {})...",
            ai_config.filter_model
        );
    } else {
        println!("Starting SCAVENGER daemon...");
    }

    todo!("Daemon implementation not yet available")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_builds_error_on_missing_socket() {
        let resp = send(Path::new("/tmp/scavenger-test-nonexistent.sock"), json!({"command": "status"}));
        assert_eq!(resp["status"], "error");
        let msg = resp["message"].as_str().unwrap();
        assert!(
            msg.contains("Daemon not running") || msg.contains("Socket error"),
            "unexpected message: {msg}"
        );
    }

    #[test]
    fn cmd_list_profiles_formats_output() {
        use crate::models::{Keyword, Profile};

        let config = AppConfig {
            global_config: crate::config::GlobalConfig::default(),
            profiles: vec![
                Profile {
                    id: "bikes".into(),
                    name: "Mountain Bikes".into(),
                    keywords: vec![Keyword::Single("bike".into())],
                    negative_keywords: vec![],
                    sources: vec!["ebay".into(), "craigslist".into()],
                    price_min: None,
                    price_max: Some(500.0),
                    poll_interval_sec: 3600,
                    alert_priority: "high".into(),
                    enabled: true,
                    tags: vec![],
                    escalation_keywords: vec![],
                    location_radius_mi: None,
                },
                Profile {
                    id: "records".into(),
                    name: "Vinyl Records".into(),
                    keywords: vec![Keyword::Single("vinyl".into())],
                    negative_keywords: vec![],
                    sources: vec!["ebay".into()],
                    price_min: None,
                    price_max: None,
                    poll_interval_sec: 7200,
                    alert_priority: "normal".into(),
                    enabled: false,
                    tags: vec![],
                    escalation_keywords: vec![],
                    location_radius_mi: None,
                },
            ],
        };

        // Just verify it doesn't panic; output goes to stdout.
        cmd_list_profiles(&config);
    }
}
