//! Adapter: `UnifiedAgent` → legacy `AgentTrait` for MCP/HTTP/D-Bus surfaces.

use std::sync::Arc;

use async_trait::async_trait;
use simd_json::json;

use crate::agents::base::{AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;

use super::agent_trait::{AgentRequest, FileContext, UnifiedAgent};

/// Bridges unified agents into the existing AgentTrait call sites.
pub struct UnifiedAgentAdapter {
    agent: Arc<dyn UnifiedAgent>,
    /// Public catalog type requested by the caller (may differ from `agent.id()`).
    catalog_type: String,
    instance_id: String,
    profile: SecurityProfile,
}

impl UnifiedAgentAdapter {
    pub fn new(agent: Arc<dyn UnifiedAgent>, catalog_type: String, instance_id: String) -> Self {
        let profile = agent
            .security_profile()
            .cloned()
            .unwrap_or_else(|| SecurityProfile::read_only_analysis(agent.id(), vec![]));
        Self {
            agent,
            catalog_type,
            instance_id,
            profile,
        }
    }
}

#[async_trait]
impl AgentTrait for UnifiedAgentAdapter {
    fn agent_type(&self) -> &str {
        &self.catalog_type
    }

    fn name(&self) -> &str {
        self.agent.name()
    }

    fn description(&self) -> &str {
        self.agent.description()
    }

    fn operations(&self) -> Vec<String> {
        self.agent
            .operations()
            .into_iter()
            .map(str::to_string)
            .collect()
    }

    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        let mut request_args = match task.args.as_ref() {
            Some(raw) => {
                let mut bytes = raw.clone().into_bytes();
                match simd_json::to_owned_value(&mut bytes) {
                    Ok(v) => v,
                    Err(_) => json!({ "query": raw }),
                }
            }
            None => json!({}),
        };

        if let simd_json::OwnedValue::Object(ref mut obj) = request_args {
            if let Some(path) = task.path.as_ref() {
                obj.insert("path".into(), json!(path));
            }
            for (k, v) in &task.config {
                obj.insert(k.clone(), v.clone());
            }
        }

        let request = AgentRequest {
            operation: task.operation.clone(),
            args: request_args,
            context: Some(format!("instance={}", self.instance_id)),
            files: Vec::<FileContext>::new(),
        };

        let response = self.agent.execute(request).await;
        let data = simd_json::to_string(&response.data).unwrap_or_else(|_| "{}".to_string());
        if response.success {
            Ok(TaskResult::success(&task.operation, data)
                .with_metadata("message", json!(response.message))
                .with_metadata("agent", json!(self.catalog_type))
                .with_metadata("instance_id", json!(self.instance_id)))
        } else {
            Err(response.message)
        }
    }
}
