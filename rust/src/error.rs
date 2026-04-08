use thiserror::Error;

#[derive(Debug, Error)]
pub enum ScavengerError {
    #[error("config error: {0}")]
    Config(String),

    #[error("database error: {0}")]
    Database(String),

    #[error("plugin error: {0}")]
    Plugin(String),

    #[error("AI error: {0}")]
    Ai(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("browser error: {0}")]
    Browser(String),

    #[error("scoring error: {0}")]
    Scoring(String),

    #[error("parse error: {0}")]
    Parse(String),
}

pub type Result<T> = std::result::Result<T, ScavengerError>;
