//! Error types for ASD-Nexus client and sidecar management.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum NexusError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("URL parse error: {0}")]
    Url(#[from] url::ParseError),

    #[error("HTTP request error: {0}")]
    Http(String),

    #[error("HTTP error response (status {status}): {body}")]
    HttpStatus { status: u16, body: String },

    #[error("Sidecar process start failed: {0}")]
    SidecarStartFailed(String),

    #[error("Sidecar process exited prematurely (exit code: {code:?}): {stderr}")]
    SidecarExited {
        code: Option<i32>,
        stderr: String,
    },

    #[error("Timed out waiting for sidecar ready handshake: {0}")]
    SidecarTimeout(String),

    #[error("SSE stream error: {0}")]
    StreamError(String),

    #[error("Terminal open error: {0}")]
    TerminalError(String),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, NexusError>;
