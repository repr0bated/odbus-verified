//! JavaScript/TypeScript Executor Agent

use async_trait::async_trait;
use simd_json::json;
use simd_json::prelude::*;

use super::super::agent_trait::{
    AgentCapability, AgentCategory, AgentRequest, AgentResponse, UnifiedAgent,
};
use super::super::prompts::languages::JAVASCRIPT;
use super::base::ExecutionAgent;
use crate::security::SecurityProfile;

pub struct JavaScriptExecutor {
    base: ExecutionAgent,
}

impl JavaScriptExecutor {
    pub fn new() -> Self {
        let mut base = ExecutionAgent::new(
            "javascript-executor",
            "JavaScript/TypeScript Executor",
            "Executes JavaScript/TypeScript via Node.js. Supports npm, pnpm, jest, and eslint.",
            "javascript",
            vec![
                "node", "npm", "npx", "pnpm", "eslint", "prettier", "jest", "vitest", "tsc",
            ],
        );
        base.knowledge = JAVASCRIPT.to_string();
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
}

impl Default for JavaScriptExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UnifiedAgent for JavaScriptExecutor {
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
        let path = request
            .args
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or(".");

        match request.operation.as_str() {
            "run" => {
                let script = request
                    .args
                    .get("script")
                    .and_then(|v| v.as_str())
                    .unwrap_or("start");
                match self
                    .base
                    .execute_command("npm", &["run", script], Some(path), 300)
                    .await
                {
                    Ok((stdout, stderr, code)) => AgentResponse::success(
                        json!({ "stdout": stdout, "stderr": stderr, "exit_code": code }),
                        if code == 0 {
                            "Script completed"
                        } else {
                            "Script failed"
                        },
                    ),
                    Err(e) => AgentResponse::failure(e),
                }
            }
            "test" => {
                // Try vitest first, fall back to jest
                match self
                    .base
                    .execute_command("npx", &["vitest", "run"], Some(path), 300)
                    .await
                {
                    Ok((stdout, stderr, code)) => AgentResponse::success(
                        json!({ "stdout": stdout, "stderr": stderr, "exit_code": code }),
                        if code == 0 {
                            "Tests passed"
                        } else {
                            "Tests failed"
                        },
                    ),
                    Err(_) => {
                        // Try jest
                        match self
                            .base
                            .execute_command("npx", &["jest"], Some(path), 300)
                            .await
                        {
                            Ok((stdout, stderr, code)) => AgentResponse::success(
                                json!({ "stdout": stdout, "stderr": stderr, "exit_code": code }),
                                if code == 0 {
                                    "Tests passed"
                                } else {
                                    "Tests failed"
                                },
                            ),
                            Err(e) => AgentResponse::failure(e),
                        }
                    }
                }
            }
            "lint" => {
                match self
                    .base
                    .execute_command("npx", &["eslint", "."], Some(path), 120)
                    .await
                {
                    Ok((stdout, stderr, code)) => AgentResponse::success(
                        json!({ "output": stdout, "errors": stderr, "exit_code": code }),
                        if code == 0 {
                            "No linting issues"
                        } else {
                            "Linting issues found"
                        },
                    ),
                    Err(e) => AgentResponse::failure(e),
                }
            }
            "format" => {
                match self
                    .base
                    .execute_command("npx", &["prettier", "--write", "."], Some(path), 60)
                    .await
                {
                    Ok((stdout, _, code)) => AgentResponse::success(
                        json!({ "output": stdout, "exit_code": code }),
                        "Code formatted",
                    ),
                    Err(e) => AgentResponse::failure(e),
                }
            }
            "typecheck" => {
                match self
                    .base
                    .execute_command("npx", &["tsc", "--noEmit"], Some(path), 120)
                    .await
                {
                    Ok((stdout, stderr, code)) => AgentResponse::success(
                        json!({ "output": stdout, "errors": stderr, "exit_code": code }),
                        if code == 0 {
                            "No type errors"
                        } else {
                            "Type errors found"
                        },
                    ),
                    Err(e) => AgentResponse::failure(e),
                }
            }
            "install" => {
                match self
                    .base
                    .execute_command("npm", &["install"], Some(path), 300)
                    .await
                {
                    Ok((stdout, stderr, code)) => AgentResponse::success(
                        json!({ "output": stdout, "errors": stderr, "exit_code": code }),
                        if code == 0 {
                            "Dependencies installed"
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
