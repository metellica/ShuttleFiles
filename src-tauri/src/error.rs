use serde::{Serialize, Serializer};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{0}")]
    Io(String),
    #[error("{0}")]
    InvalidPath(String),
    #[error("{0}")]
    Config(String),
    /// The user cancelled a background job; not a real failure.
    #[error("Cancelled")]
    Cancelled,
}

/// Tauri commands must return a serializable error; the frontend only
/// ever needs the message.
impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Io(e.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::Config(e.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
