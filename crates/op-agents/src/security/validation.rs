//! Input validation and sanitization for secure agent execution
//!
//! Provides validation functions for:
//! - Input strings (preventing injection attacks)
//! - File paths (ensuring allowed directories)
//! - Commands (whitelisting)
//! - Arguments (sanitization)

use std::path::{Path, PathBuf};
use thiserror::Error;

/// Characters forbidden in user input to prevent injection
pub const FORBIDDEN_CHARS: &[char] = &[
    '$', '`', ';', '&', '|', '>', '<', '(', ')', '{', '}', '\n', '\r', '\0',
];

/// Maximum length for various input types
pub const MAX_PATH_LENGTH: usize = 4096;
pub const MAX_COMMAND_LENGTH: usize = 256;
pub const MAX_ARGS_LENGTH: usize = 4096;
pub const MAX_INPUT_LENGTH: usize = 1_000_000; // 1MB

/// Validation error types
#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("Input contains forbidden character: {0:?}")]
    ForbiddenCharacter(char),

    #[error("Input exceeds maximum length ({0} > {1})")]
    TooLong(usize, usize),

    #[error("Empty input not allowed")]
    Empty,

    #[error("Path not within allowed directories: {0}")]
    PathNotAllowed(PathBuf),

    #[error("Path traversal detected: {0}")]
    PathTraversal(PathBuf),

    #[error("Command not whitelisted: {0}")]
    CommandNotAllowed(String),

    #[error("Invalid path: {0}")]
    InvalidPath(String),
}

/// Security errors during execution
#[derive(Debug, Error)]
pub enum SecurityError {
    #[error("Validation failed: {0}")]
    Validation(#[from] ValidationError),

    #[error("Execution timeout after {0} seconds")]
    Timeout(u64),

    #[error("Memory limit exceeded: {0} MB")]
    MemoryExceeded(u64),

    #[error("Output size exceeded: {0} bytes")]
    OutputExceeded(usize),

    #[error("Operation requires approval")]
    RequiresApproval,

    #[error("Agent not authorized for operation: {0}")]
    Unauthorized(String),

    #[error("Resource limit exceeded: {0}")]
    ResourceLimit(String),
}

/// Validate a general input string
pub fn validate_input(input: &str) -> Result<&str, ValidationError> {
    if input.is_empty() {
        return Err(ValidationError::Empty);
    }

    if input.len() > MAX_INPUT_LENGTH {
        return Err(ValidationError::TooLong(input.len(), MAX_INPUT_LENGTH));
    }

    for c in input.chars() {
        if FORBIDDEN_CHARS.contains(&c) {
            return Err(ValidationError::ForbiddenCharacter(c));
        }
    }

    Ok(input)
}

/// Validate a file path against allowed directories
pub fn validate_path(
    path: &str,
    allowed_dirs: &[PathBuf],
    forbidden_dirs: &[PathBuf],
) -> Result<PathBuf, ValidationError> {
    if path.len() > MAX_PATH_LENGTH {
        return Err(ValidationError::TooLong(path.len(), MAX_PATH_LENGTH));
    }

    // Check for forbidden characters
    for c in path.chars() {
        if FORBIDDEN_CHARS.contains(&c) {
            return Err(ValidationError::ForbiddenCharacter(c));
        }
    }

    // Parse and canonicalize the path
    let path_buf = PathBuf::from(path);

    // Check for path traversal attempts
    if path.contains("..") {
        // Allow .. only if the canonicalized path is still within allowed dirs
        // For now, reject any ..
        return Err(ValidationError::PathTraversal(path_buf));
    }

    // Check forbidden directories first (takes precedence)
    for forbidden in forbidden_dirs {
        if path_buf.starts_with(forbidden) {
            return Err(ValidationError::PathNotAllowed(path_buf));
        }
    }

    // Check allowed directories
    let is_allowed = allowed_dirs
        .iter()
        .any(|allowed| path_buf.starts_with(allowed));

    if !is_allowed {
        return Err(ValidationError::PathNotAllowed(path_buf));
    }

    Ok(path_buf)
}

/// Validate a command against whitelist
pub fn validate_command<'a>(
    command: &'a str,
    whitelist: &[String],
) -> Result<&'a str, ValidationError> {
    if command.is_empty() {
        return Err(ValidationError::Empty);
    }

    if command.len() > MAX_COMMAND_LENGTH {
        return Err(ValidationError::TooLong(command.len(), MAX_COMMAND_LENGTH));
    }

    // Extract the base command (first component)
    let base_command = command.split_whitespace().next().unwrap_or(command);

    // Extract just the command name without path
    let cmd_name = Path::new(base_command)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(base_command);

    if !whitelist
        .iter()
        .any(|allowed| allowed == cmd_name || allowed == base_command)
    {
        return Err(ValidationError::CommandNotAllowed(command.to_string()));
    }

    Ok(command)
}

