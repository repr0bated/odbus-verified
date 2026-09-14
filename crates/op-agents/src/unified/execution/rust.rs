//! Rust Executor Agent

use async_trait::async_trait;
use simd_json::json;
use simd_json::prelude::*;
use std::path::PathBuf;

use super::super::agent_trait::{
    AgentCapability, AgentCategory, AgentRequest, AgentResponse, UnifiedAgent,
};
use super::super::prompts::languages::RUST;
use super::base::ExecutionAgent;
use crate::security::SecurityProfile;

pub struct RustExecutor {
    base: ExecutionAgent,
}

impl RustExecutor {
    pub fn new() -> Self {
        let mut base = ExecutionAgent::new(
            "rust-executor",
            "Rust Executor",
            "Executes Rust code via cargo. Supports build, test, clippy, and format.",
            "rust",
            vec!["cargo", "rustc", "rustfmt", "clippy-driver"],
        );
        base.knowledge = RUST.to_string();
        base.operations = vec![
            "check".to_string(),
            "build".to_string(),
            "test".to_string(),
            "clippy".to_string(),
            "format".to_string(),
            "run".to_string(),
        ];
        // OP-DBUS source checkouts and the session-close Rust Pro trigger live
        // under /srv/git. Keep this scoped to the Rust executor rather than
        // widening every code-execution agent profile.
        base.security_profile
            .config
            .allowed_read_paths
            .push(PathBuf::from("/srv/git"));
        base.security_profile
            .config
            .allowed_write_paths
            .push(PathBuf::from("/srv/git"));
        Self { base }
    }
}

impl Default for RustExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UnifiedAgent for RustExecutor {
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

        let (cmd, args, timeout) = match request.operation.as_str() {
            "check" => ("cargo", vec!["check"], 120),
            "build" => {
                let release = request
                    .args
                    .get("release")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let mut args = vec!["build"];
                if release {
                    args.push("--release");
                }
                ("cargo", args, 600)
            }
            "test" => ("cargo", vec!["test"], 600),
            "clippy" => ("cargo", vec!["clippy", "--", "-D", "warnings"], 120),
            "format" => ("cargo", vec!["fmt"], 60),
            "run" => {
                let release = request
                    .args
                    .get("release")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let mut args = vec!["run"];
                if release {
                    args.push("--release");
                }
                ("cargo", args, 300)
            }
            _ => {
                return AgentResponse::failure(format!("Unknown operation: {}", request.operation))
            }
        };

        let args_str: Vec<&str> = args.to_vec();
        match self
            .base
            .execute_command(cmd, &args_str, Some(path), timeout)
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
                    format!("{} failed with code {}", request.operation, code)
                },
            ),
            Err(e) => AgentResponse::failure(e),
        }
    }
}
