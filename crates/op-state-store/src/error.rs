use thiserror::Error;

#[derive(Error, Debug)]
pub enum StateStoreError {
    #[error("Serialization error: {0}")]
    Serialization(#[from] simd_json::Error),
    #[error("Job not found: {0}")]
    NotFound(String),
}

pub type Result<T> = std::result::Result<T, StateStoreError>;
