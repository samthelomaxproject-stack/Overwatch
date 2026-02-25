use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Scanner not available: {0}")]
    ScannerUnavailable(String),

    #[error("{0}")]
    Other(String),
}
