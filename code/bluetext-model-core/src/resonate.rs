//! Resonate integration for durable command execution.
//!
//! Commands use Resonate to guarantee durable execution:
//! - In simulation: `Resonate::local()` (in-memory, single-process)
//! - In production: full Resonate server for cross-service durability

pub use resonate::prelude::*;

/// Create a Resonate instance for local (simulation) mode.
/// No server needed — all execution is in-memory, single-process.
pub fn local() -> Resonate {
    Resonate::local()
}

/// Create a Resonate instance connected to a remote server.
/// Used in production mode for cross-service durable execution.
pub fn remote(url: &str) -> Resonate {
    Resonate::new(ResonateConfig {
        url: Some(url.to_string()),
        ..Default::default()
    })
}

/// Error type for command execution failures.
#[derive(Debug, Clone)]
pub struct CommandError {
    pub message: String,
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CommandError: {}", self.message)
    }
}

impl std::error::Error for CommandError {}

impl From<String> for CommandError {
    fn from(s: String) -> Self {
        CommandError { message: s }
    }
}

impl From<&str> for CommandError {
    fn from(s: &str) -> Self {
        CommandError {
            message: s.to_string(),
        }
    }
}

/// Error type for request execution failures.
#[derive(Debug, Clone)]
pub struct RequestError {
    pub message: String,
}

impl std::fmt::Display for RequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RequestError: {}", self.message)
    }
}

impl std::error::Error for RequestError {}

impl From<String> for RequestError {
    fn from(s: String) -> Self {
        RequestError { message: s }
    }
}

impl From<&str> for RequestError {
    fn from(s: &str) -> Self {
        RequestError {
            message: s.to_string(),
        }
    }
}