/// Validate and sanitize command arguments
pub fn validate_args(args: &str) -> Result<Vec<String>, ValidationError> {
    if args.len() > MAX_ARGS_LENGTH {
        return Err(ValidationError::TooLong(args.len(), MAX_ARGS_LENGTH));
    }

    // Check for forbidden characters
    for c in args.chars() {
        if FORBIDDEN_CHARS.contains(&c) {
            return Err(ValidationError::ForbiddenCharacter(c));
        }
    }

    // Split arguments safely
    let parsed_args: Vec<String> = shell_words::split(args)
        .map_err(|_| ValidationError::InvalidPath("Invalid argument format".to_string()))?;

    Ok(parsed_args)
}

/// Validate JSON task input
pub fn validate_json_input(json: &str) -> Result<simd_json::OwnedValue, ValidationError> {
    if json.len() > MAX_INPUT_LENGTH {
        return Err(ValidationError::TooLong(json.len(), MAX_INPUT_LENGTH));
    }

    // Parse JSON to ensure it's valid
    serde_json::from_str(json).map_err(|_| ValidationError::InvalidPath("Invalid JSON".to_string()))
}

/// Sanitize output by truncating if necessary
pub fn sanitize_output(output: &str, max_size: usize) -> String {
    if output.len() <= max_size {
        output.to_string()
    } else {
        let truncated = &output[..max_size];
        format!("{}... [truncated, {} bytes total]", truncated, output.len())
    }
}

/// Validate environment variable name
pub fn validate_env_name(name: &str) -> Result<&str, ValidationError> {
    if name.is_empty() {
        return Err(ValidationError::Empty);
    }

    // Only allow alphanumeric and underscore
    if !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err(ValidationError::ForbiddenCharacter(
            name.chars()
                .find(|c| !c.is_alphanumeric() && *c != '_')
                .unwrap(),
        ));
    }

    Ok(name)
}

/// Validate environment variable value
pub fn validate_env_value(value: &str) -> Result<&str, ValidationError> {
    // Allow most characters but check for null bytes
    if value.contains('\0') {
        return Err(ValidationError::ForbiddenCharacter('\0'));
    }

    if value.len() > MAX_PATH_LENGTH {
        return Err(ValidationError::TooLong(value.len(), MAX_PATH_LENGTH));
    }

    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_input() {
        assert!(validate_input("hello world").is_ok());
        assert!(validate_input("test;rm -rf").is_err());
        assert!(validate_input("$(whoami)").is_err());
        assert!(validate_input("").is_err());
    }

    #[test]
    fn test_validate_path() {
        let allowed = vec![PathBuf::from("/home"), PathBuf::from("/tmp")];
        let forbidden = vec![PathBuf::from("/etc")];

        assert!(validate_path("/home/user/file.txt", &allowed, &forbidden).is_ok());
        assert!(validate_path("/tmp/test", &allowed, &forbidden).is_ok());
        assert!(validate_path("/etc/passwd", &allowed, &forbidden).is_err());
        assert!(validate_path("/root/file", &allowed, &forbidden).is_err());
    }

    #[test]
    fn test_validate_command() {
        let whitelist = vec!["python".to_string(), "cargo".to_string()];

        assert!(validate_command("python", &whitelist).is_ok());
        assert!(validate_command("/usr/bin/python", &whitelist).is_ok());
        assert!(validate_command("cargo", &whitelist).is_ok());
        assert!(validate_command("rm", &whitelist).is_err());
    }

    #[test]
    fn test_path_traversal() {
        let allowed = vec![PathBuf::from("/home")];
        let forbidden = vec![];

        assert!(validate_path("/home/../etc/passwd", &allowed, &forbidden).is_err());
        assert!(validate_path("/home/user/../../../etc", &allowed, &forbidden).is_err());
    }
}
