//! Auto-Discovery and Creation of Plugins
//!
//! This module provides the capability to automatically discover system services
//! and create corresponding state plugins.
//!
//! **Proto Generation:** Automatically generates gRPC proto method definitions
//! from plugin schemas, following gRPC best practices (typed enums, Structs, no success fields).

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use op_agents::{create_agent, AgentTask};
use op_state::StatePlugin;
use op_state_store::PluginSchema;
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Auto-creator for systemd-based plugins
pub struct SystemdAutoCreator;

impl SystemdAutoCreator {
    /// Discover system/host service units and create plugins
    pub async fn discover_units() -> Result<Vec<(String, Value)>> {
        let mut plugins = Vec::new();

        // Discover active host services from runit supervisor if available
        let sv_dir = Path::new("/run/runit/service");
        if sv_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(sv_dir) {
                for entry in entries.flatten() {
                    let file_name = entry.file_name();
                    let name = file_name.to_string_lossy();
                    if !name.starts_with('.') {
                        plugins.push((
                            name.to_string(),
                            json!({
                                "type": "runit",
                                "name": name.to_string(),
                                "state": "active",
                                "enabled": true
                            }),
                        ));
                    }
                }
            }
        }

        if plugins.is_empty() {
            let fallback_units = ["op-grpc-bridge", "op-web"];
            for unit in fallback_units {
                plugins.push((
                    unit.to_string(),
                    json!({
                        "type": "service",
                        "name": unit,
                        "state": "active",
                        "enabled": true
                    }),
                ));
            }
        }

        Ok(plugins)
    }
}

/// Generic auto-plugin that can wrap discovered services
pub struct AutoPlugin {
    name: String,
    _category: String,
    schema: PluginSchema,
    current_state: Arc<RwLock<Value>>,
}

impl AutoPlugin {
    pub fn new(name: &str, category: &str, initial_state: Value) -> Self {
        let schema = Self::build_auto_schema(name, category);
        Self {
            name: name.to_string(),
            _category: category.to_string(),
            schema,
            current_state: Arc::new(RwLock::new(initial_state)),
        }
    }

    pub fn new_with_schema(
        name: &str,
        category: &str,
        initial_state: Value,
        schema: PluginSchema,
    ) -> Self {
        Self {
            name: name.to_string(),
            _category: category.to_string(),
            schema,
            current_state: Arc::new(RwLock::new(initial_state)),
        }
    }

    pub async fn create_from_requested_info(name: &str, requested_info: &str) -> Self {
        let research = match Self::query_elements_via_agent(requested_info).await {
            Ok(value) => value,
            Err(error) => json!({
                "query": requested_info,
                "web_results": [],
                "recommended_fields": [],
                "pending_human_review": true,
                "review_reason": format!("web research unavailable: {}", error),
            }),
        };

        let pending_human_review = research
            .get("pending_human_review")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let review_reason = research
            .get("review_reason")
            .and_then(|v| v.as_str())
            .unwrap_or("Insufficient evidence; human review required.");
        let recommended_fields = research
            .get("recommended_fields")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let web_results_count = research
            .get("web_results")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);

        let state = json!({
            "plugin_id": name,
            "requested_info": requested_info,
            "status": if pending_human_review { "draft_pending_review" } else { "draft_researched" },
            "pending_human_review": pending_human_review,
            "review_reason": review_reason,
            "recommended_fields": recommended_fields,
            "research": research,
            "web_results_count": web_results_count,
            "created_at": chrono::Utc::now().to_rfc3339(),
        });

