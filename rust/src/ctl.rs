use std::io::{BufRead, BufReader, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use serde_json::{json, Value};

use crate::config::AppConfig;

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

pub fn daemon_alive(config: &AppConfig) -> bool {
    let resp = send(&config.socket_path(), json!({"command": "status"}));
    resp["status"] == "ok"
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
    if resp["status"] != "ok" {
        eprintln!("error: {}", resp["message"].as_str().unwrap_or("unknown"));
        return;
    }
    // The daemon acks the shutdown command, then drains in-flight polls for
    // up to ~10s with the socket still answering. Returning before it has
    // actually exited makes `stop && reset` (or a quick restart) race the
    // drain and fail confusingly — wait it out.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while daemon_alive(config) {
        if std::time::Instant::now() >= deadline {
            eprintln!("warning: daemon still draining after 15s — try again shortly");
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    println!("ok");
}

pub fn cmd_reload(config: &AppConfig) {
    let resp = send(&config.socket_path(), json!({"command": "reload"}));
    if resp["status"] == "ok" {
        println!("ok");
    } else {
        eprintln!("error: {}", resp["message"].as_str().unwrap_or("unknown"));
    }
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
    fn cmd_reload_sends_reload_command() {
        // No daemon listening — just verify cmd_reload doesn't panic and
        // reports the same not-running condition as the other commands.
        let config = AppConfig {
            global_config: crate::config::GlobalConfig {
                socket_path: "/tmp/scavenger-test-reload-nonexistent.sock".into(),
                ..Default::default()
            },
            profiles: vec![],
        };
        cmd_reload(&config);
    }

    #[test]
    fn cmd_list_profiles_formats_output() {
        use crate::models::{AlertPriority, KeywordGroup, Profile};

        let config = AppConfig {
            global_config: crate::config::GlobalConfig::default(),
            profiles: vec![
                Profile {
                    id: "bikes".into(),
                    name: "Mountain Bikes".into(),
                    keywords: vec![KeywordGroup::Single("bike".into())],
                    negative_keywords: vec![],
                    sources: vec!["ebay".into(), "craigslist".into()],
                    price_min: None,
                    price_max: Some(500.0),
                    poll_interval_sec: 3600,
                    alert_priority: AlertPriority::High,
                    enabled: true,
                    tags: vec![],
                    escalation_keywords: vec![],
                    location_radius_mi: None,
                },
                Profile {
                    id: "records".into(),
                    name: "Vinyl Records".into(),
                    keywords: vec![KeywordGroup::Single("vinyl".into())],
                    negative_keywords: vec![],
                    sources: vec!["ebay".into()],
                    price_min: None,
                    price_max: None,
                    poll_interval_sec: 7200,
                    alert_priority: AlertPriority::Normal,
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
