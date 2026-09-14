//! Input Validation and Sanitization for Tool Execution
//!
//! Provides comprehensive input validation while preserving full control
//! for the chatbot orchestrator system.
//!
//! Uses simd-json for high-performance JSON processing while maintaining
//! compatibility with the existing serde_json ecosystem.

use anyhow::{anyhow, Result};
use jsonschema::Validator;
use serde_json::Value; // Keep for compatibility with existing code
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{error, warn};

/// Characters forbidden in user input to prevent injection
pub const FORBIDDEN_CHARS: &[char] = &[
    '$', '`', ';', '&', '|', '>', '<', '(', ')', '{', '}', '\n', '\r', '\0',
];

/// Maximum length for various input types
pub const MAX_PATH_LENGTH: usize = 4096;
pub const MAX_COMMAND_LENGTH: usize = 256;
pub const MAX_ARGS_LENGTH: usize = 4096;
pub const MAX_INPUT_LENGTH: usize = 1_000_000; // 1MB

/// Configuration for input validation behavior
#[derive(Debug, Clone)]
pub struct ValidationConfig {
    /// Whether to enforce strict schema validation
    pub strict_validation: bool,
    /// Whether to sanitize inputs for injection attacks
    pub sanitize_inputs: bool,
    /// Sessions that bypass validation (chatbot orchestrator)
    pub trusted_sessions: HashSet<String>,
    /// Maximum input size (bytes)
    pub max_input_size: usize,
    /// Allowed command whitelist for shell tools
    pub command_whitelist: HashSet<String>,
    /// Allowed directories for file operations
    pub allowed_dirs: Vec<PathBuf>,
    /// Forbidden directories for file operations
    pub forbidden_dirs: Vec<PathBuf>,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        let mut trusted_sessions = HashSet::new();
        // Chatbot orchestrator sessions get full control
        trusted_sessions.insert("chatbot".to_string());
        trusted_sessions.insert("orchestrator".to_string());
        trusted_sessions.insert("system".to_string());

        // Default command whitelist for shell tools
        let mut command_whitelist = HashSet::new();
        // Allow common DevOps commands for non-trusted sessions
        for cmd in [
            "ls",
            "cat",
            "grep",
            "find",
            "ps",
            "top",
            "df",
            "du",
            "free",
            "uptime",
            "whoami",
            "id",
            "pwd",
            "date",
            "uname",
            "which",
            "whereis",
            "file",
            "head",
            "tail",
            "wc",
            "sort",
            "uniq",
            "cut",
            "awk",
            "sed",
            "git",
            "docker",
            "kubectl",
            "systemctl",
            "journalctl",
            "curl",
            "wget",
        ]
        .iter()
        {
            command_whitelist.insert(cmd.to_string());
        }

        // Default allowed directories (home directory and temp)
        let allowed_dirs = vec![
            PathBuf::from("/tmp"),
            PathBuf::from("/var/tmp"),
            PathBuf::from("/home"),
        ];

        // Forbidden system directories
        let forbidden_dirs = vec![
            PathBuf::from("/boot"),
            PathBuf::from("/dev"),
            PathBuf::from("/proc/sys"),
            PathBuf::from("/sys"),
            PathBuf::from("/root"),
            PathBuf::from("/etc/shadow"),
            PathBuf::from("/etc/passwd"),
        ];

        Self {
            strict_validation: true,
            sanitize_inputs: true,
            trusted_sessions,
            max_input_size: 10 * 1024 * 1024, // 10MB
            command_whitelist,
            allowed_dirs,
            forbidden_dirs,
        }
    }
}

/// Input validator for tool execution
#[derive(Clone)]
pub struct InputValidator {
    config: ValidationConfig,
    schema_cache: Arc<tokio::sync::RwLock<std::collections::HashMap<String, Arc<Validator>>>>,
}

impl Default for InputValidator {
    fn default() -> Self {
        Self::new()
    }
}

impl InputValidator {
    /// Create a new validator with default config
    pub fn new() -> Self {
        Self::with_config(ValidationConfig::default())
    }

    /// Create a new validator with custom config
    pub fn with_config(config: ValidationConfig) -> Self {
        Self {
            config,
            schema_cache: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        }
    }

