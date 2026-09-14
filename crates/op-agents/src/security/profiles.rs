//! Security profiles for different agent execution types
//!
//! Defines security constraints for four main agent categories:
//! - CodeExecution: Language-specific execution with strict sandboxing
//! - ReadOnlyAnalysis: Read-only analysis with tool whitelisting
//! - ContentGeneration: Documentation/content generation with write limits
//! - Orchestration: Meta-agents that coordinate other agents

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

/// Agent security profile categories
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProfileCategory {
    /// Code execution agents (language pros, shell agents)
    CodeExecution,
    /// Analysis agents (reviewers, auditors)
    #[default]
    ReadOnlyAnalysis,
    /// Content generation agents (docs, tutorials)
    ContentGeneration,
    /// Orchestration agents (meta-agents, coordinators)
    Orchestration,
}

/// Security profile configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// Profile category
    pub category: ProfileCategory,

    /// Commands allowed for execution
    #[serde(default)]
    pub allowed_commands: HashSet<String>,

    /// Paths allowed for reading
    #[serde(default)]
    pub allowed_read_paths: Vec<PathBuf>,

    /// Paths allowed for writing
    #[serde(default)]
    pub allowed_write_paths: Vec<PathBuf>,

    /// Paths explicitly forbidden (takes precedence)
    #[serde(default)]
    pub forbidden_paths: Vec<PathBuf>,

    /// Tools allowed for analysis agents
    #[serde(default)]
    pub allowed_tools: HashSet<String>,

    /// Subagents allowed for orchestration
    #[serde(default)]
    pub allowed_subagents: HashSet<String>,

    /// Execution timeout
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,

    /// Maximum memory (MB)
    #[serde(default = "default_max_memory_mb")]
    pub max_memory_mb: u64,

    /// Maximum output size (bytes)
    #[serde(default = "default_max_output_size")]
    pub max_output_size: usize,

    /// Maximum concurrent operations
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: usize,

    /// Whether operations require approval
    #[serde(default)]
    pub requires_approval: bool,

    /// Whether agent has root privileges
    #[serde(default)]
    pub requires_root: bool,
}

fn default_timeout_secs() -> u64 {
    60
}
fn default_max_memory_mb() -> u64 {
    512
}
fn default_max_output_size() -> usize {
    1_000_000
} // 1MB
fn default_max_concurrent() -> usize {
    1
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            category: ProfileCategory::ReadOnlyAnalysis,
            allowed_commands: HashSet::new(),
            allowed_read_paths: vec![PathBuf::from("/home"), PathBuf::from("/tmp")],
            allowed_write_paths: vec![],
            forbidden_paths: vec![
                PathBuf::from("/etc"),
                PathBuf::from("/root"),
                PathBuf::from("/var/lib"),
                PathBuf::from("/sys"),
                PathBuf::from("/proc"),
            ],
            allowed_tools: HashSet::new(),
            allowed_subagents: HashSet::new(),
            timeout_secs: 60,
            max_memory_mb: 512,
            max_output_size: 1_000_000,
            max_concurrent: 1,
            requires_approval: false,
            requires_root: false,
        }
    }
}

/// Complete security profile including runtime configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityProfile {
    /// Agent type identifier
    pub agent_type: String,

    /// Human-readable name
    pub name: String,

    /// Description
    pub description: String,

    /// Security configuration
    #[serde(flatten)]
    pub config: SecurityConfig,

    /// Operations this profile allows
    #[serde(default)]
    pub operations: Vec<OperationSecurity>,
}

/// Per-operation security settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationSecurity {
    /// Operation name
    pub name: String,

    /// Whether this operation requires explicit approval
    #[serde(default)]
    pub requires_approval: bool,

    /// Custom timeout for this operation
    pub timeout_secs: Option<u64>,

    /// Additional commands allowed for this operation
    #[serde(default)]
    pub extra_commands: HashSet<String>,

    /// Risk level (for UI display)
    #[serde(default)]
    pub risk_level: RiskLevel,
}

/// Risk level classification
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    #[default]
    Low,
    Medium,
    High,
    Critical,
}

