use anyhow::{Context, Result};
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::PluginSchema;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Agent configuration plugin state schema.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.agent-config.schema@v1"))]
#[schemars(extend("x-oscal-category" = "agents"))]
pub struct AgentConfigState {
    /// List of agent configurations.
    #[serde(default)]
    #[schemars(
        description = "List of agent configurations",
        extend("x-oscal-subid" = "exp.software.plugin.agent-config.agents.render@v1")
    )]
    pub agents: Vec<AgentConfig>,
}

/// Individual agent configuration.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AgentConfig {
    /// Agent name.
    #[schemars(
        description = "Agent name",
        extend("x-oscal-subid" = "exp.software.agent-config.agent.name.render@v1")
    )]
    pub name: String,
    /// Whether the agent is enabled.
    #[schemars(
        description = "Whether the agent is enabled",
        extend("x-oscal-subid" = "exp.software.agent-config.agent.enabled.render@v1")
    )]
    pub enabled: bool,
    /// Default model override.
    #[schemars(
        description = "Default model override",
        extend("x-oscal-subid" = "exp.software.agent-config.agent.model.render@v1")
    )]
    pub model: Option<String>,
    /// Enabled tool names.
    #[serde(default)]
    #[schemars(
        description = "Enabled tool names",
        extend("x-oscal-subid" = "exp.software.agent-config.agent.tools.render@v1")
    )]
    pub tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AgentNameInput {
    pub name: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct UpdateConfigInput {
    pub config: AgentConfig,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RegisterToolInput {
    pub name: String,
    pub tool: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AgentConfigOutput {
    pub config: Option<AgentConfig>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AgentToolsOutput {
    pub tools: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AgentMutationOutput {
    pub config: AgentConfig,
    pub revision: u64,
}

pub struct AgentConfigPlugin;

impl Default for AgentConfigPlugin {
    fn default() -> Self {
        Self
    }
}

impl AgentConfigPlugin {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl StatePlugin for AgentConfigPlugin {
    fn name(&self) -> &str {
        "agent_config"
    }

    fn version(&self) -> &str {
        "2.0.0"
    }

    fn schema(&self) -> Option<op_state_store::PluginSchema> {
        Some(agent_config_schema())
    }

    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        Ok(StateDiff {
            plugin: self.name().to_string(),
            actions: vec![],
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: "unknown".to_string(),
                desired_hash: "unknown".to_string(),
            },
        })
    }

    async fn apply_state(&self, _diff: &StateDiff) -> Result<ApplyResult> {
        Ok(ApplyResult {
            success: true,
            changes_applied: vec![],
            errors: vec![],
            checkpoint: None,
        })
    }

    async fn verify_state(&self, _desired: &Value) -> Result<bool> {
        Ok(true)
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        Ok(Checkpoint {
            id: uuid::Uuid::new_v4().to_string(),
            plugin: self.name().to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: simd_json::json!(null),
            backend_checkpoint: None,
        })
    }

    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        Ok(())
    }

    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: true,
            supports_checkpoints: true,
            supports_verification: true,
            atomic_operations: false,
        }
    }
}

/// Canonical `agent_config` schema, derived from the structs via schemars.
pub fn agent_config_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(AgentConfigState))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "agent_config",
        "2.0.0",
        "Agent configuration and tool assignments",
        &root,
    );

    // Add methods
    use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;
    use op_state_store::SideEffect;

    schema.methods.insert(
        "get_config".to_string(),
        method_decl_from_schemars_with_output::<AgentNameInput, AgentConfigOutput>(
            "get_config",
            SideEffect::Read,
            true,
            "agent.read",
            "obs.service.agent.config.get@v1",
        ),
    );
    schema.methods.insert(
        "update_config".to_string(),
        method_decl_from_schemars_with_output::<UpdateConfigInput, AgentMutationOutput>(
            "update_config",
            SideEffect::Mutation,
            false,
            "agent.invoke",
            "mut.service.agent.config.update@v1",
        ),
    );
    schema.methods.insert(
        "list_tools".to_string(),
        method_decl_from_schemars_with_output::<AgentNameInput, AgentToolsOutput>(
            "list_tools",
            SideEffect::Read,
            true,
            "agent.read",
            "obs.service.agent.tool.list@v1",
        ),
    );
    schema.methods.insert(
        "register_tool".to_string(),
        method_decl_from_schemars_with_output::<RegisterToolInput, AgentMutationOutput>(
            "register_tool",
            SideEffect::Mutation,
            false,
            "agent.invoke",
            "mut.service.agent.tool.register@v1",
        ),
    );
    schema.methods.insert(
        "reset_config".to_string(),
        method_decl_from_schemars_with_output::<AgentNameInput, AgentMutationOutput>(
            "reset_config",
            SideEffect::Mutation,
            true,
            "agent.invoke",
            "mut.service.agent.config.reset@v1",
        ),
    );

    schema.capabilities.insert(
        "agent.read".to_string(),
        op_state_store::CapabilityDecl {
            id: "agent.read".to_string(),
            description: "Grants: get_config, list_tools.".to_string(),
        },
    );
    schema.capabilities.insert(
        "agent.invoke".to_string(),
        op_state_store::CapabilityDecl {
            id: "agent.invoke".to_string(),
            description: "Grants: update_config, register_tool, reset_config.".to_string(),
        },
    );

    schema
}