    /// Validate and sanitize input for tool execution
    pub async fn validate_input(
        &self,
        tool_name: &str,
        input: &Value,
        schema: &Value,
        session_id: Option<&str>,
    ) -> Result<ValidatedInput> {
        let session_id = session_id.unwrap_or("anonymous");

        // Check input size
        if let Err(e) = self.check_input_size(input) {
            error!(tool = %tool_name, session = %session_id, error = %e, "Input size validation failed");
            return Err(e);
        }

        // Trusted sessions (chatbot orchestrator) get minimal validation
        let is_trusted = self.config.trusted_sessions.contains(session_id);

        let mut validation_errors = Vec::new();
        let mut sanitized_input = input.clone();

        // Schema validation (always run for safety, but may be non-blocking for trusted)
        if let Err(e) = self
            .validate_schema(tool_name, &sanitized_input, schema)
            .await
        {
            if is_trusted && !self.config.strict_validation {
                warn!(tool = %tool_name, session = %session_id, "Schema validation bypassed for trusted session");
            } else {
                validation_errors.push(format!("Schema validation failed: {}", e));
            }
        }

        // Input sanitization
        if self.config.sanitize_inputs {
            if let Err(e) = self.sanitize_input(&mut sanitized_input) {
                if is_trusted && !self.config.strict_validation {
                    warn!(tool = %tool_name, session = %session_id, "Input sanitization bypassed for trusted session");
                } else {
                    validation_errors.push(format!("Input sanitization failed: {}", e));
                }
            }
        }

        // Security validation for shell commands, paths, etc.
        if let Err(e) = self.security_validate(tool_name, &sanitized_input, is_trusted) {
            if is_trusted && !self.config.strict_validation {
                warn!(tool = %tool_name, session = %session_id, "Security validation bypassed for trusted session");
            } else {
                validation_errors.push(format!("Security validation failed: {}", e));
            }
        }

        // Return validation result
        Ok(ValidatedInput {
            input: sanitized_input,
            is_valid: validation_errors.is_empty(),
            validation_errors,
            was_sanitized: self.config.sanitize_inputs && !is_trusted,
            session_trusted: is_trusted,
        })
    }

    /// Check input size limits
    fn check_input_size(&self, input: &Value) -> Result<()> {
        let input_str = serde_json::to_string(input)
            .map_err(|e| anyhow!("Failed to serialize input for size check: {}", e))?;

        if input_str.len() > self.config.max_input_size {
            return Err(anyhow!(
                "Input size {} bytes exceeds maximum {} bytes",
                input_str.len(),
                self.config.max_input_size
            ));
        }

        Ok(())
    }

    /// Validate input against JSON schema
    async fn validate_schema(&self, tool_name: &str, input: &Value, schema: &Value) -> Result<()> {
        // Create schema key for caching
        let schema_key = format!("{}:{}", tool_name, serde_json::to_string(schema)?);

        // Get or create compiled schema
        // Keep the read lock scoped to the lookup. On a cache miss, compiling
        // and acquiring the write lock while a read guard is alive deadlocks
        // every caller waiting for the same schema.
        let cached_schema = {
            let cache = self.schema_cache.read().await;
            cache.get(&schema_key).cloned()
        };

        let compiled_schema = if let Some(schema) = cached_schema {
            schema
        } else {
            // Compile outside the lock, then publish exactly one shared schema
            // for concurrent first-use callers.
            let compiled = jsonschema::validator_for(schema)
                .map_err(|e| anyhow!("Failed to compile schema for {}: {}", tool_name, e))?;
            let arc_schema = Arc::new(compiled);

            let mut cache = self.schema_cache.write().await;
            cache
                .entry(schema_key)
                .or_insert_with(|| arc_schema.clone())
                .clone()
        };

        // Validate against schema. jsonschema >= 0.20 returns a single error from
        // `validate`; `iter_errors` is the all-errors iterator the old
        // `JSONSchema::validate` used to return.
        let error_messages: Vec<String> = compiled_schema
            .iter_errors(input)
            .map(|e| format!("{} at path: {}", e.instance_path, e))
            .collect();
        if !error_messages.is_empty() {
            return Err(anyhow!(
                "Schema validation failed: {}",
                error_messages.join("; ")
            ));
        }

        Ok(())
    }

