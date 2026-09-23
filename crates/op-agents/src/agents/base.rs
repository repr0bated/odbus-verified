//! Base agent trait and common types
//!
//! Defines the common interface for all agents and shared types.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::security::{SandboxExecutor, SandboxResult, SecurityProfile};

/// Agent task input
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTask {
    /// Task type identifier (matches agent type)
    #[serde(rename = "type")]
    pub task_type: String,

    /// Operation to perform
    pub operation: String,

    /// Working path (optional)
    #[serde(default)]
    pub path: Option<String>,

    /// Additional arguments
    #[serde(default)]
    pub args: Option<String>,

    /// Additional configuration
    #[serde(default)]
    pub config: HashMap<String, simd_json::OwnedValue>,
}

impl AgentTask {
    pub fn new(task_type: &str, operation: &str) -> Self {
        Self {
            task_type: task_type.to_string(),
            operation: operation.to_string(),
            path: None,
            args: None,
            config: HashMap::new(),
        }
    }

    pub fn with_path(mut self, path: &str) -> Self {
        self.path = Some(path.to_string());
        self
    }

    pub fn with_args(mut self, args: &str) -> Self {
        self.args = Some(args.to_string());
        self
    }

    pub fn with_config(mut self, key: &str, value: simd_json::OwnedValue) -> Self {
        self.config.insert(key.to_string(), value);
        self
    }
}

/// Agent task result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    /// Whether the operation succeeded
    pub success: bool,

    /// Operation that was performed
    pub operation: String,

    /// Result data
    pub data: String,

    /// Additional metadata
    #[serde(default)]
    pub metadata: HashMap<String, simd_json::OwnedValue>,
}

impl TaskResult {
    pub fn success(operation: &str, data: String) -> Self {
        Self {
            success: true,
            operation: operation.to_string(),
            data,
            metadata: HashMap::new(),
        }
    }

    pub fn failure(operation: &str, error: String) -> Self {
        Self {
            success: false,
            operation: operation.to_string(),
            data: error,
            metadata: HashMap::new(),
        }
    }

    pub fn from_execution(operation: &str, result: &SandboxResult) -> Self {
        let data = format!("stdout:\n{}\n\nstderr:\n{}", result.stdout, result.stderr);

        Self {
            success: result.success,
            operation: operation.to_string(),
            data,
            metadata: {
                let mut meta = HashMap::new();
                meta.insert(
                    "duration_ms".to_string(),
                    simd_json::json!(result.duration_ms),
                );
                meta.insert("truncated".to_string(), simd_json::json!(result.truncated));
                meta
            },
        }
    }

    pub fn with_metadata(mut self, key: &str, value: simd_json::OwnedValue) -> Self {
        self.metadata.insert(key.to_string(), value);
        self
    }

    pub fn to_json(&self) -> String {
        simd_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
}

/// Agent execution context
pub struct AgentContext {
    /// Agent ID
    pub agent_id: String,

    /// Security profile
    pub profile: SecurityProfile,

    /// Sandbox executor
    pub executor: SandboxExecutor,

    /// Working directory
    pub working_dir: Option<PathBuf>,
}

impl AgentContext {
    pub fn new(agent_id: String, profile: SecurityProfile) -> Self {
        let executor = SandboxExecutor::new(profile.clone());
        Self {
            agent_id,
            profile,
            executor,
            working_dir: None,
        }
    }

    pub fn with_working_dir(mut self, dir: PathBuf) -> Self {
        self.working_dir = Some(dir);
        self
    }
}

/// Base trait for all agents
#[async_trait]
pub trait AgentTrait: Send + Sync {
    /// Get agent type identifier
    fn agent_type(&self) -> &str;

    /// Get agent display name
    fn name(&self) -> &str;

    /// Get agent description
    fn description(&self) -> &str;

    /// Get supported operations
    fn operations(&self) -> Vec<String>;

    /// Get security profile
    fn security_profile(&self) -> &SecurityProfile;

    /// Execute a task
    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String>;

    /// Get agent status
    fn get_status(&self) -> String {
        format!("{} is running", self.name())
    }

    /// Check if agent supports an operation
    fn supports_operation(&self, op: &str) -> bool {
        self.operations().iter().any(|o| o == op)
    }
}

/// Common validation functions for agents
pub mod validation {
    use std::path::{Component, Path};
    pub const FORBIDDEN_CHARS: &[char] = &[
        '$', '`', ';', '&', '|', '>', '<', '(', ')', '{', '}', '\n', '\r',
    ];
    pub const MAX_PATH_LENGTH: usize = 4096;
    pub const MAX_ARGS_LENGTH: usize = 256;

    pub fn validate_path(path: &str, allowed_dirs: &[&str]) -> Result<String, String> {
        if path.len() > MAX_PATH_LENGTH {
            return Err("Path exceeds maximum length".to_string());
        }

        for c in FORBIDDEN_CHARS {
            if path.contains(*c) {
                return Err(format!("Path contains forbidden character: {:?}", c));
            }
        }

        let requested = Path::new(path);
        if requested
            .components()
            .any(|part| matches!(part, Component::ParentDir))
        {
            return Err("Path traversal is not allowed".to_string());
        }
        let canonical = requested
            .canonicalize()
            .map_err(|e| format!("Failed to resolve path: {e}"))?;
        let is_allowed = allowed_dirs.iter().any(|dir| {
            Path::new(dir)
                .canonicalize()
                .is_ok_and(|root| canonical.starts_with(root))
        });
        if !is_allowed {
            return Err(format!(
                "Path must be within allowed directories: {:?}",
                allowed_dirs
            ));
        }

        Ok(canonical.to_string_lossy().into_owned())
    }

    pub fn validate_args(args: &str) -> Result<String, String> {
        if args.len() > MAX_ARGS_LENGTH {
            return Err("Args string too long".to_string());
        }

        for c in FORBIDDEN_CHARS {
            if args.contains(*c) {
                return Err(format!("Args contains forbidden character: {:?}", c));
            }
        }

        Ok(args.to_string())
    }
}

#[cfg(test)]
mod path_security_tests {
    use super::validation::validate_path;

    #[test]
    fn debugger_path_rejects_traversal_symlink_and_prefix_lookalike() {
        let root = tempfile::tempdir().unwrap();
        let allowed = root.path().join("allowed");
        std::fs::create_dir(&allowed).unwrap();
        let inside = allowed.join("log");
        std::fs::write(&inside, "ok").unwrap();
        let outside = root.path().join("outside");
        std::fs::write(&outside, "secret").unwrap();
        std::os::unix::fs::symlink(&outside, allowed.join("link")).unwrap();
        let root_str = allowed.to_str().unwrap();
        assert!(validate_path(inside.to_str().unwrap(), &[root_str]).is_ok());
        assert!(validate_path(allowed.join("../outside").to_str().unwrap(), &[root_str]).is_err());
        assert!(validate_path(allowed.join("link").to_str().unwrap(), &[root_str]).is_err());
        assert!(validate_path(outside.to_str().unwrap(), &[root_str]).is_err());
    }
}

/// Macro for implementing common agent boilerplate
#[macro_export]
macro_rules! impl_agent_common {
    ($agent:ty, $type:expr, $name:expr, $desc:expr, $ops:expr) => {
        impl $agent {
            pub fn agent_type(&self) -> &str {
                $type
            }
            pub fn name(&self) -> &str {
                $name
            }
            pub fn description(&self) -> &str {
                $desc
            }
            pub fn operations(&self) -> Vec<String> {
                $ops.iter().map(|s| s.to_string()).collect()
            }
        }
    };
}
