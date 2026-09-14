//! Error types for op-gateway

#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    #[error("authentication failed: {0}")]
    AuthFailed(String),
    #[error("session not found: {0}")]
    SessionNotFound(String),
    #[error("crypto error: {0}")]
    Crypto(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("internal error: {0}")]
    Internal(String),
}

impl GatewayError {
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }

    pub fn auth_failed(msg: impl Into<String>) -> Self {
        Self::AuthFailed(msg.into())
    }

    pub fn crypto(msg: impl Into<String>) -> Self {
        Self::Crypto(msg.into())
    }

    pub fn storage(msg: impl Into<String>) -> Self {
        Self::Storage(msg.into())
    }
}
