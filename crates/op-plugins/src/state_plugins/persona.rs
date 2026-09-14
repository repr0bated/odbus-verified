//! GB.Persona plugin — schemars-seeded, D-Bus-backed agent persona catalog.
//!
//! Projects the live agent-persona catalog from `op-agents` through the
//! `org.opdbus.v1.plugins` surface. The catalog is live, not mocked.

use super::plugin_scaffold_helpers::{method_decl_from_schemars_with_output, AckOutput};
use anyhow::Result;
use async_trait::async_trait;
use op_agents::builtin_agent_descriptors;
use op_state::{
    ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateAction, StateDiff, StatePlugin,
};
use op_state_store::{PluginSchema, SideEffect};
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;

// =============================================================================
// PLUGIN ENTRY: identity and typed schema seed
// =============================================================================

const PLUGIN_NAME: &str = "persona";
const PLUGIN_VERSION: &str = "1.0.0";
const PLUGIN_CATEGORY: &str = "agents";
const PLUGIN_DESCRIPTION: &str =
    "Agent persona catalog — built-in agent types, names, descriptions, operations";
const PLUGIN_DISPLAY_NAME: &str = "GB.Persona";

/// A single agent persona.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "exp.agent.persona.persona.render@v1"))]
pub struct Persona {
    /// Agent type identifier.
    #[schemars(
        description = "Agent type identifier",
        extend("x-oscal-subid" = "exp.agent.persona.agent-type.render@v1")
    )]
    pub agent_type: String,

    /// Display name.
    #[schemars(
        description = "Display name",
        extend("x-oscal-subid" = "exp.agent.persona.name.render@v1")
    )]
    pub name: String,

    /// Description of the persona.
    #[schemars(
        description = "Description of the persona",
        extend("x-oscal-subid" = "exp.agent.persona.description.render@v1")
    )]
    pub description: String,

    /// Agent category: execution | persona | orchestration.
    #[schemars(
        description = "Agent category (execution, persona, orchestration)",
        extend("x-oscal-subid" = "exp.agent.persona.category.render@v1")
    )]
    pub category: String,

    /// Supported operations.
    #[serde(default)]
    #[schemars(
        description = "Supported operations",
        extend("x-oscal-subid" = "exp.agent.persona.operations.render@v1")
    )]
    pub operations: Vec<String>,

    /// Per-operation JSON Schemas (input + output).
    #[serde(default)]
    #[schemars(
        description = "Per-operation JSON Schemas",
        extend("x-oscal-subid" = "sch.agent.persona.operation-schemas.render@v1")
    )]
    pub operation_schemas: Vec<PersonaOperationSchema>,
}

/// JSON Schema contract for one agent operation.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.agent.persona.operation.schema@v1"))]
pub struct PersonaOperationSchema {
    pub name: String,
    pub description: String,
    /// JSON Schema for operation arguments.
    #[schemars(with = "serde_json::Value")]
    pub input_schema: serde_json::Value,
    /// JSON Schema for operation results.
    #[schemars(with = "serde_json::Value")]
    pub output_schema: serde_json::Value,
}

/// Persona catalog live state.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.agent.plugin.persona.schema@v1"))]
#[schemars(extend("x-oscal-category" = "agents"))]
pub struct PersonaState {
    /// Current status.
    #[schemars(
        description = "Current status",
        extend("x-oscal-subid" = "obs.agent.persona.status.observe@v1")
    )]
    pub status: String,

    /// Number of personas in the catalog.
    #[schemars(
        description = "Number of personas in the catalog",
        extend("x-oscal-subid" = "obs.agent.persona.count.observe@v1")
    )]
    pub persona_count: usize,

    /// Available personas.
    #[serde(default)]
    #[schemars(
        description = "Available personas",
        extend("x-oscal-subid" = "exp.agent.persona.catalog.render@v1")
    )]
    pub personas: Vec<Persona>,
}

// =============================================================================
// METHOD INPUT/OUTPUT TYPES
// =============================================================================

/// Input for GetIdentity method.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "obs.agent.persona.identity.get.input@v1"))]
pub struct GetIdentityInput {
    /// Agent type to get identity for.
    pub agent_type: String,
}

/// Output for GetIdentity method.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "obs.agent.persona.identity.get.output@v1"))]
pub struct GetIdentityOutput {
    /// The persona identity, if found.
    pub persona: Option<Persona>,
}

/// Input for SetIdentity method.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "mut.agent.persona.identity.set.input@v1"))]
pub struct SetIdentityInput {
    /// Agent type to set identity for.
    pub agent_type: String,
    /// New name.
    pub name: String,
    /// New description (optional).
    #[serde(default)]
    pub description: Option<String>,
}

/// Input for Authenticate method.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "mut.agent.persona.authenticate.input@v1"))]
pub struct AuthenticateInput {
    /// Agent type to authenticate.
    pub agent_type: String,
    /// Credentials map.
    pub credentials: std::collections::HashMap<String, String>,
}

