//! Developer Experience Optimizer Agent

use async_trait::async_trait;
use std::process::Command;

use crate::agents::base::{validation, AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;

const ALLOWED_DIRS: &[&str] = &["/tmp", "/home", "/opt"];

pub struct DxOptimizerAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl DxOptimizerAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            agent_id,
            profile: SecurityProfile::orchestration(
                "dx-optimizer",
                vec!["code-reviewer", "debugger", "performance-engineer"],
            ),
        }
    }

    fn analyze_setup(&self, path: Option<&str>) -> Result<String, String> {
        let dir = path.unwrap_or(".");
        let validated_path = validation::validate_path(dir, ALLOWED_DIRS)?;

        let mut analysis = String::from("DX Analysis Report:\n\n");

        // Check for common config files
        let configs = [
            ("package.json", "Node.js project"),
            ("Cargo.toml", "Rust project"),
            ("pyproject.toml", "Python project"),
            ("go.mod", "Go project"),
            (".eslintrc", "ESLint configured"),
            (".prettierrc", "Prettier configured"),
            ("Dockerfile", "Docker configured"),
            ("docker-compose.yml", "Docker Compose configured"),
            (".github/workflows", "GitHub Actions configured"),
        ];

        for (file, desc) in configs {
            let check_path = format!("{}/{}", validated_path, file);
            if std::path::Path::new(&check_path).exists() {
                analysis.push_str(&format!("✓ {} ({})\n", desc, file));
            }
        }

        Ok(analysis)
    }

    fn suggest_improvements(&self, path: Option<&str>) -> Result<String, String> {
        let dir = path.unwrap_or(".");
        let validated_path = validation::validate_path(dir, ALLOWED_DIRS)?;

        let mut suggestions = String::from("DX Improvement Suggestions:\n\n");

        // Check what's missing
        let configs = [
            (
                ".editorconfig",
                "Add EditorConfig for consistent formatting",
            ),
            (".gitignore", "Add .gitignore for clean version control"),
            ("README.md", "Add README.md for documentation"),
            ("CONTRIBUTING.md", "Add contribution guidelines"),
            (".pre-commit-config.yaml", "Add pre-commit hooks"),
        ];

        for (file, suggestion) in configs {
            let check_path = format!("{}/{}", validated_path, file);
            if !std::path::Path::new(&check_path).exists() {
                suggestions.push_str(&format!("• {}\n", suggestion));
            }
        }

        Ok(suggestions)
    }

    fn git_hooks_status(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("git");
        cmd.arg("config").arg("--get").arg("core.hooksPath");

        if let Some(dir) = path {
            let validated_path = validation::validate_path(dir, ALLOWED_DIRS)?;
            cmd.current_dir(validated_path);
        }

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);

        if stdout.is_empty() {
            Ok("Git hooks: Using default .git/hooks directory".to_string())
        } else {
            Ok(format!(
                "Git hooks: Custom path configured: {}",
                stdout.trim()
            ))
        }
    }
}

#[async_trait]
impl AgentTrait for DxOptimizerAgent {
    fn agent_type(&self) -> &str {
        "dx-optimizer"
    }
    fn name(&self) -> &str {
        "DX Optimizer"
    }
    fn description(&self) -> &str {
        "Developer experience optimization and workflow improvement"
    }

    fn operations(&self) -> Vec<String> {
        vec![
            "analyze".to_string(),
            "suggest".to_string(),
            "hooks".to_string(),
        ]
    }

    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        let result = match task.operation.as_str() {
            "analyze" => self.analyze_setup(task.path.as_deref()),
            "suggest" => self.suggest_improvements(task.path.as_deref()),
            "hooks" => self.git_hooks_status(task.path.as_deref()),
            _ => Err(format!("Unknown operation: {}", task.operation)),
        };

        match result {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