impl SecurityProfile {
    /// Create a profile for code execution agents (language pros)
    pub fn code_execution(agent_type: &str, commands: Vec<&str>) -> Self {
        Self {
            agent_type: agent_type.to_string(),
            name: format!("{} Pro", agent_type.replace('-', " ").to_uppercase()),
            description: format!("Code execution agent for {}", agent_type),
            config: SecurityConfig {
                category: ProfileCategory::CodeExecution,
                allowed_commands: commands.into_iter().map(|s| s.to_string()).collect(),
                allowed_read_paths: vec![
                    PathBuf::from("/home"),
                    PathBuf::from("/tmp"),
                    PathBuf::from("/opt"),
                ],
                allowed_write_paths: vec![PathBuf::from("/tmp")],
                forbidden_paths: vec![
                    PathBuf::from("/etc"),
                    PathBuf::from("/root"),
                    PathBuf::from("/var"),
                    PathBuf::from("/sys"),
                    PathBuf::from("/proc"),
                ],
                timeout_secs: 300,
                max_memory_mb: 2048,
                max_output_size: 5_000_000, // 5MB for build output
                ..Default::default()
            },
            operations: vec![],
        }
    }

    /// Create a profile for read-only analysis agents
    pub fn read_only_analysis(agent_type: &str, tools: Vec<&str>) -> Self {
        Self {
            agent_type: agent_type.to_string(),
            name: agent_type.replace('-', " ").to_string(),
            description: format!("Read-only analysis agent: {}", agent_type),
            config: SecurityConfig {
                category: ProfileCategory::ReadOnlyAnalysis,
                allowed_tools: tools.into_iter().map(|s| s.to_string()).collect(),
                allowed_read_paths: vec![
                    PathBuf::from("/home"),
                    PathBuf::from("/tmp"),
                    PathBuf::from("/opt"),
                ],
                allowed_write_paths: vec![], // Read-only
                timeout_secs: 120,
                max_memory_mb: 1024,
                ..Default::default()
            },
            operations: vec![],
        }
    }

    /// Create a profile for content generation agents
    pub fn content_generation(agent_type: &str) -> Self {
        Self {
            agent_type: agent_type.to_string(),
            name: agent_type.replace('-', " ").to_string(),
            description: format!("Content generation agent: {}", agent_type),
            config: SecurityConfig {
                category: ProfileCategory::ContentGeneration,
                allowed_read_paths: vec![PathBuf::from("/home"), PathBuf::from("/tmp")],
                allowed_write_paths: vec![PathBuf::from("/tmp")],
                timeout_secs: 180,
                max_output_size: 10_000_000, // 10MB for docs
                ..Default::default()
            },
            operations: vec![],
        }
    }

    /// Create a profile for orchestration agents
    pub fn orchestration(agent_type: &str, subagents: Vec<&str>) -> Self {
        Self {
            agent_type: agent_type.to_string(),
            name: agent_type.replace('-', " ").to_string(),
            description: format!("Orchestration agent: {}", agent_type),
            config: SecurityConfig {
                category: ProfileCategory::Orchestration,
                allowed_subagents: subagents.into_iter().map(|s| s.to_string()).collect(),
                max_concurrent: 5,
                timeout_secs: 600, // Longer timeout for coordinated tasks
                ..Default::default()
            },
            operations: vec![],
        }
    }

    /// Check if a command is allowed
    pub fn is_command_allowed(&self, cmd: &str) -> bool {
        self.config.allowed_commands.contains(cmd)
    }

    /// Check if a tool is allowed
    pub fn is_tool_allowed(&self, tool: &str) -> bool {
        self.config.allowed_tools.contains(tool)
    }

    /// Check if a path can be read
    pub fn can_read_path(&self, path: &std::path::Path) -> bool {
        // Check forbidden paths first
        for forbidden in &self.config.forbidden_paths {
            if path.starts_with(forbidden) {
                return false;
            }
        }

        // Then check allowed paths
        for allowed in &self.config.allowed_read_paths {
            if path.starts_with(allowed) {
                return true;
            }
        }

        false
    }

    /// Check if a path can be written
    pub fn can_write_path(&self, path: &std::path::Path) -> bool {
        // Check forbidden paths first
        for forbidden in &self.config.forbidden_paths {
            if path.starts_with(forbidden) {
                return false;
            }
        }

        // Then check allowed paths
        for allowed in &self.config.allowed_write_paths {
            if path.starts_with(allowed) {
                return true;
            }
        }

        false
    }

    /// Get timeout as Duration
    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.config.timeout_secs)
    }
}

/// Pre-defined security profiles for common agent types
pub mod presets {
    use super::*;

