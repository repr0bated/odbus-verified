//! Error types for op-dbus-v2

use thiserror::Error;

/// Main error type for op-dbus operations
#[derive(Error, Debug)]
pub enum Error {
    #[error("DBus error: {0}")]
    Dbus(#[from] zbus::Error),

    #[error("DBus FDO error: {0}")]
    DbusFdo(#[from] zbus::fdo::Error),

    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Introspection error: {0}")]
    Introspection(String),

    #[error("Tool execution error: {0}")]
    ToolExecution(String),

    #[error("Plugin error: {0}")]
    Plugin(String),

    #[error("Agent error: {0}")]
    Agent(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] simd_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

/// Result type alias using our Error type
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// Create a connection error
    pub fn connection(msg: impl Into<String>) -> Self {
        Error::Connection(msg.into())
    }

    /// Create an introspection error
    pub fn introspection(msg: impl Into<String>) -> Self {
        Error::Introspection(msg.into())
    }

    /// Create a tool execution error
    pub fn tool_execution(msg: impl Into<String>) -> Self {
        Error::ToolExecution(msg.into())
    }

    /// Create a plugin error
    pub fn plugin(msg: impl Into<String>) -> Self {
        Error::Plugin(msg.into())
    }

    /// Create an agent error
    pub fn agent(msg: impl Into<String>) -> Self {
        Error::Agent(msg.into())
    }

    /// Create a not found error
    pub fn not_found(msg: impl Into<String>) -> Self {
        Error::NotFound(msg.into())
    }

    /// Create an internal error
    pub fn internal(msg: impl Into<String>) -> Self {
        Error::Internal(msg.into())
    }
}

impl From<anyhow::Error> for Error {
    fn from(err: anyhow::Error) -> Self {
        Error::Internal(err.to_string())
    }
}
