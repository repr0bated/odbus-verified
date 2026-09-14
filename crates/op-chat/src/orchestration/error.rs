//! Comprehensive error types for orchestration
//!
//! Provides structured errors with:
//! - Error codes for programmatic handling
//! - Retryable vs non-retryable classification
//! - Context propagation
//! - Conversion to/from gRPC status

use std::fmt;
use std::time::Duration;

/// Error codes for orchestration errors
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    // Connection errors (1xx)
    ConnectionFailed = 100,
    ConnectionTimeout = 101,
    ConnectionClosed = 102,
    ConnectionRefused = 103,

    // Session errors (2xx)
    SessionNotFound = 200,
    SessionExpired = 201,
    SessionInvalid = 202,
    SessionLimitExceeded = 203,

    // Agent errors (3xx)
    AgentNotFound = 300,
    AgentUnavailable = 301,
    AgentTimeout = 302,
    AgentBusy = 303,
    AgentStartFailed = 304,
    AgentStopFailed = 305,
    AgentUnresponsive = 306,

    // Execution errors (4xx)
    ExecutionFailed = 400,
    ExecutionTimeout = 401,
    ExecutionCancelled = 402,
    InvalidArguments = 403,
    OperationNotSupported = 404,
    ResourceNotFound = 405,
    PermissionDenied = 406,
    RateLimited = 407,

    // Workstack errors (5xx)
    WorkstackNotFound = 500,
    PhaseNotFound = 501,
    PhaseFailed = 502,
    RollbackFailed = 503,
    DependencyFailed = 504,
    CircularDependency = 505,

    // Internal errors (9xx)
    InternalError = 900,
    Serialization = 901,
    Deserialization = 902,
    Configuration = 903,
    Unknown = 999,
}

