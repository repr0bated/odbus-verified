use thiserror::Error;

#[derive(Error, Debug)]
pub enum DynamicLoaderError {
    #[error("Tool loading error: {0}")]
    LoadingError(String),

    #[error("Cache eviction error: {0}")]
    CacheError(String),

    #[error("Execution tracking integration error: {0}")]
    TrackingError(String),

    #[error("Tool not found in registry or cache: {0}")]
    ToolNotFound(String),

    #[error("Strategy selection error: {0}")]
    StrategyError(String),
}
