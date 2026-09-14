//! Python Executor Agent
//!
//! Executes Python code with sandboxing.

use async_trait::async_trait;
use simd_json::json;
use simd_json::prelude::*;

use super::super::agent_trait::{
    AgentCapability, AgentCategory, AgentRequest, AgentResponse, UnifiedAgent,
};
use super::super::prompts::languages::PYTHON;
use super::base::ExecutionAgent;
use crate::security::SecurityProfile;

pub struct PythonExecutor {
    base: ExecutionAgent,
}

impl PythonExecutor {
    pub fn new() -> Self {
        let mut base = ExecutionAgent::new(
            "python-executor",
            "Python Executor",
            "Executes Python code with sandboxing. Supports pytest, ruff, mypy, and uv.",
            "python",
            vec![
                "python", "python3", "pip", "pip3", "uv", "ruff", "pytest", "mypy", "black",
                "isort",
            ],
        );
        base.knowledge = PYTHON.to_string();
        base.operations = vec![
            "run".to_string(),
            "test".to_string(),
            "lint".to_string(),
            "format".to_string(),
            "typecheck".to_string(),
            "install".to_string(),
        ];
        Self { base }
    }

    async fn run_python(&self, code: &str, args: &[&str]) -> AgentResponse {
        // Write code to temp file
        let temp_file = "/tmp/python_exec.py";
        if let Err(e) = tokio::fs::write(temp_file, code).await {
            return AgentResponse::failure(format!("Failed to write temp file: {}", e));
        }

        let mut cmd_args = vec![temp_file];
        cmd_args.extend(args);

        match self
            .base
            .execute_command("python3", &cmd_args, None, 60)
            .await
        {
            Ok((stdout, stderr, code)) => {
                if code == 0 {
                    AgentResponse::success(
                        json!({ "stdout": stdout, "stderr": stderr, "exit_code": code }),
                        "Python code executed successfully",
                    )
                } else {
                    AgentResponse::failure(format!("Python exited with code {}: {}", code, stderr))
                }
            }
            Err(e) => AgentResponse::failure(e),
        }
    }

    async fn run_pytest(&self, path: &str, args: &[&str]) -> AgentResponse {
        let mut cmd_args = vec!["-m", "pytest", path, "-v"];
        cmd_args.extend(args);

        match self
            .base
            .execute_command("python3", &cmd_args, None, 300)
            .await
        {
            Ok((stdout, stderr, code)) => AgentResponse::success(
                json!({
                    "stdout": stdout,
                    "stderr": stderr,
                    "exit_code": code,
                    "passed": code == 0
                }),
                if code == 0 {
                    "All tests passed"
                } else {
                    "Some tests failed"
                },
            ),
            Err(e) => AgentResponse::failure(e),
        }
    }

    async fn run_ruff(&self, path: &str, fix: bool) -> AgentResponse {
        let mut args = vec!["check", path];
        if fix {
            args.push("--fix");
        }

        match self.base.execute_command("ruff", &args, None, 60).await {
            Ok((stdout, stderr, code)) => AgentResponse::success(
                json!({
                    "output": stdout,
                    "errors": stderr,
                    "exit_code": code,
                    "clean": code == 0
                }),
                if code == 0 {
                    "No linting issues"
                } else {
                    "Linting issues found"
                },
            ),
            Err(e) => AgentResponse::failure(e),
        }
    }

    async fn run_mypy(&self, path: &str) -> AgentResponse {
        match self
            .base
            .execute_command("mypy", &[path, "--strict"], None, 120)
            .await
        {
            Ok((stdout, stderr, code)) => AgentResponse::success(
                json!({
                    "output": stdout,
                    "errors": stderr,
                    "exit_code": code,
                    "type_safe": code == 0
                }),
                if code == 0 {
                    "No type errors"
                } else {
                    "Type errors found"
                },
            ),
            Err(e) => AgentResponse::failure(e),
        }
    }
}

impl Default for PythonExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UnifiedAgent for PythonExecutor {
    fn id(&self) -> &str {
        self.base.id()
    }

    fn name(&self) -> &str {
        self.base.name()
    }

    fn description(&self) -> &str {
        self.base.description()
    }

    fn category(&self) -> AgentCategory {
        AgentCategory::Execution
    }

    fn capabilities(&self) -> Vec<AgentCapability> {
        self.base.capabilities()
    }

    fn system_prompt(&self) -> &str {
        self.base.system_prompt()
    }

    fn knowledge_base(&self) -> Option<&str> {
        self.base.knowledge_base()
    }

    fn security_profile(&self) -> Option<&SecurityProfile> {
        self.base.security_profile()
    }

    fn operations(&self) -> Vec<&str> {
        self.base.operations()
    }

    async fn execute(&self, request: AgentRequest) -> AgentResponse {
        match request.operation.as_str() {
            "run" => {
                let code = request
                    .args
                    .get("code")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let args: Vec<&str> = request
                    .args
                    .get("args")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                    .unwrap_or_default();
                self.run_python(code, &args).await
            }
            "test" => {
                let path = request
                    .args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or(".");
                let args: Vec<&str> = request
                    .args
                    .get("args")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                    .unwrap_or_default();
                self.run_pytest(path, &args).await
            }
            "lint" => {
                let path = request
                    .args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or(".");
                let fix = request
                    .args
                    .get("fix")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                self.run_ruff(path, fix).await
            }
            "typecheck" => {
                let path = request
                    .args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or(".");
                self.run_mypy(path).await
            }
            "format" => {
                let path = request
                    .args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or(".");
                // Run both black and isort
                match self
                    .base
                    .execute_command("ruff", &["format", path], None, 60)
                    .await
                {
                    Ok((stdout, _, code)) => AgentResponse::success(
                        json!({ "output": stdout, "exit_code": code }),
                        "Code formatted",
                    ),
                    Err(e) => AgentResponse::failure(e),
                }
            }
            "install" => {
                let packages: Vec<&str> = request
                    .args
                    .get("packages")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                    .unwrap_or_default();

                if packages.is_empty() {
                    return AgentResponse::failure("No packages specified");
                }

                let mut args = vec!["pip", "install"];
                args.extend(packages.iter());

                match self.base.execute_command("uv", &args, None, 300).await {
                    Ok((stdout, stderr, code)) => AgentResponse::success(
                        json!({ "output": stdout, "errors": stderr, "exit_code": code }),
                        if code == 0 {
                            "Packages installed"
                        } else {
                            "Installation failed"
                        },
                    ),
                    Err(e) => AgentResponse::failure(e),
                }
            }
            _ => AgentResponse::failure(format!("Unknown operation: {}", request.operation)),
        }
    }
}