impl ErrorCode {
    /// Get the error code as a string
    pub fn as_str(&self) -> &'static str {
        match self {
            ErrorCode::ConnectionFailed => "CONNECTION_FAILED",
            ErrorCode::ConnectionTimeout => "CONNECTION_TIMEOUT",
            ErrorCode::ConnectionClosed => "CONNECTION_CLOSED",
            ErrorCode::ConnectionRefused => "CONNECTION_REFUSED",
            ErrorCode::SessionNotFound => "SESSION_NOT_FOUND",
            ErrorCode::SessionExpired => "SESSION_EXPIRED",
            ErrorCode::SessionInvalid => "SESSION_INVALID",
            ErrorCode::SessionLimitExceeded => "SESSION_LIMIT_EXCEEDED",
            ErrorCode::AgentNotFound => "AGENT_NOT_FOUND",
            ErrorCode::AgentUnavailable => "AGENT_UNAVAILABLE",
            ErrorCode::AgentTimeout => "AGENT_TIMEOUT",
            ErrorCode::AgentBusy => "AGENT_BUSY",
            ErrorCode::AgentStartFailed => "AGENT_START_FAILED",
            ErrorCode::AgentStopFailed => "AGENT_STOP_FAILED",
            ErrorCode::AgentUnresponsive => "AGENT_UNRESPONSIVE",
            ErrorCode::ExecutionFailed => "EXECUTION_FAILED",
            ErrorCode::ExecutionTimeout => "EXECUTION_TIMEOUT",
            ErrorCode::ExecutionCancelled => "EXECUTION_CANCELLED",
            ErrorCode::InvalidArguments => "INVALID_ARGUMENTS",
            ErrorCode::OperationNotSupported => "OPERATION_NOT_SUPPORTED",
            ErrorCode::ResourceNotFound => "RESOURCE_NOT_FOUND",
            ErrorCode::PermissionDenied => "PERMISSION_DENIED",
            ErrorCode::RateLimited => "RATE_LIMITED",
            ErrorCode::WorkstackNotFound => "WORKSTACK_NOT_FOUND",
            ErrorCode::PhaseNotFound => "PHASE_NOT_FOUND",
            ErrorCode::PhaseFailed => "PHASE_FAILED",
            ErrorCode::RollbackFailed => "ROLLBACK_FAILED",
            ErrorCode::DependencyFailed => "DEPENDENCY_FAILED",
            ErrorCode::CircularDependency => "CIRCULAR_DEPENDENCY",
            ErrorCode::InternalError => "INTERNAL_ERROR",
            ErrorCode::Serialization => "SERIALIZATION_ERROR",
            ErrorCode::Deserialization => "DESERIALIZATION_ERROR",
            ErrorCode::Configuration => "CONFIGURATION_ERROR",
            ErrorCode::Unknown => "UNKNOWN",
        }
    }

    /// Check if this error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            ErrorCode::ConnectionTimeout
                | ErrorCode::ConnectionFailed
                | ErrorCode::AgentTimeout
                | ErrorCode::AgentBusy
                | ErrorCode::AgentUnresponsive
                | ErrorCode::ExecutionTimeout
                | ErrorCode::RateLimited
        )
    }

    /// Suggested retry delay for this error
    pub fn suggested_retry_delay(&self) -> Option<Duration> {
        match self {
            ErrorCode::ConnectionTimeout => Some(Duration::from_secs(1)),
            ErrorCode::ConnectionFailed => Some(Duration::from_secs(2)),
            ErrorCode::AgentTimeout => Some(Duration::from_secs(1)),
            ErrorCode::AgentBusy => Some(Duration::from_millis(500)),
            ErrorCode::AgentUnresponsive => Some(Duration::from_secs(5)),
            ErrorCode::ExecutionTimeout => Some(Duration::from_secs(2)),
            ErrorCode::RateLimited => Some(Duration::from_secs(10)),
            _ => None,
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Main orchestration error type
#[derive(Debug)]
pub struct OrchestrationError {
    /// Error code
    pub code: ErrorCode,
    /// Human-readable message
    pub message: String,
    /// Additional details (JSON)
    pub details: Option<String>,
    /// Source error
    pub source: Option<Box<dyn std::error::Error + Send + Sync>>,
    /// Stack trace (if available)
    pub stack_trace: Option<String>,
    /// Retry information
    pub retry_info: Option<RetryInfo>,
}

/// Retry information for retryable errors
#[derive(Debug, Clone)]
pub struct RetryInfo {
    /// Whether this error is retryable
    pub retryable: bool,
    /// Suggested delay before retry
    pub delay: Duration,
    /// Maximum retry attempts
    pub max_attempts: u32,
    /// Current attempt (if tracking)
    pub current_attempt: Option<u32>,
}

impl OrchestrationError {
    /// Create a new error
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        let message = message.into();
        let retry_info = if code.is_retryable() {
            Some(RetryInfo {
                retryable: true,
                delay: code
                    .suggested_retry_delay()
                    .unwrap_or(Duration::from_secs(1)),
                max_attempts: 3,
                current_attempt: None,
            })
        } else {
            None
        };

        Self {
            code,
            message,
            details: None,
            source: None,
            stack_trace: None,
            retry_info,
        }
    }

    /// Add details to the error
    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }

    /// Add source error
    pub fn with_source(mut self, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        self.source = Some(Box::new(source));
        self
    }

    /// Add stack trace
    pub fn with_backtrace(mut self) -> Self {
        self.stack_trace = Some(std::backtrace::Backtrace::capture().to_string());
        self
    }

    /// Check if this error is retryable
    pub fn is_retryable(&self) -> bool {
        self.retry_info
            .as_ref()
            .map(|r| r.retryable)
            .unwrap_or(false)
    }

    /// Get suggested retry delay
    pub fn retry_delay(&self) -> Option<Duration> {
        self.retry_info.as_ref().map(|r| r.delay)
    }

    // Convenience constructors

    pub fn connection_failed(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::ConnectionFailed, message)
    }

    pub fn connection_timeout(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::ConnectionTimeout, message)
    }

    pub fn session_not_found(session_id: &str) -> Self {
        Self::new(
            ErrorCode::SessionNotFound,
            format!("Session not found: {}", session_id),
        )
    }

    pub fn agent_not_found(agent_id: &str) -> Self {
        Self::new(
            ErrorCode::AgentNotFound,
            format!("Agent not found: {}", agent_id),
        )
    }

    pub fn agent_unavailable(agent_id: &str, reason: &str) -> Self {
        Self::new(
            ErrorCode::AgentUnavailable,
            format!("Agent {} unavailable: {}", agent_id, reason),
        )
    }

    pub fn agent_timeout(agent_id: &str, operation: &str, timeout: Duration) -> Self {
        Self::new(
            ErrorCode::AgentTimeout,
            format!(
                "Agent {} timed out during {}: {:?}",
                agent_id, operation, timeout
            ),
        )
    }

    pub fn execution_failed(agent_id: &str, operation: &str, reason: &str) -> Self {
        Self::new(
            ErrorCode::ExecutionFailed,
            format!("{}:{} failed: {}", agent_id, operation, reason),
        )
    }

    pub fn execution_timeout(agent_id: &str, operation: &str) -> Self {
        Self::new(
            ErrorCode::ExecutionTimeout,
            format!("Execution timeout: {}:{}", agent_id, operation),
        )
    }

    pub fn invalid_arguments(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidArguments, message)
    }

    pub fn workstack_not_found(workstack_id: &str) -> Self {
        Self::new(
            ErrorCode::WorkstackNotFound,
            format!("Workstack not found: {}", workstack_id),
        )
    }

    pub fn phase_failed(phase_id: &str, reason: &str) -> Self {
        Self::new(
            ErrorCode::PhaseFailed,
            format!("Phase {} failed: {}", phase_id, reason),
        )
    }

    pub fn rollback_failed(reason: &str) -> Self {
        Self::new(
            ErrorCode::RollbackFailed,
            format!("Rollback failed: {}", reason),
        )
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InternalError, message)
    }

    pub fn serialization(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Serialization, message)
    }
}

