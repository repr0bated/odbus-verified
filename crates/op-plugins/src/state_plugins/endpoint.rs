use anyhow::Result;
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::PluginSchema;
use serde::{Deserialize, Serialize};
use simd_json::json;
use simd_json::prelude::*;
use simd_json::OwnedValue as Value;

/// Endpoint declaration state.
#[derive(Debug, Clone, Serialize, Deserialize, Default, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.endpoint.schema@v1"))]
#[schemars(extend("x-oscal-category" = "service"))]
pub struct EndpointState {
    /// Declared endpoints.
    #[serde(default)]
    #[schemars(
        description = "Declared endpoints",
        example = &["192.168.1.100:8080"],
        extend("x-oscal-subid" = "exp.service.endpoint.endpoints.render@v1")
    )]
    pub endpoints: Vec<String>,
}

pub struct EndpointPlugin;

impl Default for EndpointPlugin {
    fn default() -> Self {
        Self
    }
}

impl EndpointPlugin {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EndpointInput {
    /// Exact endpoint declaration to look up, add or remove. Registration
    /// records a declaration; it does not open a listener or route traffic.
    pub endpoint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ListEndpointsOutput {
    pub endpoints: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GetEndpointOutput {
    pub endpoint: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EndpointRegistrationOutput {
    pub endpoint: String,
    pub present: bool,
    pub revision: u64,
}

fn endpoint_state(value: Option<serde_json::Value>) -> Result<EndpointState> {
    value
        .map(serde_json::from_value)
        .transpose()
        .map(|s| s.unwrap_or_default())
        .map_err(Into::into)
}

fn checked_endpoint(args: &serde_json::Value) -> Result<String> {
    let input: EndpointInput = serde_json::from_value(args.clone())?;
    anyhow::ensure!(!input.endpoint.is_empty() && input.endpoint.len() <= 2048
        && input.endpoint.trim() == input.endpoint && !input.endpoint.chars().any(char::is_control),
        "endpoint must be a nonempty declaration without surrounding whitespace or control characters");
    Ok(input.endpoint)
}

pub async fn dispatch_endpoint_method(
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    let store = super::runtime_documents::runtime_documents().await?;
    dispatch_endpoint_with_store(&store, method, args).await
}

pub async fn dispatch_endpoint_with_store(
    store: &super::runtime_documents::RuntimeDocuments,
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    match method {
        "list_endpoints" => {
            let mut state = endpoint_state(store.read("endpoint").await?.map(|r| r.document))?;
            state.endpoints.sort();
            Ok(serde_json::to_value(ListEndpointsOutput {
                endpoints: state.endpoints,
            })?)
        }
        "get_endpoint" => {
            let endpoint = checked_endpoint(args)?;
            let state = endpoint_state(store.read("endpoint").await?.map(|r| r.document))?;
            Ok(serde_json::to_value(GetEndpointOutput {
                endpoint: state.endpoints.into_iter().find(|item| item == &endpoint),
            })?)
        }
        "add_endpoint" | "remove_endpoint" => {
            let endpoint = checked_endpoint(args)?;
            let key = endpoint.clone();
            let add = method == "add_endpoint";
            let row = store
                .update("endpoint", move |old| {
                    let mut state = endpoint_state(old)?;
                    if add {
                        state.endpoints.push(key);
                    } else {
                        state.endpoints.retain(|item| item != &key);
                    }
                    state.endpoints.sort();
                    state.endpoints.dedup();
                    Ok(serde_json::to_value(state)?)
                })
                .await?;
            let state = endpoint_state(Some(row.document))?;
            Ok(serde_json::to_value(EndpointRegistrationOutput {
                present: state.endpoints.contains(&endpoint),
                endpoint,
                revision: row.revision,
            })?)
        }
        _ => anyhow::bail!("unknown endpoint method '{method}'"),
    }
}

#[async_trait]
impl StatePlugin for EndpointPlugin {
    fn name(&self) -> &str {
        "endpoint"
    }

    fn version(&self) -> &str {
        "2.0.0"
    }

    fn schema(&self) -> Option<op_state_store::PluginSchema> {
        Some(endpoint_schema())
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
            state_snapshot: Value::null(),
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

/// Derived `endpoint` schema from the `EndpointState` struct.
pub fn endpoint_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(EndpointState))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "endpoint",
        "2.0.0",
        "Endpoint configuration",
        &root,
    );

    // Add methods
    use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;
    use op_state_store::SideEffect;

    schema.methods.insert(
        "list_endpoints".to_string(),
        method_decl_from_schemars_with_output::<(), ListEndpointsOutput>(
            "list_endpoints",
            SideEffect::Read,
            true,
            "endpoint.read",
            "obs.service.endpoint.list@v1",
        ),
    );
    schema.methods.insert(
        "get_endpoint".to_string(),
        method_decl_from_schemars_with_output::<EndpointInput, GetEndpointOutput>(
            "get_endpoint",
            SideEffect::Read,
            true,
            "endpoint.read",
            "obs.service.endpoint.get@v1",
        ),
    );
    schema.methods.insert(
        "add_endpoint".to_string(),
        method_decl_from_schemars_with_output::<EndpointInput, EndpointRegistrationOutput>(
            "add_endpoint",
            SideEffect::Mutation,
            false,
            "endpoint.invoke",
            "mut.service.endpoint.add@v1",
        ),
    );
    schema.methods.insert(
        "remove_endpoint".to_string(),
        method_decl_from_schemars_with_output::<EndpointInput, EndpointRegistrationOutput>(
            "remove_endpoint",
            SideEffect::Mutation,
            true,
            "endpoint.invoke",
            "mut.service.endpoint.remove@v1",
        ),
    );

    schema.capabilities.insert(
        "endpoint.read".to_string(),
        op_state_store::CapabilityDecl {
            id: "endpoint.read".to_string(),
            description: "Grants: list_endpoints, get_endpoint.".to_string(),
        },
    );
    schema.capabilities.insert(
        "endpoint.invoke".to_string(),
        op_state_store::CapabilityDecl {
            id: "endpoint.invoke".to_string(),
            description: "Grants: add_endpoint, remove_endpoint.".to_string(),
        },
    );

    schema
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_is_schemars_seeded_and_typed() {
        let schema = endpoint_schema();
        assert_eq!(schema.name, "endpoint");
        assert_eq!(schema.version, "2.0.0");
        assert!(schema.fields.contains_key("endpoints"));
        assert!(schema.methods.contains_key("list_endpoints"));
        assert!(schema.methods.contains_key("get_endpoint"));
        assert!(schema.methods.contains_key("add_endpoint"));
        assert!(schema.methods.contains_key("remove_endpoint"));
    }

    /// Every `x-oscal-subid` annotation in the derived schema must be a valid
    /// OSCAL subid according to the canonical taxonomy.
    #[test]
    fn all_subids_are_valid() {
        let raw = serde_json::to_value(schemars::schema_for!(EndpointState)).unwrap();
        let mut subids = Vec::new();
        collect_subids(&raw, &mut subids);
        assert!(!subids.is_empty(), "expected at least one subid");
        for subid in subids {
            crate::state_plugins::common::oscal::validate_subid(&subid)
                .unwrap_or_else(|e| panic!("invalid subid {subid}: {e}"));
        }
    }

    fn collect_subids(value: &serde_json::Value, out: &mut Vec<String>) {
        if let serde_json::Value::Object(map) = value {
            if let Some(subid) = map.get("x-oscal-subid").and_then(|v| v.as_str()) {
                out.push(subid.to_string());
            }
            for v in map.values() {
                collect_subids(v, out);
            }
        } else if let serde_json::Value::Array(arr) = value {
            for v in arr {
                collect_subids(v, out);
            }
        }
    }

    #[tokio::test]
    async fn registration_lookup_and_removal_use_committed_state() {
        use serde_json::json;
        let store = super::super::runtime_documents::RuntimeDocuments::new(
            op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap(),
        );
        let input = json!({"endpoint":"example.test:8080"});
        assert_eq!(
            dispatch_endpoint_with_store(&store, "get_endpoint", &input)
                .await
                .unwrap()["endpoint"],
            serde_json::Value::Null
        );
        assert_eq!(
            dispatch_endpoint_with_store(&store, "add_endpoint", &input)
                .await
                .unwrap()["present"],
            true
        );
        dispatch_endpoint_with_store(&store, "add_endpoint", &input)
            .await
            .unwrap();
        assert_eq!(
            dispatch_endpoint_with_store(&store, "list_endpoints", &json!({}))
                .await
                .unwrap()["endpoints"],
            json!(["example.test:8080"])
        );
        assert_eq!(
            dispatch_endpoint_with_store(&store, "get_endpoint", &input)
                .await
                .unwrap()["endpoint"],
            "example.test:8080"
        );
        assert!(
            dispatch_endpoint_with_store(&store, "remove_endpoint", &json!({}))
                .await
                .is_err()
        );
        assert_eq!(
            dispatch_endpoint_with_store(&store, "remove_endpoint", &input)
                .await
                .unwrap()["present"],
            false
        );
        assert_eq!(
            dispatch_endpoint_with_store(&store, "list_endpoints", &json!({}))
                .await
                .unwrap()["endpoints"],
            json!([])
        );
    }
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new("endpoint", |_ctx| std::sync::Arc::new(EndpointPlugin::new()))
}