    /// Python Pro agent profile
    pub fn python_pro() -> SecurityProfile {
        let mut profile = SecurityProfile::code_execution(
            "python-pro",
            vec![
                "python", "python3", "pip", "pip3", "uv", "ruff", "pytest", "mypy", "black",
                "isort",
            ],
        );
        profile.operations = vec![
            OperationSecurity {
                name: "run".to_string(),
                requires_approval: false,
                timeout_secs: Some(60),
                extra_commands: HashSet::new(),
                risk_level: RiskLevel::Medium,
            },
            OperationSecurity {
                name: "test".to_string(),
                requires_approval: false,
                timeout_secs: Some(300),
                extra_commands: HashSet::new(),
                risk_level: RiskLevel::Low,
            },
            OperationSecurity {
                name: "lint".to_string(),
                requires_approval: false,
                timeout_secs: Some(60),
                extra_commands: HashSet::new(),
                risk_level: RiskLevel::Low,
            },
            OperationSecurity {
                name: "format".to_string(),
                requires_approval: false,
                timeout_secs: Some(60),
                extra_commands: HashSet::new(),
                risk_level: RiskLevel::Low,
            },
            OperationSecurity {
                name: "install".to_string(),
                requires_approval: true,
                timeout_secs: Some(300),
                extra_commands: HashSet::new(),
                risk_level: RiskLevel::High,
            },
        ];
        profile
    }

    /// Rust Pro agent profile
    pub fn rust_pro() -> SecurityProfile {
        let mut profile = SecurityProfile::code_execution(
            "rust-pro",
            vec!["cargo", "rustc", "rustfmt", "clippy-driver"],
        );
        profile.operations = vec![
            OperationSecurity {
                name: "check".to_string(),
                requires_approval: false,
                timeout_secs: Some(120),
                extra_commands: HashSet::new(),
                risk_level: RiskLevel::Low,
            },
            OperationSecurity {
                name: "build".to_string(),
                requires_approval: false,
                timeout_secs: Some(600),
                extra_commands: HashSet::new(),
                risk_level: RiskLevel::Medium,
            },
            OperationSecurity {
                name: "test".to_string(),
                requires_approval: false,
                timeout_secs: Some(600),
                extra_commands: HashSet::new(),
                risk_level: RiskLevel::Medium,
            },
            OperationSecurity {
                name: "clippy".to_string(),
                requires_approval: false,
                timeout_secs: Some(120),
                extra_commands: HashSet::new(),
                risk_level: RiskLevel::Low,
            },
        ];
        profile
    }

    /// Go Pro agent profile
    pub fn golang_pro() -> SecurityProfile {
        SecurityProfile::code_execution(
            "golang-pro",
            vec!["go", "gofmt", "golint", "staticcheck", "gopls"],
        )
    }

    /// JavaScript Pro agent profile
    pub fn javascript_pro() -> SecurityProfile {
        SecurityProfile::code_execution(
            "javascript-pro",
            vec![
                "node", "npm", "npx", "yarn", "pnpm", "eslint", "prettier", "jest", "vitest",
            ],
        )
    }

    /// TypeScript Pro agent profile
    pub fn typescript_pro() -> SecurityProfile {
        SecurityProfile::code_execution(
            "typescript-pro",
            vec![
                "node", "npm", "npx", "yarn", "pnpm", "tsc", "eslint", "prettier", "jest", "vitest",
            ],
        )
    }

    /// Code Reviewer profile
    pub fn code_reviewer() -> SecurityProfile {
        SecurityProfile::read_only_analysis(
            "code-reviewer",
            vec!["rg", "grep", "wc", "cloc", "tokei", "diff", "git"],
        )
    }

    /// Security Auditor profile
    pub fn security_auditor() -> SecurityProfile {
        SecurityProfile::read_only_analysis(
            "security-auditor",
            vec![
                "rg",
                "grep",
                "semgrep",
                "bandit",
                "safety",
                "npm audit",
                "cargo audit",
            ],
        )
    }

    /// Docs Architect profile
    pub fn docs_architect() -> SecurityProfile {
        SecurityProfile::content_generation("docs-architect")
    }

    /// TDD Orchestrator profile
    pub fn tdd_orchestrator() -> SecurityProfile {
        SecurityProfile::orchestration(
            "tdd-orchestrator",
            vec!["code-reviewer", "test-automator", "debugger"],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_python_pro_profile() {
        let profile = presets::python_pro();
        assert!(profile.is_command_allowed("python3"));
        assert!(profile.is_command_allowed("pytest"));
        assert!(!profile.is_command_allowed("rm"));
    }

    #[test]
    fn test_path_validation() {
        let profile = presets::python_pro();
        assert!(profile.can_read_path(std::path::Path::new("/home/user/project")));
        assert!(!profile.can_read_path(std::path::Path::new("/etc/passwd")));
        assert!(profile.can_write_path(std::path::Path::new("/tmp/test.py")));
        assert!(!profile.can_write_path(std::path::Path::new("/etc/test")));
    }
}
