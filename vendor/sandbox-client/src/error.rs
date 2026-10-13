use std::{fmt, time::Duration};

use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

/// Client errors.
///
/// `Display` / `Debug` are log-safe for `Api` and `Unexpected`: they omit server
/// `message` / `details` / raw bodies (which may echo secrets). Full text remains
/// on the struct fields / accessors for deliberate handling.
#[derive(Error)]
pub enum Error {
    #[error("config: {0}")]
    Config(String),

    #[error("http: {0}")]
    Http(reqwest::Error),

    /// Server API failure. `Display` shows status/code/request_id only.
    #[error("api {status}: {code} (request_id={request_id})")]
    Api {
        /// Parsed server Retry-After, available even when retries are exhausted.
        retry_after: Option<Duration>,
        status: u16,
        code: String,
        message: String,
        request_id: String,
        details: Option<Box<serde_json::Value>>,
    },

    #[error("response exceeds configured limit of {limit} bytes")]
    ResponseTooLarge { limit: usize },

    /// Non-JSON or non-envelope HTTP error; the HTTP status remains inspectable.
    #[error("HTTP {status} (body redacted)")]
    HttpStatus {
        status: u16,
        body: String,
        retry_after: Option<Duration>,
    },

    #[error("event stream aborted")]
    Aborted,

    #[error("unexpected response (body redacted)")]
    Unexpected(String),

    #[error("invalid sandbox id `{0}` (expected bx_ + 8 chars)")]
    InvalidSandboxId(String),

    #[error("timeout waiting for box {sandbox_id} (last state={last_state})")]
    WaitTimeout {
        sandbox_id: String,
        last_state: String,
    },

    /// The Sandbox entered a state that cannot satisfy a readiness wait.
    #[error("box {sandbox_id} entered terminal state {state}")]
    SandboxTerminal { sandbox_id: String, state: String },

    #[error("box {sandbox_id} entered error state")]
    SandboxFailed { sandbox_id: String },

    #[error("response decoding failed (details redacted)")]
    Serde(#[from] serde_json::Error),
}

impl From<reqwest::Error> for Error {
    fn from(error: reqwest::Error) -> Self {
        // URLs may include signed tokens or private paths. Keep transport metadata
        // and the error source without accidentally exposing the request URL.
        Self::Http(error.without_url())
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(msg) => f.debug_tuple("Config").field(msg).finish(),
            Self::Http(e) => f.debug_tuple("Http").field(e).finish(),
            Self::Api {
                status,
                code,
                message,
                request_id,
                details,
                ..
            } => f
                .debug_struct("Api")
                .field("status", status)
                .field("code", code)
                .field("message_len", &message.len())
                .field("request_id", request_id)
                .field("has_details", &details.is_some())
                .finish(),
            Self::ResponseTooLarge { limit } => f
                .debug_struct("ResponseTooLarge")
                .field("limit", limit)
                .finish(),
            Self::Unexpected(body) => f
                .debug_struct("Unexpected")
                .field("body_len", &body.len())
                .finish(),
            Self::InvalidSandboxId(id) => f.debug_tuple("InvalidSandboxId").field(id).finish(),
            Self::WaitTimeout {
                sandbox_id,
                last_state,
            } => f
                .debug_struct("WaitTimeout")
                .field("sandbox_id", sandbox_id)
                .field("last_state", last_state)
                .finish(),
            Self::SandboxTerminal { sandbox_id, state } => f
                .debug_struct("SandboxTerminal")
                .field("sandbox_id", sandbox_id)
                .field("state", state)
                .finish(),
            Self::SandboxFailed { sandbox_id } => f
                .debug_struct("SandboxFailed")
                .field("sandbox_id", sandbox_id)
                .finish(),
            Self::Serde(e) => f
                .debug_struct("Serde")
                .field("line", &e.line())
                .field("column", &e.column())
                .finish(),
            Self::HttpStatus { status, body, .. } => f
                .debug_struct("HttpStatus")
                .field("status", status)
                .field("body_len", &body.len())
                .finish(),
            Self::Aborted => f.write_str("Aborted"),
        }
    }
}

impl Error {
    /// Server Retry-After as a delay measured when the response arrived.
    /// Available on API/HTTP status errors, including mutation failures.
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::Api { retry_after, .. } | Self::HttpStatus { retry_after, .. } => *retry_after,
            _ => None,
        }
    }

    /// Whether a **safe automatic retry** of an idempotent request may help.
    ///
    /// Does **not** treat every 409 as retryable — only known transient codes
    /// (`boat_starting`). Never retry `command` via this alone;
    /// Ascii documents that command execution is not safely auto-retried.
    pub fn is_retryable(&self) -> bool {
        match self {
            Error::Api { status, code, .. } => match *status {
                429 => true,
                502..=504 => true,
                409 => code == "boat_starting",
                _ => false,
            },
            Error::Http(e) => e.is_timeout() || e.is_connect(),
            Error::HttpStatus { status, .. } => matches!(status, 429 | 502..=504),
            _ => false,
        }
    }

    pub fn status(&self) -> Option<u16> {
        match self {
            Error::Api { status, .. } | Error::HttpStatus { status, .. } => Some(*status),
            Error::Http(e) => e.status().map(|s| s.as_u16()),
            _ => None,
        }
    }

    /// Server-provided API message, if this is an `Api` error.
    /// Prefer this over relying on `Display` (which omits the message).
    pub fn api_message(&self) -> Option<&str> {
        match self {
            Error::Api { message, .. } => Some(message.as_str()),
            _ => None,
        }
    }

    /// Server-provided API details JSON, if present.
    pub fn api_details(&self) -> Option<&serde_json::Value> {
        match self {
            Error::Api { details, .. } => details.as_deref(),
            _ => None,
        }
    }

    /// Raw unexpected response body (truncated), if this is `Unexpected`.
    pub fn unexpected_body(&self) -> Option<&str> {
        match self {
            Error::Unexpected(body) | Error::HttpStatus { body, .. } => Some(body.as_str()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn api_display_and_debug_omit_message_and_details() {
        let err = Error::Api {
            retry_after: None,
            status: 400,
            code: "bad_request".into(),
            message: "leaked-secret-token-xyz".into(),
            request_id: "req_1".into(),
            details: Some(Box::new(json!({"token": "also-secret"}))),
        };
        let display = err.to_string();
        assert!(display.contains("bad_request"));
        assert!(display.contains("req_1"));
        assert!(!display.contains("leaked-secret"));
        assert!(!display.contains("also-secret"));

        let debug = format!("{err:?}");
        assert!(debug.contains("message_len"));
        assert!(debug.contains("has_details"));
        assert!(!debug.contains("leaked-secret"));
        assert!(!debug.contains("also-secret"));

        assert_eq!(err.api_message(), Some("leaked-secret-token-xyz"));
        assert!(err.api_details().is_some());
    }

    #[test]
    fn unexpected_display_and_debug_omit_body() {
        let err = Error::Unexpected("HTTP 500 — secret-in-body".into());
        assert!(!err.to_string().contains("secret-in-body"));
        assert!(!format!("{err:?}").contains("secret-in-body"));
        assert_eq!(err.unexpected_body(), Some("HTTP 500 — secret-in-body"));
    }
}
