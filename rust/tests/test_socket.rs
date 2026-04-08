use std::sync::Arc;

use scavenger::daemon::socket::SocketServer;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

async fn start_server(dir: &std::path::Path) -> Arc<SocketServer> {
    let socket_path = dir.join("test.sock");
    let server = Arc::new(SocketServer::new(socket_path));

    server
        .register_status_handler(Arc::new(|| {
            serde_json::json!({
                "state": "running",
                "active_polls": ["ebay"],
                "profiles": ["test-profile"],
                "bot_blocks": {}
            })
        }))
        .await;

    server
        .register_shutdown_handler(Arc::new(|| {
            // no-op for tests
        }))
        .await;

    server.start().await.expect("failed to start socket server");
    server
}

async fn send_command(
    socket_path: &std::path::Path,
    command: &serde_json::Value,
) -> serde_json::Value {
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
async fn status_command_returns_data() {
    let dir = tempfile::tempdir().unwrap();
    let server = start_server(dir.path()).await;
    let socket_path = dir.path().join("test.sock");

    let resp = send_command(&socket_path, &serde_json::json!({"command": "status"})).await;

    assert_eq!(resp["status"], "ok");
    assert_eq!(resp["data"]["state"], "running");
    assert_eq!(resp["data"]["active_polls"][0], "ebay");

    server.stop().await;
}

#[tokio::test]
async fn poll_command_returns_ok() {
    let dir = tempfile::tempdir().unwrap();
    let server = start_server(dir.path()).await;
    let socket_path = dir.path().join("test.sock");

    let resp = send_command(
        &socket_path,
        &serde_json::json!({"command": "poll", "profile_id": "test-1"}),
    )
    .await;

    assert_eq!(resp["status"], "ok");
    assert_eq!(resp["data"]["profile_id"], "test-1");

    server.stop().await;
}

#[tokio::test]
async fn poll_command_without_profile_id_errors() {
    let dir = tempfile::tempdir().unwrap();
    let server = start_server(dir.path()).await;
    let socket_path = dir.path().join("test.sock");

    let resp = send_command(&socket_path, &serde_json::json!({"command": "poll"})).await;

    assert_eq!(resp["status"], "error");
    assert!(resp["message"]
        .as_str()
        .unwrap()
        .contains("profile_id required"));

    server.stop().await;
}

#[tokio::test]
async fn unknown_command_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let server = start_server(dir.path()).await;
    let socket_path = dir.path().join("test.sock");

    let resp = send_command(&socket_path, &serde_json::json!({"command": "explode"})).await;

    assert_eq!(resp["status"], "error");
    assert!(resp["message"]
        .as_str()
        .unwrap()
        .contains("unknown command: explode"));

    server.stop().await;
}

#[tokio::test]
async fn invalid_json_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let server = start_server(dir.path()).await;
    let socket_path = dir.path().join("test.sock");

    let stream = UnixStream::connect(&socket_path).await.unwrap();
    let (reader, mut writer) = stream.into_split();
    writer.write_all(b"not json\n").await.unwrap();
    writer.shutdown().await.unwrap();

    let mut reader = BufReader::new(reader);
    let mut response = String::new();
    reader.read_line(&mut response).await.unwrap();
    let resp: serde_json::Value = serde_json::from_str(&response).unwrap();

    assert_eq!(resp["status"], "error");
    assert!(resp["message"].as_str().unwrap().contains("invalid JSON"));

    server.stop().await;
}

#[tokio::test]
async fn shutdown_command_returns_ok() {
    let dir = tempfile::tempdir().unwrap();
    let server = start_server(dir.path()).await;
    let socket_path = dir.path().join("test.sock");

    let resp = send_command(&socket_path, &serde_json::json!({"command": "shutdown"})).await;

    assert_eq!(resp["status"], "ok");
    assert_eq!(resp["data"]["message"], "shutting down");

    server.stop().await;
}

#[tokio::test]
async fn reload_command_returns_ok() {
    let dir = tempfile::tempdir().unwrap();
    let server = start_server(dir.path()).await;
    let socket_path = dir.path().join("test.sock");

    let resp = send_command(&socket_path, &serde_json::json!({"command": "reload"})).await;

    assert_eq!(resp["status"], "ok");
    assert_eq!(resp["data"]["message"], "config reloaded");

    server.stop().await;
}

#[tokio::test]
async fn socket_permissions_are_0600() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let _server = start_server(dir.path()).await;
    let socket_path = dir.path().join("test.sock");

    let metadata = std::fs::metadata(&socket_path).unwrap();
    let mode = metadata.permissions().mode() & 0o777;
    assert_eq!(
        mode, 0o600,
        "socket should have 0600 permissions, got {:o}",
        mode
    );
}
