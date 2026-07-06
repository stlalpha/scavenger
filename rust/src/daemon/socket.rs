use std::io::{Error as IoError, ErrorKind};
use std::os::unix::fs::FileTypeExt;
use std::os::unix::net::UnixStream as StdUnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::Mutex;
use tracing::error;

const MAX_INPUT: usize = 65536;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

type StatusHandler = Arc<dyn Fn() -> serde_json::Value + Send + Sync>;
type PollHandler = Arc<
    dyn Fn(String) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>
        + Send
        + Sync,
>;
type ReloadHandler = Arc<
    dyn Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send + Sync,
>;
type ShutdownHandler = Arc<dyn Fn() + Send + Sync>;

#[derive(Clone)]
struct Handlers {
    status: Arc<Mutex<Option<StatusHandler>>>,
    poll: Arc<Mutex<Option<PollHandler>>>,
    reload: Arc<Mutex<Option<ReloadHandler>>>,
    shutdown: Arc<Mutex<Option<ShutdownHandler>>>,
}

impl Handlers {
    fn new() -> Self {
        Self {
            status: Arc::new(Mutex::new(None)),
            poll: Arc::new(Mutex::new(None)),
            reload: Arc::new(Mutex::new(None)),
            shutdown: Arc::new(Mutex::new(None)),
        }
    }
}

pub struct SocketServer {
    path: PathBuf,
    handlers: Handlers,
    cancel: tokio::sync::watch::Sender<bool>,
    cancel_rx: tokio::sync::watch::Receiver<bool>,
    accept_handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl SocketServer {
    pub fn new(path: PathBuf) -> Self {
        let (cancel, cancel_rx) = tokio::sync::watch::channel(false);
        Self {
            path,
            handlers: Handlers::new(),
            cancel,
            cancel_rx,
            accept_handle: Mutex::new(None),
        }
    }

    pub async fn register_status_handler(&self, handler: StatusHandler) {
        *self.handlers.status.lock().await = Some(handler);
    }

    pub async fn register_poll_handler(&self, handler: PollHandler) {
        *self.handlers.poll.lock().await = Some(handler);
    }

    pub async fn register_reload_handler(&self, handler: ReloadHandler) {
        *self.handlers.reload.lock().await = Some(handler);
    }

    pub async fn register_shutdown_handler(&self, handler: ShutdownHandler) {
        *self.handlers.shutdown.lock().await = Some(handler);
    }

    pub async fn start(&self) -> Result<(), BoxError> {
        prepare_socket_parent(&self.path)?;
        remove_stale_socket(&self.path)?;

        let listener = UnixListener::bind(&self.path)?;
        set_permissions(&self.path, 0o600)?;

        let handlers = self.handlers.clone();
        let mut cancel_rx = self.cancel_rx.clone();

        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    result = listener.accept() => {
                        match result {
                            Ok((stream, _)) => {
                                let h = handlers.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = handle_connection(stream, &h).await {
                                        error!(error = %e, "socket connection error");
                                    }
                                });
                            }
                            Err(e) => {
                                error!(error = %e, "socket accept error");
                            }
                        }
                    }
                    _ = cancel_rx.changed() => {
                        break;
                    }
                }
            }
        });

        *self.accept_handle.lock().await = Some(handle);
        Ok(())
    }

    pub async fn stop(&self) {
        let _ = self.cancel.send(true);
        if let Some(handle) = self.accept_handle.lock().await.take() {
            handle.abort();
            let _ = handle.await;
        }
        if self.path.exists() {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn prepare_socket_parent(path: &Path) -> Result<(), BoxError> {
    let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) else {
        return Ok(());
    };

    let existed_before = match std::fs::symlink_metadata(parent) {
        Ok(_) => true,
        Err(e) if e.kind() == ErrorKind::NotFound => false,
        Err(e) => return Err(e.into()),
    };

    std::fs::create_dir_all(parent)?;

    let parent_type = std::fs::symlink_metadata(parent)?.file_type();
    if parent_type.is_symlink() {
        return Ok(());
    }

    if !parent_type.is_dir() {
        return Err(IoError::new(
            ErrorKind::InvalidInput,
            format!("socket parent is not a directory: {}", parent.display()),
        )
        .into());
    }

    if !existed_before || is_scavenger_runtime_dir(parent) {
        set_permissions(parent, 0o700)?;
    }

    Ok(())
}

fn is_scavenger_runtime_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == "scavenger")
}

