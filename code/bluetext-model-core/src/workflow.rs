use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

// ── Error type ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct WorkflowError {
    pub message: String,
}

impl std::fmt::Display for WorkflowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WorkflowError: {}", self.message)
    }
}

impl std::error::Error for WorkflowError {}

impl From<String> for WorkflowError {
    fn from(s: String) -> Self {
        WorkflowError { message: s }
    }
}

impl From<&str> for WorkflowError {
    fn from(s: &str) -> Self {
        WorkflowError {
            message: s.to_string(),
        }
    }
}

// ── Client trait ────────────────────────────────────────────────────

/// Trait for service clients used in commands/requests.
/// Clients are thin endpoint wrappers — no business logic.
pub trait ServiceClient: Send + Sync {
    fn call(
        &self,
        method: &str,
        args: &Value,
    ) -> Pin<Box<dyn Future<Output = Result<Value, WorkflowError>> + Send + '_>>;
}

// ── Command context ─────────────────────────────────────────────────

/// Runtime context for command execution.
///
/// When the `resonate` feature is enabled, this wraps `resonate::Context`
/// for durable execution. Without the feature, provides basic async support.
pub struct CommandContext {
    services: std::collections::HashMap<String, Box<dyn ServiceClient>>,
    _resonate_ctx: Option<crate::resonate::Context>,
}

impl CommandContext {
    pub fn new() -> Self {
        CommandContext {
            services: std::collections::HashMap::new(),
            _resonate_ctx: None,
        }
    }

    pub fn with_resonate(ctx: crate::resonate::Context) -> Self {
        CommandContext {
            services: std::collections::HashMap::new(),
            _resonate_ctx: Some(ctx),
        }
    }

    pub fn register_service(&mut self, name: &str, client: Box<dyn ServiceClient>) {
        self.services.insert(name.to_string(), client);
    }

    pub fn service(&self, name: &str) -> Option<&dyn ServiceClient> {
        self.services.get(name).map(|s| s.as_ref())
    }

    /// Wait until a condition returns true, polling with backoff.
    pub async fn await_condition<F: Fn() -> bool>(
        &self,
        _name: &str,
        check: F,
    ) -> Result<(), WorkflowError> {
        let mut interval = Duration::from_millis(100);
        let max_interval = Duration::from_secs(5);
        let timeout = Duration::from_secs(300);
        let start = std::time::Instant::now();

        loop {
            if check() {
                return Ok(());
            }
            if start.elapsed() > timeout {
                return Err(WorkflowError::from("Condition timed out"));
            }
            tokio::time::sleep(interval).await;
            interval = (interval * 2).min(max_interval);
        }
    }

    /// Sleep for a duration.
    pub async fn sleep(&self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }
}

impl Default for CommandContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse a duration string like "30 days", "5 minutes", "1 hour".
pub fn parse_duration(s: &str) -> Duration {
    let parts: Vec<&str> = s.trim().splitn(2, ' ').collect();
    if parts.len() != 2 {
        return Duration::from_secs(0);
    }
    let value: u64 = parts[0].parse().unwrap_or(0);
    let unit = parts[1].trim_end_matches('s'); // normalize "days" -> "day"
    match unit {
        "second" | "sec" => Duration::from_secs(value),
        "minute" | "min" => Duration::from_secs(value * 60),
        "hour" | "hr" => Duration::from_secs(value * 3600),
        "day" => Duration::from_secs(value * 86400),
        "week" => Duration::from_secs(value * 604800),
        _ => Duration::from_secs(0),
    }
}
