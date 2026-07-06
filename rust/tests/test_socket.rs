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
    let server = start_server(dir.path()).await;
    let socket_path = dir.path().join("test.sock");

    let metadata = std::fs::metadata(&socket_path).unwrap();
    let mode = metadata.permissions().mode() & 0o777;
    assert_eq!(
        mode, 0o600,
        "socket should have 0600 permissions, got {:o}",
        mode
    );

    server.stop().await;
}

#[tokio::test]
async fn existing_non_scavenger_parent_permissions_are_unchanged() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();

    let server = Arc::new(SocketServer::new(dir.path().join("test.sock")));
    server.start().await.expect("failed to start socket server");

    let mode = std::fs::metadata(dir.path()).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        mode, 0o755,
        "socket startup should not chmod arbitrary existing parent dirs"
    );

    server.stop().await;
}

#[tokio::test]
async fn app_owned_socket_parent_permissions_are_0700() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let socket_dir = root.path().join("scavenger");
    let server = Arc::new(SocketServer::new(socket_dir.join("daemon.sock")));

    server.start().await.expect("failed to start socket server");

    let mode = std::fs::metadata(&socket_dir).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700, "app-owned runtime dir should be private");

    server.stop().await;
}

#[tokio::test]
async fn start_refuses_existing_regular_file() {
    let dir = tempfile::tempdir().unwrap();
    let socket_path = dir.path().join("test.sock");
    std::fs::write(&socket_path, "not a socket").unwrap();

    let server = Arc::new(SocketServer::new(socket_path.clone()));
    let err = server
        .start()
        .await
        .expect_err("regular files must not be removed");

    assert!(
        err.to_string().contains("refusing to remove non-socket"),
        "unexpected error: {err}"
    );
    assert_eq!(
        std::fs::read_to_string(&socket_path).unwrap(),
        "not a socket"
    );
}

#[tokio::test]
async fn start_removes_stale_socket_file() {
    use std::os::unix::fs::FileTypeExt;
    use std::os::unix::net::UnixListener as StdUnixListener;

    let dir = tempfile::tempdir().unwrap();
    let socket_path = dir.path().join("test.sock");

    let listener = StdUnixListener::bind(&socket_path).unwrap();
    drop(listener);
    assert!(std::fs::symlink_metadata(&socket_path)
        .unwrap()
        .file_type()
        .is_socket());

    let server = Arc::new(SocketServer::new(socket_path.clone()));
    server
        .start()
        .await
        .expect("stale socket file should be replaced");

    assert!(std::fs::symlink_metadata(&socket_path)
        .unwrap()
        .file_type()
        .is_socket());

    server.stop().await;
}

#[tokio::test]
async fn start_refuses_active_socket_file() {
    use std::os::unix::net::UnixListener as StdUnixListener;

    let dir = tempfile::tempdir().unwrap();
    let socket_path = dir.path().join("test.sock");
    let listener = StdUnixListener::bind(&socket_path).unwrap();

    let server = Arc::new(SocketServer::new(socket_path.clone()));
    let err = server
        .start()
        .await
        .expect_err("active daemon sockets must not be removed");

    assert!(
        err.to_string().contains("daemon socket is already active"),
        "unexpected error: {err}"
    );
    assert!(socket_path.exists());

    drop(listener);
    let _ = std::fs::remove_file(&socket_path);
}
