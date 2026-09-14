//! Database Optimizer Agent

use async_trait::async_trait;
use std::process::Command;

use crate::agents::base::{validation, AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;

const ALLOWED_DIRS: &[&str] = &["/tmp", "/home", "/opt"];

pub struct DatabaseOptimizerAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl DatabaseOptimizerAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            agent_id,
            profile: SecurityProfile::read_only_analysis(
                "database-optimizer",
                vec!["sqlite3", "psql"],
            ),
        }
    }

    fn explain_query(&self, db_path: Option<&str>, query: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("sqlite3");

        if let Some(db) = db_path {
            let validated_path = validation::validate_path(db, ALLOWED_DIRS)?;
            cmd.arg(validated_path);
        } else {
            return Err("Database path required".to_string());
        }

        if let Some(q) = query {
            // Only allow SELECT queries
            let q_upper = q.to_uppercase();
            if !q_upper.trim().starts_with("SELECT") {
                return Err("Only SELECT queries allowed for EXPLAIN".to_string());
            }
            cmd.arg(format!("EXPLAIN QUERY PLAN {}", q));
        } else {
            return Err("Query required".to_string());
        }

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        Ok(format!("Query plan:\n{}\n{}", stdout, stderr))
    }

    fn list_indexes(&self, db_path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("sqlite3");

        if let Some(db) = db_path {
            let validated_path = validation::validate_path(db, ALLOWED_DIRS)?;
            cmd.arg(validated_path);
        } else {
            return Err("Database path required".to_string());
        }

        cmd.arg(".indexes");

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        Ok(format!("Indexes:\n{}\n{}", stdout, stderr))
    }

    fn analyze_stats(&self, db_path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("sqlite3");

        if let Some(db) = db_path {
            let validated_path = validation::validate_path(db, ALLOWED_DIRS)?;
            cmd.arg(validated_path);
        } else {
            return Err("Database path required".to_string());
        }

        cmd.arg("SELECT * FROM sqlite_stat1;");

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        Ok(format!("Statistics:\n{}\n{}", stdout, stderr))
    }
}

#[async_trait]
impl AgentTrait for DatabaseOptimizerAgent {
    fn agent_type(&self) -> &str {
        "database-optimizer"
    }
    fn name(&self) -> &str {
        "Database Optimizer"
    }
    fn description(&self) -> &str {
        "Database query optimization and analysis"
    }

    fn operations(&self) -> Vec<String> {
        vec![
            "explain".to_string(),
            "indexes".to_string(),
            "stats".to_string(),
        ]
    }

    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        let result = match task.operation.as_str() {
            "explain" => self.explain_query(task.path.as_deref(), task.args.as_deref()),
            "indexes" => self.list_indexes(task.path.as_deref()),
            "stats" => self.analyze_stats(task.path.as_deref()),
            _ => Err(format!("Unknown operation: {}", task.operation)),
        };

        match result {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
