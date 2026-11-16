use thiserror::Error;

pub type Result<T> = std::result::Result<T, CoreError>;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("invalid query: {0}")]
    InvalidQuery(String),

    #[error("serialization: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("storage backend not implemented: {0}")]
    NotImplemented(String),

    #[cfg(feature = "sqlite")]
    #[error("database: {0}")]
    Database(#[from] rusqlite::Error),
}