        let schema = Self::build_auto_schema(name, "auto_discovered");
        Self::new_with_schema(name, "auto_discovered", state, schema)
    }

    async fn query_elements_via_agent(requested_info: &str) -> Result<Value> {
        let agent_id = format!("plugin-auto-create-{}", uuid::Uuid::new_v4());
        let agent = create_agent("search-specialist", agent_id)
            .map_err(|e| anyhow!("failed to create search-specialist agent: {}", e))?;

        let args_payload = json!({
            "query": format!(
                "{} required configuration fields plugin schema state model capabilities",
                requested_info
            ),
            "requested": requested_info,
        });
        let args_json = simd_json::to_string(&args_payload)
            .map_err(|e| anyhow!("failed to encode agent args: {}", e))?;
        let task = AgentTask {
            task_type: "search-specialist".to_string(),
            operation: "research_plugin_elements".to_string(),
            path: None,
            args: Some(args_json),
            config: HashMap::new(),
        };

        let result = agent
            .execute(task)
            .await
            .map_err(|e| anyhow!("search-specialist execution failed: {}", e))?;
        if !result.success {
            return Err(anyhow!(
                "search-specialist reported failure: {}",
                result.data
            ));
        }

        let parsed = serde_json::from_str(&result.data)
            .map_err(|e| anyhow!("invalid agent JSON response: {}", e))?;
        Ok(parsed)
    }

    fn build_auto_schema(name: &str, category: &str) -> PluginSchema {
        PluginSchema::builder(name)
            .version("1.0.0")
            .category(category)
            .description("Auto-created plugin draft. Requires human review before production use.")
            .string_field("plugin_id", true, "Canonical plugin identifier")
            .string_field(
                "requested_info",
                true,
                "Original user request for plugin creation",
            )
            .string_field(
                "status",
                true,
                "Lifecycle status of this auto-created draft",
            )
            .boolean_field(
                "pending_human_review",
                true,
                "If true, plugin must be reviewed before activation",
            )
            .string_field(
                "review_reason",
                true,
                "Reason this draft requires human review",
            )
            .array_field(
                "recommended_fields",
                op_state_store::FieldType::String,
                false,
                "Field suggestions gathered from web research",
            )
            .field(
                "research",
                op_state_store::FieldSchema {
                    field_type: op_state_store::FieldType::Any,
                    required: false,
                    description: "Raw research payload from search-specialist agent".to_string(),
                    default: None,
                    example: None,
                    constraints: Vec::new(),
                    read_only: true,
                    read_only_when: None,
                },
            )
            .integer_field(
                "web_results_count",
                false,
                "Count of discovered web results",
            )
            .string_field("created_at", true, "RFC3339 creation timestamp")
            .build()
    }
}

#[async_trait]
impl StatePlugin for AutoPlugin {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn schema(&self) -> Option<PluginSchema> {
        Some(self.schema.clone())
    }

    async fn calculate_diff(
        &self,
        current: &Value,
        desired: &Value,
    ) -> Result<op_state::StateDiff> {
        // Simple generic diff: if not equal, replace
        let mut actions = Vec::new();
        if current != desired {
            actions.push(op_state::StateAction::Create {
                resource: self.name.clone(),
                config: desired.clone(),
            });
        }

        Ok(op_state::StateDiff {
            plugin: self.name.clone(),
            actions,
            metadata: op_state::DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: format!("{:x}", md5::compute(simd_json::to_string(current)?)),
                desired_hash: format!("{:x}", md5::compute(simd_json::to_string(desired)?)),
            },
        })
    }

    async fn apply_state(&self, diff: &op_state::StateDiff) -> Result<op_state::ApplyResult> {
        let changes = Vec::new();
        let errors = Vec::new();

        for action in &diff.actions {
            if let op_state::StateAction::Create { config, .. } = action {
                let mut state = self.current_state.write().await;
                *state = config.clone();
            }
        }

        Ok(op_state::ApplyResult {
            success: true,
            changes_applied: changes,
            errors,
            checkpoint: None,
        })
    }

    async fn verify_state(&self, desired: &Value) -> Result<bool> {
        let current = self.current_state.read().await;
        Ok(*current == desired)
    }

    async fn create_checkpoint(&self) -> Result<op_state::Checkpoint> {
        let state = self.current_state.read().await;
        Ok(op_state::Checkpoint {
            id: uuid::Uuid::new_v4().to_string(),
            plugin: self.name.clone(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: state.clone(),
            backend_checkpoint: None,
        })
    }

    async fn rollback(&self, checkpoint: &op_state::Checkpoint) -> Result<()> {
        let mut state = self.current_state.write().await;
        *state = checkpoint.state_snapshot.clone();
        Ok(())
    }

    fn capabilities(&self) -> op_state::PluginCapabilities {
        op_state::PluginCapabilities {
            supports_rollback: true,
            supports_checkpoints: true,
            supports_verification: true,
            atomic_operations: true,
        }
    }
}
