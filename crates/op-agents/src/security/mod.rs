//! Security module for agent execution sandboxing and validation
//!
//! Provides:
//! - Security profiles for different agent categories
//! - Input validation and sanitization
//! - Sandboxed execution with resource limits
//! - Path and command whitelisting

pub mod profiles;
pub mod sandbox;
pub mod validation;

pub use profiles::{ProfileCategory, SecurityConfig, SecurityProfile};
pub use sandbox::{ResourceLimits, SandboxExecutor, SandboxResult};
pub use validation::{
    validate_args, validate_command, validate_input, validate_path, SecurityError, ValidationError,
};
