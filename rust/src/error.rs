use thiserror::Error;

#[derive(Debug, Error)]
pub enum ScavengerError {
    #[error("config error: {0}")]
    Config(String),

    #[error("database error: {0}")]
    Database(String),

    #[error("plugin error: {0}")]
    Plugin(String),

    #[error("ai error: {0}")]
    Ai(String),

    #[error("daemon error: {0}")]
    Daemon(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, ScavengerError>;