/// Output for Authenticate method.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "obs.agent.persona.authenticate.output@v1"))]
pub struct AuthenticateOutput {
    /// Whether authentication succeeded.
    pub authenticated: bool,
    /// Auth token, if successful.
    #[serde(default)]
    pub token: Option<String>,
}

/// Input for Execute — run a catalog agent operation.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "mut.agent.persona.execute.input@v1"))]
pub struct ExecuteInput {
    /// Agent type from the catalog (e.g. rust-pro, memory).
    pub agent_type: String,
    /// Operation name advertised by that agent.
    pub operation: String,
    /// Optional working path.
    #[serde(default)]
    pub path: Option<String>,
    /// Free-form query (persona agents).
    #[serde(default)]
    pub query: Option<String>,
    /// Operation arguments object.
    #[serde(default)]
    #[schemars(with = "serde_json::Value")]
    pub args: Option<serde_json::Value>,
}

/// Output for Execute.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "obs.agent.persona.execute.output@v1"))]
pub struct ExecuteOutput {
    pub success: bool,
    pub agent_type: String,
    pub operation: String,
    #[schemars(with = "serde_json::Value")]
    pub data: serde_json::Value,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    #[schemars(with = "serde_json::Value")]
    pub metadata: Option<serde_json::Value>,
}

// =============================================================================
// PLUGIN BODY: D-Bus-backed behavior only
// =============================================================================

pub struct PersonaPlugin;

impl Default for PersonaPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl PersonaPlugin {
    pub fn new() -> Self {
        Self
    }

    /// Enumerate the live persona catalog from the real agent implementations.
    fn live() -> PersonaState {
        let personas: Vec<Persona> = builtin_agent_descriptors()
            .into_iter()
            .map(|d| {
                let category = format!("{:?}", d.category).to_lowercase();
                let operation_schemas = d
                    .operation_schemas
                    .iter()
                    .map(|op| PersonaOperationSchema {
                        name: op.name.clone(),
                        description: op.description.clone(),
                        input_schema: serde_json::from_str(
                            &simd_json::to_string(&op.input_schema).unwrap_or_else(|_| "{}".into()),
                        )
                        .unwrap_or(serde_json::json!({})),
                        output_schema: serde_json::from_str(
                            &simd_json::to_string(&op.output_schema)
                                .unwrap_or_else(|_| "{}".into()),
                        )
                        .unwrap_or(serde_json::json!({})),
                    })
                    .collect();
                Persona {
                    agent_type: d.agent_type,
                    name: d.name,
                    description: d.description,
                    category,
                    operations: d.operations,
                    operation_schemas,
                }
            })
            .collect();
        PersonaState {
            status: "active".to_string(),
            persona_count: personas.len(),
            personas,
        }
    }
}

/// Dispatch persona plugin methods (catalog + execute).
pub async fn dispatch_persona_method(
    method: &str,
    args: &serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    match method {
        "get_identity" | "GetIdentity" => {
            let agent_type = args
                .get("agent_type")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let persona = PersonaPlugin::live()
                .personas
                .into_iter()
                .find(|p| p.agent_type == agent_type);
            Ok(serde_json::to_value(GetIdentityOutput { persona })?)
        }
        "set_identity" | "SetIdentity" => {
            anyhow::bail!("persona catalog is read-only (code-defined in op-agents)")
        }
        "authenticate" | "Authenticate" => Ok(serde_json::to_value(AuthenticateOutput {
            authenticated: false,
            token: None,
        })?),
        "execute" | "Execute" => {
            let input: ExecuteInput = serde_json::from_value(args.clone())
                .map_err(|e| anyhow::anyhow!("invalid execute args: {e}"))?;
            let mut task = op_agents::AgentTask::new(&input.agent_type, &input.operation);
            if let Some(path) = &input.path {
                task = task.with_path(path);
            }
            let mut arg_obj = serde_json::Map::new();
            if let Some(serde_json::Value::Object(map)) = input.args.clone() {
                arg_obj.extend(map);
            }
            if let Some(query) = &input.query {
                arg_obj.insert("query".into(), serde_json::Value::String(query.clone()));
            }
            if !arg_obj.is_empty() {
                task = task.with_args(&serde_json::Value::Object(arg_obj).to_string());
            }

            let agent =
                op_agents::create_agent(&input.agent_type, format!("persona-{}", input.agent_type))
                    .map_err(anyhow::Error::msg)?;
            match agent.execute(task).await {
                Ok(result) => {
                    let data: serde_json::Value = serde_json::from_str(&result.data)
                        .unwrap_or_else(|_| serde_json::Value::String(result.data.clone()));
                    let metadata = serde_json::to_value(&result.metadata).ok();
                    Ok(serde_json::to_value(ExecuteOutput {
                        success: result.success,
                        agent_type: input.agent_type,
                        operation: result.operation,
                        data,
                        message: metadata
                            .as_ref()
                            .and_then(|m| m.get("message"))
                            .and_then(|v| v.as_str())
                            .map(str::to_string),
                        metadata,
                    })?)
                }
                Err(e) => Ok(serde_json::to_value(ExecuteOutput {
                    success: false,
                    agent_type: input.agent_type,
                    operation: input.operation,
                    data: serde_json::Value::Null,
                    message: Some(e),
                    metadata: None,
                })?),
            }
        }
        other => anyhow::bail!("unknown persona method: {other}"),
    }
}

