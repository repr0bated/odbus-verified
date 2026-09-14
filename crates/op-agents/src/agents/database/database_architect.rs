//! Database Architect Agent

use async_trait::async_trait;
use std::process::Command;

use crate::agents::base::{validation, AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;

const ALLOWED_DIRS: &[&str] = &["/tmp", "/home", "/opt"];

pub struct DatabaseArchitectAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl DatabaseArchitectAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            agent_id,
            profile: SecurityProfile::read_only_analysis(
                "database-architect",
                vec!["psql", "mysql", "sqlite3"],
            ),
        }
    }

    fn get_schema(&self, db_path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("sqlite3");

        if let Some(db) = db_path {
            let validated_path = validation::validate_path(db, ALLOWED_DIRS)?;
            cmd.arg(validated_path);
        } else {
            return Err("Database path required".to_string());
        }

        cmd.arg(".schema");

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        Ok(format!("Schema:\n{}\n{}", stdout, stderr))
    }

    fn list_tables(&self, db_path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("sqlite3");

        if let Some(db) = db_path {
            let validated_path = validation::validate_path(db, ALLOWED_DIRS)?;
            cmd.arg(validated_path);
        } else {
            return Err("Database path required".to_string());
        }

        cmd.arg(".tables");

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        Ok(format!("Tables:\n{}\n{}", stdout, stderr))
    }

    fn describe_table(&self, db_path: Option<&str>, table: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("sqlite3");

        if let Some(db) = db_path {
            let validated_path = validation::validate_path(db, ALLOWED_DIRS)?;
            cmd.arg(validated_path);
        } else {
            return Err("Database path required".to_string());
        }

        if let Some(t) = table {
            validation::validate_args(t)?;
            cmd.arg(format!("PRAGMA table_info({});", t));
        } else {
            return Err("Table name required".to_string());
        }

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        Ok(format!("Table info:\n{}\n{}", stdout, stderr))
    }
}

#[async_trait]
impl AgentTrait for DatabaseArchitectAgent {
    fn agent_type(&self) -> &str {
        "database-architect"
    }
    fn name(&self) -> &str {
        "Database Architect"
    }
    fn description(&self) -> &str {
        "Database schema analysis and design"
    }

    fn operations(&self) -> Vec<String> {
        vec![
            "schema".to_string(),
            "tables".to_string(),
            "describe".to_string(),
        ]
    }

    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        let result = match task.operation.as_str() {
            "schema" => self.get_schema(task.path.as_deref()),
            "tables" => self.list_tables(task.path.as_deref()),
            "describe" => self.describe_table(task.path.as_deref(), task.args.as_deref()),
            _ => Err(format!("Unknown operation: {}", task.operation)),
        };

        match result {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