    /// Sanitize input to prevent injection attacks
    fn sanitize_input(&self, input: &mut Value) -> Result<()> {
        // Recursive sanitization function
        fn sanitize_value(value: &mut Value) -> Result<()> {
            match value {
                Value::String(s) => {
                    // Remove null bytes and control characters except newlines and tabs
                    *s = s
                        .chars()
                        .filter(|c| *c != '\0' && (*c >= ' ' || *c == '\n' || *c == '\t'))
                        .collect();

                    // Check for suspicious patterns in non-trusted contexts
                    if s.contains("../../../") || s.contains("..\\") {
                        return Err(anyhow!(
                            "Potentially dangerous path traversal pattern detected"
                        ));
                    }
                }
                Value::Array(arr) => {
                    for item in arr.iter_mut() {
                        sanitize_value(item)?;
                    }
                }
                Value::Object(obj) => {
                    for (key, val) in obj.iter_mut() {
                        // Sanitize keys too
                        if key.contains("..") || key.contains('\0') {
                            return Err(anyhow!("Invalid object key detected"));
                        }
                        sanitize_value(val)?;
                    }
                }
                _ => {} // Other types are safe as-is
            }
            Ok(())
        }

        sanitize_value(input)
    }

    /// Additional security validation for specific tool types
    fn security_validate(&self, tool_name: &str, input: &Value, is_trusted: bool) -> Result<()> {
        // Trusted sessions bypass most security checks
        if is_trusted {
            return Ok(());
        }

        // Shell tools need extra validation
        if tool_name.contains("shell") || tool_name.contains("exec") {
            if let Some(cmd) = extract_command_from_input(input) {
                // Validate against command whitelist
                let base_cmd = cmd.split_whitespace().next().unwrap_or(&cmd);
                if !self.config.command_whitelist.contains(base_cmd) {
                    return Err(anyhow!(
                        "Command '{}' is not whitelisted for non-trusted sessions",
                        base_cmd
                    ));
                }

                // Check for dangerous patterns even in whitelisted commands
                let dangerous_patterns = [
                    "rm -rf /",
                    "sudo rm",
                    "mkfs",
                    "dd if=/dev/",
                    ">/etc/",
                    "format",
                    "fdisk /dev/",
                    "chmod 777 /",
                    "chown root",
                ];

                for pattern in &dangerous_patterns {
                    if cmd.to_lowercase().contains(pattern) {
                        return Err(anyhow!(
                            "Dangerous command pattern '{}' detected in shell command",
                            pattern
                        ));
                    }
                }

                // Validate command arguments for injection
                validate_input(&cmd)
                    .map_err(|e| anyhow!("Shell command validation failed: {}", e))?;
            }
        }

        // File operation tools need path validation
        if tool_name.contains("file") || tool_name.contains("fs") {
            if let Some(path) = extract_path_from_input(input) {
                let path_buf = PathBuf::from(&path);

                // Check path traversal
                if path.contains("..") {
                    return Err(anyhow!(
                        "Path traversal not allowed in non-trusted sessions: {}",
                        path
                    ));
                }

                // Check against forbidden directories first
                for forbidden in &self.config.forbidden_dirs {
                    if path_buf.starts_with(forbidden) {
                        return Err(anyhow!(
                            "Access to forbidden path '{}' is not allowed",
                            forbidden.display()
                        ));
                    }
                }

                // Check if path is within allowed directories
                let is_allowed = self
                    .config
                    .allowed_dirs
                    .iter()
                    .any(|allowed| path_buf.starts_with(allowed));

                if !is_allowed {
                    return Err(anyhow!(
                        "Path '{}' is not within allowed directories for non-trusted sessions",
                        path
                    ));
                }

                // Validate path input for forbidden characters
                validate_input(&path).map_err(|e| anyhow!("Path validation failed: {}", e))?;
            }
        }

        // General input validation for all tools
        if let Some(input_str) = extract_string_from_input(input) {
            validate_input(&input_str).map_err(|e| anyhow!("Input validation failed: {}", e))?;
        }

        Ok(())
    }
}