fn remove_stale_socket(path: &Path) -> Result<(), BoxError> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };

    if !metadata.file_type().is_socket() {
        return Err(IoError::new(
            ErrorKind::AlreadyExists,
            format!(
                "refusing to remove non-socket path at daemon socket location: {}",
                path.display()
            ),
        )
        .into());
    }

    match StdUnixStream::connect(path) {
        Ok(_) => Err(IoError::new(
            ErrorKind::AlreadyExists,
            format!("daemon socket is already active: {}", path.display()),
        )
        .into()),
        Err(e) if e.kind() == ErrorKind::ConnectionRefused => {
            std::fs::remove_file(path)?;
            Ok(())
        }
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(IoError::new(
            e.kind(),
            format!(
                "could not verify stale daemon socket at {}: {}",
                path.display(),
                e
            ),
        )
        .into()),
    }
}

fn set_permissions(path: &Path, mode: u32) -> Result<(), BoxError> {
    use std::os::unix::fs::PermissionsExt;
    let perms = std::fs::Permissions::from_mode(mode);
    std::fs::set_permissions(path, perms)?;
    Ok(())
}

async fn handle_connection(stream: tokio::net::UnixStream, handlers: &Handlers) -> Result<(), BoxError> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    let bytes_read = reader.read_line(&mut line).await?;
    if bytes_read == 0 {
        return Ok(());
    }

    let response = if line.len() > MAX_INPUT {
        serde_json::json!({"status": "error", "message": "request too large"})
    } else {
        match serde_json::from_str::<serde_json::Value>(&line) {
            Err(_) => serde_json::json!({"status": "error", "message": "invalid JSON"}),
            Ok(request) => dispatch(request, handlers).await,
        }
    };

    let mut out = serde_json::to_string(&response)?;
    out.push('\n');
    writer.write_all(out.as_bytes()).await?;
    writer.shutdown().await?;
    Ok(())
}

async fn dispatch(request: serde_json::Value, handlers: &Handlers) -> serde_json::Value {
    let cmd = request.get("command").and_then(|v| v.as_str());

    match cmd {
        Some("status") => {
            let data = {
                let handler = handlers.status.lock().await;
                match handler.as_ref() {
                    Some(h) => h(),
                    None => serde_json::json!({"state": "running"}),
                }
            };
            serde_json::json!({"status": "ok", "data": data})
        }
        Some("poll") => {
            let profile_id = request
                .get("profile_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            match profile_id {
                None => {
                    serde_json::json!({"status": "error", "message": "profile_id required"})
                }
                Some(pid) => {
                    let handler = handlers.poll.lock().await;
                    if let Some(h) = handler.as_ref() {
                        let fut = h(pid.clone());
                        tokio::spawn(fut);
                    }
                    serde_json::json!({"status": "ok", "data": {"profile_id": pid}})
                }
            }
        }
        Some("reload") => {
            let handler = handlers.reload.lock().await;
            if let Some(h) = handler.as_ref() {
                h().await;
            }
            serde_json::json!({"status": "ok", "data": {"message": "config reloaded"}})
        }
        Some("shutdown") => {
            let handler = handlers.shutdown.lock().await;
            if let Some(h) = handler.as_ref() {
                h();
            }
            serde_json::json!({"status": "ok", "data": {"message": "shutting down"}})
        }
        Some(other) => {
            serde_json::json!({"status": "error", "message": format!("unknown command: {}", other)})
        }
        None => {
            serde_json::json!({"status": "error", "message": "unknown command: null"})
        }
    }
}