impl fmt::Display for OrchestrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)?;
        if let Some(ref details) = self.details {
            write!(f, " ({})", details)?;
        }
        Ok(())
    }
}

impl std::error::Error for OrchestrationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|e| e.as_ref() as &(dyn std::error::Error + 'static))
    }
}

// Conversion from common error types

impl From<simd_json::Error> for OrchestrationError {
    fn from(err: simd_json::Error) -> Self {
        Self::new(ErrorCode::Deserialization, err.to_string())
    }
}

impl From<std::io::Error> for OrchestrationError {
    fn from(err: std::io::Error) -> Self {
        use std::io::ErrorKind;

        let code = match err.kind() {
            ErrorKind::ConnectionRefused => ErrorCode::ConnectionRefused,
            ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted => {
                ErrorCode::ConnectionClosed
            }
            ErrorKind::TimedOut => ErrorCode::ConnectionTimeout,
            ErrorKind::NotFound => ErrorCode::ResourceNotFound,
            ErrorKind::PermissionDenied => ErrorCode::PermissionDenied,
            _ => ErrorCode::InternalError,
        };

        Self::new(code, err.to_string())
    }
}

impl From<tokio::time::error::Elapsed> for OrchestrationError {
    fn from(err: tokio::time::error::Elapsed) -> Self {
        Self::new(ErrorCode::ExecutionTimeout, err.to_string())
    }
}

impl<T> From<tokio::sync::mpsc::error::SendError<T>> for OrchestrationError {
    fn from(err: tokio::sync::mpsc::error::SendError<T>) -> Self {
        Self::new(
            ErrorCode::ConnectionClosed,
            format!("Channel send failed: {}", err),
        )
    }
}

impl From<anyhow::Error> for OrchestrationError {
    fn from(err: anyhow::Error) -> Self {
        Self::new(ErrorCode::InternalError, err.to_string())
    }
}