/// Result of input validation
#[derive(Debug, Clone)]
pub struct ValidatedInput {
    /// The validated and potentially sanitized input
    pub input: Value,
    /// Whether the input passed all validations
    pub is_valid: bool,
    /// List of validation errors (if any)
    pub validation_errors: Vec<String>,
    /// Whether the input was sanitized
    pub was_sanitized: bool,
    /// Whether the session is trusted (chatbot orchestrator)
    pub session_trusted: bool,
}

impl ValidatedInput {
    /// Get the validated input for execution
    pub fn into_input(self) -> Value {
        self.input
    }

    /// Check if execution should proceed
    pub fn should_proceed(&self) -> bool {
        self.is_valid || self.session_trusted
    }
}

/// Extract command string from input JSON
fn extract_command_from_input(input: &Value) -> Option<String> {
    if let Some(cmd) = input.get("command").and_then(|v| v.as_str()) {
        return Some(cmd.to_string());
    }

    if let Some(cmd) = input.get("cmd").and_then(|v| v.as_str()) {
        return Some(cmd.to_string());
    }

    if let Some(args) = input.get("args").and_then(|v| v.as_array()) {
        if let Some(first) = args.first().and_then(|v| v.as_str()) {
            return Some(first.to_string());
        }
    }

    None
}

/// Extract path string from input JSON
fn extract_path_from_input(input: &Value) -> Option<String> {
    if let Some(path) = input.get("path").and_then(|v| v.as_str()) {
        return Some(path.to_string());
    }

    if let Some(path) = input.get("file").and_then(|v| v.as_str()) {
        return Some(path.to_string());
    }

    if let Some(path) = input.get("directory").and_then(|v| v.as_str()) {
        return Some(path.to_string());
    }

    None
}

/// Extract string value from input JSON
fn extract_string_from_input(input: &Value) -> Option<String> {
    if let Some(s) = input.as_str() {
        return Some(s.to_string());
    }

    // Look for common string fields
    for field in ["text", "content", "data", "input", "value"] {
        if let Some(s) = input.get(field).and_then(|v| v.as_str()) {
            return Some(s.to_string());
        }
    }

    None
}

