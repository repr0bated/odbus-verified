//! C Pro Agent - C development environment

use async_trait::async_trait;
use std::process::Command;

use crate::agents::base::{validation, AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;

const ALLOWED_DIRS: &[&str] = &["/tmp", "/home", "/opt"];

pub struct CProAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl CProAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            agent_id,
            profile: SecurityProfile::code_execution(
                "c-pro",
                vec!["gcc", "clang", "make", "cmake", "gdb", "valgrind"],
            ),
        }
    }

    fn gcc_compile(&self, path: Option<&str>, args: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("gcc");
        cmd.arg("-Wall").arg("-Wextra");

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

    fn make_build(&self, path: Option<&str>, target: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("make");

        if let Some(t) = target {
            validation::validate_args(t)?;
            cmd.arg(t);
        }

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

    fn cmake_configure(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("cmake");
        cmd.arg("-B").arg("build");

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.current_dir(validated_path);
        }

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            Ok(format!(
                "CMake configure succeeded\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        } else {
            Ok(format!(
                "CMake configure failed\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        }
    }
}

#[async_trait]
impl AgentTrait for CProAgent {
    fn agent_type(&self) -> &str {
        "c-pro"
    }
    fn name(&self) -> &str {
        "C Pro Agent"
    }
    fn description(&self) -> &str {
        "C development environment with GCC, Make, and CMake"
    }

    fn operations(&self) -> Vec<String> {
        vec![
            "compile".to_string(),
            "make".to_string(),
            "cmake".to_string(),
        ]
    }

    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "c-pro" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }

        let result = match task.operation.as_str() {
            "compile" => self.gcc_compile(task.path.as_deref(), task.args.as_deref()),
            "make" => self.make_build(task.path.as_deref(), task.args.as_deref()),
            "cmake" => self.cmake_configure(task.path.as_deref()),
            _ => Err(format!("Unknown operation: {}", task.operation)),
        };

        match result {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