// Conversion to tonic Status (for gRPC)
impl From<OrchestrationError> for tonic::Status {
    fn from(err: OrchestrationError) -> Self {
        use tonic::Code;

        let code = match err.code {
            ErrorCode::ConnectionFailed | ErrorCode::ConnectionRefused => Code::Unavailable,
            ErrorCode::ConnectionTimeout
            | ErrorCode::AgentTimeout
            | ErrorCode::ExecutionTimeout => Code::DeadlineExceeded,
            ErrorCode::SessionNotFound
            | ErrorCode::AgentNotFound
            | ErrorCode::ResourceNotFound
            | ErrorCode::WorkstackNotFound
            | ErrorCode::PhaseNotFound => Code::NotFound,
            ErrorCode::SessionExpired | ErrorCode::SessionInvalid => Code::Unauthenticated,
            ErrorCode::PermissionDenied => Code::PermissionDenied,
            ErrorCode::InvalidArguments => Code::InvalidArgument,
            ErrorCode::OperationNotSupported => Code::Unimplemented,
            ErrorCode::RateLimited | ErrorCode::AgentBusy | ErrorCode::SessionLimitExceeded => {
                Code::ResourceExhausted
            }
            ErrorCode::ExecutionCancelled => Code::Cancelled,
            ErrorCode::CircularDependency | ErrorCode::DependencyFailed => Code::FailedPrecondition,
            _ => Code::Internal,
        };

        let mut status = tonic::Status::new(code, err.message.clone());

        // Add error details as metadata
        if let Some(ref details) = err.details {
            if let Ok(val) = details.parse() {
                let _ = status.metadata_mut().insert("x-error-details", val);
            }
        }

        if let Ok(val) = err.code.as_str().parse() {
            let _ = status.metadata_mut().insert("x-error-code", val);
        }

        if err.is_retryable() {
            if let Ok(val) = "true".parse() {
                let _ = status.metadata_mut().insert("x-retryable", val);
            }
            if let Some(delay) = err.retry_delay() {
                if let Ok(val) = delay.as_millis().to_string().parse() {
                    let _ = status.metadata_mut().insert("x-retry-after-ms", val);
                }
            }
        }

        status
    }
}

/// Result type alias for orchestration operations
pub type OrchestrationResult<T> = Result<T, OrchestrationError>;

/// Extension trait for adding context to errors
pub trait ResultExt<T> {
    #[allow(clippy::result_large_err)]
    fn context(self, code: ErrorCode, message: impl Into<String>) -> OrchestrationResult<T>;
    #[allow(clippy::result_large_err)]
    fn with_context<F>(self, code: ErrorCode, f: F) -> OrchestrationResult<T>
    where
        F: FnOnce() -> String;
}

impl<T, E: std::error::Error + Send + Sync + 'static> ResultExt<T> for Result<T, E> {
    fn context(self, code: ErrorCode, message: impl Into<String>) -> OrchestrationResult<T> {
        self.map_err(|e| OrchestrationError::new(code, message).with_source(e))
    }

    fn with_context<F>(self, code: ErrorCode, f: F) -> OrchestrationResult<T>
    where
        F: FnOnce() -> String,
    {
        self.map_err(|e| OrchestrationError::new(code, f()).with_source(e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_codes() {
        assert!(ErrorCode::ConnectionTimeout.is_retryable());
        assert!(!ErrorCode::InvalidArguments.is_retryable());
        assert!(ErrorCode::AgentBusy.suggested_retry_delay().is_some());
    }

    #[test]
    fn test_error_display() {
        let err = OrchestrationError::agent_timeout("rust_pro", "build", Duration::from_secs(30));
        let msg = err.to_string();
        assert!(msg.contains("AGENT_TIMEOUT"));
        assert!(msg.contains("rust_pro"));
    }

    #[test]
    fn test_error_retryable() {
        let timeout_err = OrchestrationError::connection_timeout("test");
        assert!(timeout_err.is_retryable());
        assert!(timeout_err.retry_delay().is_some());

        let invalid_err = OrchestrationError::invalid_arguments("test");
        assert!(!invalid_err.is_retryable());
    }
}