/// Validate a general input string (mirrors op-agents validation)
fn validate_input(input: &str) -> Result<()> {
    if input.is_empty() {
        return Err(anyhow!("Empty input not allowed"));
    }

    if input.len() > MAX_INPUT_LENGTH {
        return Err(anyhow!(
            "Input exceeds maximum length ({} > {})",
            input.len(),
            MAX_INPUT_LENGTH
        ));
    }

    for c in input.chars() {
        if FORBIDDEN_CHARS.contains(&c) {
            return Err(anyhow!("Input contains forbidden character: {:?}", c));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_trusted_session_bypass() {
        let validator = InputValidator::new();
        let schema = json!({"type": "object"});

        // Trusted session should pass even with invalid input
        let result = validator
            .validate_input(
                "test_tool",
                &json!({"invalid": "data"}),
                &schema,
                Some("chatbot"),
            )
            .await
            .unwrap();

        assert!(result.session_trusted);
        assert!(result.should_proceed());
    }

    #[tokio::test]
    async fn test_input_sanitization() {
        let validator = InputValidator::new();
        let schema = json!({"type": "object"});

        // Test null byte removal
        let input = json!({"text": "hello\x00world"});
        let result = validator
            .validate_input("test_tool", &input, &schema, Some("anonymous"))
            .await
            .unwrap();

        assert_eq!(result.input["text"], "helloworld");
    }

    #[tokio::test]
    async fn test_shell_command_validation() {
        let validator = InputValidator::new();
        let schema = json!({"type": "object"});

        // Dangerous command should be blocked for anonymous
        let input = json!({"command": "rm -rf /"});
        let result = validator
            .validate_input("shell_tool", &input, &schema, Some("anonymous"))
            .await;

        let result = result.unwrap();
        assert!(!result.is_valid);
        assert!(!result.should_proceed());

        // But allowed for trusted session
        let result = validator
            .validate_input("shell_tool", &input, &schema, Some("chatbot"))
            .await
            .unwrap();

        assert!(result.should_proceed());
    }

    #[tokio::test]
    async fn test_path_validation() {
        let validator = InputValidator::new();
        let schema = json!({"type": "object"});

        // Restricted path should be blocked for anonymous
        let input = json!({"path": "/etc/shadow"});
        let result = validator
            .validate_input("file_tool", &input, &schema, Some("anonymous"))
            .await;

        let result = result.unwrap();
        assert!(!result.is_valid);
        assert!(!result.should_proceed());

        // But allowed for trusted session
        let result = validator
            .validate_input("file_tool", &input, &schema, Some("chatbot"))
            .await
            .unwrap();

        assert!(result.should_proceed());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_schema_cache_concurrent_miss_warm_hit_and_invalid_payload() {
        let validator = std::sync::Arc::new(InputValidator::new());
        let schema = json!({
            "type": "object",
            "properties": {"name": {"type": "string"}},
            "required": ["name"]
        });
        let valid = json!({"name": "concurrent"});

        let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(4));
        let mut tasks = Vec::new();
        for _ in 0..4 {
            let validator = validator.clone();
            let schema = schema.clone();
            let valid = valid.clone();
            let barrier = barrier.clone();
            tasks.push(tokio::spawn(async move {
                barrier.wait().await;
                validator
                    .validate_input("cache_test", &valid, &schema, Some("anonymous"))
                    .await
            }));
        }

        let concurrent = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            let mut results = Vec::new();
            for task in tasks {
                results.push(task.await.expect("validation task must not panic")?);
            }
            Ok::<_, anyhow::Error>(results)
        })
        .await
        .expect("concurrent schema cache misses must not deadlock")
        .expect("concurrent validation must succeed");
        assert!(concurrent.iter().all(ValidatedInput::should_proceed));
        assert_eq!(validator.schema_cache.read().await.len(), 1);

        let warm = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            validator.validate_input("cache_test", &valid, &schema, Some("anonymous")),
        )
        .await
        .expect("warm schema cache hit must complete")
        .unwrap();
        assert!(warm.should_proceed());

        let invalid = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            validator.validate_input(
                "cache_test",
                &json!({"name": 42}),
                &schema,
                Some("anonymous"),
            ),
        )
        .await
        .expect("invalid payload validation must complete")
        .unwrap();
        assert!(!invalid.should_proceed());
        assert!(!invalid.validation_errors.is_empty());
    }
}

#[cfg(test)]
mod validation_tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_trusted_session_bypass() {
        let validator = InputValidator::new();
        let schema = json!({"type": "object"});

        // Trusted session should pass even with invalid input
        let result = validator
            .validate_input(
                "test_tool",
                &json!({"invalid": "data"}),
                &schema,
                Some("chatbot"),
            )
            .await
            .unwrap();

        assert!(result.session_trusted);
        assert!(result.should_proceed());
    }

    #[tokio::test]
    async fn test_non_trusted_restriction() {
        let validator = InputValidator::new();
        let schema = json!({"type": "object"});

        // Non-trusted session should be restricted
        let result = validator
            .validate_input(
                "shell_tool",
                &json!({"command": "rm -rf /etc/passwd"}),
                &schema,
                Some("anonymous"),
            )
            .await
            .unwrap();

        assert!(!result.session_trusted);
        assert!(!result.should_proceed());
        assert!(!result.validation_errors.is_empty());
    }

    #[tokio::test]
    async fn test_input_sanitization() {
        let validator = InputValidator::new();
        let schema = json!({"type": "object"});

        let input_with_null_bytes = json!({"text": "hello\x00world"});
        let result = validator
            .validate_input(
                "text_tool",
                &input_with_null_bytes,
                &schema,
                Some("anonymous"),
            )
            .await
            .unwrap();

        let sanitized_text = result.input["text"].as_str().unwrap();
        assert!(!sanitized_text.contains('\0'));
        assert_eq!(sanitized_text, "helloworld");
        assert!(result.was_sanitized);
    }
}
