//! C++ Pro Agent - C++ development environment

use async_trait::async_trait;
use std::process::Command;

use crate::agents::base::{validation, AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;

const ALLOWED_DIRS: &[&str] = &["/tmp", "/home", "/opt"];

pub struct CppProAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl CppProAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            agent_id,
            profile: SecurityProfile::code_execution(
                "cpp-pro",
                vec!["g++", "clang++", "make", "cmake", "gdb", "valgrind"],
            ),
        }
    }

    fn gpp_compile(&self, path: Option<&str>, args: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("g++");
        cmd.arg("-Wall").arg("-Wextra").arg("-std=c++20");

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.arg(validated_path);
        } else {
            return Err("Path required".to_string());
        }

        if let Some(a) = args {
            validation::validate_args(a)?;
            for arg in a.split_whitespace() {
                cmd.arg(arg);
            }
        }

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            Ok(format!(
                "Compilation succeeded\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        } else {
            Ok(format!(
                "Compilation failed\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        }
    }

    fn cmake_build(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("cmake");
        cmd.arg("--build").arg("build");

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.current_dir(validated_path);
        }

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            Ok(format!(
                "Build succeeded\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        } else {
            Ok(format!(
                "Build failed\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        }
    }
}

#[async_trait]
impl AgentTrait for CppProAgent {
    fn agent_type(&self) -> &str {
        "cpp-pro"
    }
    fn name(&self) -> &str {
        "C++ Pro Agent"
    }
    fn description(&self) -> &str {
        "C++ development environment with G++, Make, and CMake"
    }

    fn operations(&self) -> Vec<String> {
        vec!["compile".to_string(), "build".to_string()]
    }

    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "cpp-pro" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }

        let result = match task.operation.as_str() {
            "compile" => self.gpp_compile(task.path.as_deref(), task.args.as_deref()),
            "build" => self.cmake_build(task.path.as_deref()),
            _ => Err(format!("Unknown operation: {}", task.operation)),
        };

        match result {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
