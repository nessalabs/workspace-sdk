use thiserror::Error;
pub type Result<T> = std::result::Result<T, Error>;

/// Failed SDK operations. A workload's nonzero exit is an execution result.
#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("unsupported capability: {0}")]
    Unsupported(String),
    #[error("state error: {0}")]
    State(String),
    #[error("command failed: {message}")]
    CommandFailed { message: String },
    #[error("provider {provider}: {message}")]
    Provider { provider: String, message: String },
    #[error("not found: {0}")]
    NotFound(String),
    #[error("operation timed out: {0}")]
    Timeout(String),
    #[error("credential required: {0}")]
    CredentialRequired(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Serialization(#[from] serde_json::Error),
}
