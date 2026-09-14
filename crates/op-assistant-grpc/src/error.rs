use thiserror::Error;
use tonic::Status;

#[derive(Debug, Error)]
pub enum AssistantError {
    #[error("Assistant resource not found: {0}")]
    NotFound(String),

    #[error("Unauthenticated: {0}")]
    Unauthenticated(String),

    #[error("Permission denied: {0}")]
    Forbidden(String),

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Assistant returned internal error: {0}")]
    Internal(String),

    #[error("Transport error: {0}")]
    Transport(String),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("D-Bus error: {0}")]
    DBus(#[from] zbus::Error),

    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("Unknown error: {0}")]
    Unknown(String),
}

impl From<AssistantError> for Status {
    fn from(err: AssistantError) -> Self {
        match err {
            AssistantError::NotFound(m) => Status::not_found(m),
            AssistantError::Unauthenticated(m) => Status::unauthenticated(m),
            AssistantError::Forbidden(m) => Status::permission_denied(m),
            AssistantError::InvalidRequest(m) => Status::invalid_argument(m),
            AssistantError::Internal(m) => Status::internal(m),
            AssistantError::Transport(m) => Status::unavailable(m),
            AssistantError::Http(e) => {
                if let Some(code) = e.status() {
                    map_http_status(code.as_u16(), e.to_string())
                } else {
                    Status::unavailable(e.to_string())
                }
            }
            AssistantError::DBus(e) => Status::unavailable(format!("dbus: {}", e)),
            AssistantError::Serde(e) => Status::internal(format!("serde: {}", e)),
            AssistantError::Unknown(m) => Status::unknown(m),
        }
    }
}

pub fn map_http_status(code: u16, message: impl Into<String>) -> Status {
    let m = message.into();
    match code {
        400 => Status::invalid_argument(m),
        401 => Status::unauthenticated(m),
        403 => Status::permission_denied(m),
        404 => Status::not_found(m),
        408 | 504 => Status::deadline_exceeded(m),
        429 => Status::resource_exhausted(m),
        500..=599 => Status::internal(m),
        _ => Status::unknown(m),
    }
}

pub type Result<T> = std::result::Result<T, AssistantError>;
