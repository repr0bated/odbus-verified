//! Go Executor Agent

use async_trait::async_trait;
use simd_json::json;
use simd_json::prelude::*;

use super::super::agent_trait::{
    AgentCapability, AgentCategory, AgentRequest, AgentResponse, UnifiedAgent,
};
use super::super::prompts::languages::GO;
use super::base::ExecutionAgent;
use crate::security::SecurityProfile;

pub struct GoExecutor {
    base: ExecutionAgent,
}

impl GoExecutor {
    pub fn new() -> Self {
        let mut base = ExecutionAgent::new(
            "go-executor",
            "Go Executor",
            "Executes Go code. Supports build, test, vet, and fmt.",
            "go",
            vec!["go", "gofmt", "staticcheck"],
        );
        base.knowledge = GO.to_string();
        base.operations = vec![
            "build".to_string(),
            "test".to_string(),
            "run".to_string(),
            "fmt".to_string(),
            "vet".to_string(),
        ];
        Self { base }
    }
}

impl Default for GoExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UnifiedAgent for GoExecutor {
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

        let (args, timeout): (Vec<&str>, u64) = match request.operation.as_str() {
            "build" => (vec!["build", "./..."], 300),
            "test" => (vec!["test", "-v", "./..."], 300),
            "run" => {
                let file = request
                    .args
                    .get("file")
                    .and_then(|v| v.as_str())
                    .unwrap_or("main.go");
                (vec!["run", file], 120)
            }
            "fmt" => (vec!["fmt", "./..."], 60),
            "vet" => (vec!["vet", "./..."], 120),
            _ => {
                return AgentResponse::failure(format!("Unknown operation: {}", request.operation))
            }
        };

        match self
            .base
            .execute_command("go", &args, Some(path), timeout)
            .await
        {
            Ok((stdout, stderr, code)) => AgentResponse::success(
                json!({
                    "stdout": stdout,
                    "stderr": stderr,
                    "exit_code": code,
                    "success": code == 0
                }),
                if code == 0 {
                    format!("{} completed successfully", request.operation)
                } else {
                    format!("{} failed", request.operation)
                },
            ),
            Err(e) => AgentResponse::failure(e),
        }
    }
}
