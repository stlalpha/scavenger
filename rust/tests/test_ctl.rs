use std::io::Write;

use tempfile::NamedTempFile;

#[test]
fn load_config_parses_profiles() {
    let toml_content = r#"
[global]
db_path = "~/.local/share/scavenger/scavenger.db"
socket_path = "/tmp/test-daemon.sock"

[[profiles]]
id = "bikes"
name = "Mountain Bikes"
keywords = ["mountain bike", ["trek", "specialized"]]
sources = ["ebay", "craigslist"]
price_max = 500.0
alert_priority = "high"

[[profiles]]
id = "records"
name = "Vinyl Records"
keywords = ["vinyl"]
sources = ["ebay"]
enabled = false
"#;

    let mut f = NamedTempFile::new().unwrap();
    f.write_all(toml_content.as_bytes()).unwrap();
    f.flush().unwrap();

    // We can't call scavenger's internal load_config from an integration test directly,
    // so we test via the binary's CLI behavior.
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_scavenger"))
        .args(["--config", f.path().to_str().unwrap(), "ctl", "list-profiles"])
        .output()
        .expect("failed to run scavenger");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[enabled] Mountain Bikes (bikes)"));
    assert!(stdout.contains("[disabled] Vinyl Records (records)"));
    assert!(stdout.contains("sources: ebay, craigslist"));
    assert!(stdout.contains("priority: high"));
    assert!(stdout.contains("priority: normal"));
}

#[test]
fn ctl_status_reports_daemon_not_running() {
    let toml_content = r#"
[global]
socket_path = "/tmp/scavenger-test-no-daemon.sock"

[[profiles]]
id = "test"
name = "Test"
keywords = ["test"]
sources = ["ebay"]
"#;

    let mut f = NamedTempFile::new().unwrap();
    f.write_all(toml_content.as_bytes()).unwrap();
    f.flush().unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_scavenger"))
        .args(["--config", f.path().to_str().unwrap(), "ctl", "status"])
        .output()
        .expect("failed to run scavenger");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Daemon not running") || stderr.contains("Socket error"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn ctl_poll_unknown_profile_exits_nonzero() {
    let toml_content = r#"
[global]
socket_path = "/tmp/scavenger-test-no-daemon.sock"

[[profiles]]
id = "test"
name = "Test"
keywords = ["test"]
sources = ["ebay"]
"#;

    let mut f = NamedTempFile::new().unwrap();
    f.write_all(toml_content.as_bytes()).unwrap();
    f.flush().unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_scavenger"))
        .args([
            "--config",
            f.path().to_str().unwrap(),
            "ctl",
            "poll",
            "nonexistent",
        ])
        .output()
        .expect("failed to run scavenger");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("profile not found: nonexistent"));
}
