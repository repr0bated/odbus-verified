//! Context Manager Agent

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::agents::base::{AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;

pub struct ContextManagerAgent {
    agent_id: String,
    profile: SecurityProfile,
    context: Arc<RwLock<HashMap<String, String>>>,
}

impl ContextManagerAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            agent_id,
            profile: SecurityProfile::orchestration("context-manager", vec!["*"]),
            context: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn save_context(&self, key: Option<&str>, value: Option<&str>) -> Result<String, String> {
        let key = key.ok_or("Key required")?;
        let value = value.ok_or("Value required")?;

        let mut ctx = self.context.write().map_err(|_| "Failed to acquire lock")?;
        ctx.insert(key.to_string(), value.to_string());

        Ok(format!("Context saved: {} = {}", key, value))
    }

    fn restore_context(&self, key: Option<&str>) -> Result<String, String> {
        let key = key.ok_or("Key required")?;

        let ctx = self.context.read().map_err(|_| "Failed to acquire lock")?;

        if let Some(value) = ctx.get(key) {
            Ok(format!("Context restored: {} = {}", key, value))
        } else {
            Err(format!("Context key not found: {}", key))
        }
    }

    fn list_context(&self) -> Result<String, String> {
        let ctx = self.context.read().map_err(|_| "Failed to acquire lock")?;

        if ctx.is_empty() {
            Ok("No context stored".to_string())
        } else {
            let entries: Vec<String> = ctx
                .iter()
                .map(|(k, v)| format!("  {} = {}", k, v))
                .collect();
            Ok(format!("Stored context:\n{}", entries.join("\n")))
        }
    }

    fn clear_context(&self) -> Result<String, String> {
        let mut ctx = self.context.write().map_err(|_| "Failed to acquire lock")?;
        ctx.clear();

        Ok("Context cleared".to_string())
    }
}

#[async_trait]
impl AgentTrait for ContextManagerAgent {
    fn agent_type(&self) -> &str {
        "context-manager"
    }
    fn name(&self) -> &str {
        "Context Manager"
    }
    fn description(&self) -> &str {
        "Session context management and state persistence"
    }

    fn operations(&self) -> Vec<String> {
        vec![
            "save".to_string(),
            "restore".to_string(),
            "list".to_string(),
            "clear".to_string(),
        ]
    }

    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        let result = match task.operation.as_str() {
            "save" => self.save_context(task.path.as_deref(), task.args.as_deref()),
            "restore" => self.restore_context(task.path.as_deref()),
            "list" => self.list_context(),
            "clear" => self.clear_context(),
            _ => Err(format!("Unknown operation: {}", task.operation)),
        };

        match result {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
