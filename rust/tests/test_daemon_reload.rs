// Real reload-handler integration test (F6a): a real Daemon, started for
// real (real scheduler, real socket server, real registered handlers),
// driven purely through the socket protocol — the same path scavenger-ctl
// uses. Asserts the scheduler's actual job set and the status handler's
// profile summary both reflect an add/remove/disable diff after reload.

use std::io::Write;

use scavenger::config::load_config;
use scavenger::daemon::Daemon;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

fn write_config(path: &std::path::Path, db_path: &std::path::Path, socket_path: &std::path::Path, profiles_toml: &str) {
    let content = format!(
        r#"
[global]
db_path = "{}"
socket_path = "{}"

{}
"#,
        db_path.display(),
        socket_path.display(),
        profiles_toml
    );
    let mut f = std::fs::File::create(path).unwrap();
    f.write_all(content.as_bytes()).unwrap();
    f.flush().unwrap();
}

async fn send(socket_path: &std::path::Path, command: &serde_json::Value) -> serde_json::Value {
    let stream = UnixStream::connect(socket_path).await.unwrap();
    let (reader, mut writer) = stream.into_split();
    let mut msg = serde_json::to_string(command).unwrap();
    msg.push('\n');
    writer.write_all(msg.as_bytes()).await.unwrap();
    writer.shutdown().await.unwrap();

    let mut reader = BufReader::new(reader);
    let mut response = String::new();
    reader.read_line(&mut response).await.unwrap();
    serde_json::from_str(&response).unwrap()
}

#[tokio::test]
async fn reload_handler_updates_scheduler_jobs_and_profile_summary() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.toml");
    let db_path = dir.path().join("scavenger.db");
    let socket_path = dir.path().join("daemon.sock");

    // No `sources` on any profile: the scheduler's first tick fires
    // immediately for a never-polled profile, and we don't want that
    // invoking a real plugin fetch here.
    write_config(
        &config_path,
        &db_path,
        &socket_path,
        r#"
[[profiles]]
id = "p1"
name = "P1"
keywords = ["x"]
sources = []
poll_interval_sec = 3600

[[profiles]]
id = "p2"
name = "P2"
keywords = ["x"]
sources = []
poll_interval_sec = 3600

[[profiles]]
id = "p3"
name = "P3"
keywords = ["x"]
sources = []
poll_interval_sec = 3600
"#,
    );

    let config = load_config(&config_path).expect("initial config should load");
    let daemon = Daemon::new(config, None, Some(config_path.clone()));
    daemon.start().await.expect("daemon should start");

    assert!(daemon.has_profile_job("p1").await);
    assert!(daemon.has_profile_job("p2").await);
    assert!(daemon.has_profile_job("p3").await);

    // Diff against the file: p1 unchanged, p2 removed, p3 disabled, p4 added.
    write_config(
        &config_path,
        &db_path,
        &socket_path,
        r#"
[[profiles]]
id = "p1"
name = "P1"
keywords = ["x"]
sources = []
poll_interval_sec = 3600

[[profiles]]
id = "p3"
name = "P3"
keywords = ["x"]
sources = []
poll_interval_sec = 3600
enabled = false

[[profiles]]
id = "p4"
name = "P4"
keywords = ["x"]
sources = []
poll_interval_sec = 3600
"#,
    );

    let resp = send(&socket_path, &serde_json::json!({"command": "reload"})).await;
    assert_eq!(resp["status"], "ok", "reload should succeed: {resp:?}");

    assert!(daemon.has_profile_job("p1").await, "unchanged profile stays registered");
    assert!(!daemon.has_profile_job("p2").await, "removed profile must be dropped");
    assert!(!daemon.has_profile_job("p3").await, "disabled profile must be dropped");
    assert!(daemon.has_profile_job("p4").await, "new profile must be registered");

    let status = send(&socket_path, &serde_json::json!({"command": "status"})).await;
    assert_eq!(status["status"], "ok");
    let profiles: Vec<String> = status["data"]["profiles"]
        .as_array()
        .expect("profiles must be an array")
        .iter()
        .map(|v| v.as_str().expect("profiles must be flat id strings").to_string())
        .collect();
    assert_eq!(profiles, vec!["p1".to_string(), "p4".to_string()]);

    daemon.shutdown().await;
}
