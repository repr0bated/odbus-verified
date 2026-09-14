//! SQL Pro Agent

use async_trait::async_trait;
use std::process::Command;

use crate::agents::base::{validation, AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;

const ALLOWED_DIRS: &[&str] = &["/tmp", "/home", "/opt"];

pub struct SqlProAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl SqlProAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            agent_id,
            profile: SecurityProfile::code_execution(
                "sql-pro",
                vec!["psql", "mysql", "sqlite3", "sqlfluff"],
            ),
        }
    }

    fn sqlite_query(&self, db_path: Option<&str>, query: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("sqlite3");
        cmd.arg("-header").arg("-column");

        if let Some(db) = db_path {
            let validated_path = validation::validate_path(db, ALLOWED_DIRS)?;
            cmd.arg(validated_path);
        } else {
            return Err("Database path required".to_string());
        }

        if let Some(q) = query {
            // Only allow SELECT queries for safety
            let q_upper = q.to_uppercase();
            if !q_upper.trim().starts_with("SELECT")
                && !q_upper.trim().starts_with(".SCHEMA")
                && !q_upper.trim().starts_with(".TABLES")
            {
                return Err("Only SELECT queries allowed".to_string());
            }
            cmd.arg(q);
        } else {
            cmd.arg(".tables");
        }

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        Ok(format!("Query result:\n{}\n{}", stdout, stderr))
    }

    fn sqlfluff_lint(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("sqlfluff");
        cmd.arg("lint");

        if let Some(file) = path {
            let validated_path = validation::validate_path(file, ALLOWED_DIRS)?;
            cmd.arg(validated_path);
        } else {
            return Err("SQL file path required".to_string());
        }

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        Ok(format!("SQLFluff lint:\n{}\n{}", stdout, stderr))
    }

    fn sqlfluff_format(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("sqlfluff");
        cmd.arg("fix").arg("--diff");

        if let Some(file) = path {
            let validated_path = validation::validate_path(file, ALLOWED_DIRS)?;
            cmd.arg(validated_path);
        } else {
            return Err("SQL file path required".to_string());
        }

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        Ok(format!("SQLFluff format:\n{}\n{}", stdout, stderr))
    }
}

#[async_trait]
impl AgentTrait for SqlProAgent {
    fn agent_type(&self) -> &str {
        "sql-pro"
    }
    fn name(&self) -> &str {
        "SQL Pro"
    }
    fn description(&self) -> &str {
        "SQL development and query execution"
    }

    fn operations(&self) -> Vec<String> {
        vec![
            "query".to_string(),
            "lint".to_string(),
            "format".to_string(),
        ]
    }

    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        let result = match task.operation.as_str() {
            "query" => self.sqlite_query(task.path.as_deref(), task.args.as_deref()),
            "lint" => self.sqlfluff_lint(task.path.as_deref()),
            "format" => self.sqlfluff_format(task.path.as_deref()),
            _ => Err(format!("Unknown operation: {}", task.operation)),
        };

        match result {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