fn valid_name(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().any(|c| c.is_control()) {
        anyhow::bail!("agent name/tool must be non-empty and contain no control characters")
    }
    Ok(value.to_string())
}

fn decode_agents(document: Option<serde_json::Value>) -> Result<Vec<AgentConfig>> {
    let document = document.unwrap_or_else(|| serde_json::json!({"agents": []}));
    serde_json::from_value(
        document
            .get("agents")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("agent_config document is missing agents"))?,
    )
    .map_err(Into::into)
}

pub async fn dispatch_agent_config_method(
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    dispatch_agent_config_with_store(
        super::runtime_documents::runtime_documents().await?,
        method,
        args,
    )
    .await
}

pub async fn dispatch_agent_config_with_store(
    store: Arc<super::runtime_documents::RuntimeDocuments>,
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    let current = store
        .read("agent_config")
        .await?
        .map(|d| d.document)
        .unwrap_or_else(|| serde_json::json!({"agents": []}));
    let agents = decode_agents(Some(current))?;
    match method {
        "get_config" => {
            let i: AgentNameInput = serde_json::from_value(args.clone())?;
            let name = valid_name(&i.name)?;
            Ok(serde_json::json!({"config": agents.iter().find(|a| a.name==name)}))
        }
        "list_tools" => {
            let i: AgentNameInput = serde_json::from_value(args.clone())?;
            let name = valid_name(&i.name)?;
            let config = agents
                .iter()
                .find(|a| a.name == name)
                .ok_or_else(|| anyhow::anyhow!("unknown agent '{name}'"))?;
            Ok(serde_json::json!({"tools":config.tools}))
        }
        "update_config" => {
            let i: UpdateConfigInput = serde_json::from_value(args.clone())?;
            let mut config = i.config;
            config.name = valid_name(&config.name)?;
            let lookup_name = config.name.clone();
            config.tools = config
                .tools
                .into_iter()
                .map(|t| valid_name(&t))
                .collect::<Result<Vec<_>>>()?;
            config.tools.sort();
            config.tools.dedup();
            let row = store
                .update("agent_config", move |old| {
                    let mut list = decode_agents(old)?;
                    if let Some(a) = list.iter_mut().find(|a| a.name == config.name) {
                        *a = config.clone()
                    } else {
                        list.push(config.clone())
                    };
                    Ok(serde_json::json!({"agents":list}))
                })
                .await?;
            let committed = decode_agents(Some(row.document))?
                .into_iter()
                .find(|a| a.name == lookup_name)
                .context("committed agent config missing")?;
            Ok(serde_json::to_value(AgentMutationOutput {
                config: committed,
                revision: row.revision,
            })?)
        }
        "register_tool" => {
            let i: RegisterToolInput = serde_json::from_value(args.clone())?;
            let name = valid_name(&i.name)?;
            let tool = valid_name(&i.tool)?;
            let lookup_name = name.clone();
            let row = store
                .update("agent_config", move |old| {
                    let mut list = decode_agents(old)?;
                    let a = list
                        .iter_mut()
                        .find(|a| a.name == name)
                        .ok_or_else(|| anyhow::anyhow!("unknown agent '{name}'"))?;
                    if !a.tools.contains(&tool) {
                        a.tools.push(tool.clone());
                        a.tools.sort();
                    }
                    Ok(serde_json::json!({"agents":list}))
                })
                .await?;
            let config = decode_agents(Some(row.document))?
                .into_iter()
                .find(|a| a.name == lookup_name)
                .context("committed agent config missing")?;
            Ok(serde_json::to_value(AgentMutationOutput {
                config,
                revision: row.revision,
            })?)
        }
        "reset_config" => {
            let i: AgentNameInput = serde_json::from_value(args.clone())?;
            let name = valid_name(&i.name)?;
            let lookup_name = name.clone();
            let row = store
                .update("agent_config", move |old| {
                    let mut list = decode_agents(old)?;
                    let a = list
                        .iter_mut()
                        .find(|a| a.name == name)
                        .ok_or_else(|| anyhow::anyhow!("unknown agent '{name}'"))?;
                    a.enabled = false;
                    a.model = None;
                    a.tools.clear();
                    Ok(serde_json::json!({"agents":list}))
                })
                .await?;
            let config = decode_agents(Some(row.document))?
                .into_iter()
                .find(|a| a.name == lookup_name)
                .context("committed agent config missing")?;
            Ok(serde_json::to_value(AgentMutationOutput {
                config,
                revision: row.revision,
            })?)
        }
        other => anyhow::bail!("unknown agent_config method: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_plugins::common::oscal::validate_subid;
    use crate::state_plugins::runtime_documents::RuntimeDocuments;

    #[test]
    fn schema_is_schemars_seeded_and_typed() {
        let schema = agent_config_schema();
        assert_eq!(schema.name, "agent_config");
        assert_eq!(schema.version, "2.0.0");
        assert_eq!(
            schema.description,
            "Agent configuration and tool assignments"
        );
        assert!(schema.fields.contains_key("agents"));
        assert!(schema.methods.contains_key("get_config"));
        assert!(schema.methods.contains_key("update_config"));
        assert!(schema.methods.contains_key("list_tools"));
        assert!(schema.methods.contains_key("register_tool"));
        assert!(schema.methods.contains_key("reset_config"));
    }

    #[test]
    fn all_subids_are_valid() {
        let raw = serde_json::to_value(schemars::schema_for!(AgentConfigState)).unwrap();
        let mut stack = vec![&raw];
        while let Some(node) = stack.pop() {
            if let Some(subid) = node.get("x-oscal-subid").and_then(|v| v.as_str()) {
                validate_subid(subid).expect("invalid subid");
            }
            if let Some(props) = node.get("properties").and_then(|v| v.as_object()) {
                for (_, v) in props {
                    stack.push(v);
                }
            }
            if let Some(defs) = node
                .get("$defs")
                .or_else(|| node.get("definitions"))
                .and_then(|v| v.as_object())
            {
                for (_, v) in defs {
                    stack.push(v);
                }
            }
        }
    }

    #[tokio::test]
    async fn runtime_agent_methods_round_trip_and_reject_unknown() {
        let store = Arc::new(RuntimeDocuments::new(
            op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap(),
        ));
        let cfg = serde_json::json!({"config":{"name":"builder","enabled":true,"model":"m1","tools":["search","search"]}});
        dispatch_agent_config_with_store(store.clone(), "update_config", &cfg)
            .await
            .unwrap();
        assert_eq!(
            dispatch_agent_config_with_store(
                store.clone(),
                "list_tools",
                &serde_json::json!({"name":"builder"})
            )
            .await
            .unwrap()["tools"],
            serde_json::json!(["search"])
        );
        dispatch_agent_config_with_store(
            store.clone(),
            "register_tool",
            &serde_json::json!({"name":"builder","tool":"write"}),
        )
        .await
        .unwrap();
        dispatch_agent_config_with_store(
            store.clone(),
            "reset_config",
            &serde_json::json!({"name":"builder"}),
        )
        .await
        .unwrap();
        assert_eq!(
            dispatch_agent_config_with_store(
                store.clone(),
                "get_config",
                &serde_json::json!({"name":"builder"})
            )
            .await
            .unwrap()["config"]["enabled"],
            false
        );
        assert!(dispatch_agent_config_with_store(
            store,
            "list_tools",
            &serde_json::json!({"name":"missing"})
        )
        .await
        .is_err());
    }
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new("agent_config", |_ctx| std::sync::Arc::new(AgentConfigPlugin::new()))
}
