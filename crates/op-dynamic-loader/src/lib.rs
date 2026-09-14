//! OP Dynamic Loader - Intelligent Tool Loading Enhancement
//!
//! Complements existing MCP tool loading by adding:
//! - LRU caching for frequently used tools
//! - Execution-aware loading decisions
//! - Integration with execution tracking
//! - Memory-efficient tool management

pub mod dynamic_registry;
pub mod error;
pub mod execution_aware_loader;
pub mod loading_strategy;

pub use dynamic_registry::DynamicToolRegistry;
pub use error::DynamicLoaderError;
pub use execution_aware_loader::ExecutionAwareLoader;
pub use loading_strategy::{LoadingStrategy, SmartLoadingStrategy};