#[async_trait]
impl StatePlugin for PersonaPlugin {
    fn name(&self) -> &str {
        PLUGIN_NAME
    }

    fn version(&self) -> &str {
        PLUGIN_VERSION
    }

    fn schema(&self) -> Option<PluginSchema> {
        Some(persona_schema())
    }

    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        Ok(StateDiff {
            plugin: PLUGIN_NAME.to_string(),
            actions: vec![StateAction::NoOp {
                resource: "personas".to_string(),
            }],
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: String::new(),
                desired_hash: String::new(),
            },
        })
    }

    async fn apply_state(&self, diff: &StateDiff) -> Result<ApplyResult> {
        let mut errors = Vec::new();
        for action in &diff.actions {
            match action {
                StateAction::NoOp { .. } => {}
                other => errors.push(format!(
                    "persona catalog is code-defined (op-agents) and read-only; \
                     cannot apply {:?}",
                    other
                )),
            }
        }
        Ok(ApplyResult {
            success: errors.is_empty(),
            changes_applied: Vec::new(),
            errors,
            checkpoint: None,
        })
    }

    async fn verify_state(&self, desired: &Value) -> Result<bool> {
        let current = simd_json::serde::to_owned_value(Self::live())?;
        Ok(&current == desired)
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        Ok(Checkpoint {
            id: uuid::Uuid::new_v4().to_string(),
            plugin: PLUGIN_NAME.to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: simd_json::serde::to_owned_value(Self::live())?,
            backend_checkpoint: None,
        })
    }

    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        Ok(())
    }

    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: false,
            supports_checkpoints: true,
            supports_verification: true,
            atomic_operations: false,
        }
    }
}

// =============================================================================
// PLUGIN EXIT: publish the single PluginSchema contract
// =============================================================================

pub fn persona_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(PersonaState))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        PLUGIN_NAME,
        PLUGIN_VERSION,
        PLUGIN_DESCRIPTION,
        &root,
    );
    schema.category = PLUGIN_CATEGORY.to_string();
    schema.display_name = Some(PLUGIN_DISPLAY_NAME.to_string());

    // GetIdentity method
    schema.methods.insert(
        "get_identity".to_string(),
        method_decl_from_schemars_with_output::<GetIdentityInput, GetIdentityOutput>(
            "get_identity",
            SideEffect::Read,
            true,
            "persona.read",
            "obs.agent.persona.identity.get@v1",
        ),
    );

    // SetIdentity method
    schema.methods.insert(
        "set_identity".to_string(),
        method_decl_from_schemars_with_output::<SetIdentityInput, AckOutput>(
            "set_identity",
            SideEffect::Mutation,
            false,
            "persona.write",
            "mut.agent.persona.identity.set@v1",
        ),
    );

    // Authenticate method
    schema.methods.insert(
        "authenticate".to_string(),
        method_decl_from_schemars_with_output::<AuthenticateInput, AuthenticateOutput>(
            "authenticate",
            SideEffect::Mutation,
            false,
            "persona.write",
            "mut.agent.persona.authenticate@v1",
        ),
    );

    // Execute — run a catalog agent operation (schema-validated entrypoint)
    schema.methods.insert(
        "execute".to_string(),
        method_decl_from_schemars_with_output::<ExecuteInput, ExecuteOutput>(
            "execute",
            SideEffect::Mutation,
            false,
            "persona.execute",
            "mut.agent.persona.execute@v1",
        ),
    );

    schema.capabilities.insert(
        "persona.read".to_string(),
        op_state_store::CapabilityDecl {
            id: "persona.read".to_string(),
            description: "Grants: get_identity.".to_string(),
        },
    );
    schema.capabilities.insert(
        "persona.write".to_string(),
        op_state_store::CapabilityDecl {
            id: "persona.write".to_string(),
            description: "Grants: set_identity, authenticate.".to_string(),
        },
    );
    schema.capabilities.insert(
        "persona.execute".to_string(),
        op_state_store::CapabilityDecl {
            id: "persona.execute".to_string(),
            description: "Grants: execute.".to_string(),
        },
    );

    schema
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new(PLUGIN_NAME, |_ctx| std::sync::Arc::new(PersonaPlugin::new()))
}
