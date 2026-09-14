use anyhow::{Context, Result};
use async_trait::async_trait;
use op_state::{
    ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateAction, StateDiff, StatePlugin,
};
use op_state_store::{FieldSchema, FieldType, PluginSchema};
use serde::{Deserialize, Serialize};
use simd_json::json;
use simd_json::OwnedValue as Value;
use std::collections::HashMap;
use std::path::PathBuf;

const DEFAULT_PRIVACY_ROUTES_PATH: &str = "/var/lib/op-dbus/privacy-routes.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrivacyRoutesState {
    #[serde(default)]
    pub routes: Vec<PrivacyRoute>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
pub struct PrivacyRoute {
    pub name: String,
    pub route_id: String,
    pub user_id: String,
    pub email: String,
    pub wireguard_public_key: String,
    pub assigned_ip: String,
    pub selector_ip: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container_name: Option<String>,
    pub ingress_port: String,
    pub next_hop: String,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

pub struct PrivacyRoutesPlugin {
    store_path: PathBuf,
}

impl Default for PrivacyRoutesPlugin {
    fn default() -> Self {
        Self::new(DEFAULT_PRIVACY_ROUTES_PATH)
    }
}

impl PrivacyRoutesPlugin {
    pub fn new(store_path: impl Into<PathBuf>) -> Self {
        Self {
            store_path: store_path.into(),
        }
    }

    async fn load_store(&self) -> Result<PrivacyRoutesState> {
        match tokio::fs::read_to_string(&self.store_path).await {
            Ok(content) => {
                let mut state: PrivacyRoutesState =
                    serde_json::from_str(&content).context("invalid privacy route store")?;
                state.routes.sort_by(|a, b| a.route_id.cmp(&b.route_id));
                Ok(state)
            }
            Err(_) => Ok(PrivacyRoutesState { routes: Vec::new() }),
        }
    }

    async fn save_store(&self, state: &PrivacyRoutesState) -> Result<()> {
        if let Some(parent) = self.store_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .context("failed to create privacy route directory")?;
        }

        let content = simd_json::to_string_pretty(state).context("serialize privacy routes")?;
        tokio::fs::write(&self.store_path, content)
            .await
            .context("write privacy routes")?;
        Ok(())
    }
}

#[async_trait]
impl StatePlugin for PrivacyRoutesPlugin {
    fn name(&self) -> &str {
        "privacy_routes"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn schema(&self) -> Option<op_state_store::PluginSchema> {
        Some(privacy_routes_schema())
    }

    async fn calculate_diff(&self, current: &Value, desired: &Value) -> Result<StateDiff> {
        let current_state: PrivacyRoutesState = simd_json::serde::from_owned_value(current.clone())
            .context("deserialize current privacy routes")?;
        let desired_state: PrivacyRoutesState = simd_json::serde::from_owned_value(desired.clone())
            .context("deserialize desired privacy routes")?;

        let current_by_id: HashMap<&str, &PrivacyRoute> = current_state
            .routes
            .iter()
            .map(|route| (route.route_id.as_str(), route))
            .collect();
        let desired_by_id: HashMap<&str, &PrivacyRoute> = desired_state
            .routes
            .iter()
            .map(|route| (route.route_id.as_str(), route))
            .collect();

        let mut actions = Vec::new();

        for desired_route in &desired_state.routes {
            match current_by_id.get(desired_route.route_id.as_str()) {
                Some(current_route) if *current_route == desired_route => {}
                Some(_) => actions.push(StateAction::Modify {
                    resource: desired_route.route_id.clone(),
                    changes: simd_json::serde::to_owned_value(desired_route.clone())
                        .context("serialize desired privacy route modify")?,
                }),
                None => actions.push(StateAction::Create {
                    resource: desired_route.route_id.clone(),
                    config: simd_json::serde::to_owned_value(desired_route.clone())
                        .context("serialize desired privacy route create")?,
                }),
            }
        }

        for current_route in &current_state.routes {
            if !desired_by_id.contains_key(current_route.route_id.as_str()) {
                actions.push(StateAction::Delete {
                    resource: current_route.route_id.clone(),
                });
            }
        }

        Ok(StateDiff {
            plugin: self.name().to_string(),
            actions,
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: format!("{:x}", md5::compute(simd_json::to_string(current)?)),
                desired_hash: format!("{:x}", md5::compute(simd_json::to_string(desired)?)),
            },
        })
    }

    async fn apply_state(&self, diff: &StateDiff) -> Result<ApplyResult> {
        let mut state = self.load_store().await?;
        let mut routes_by_id: HashMap<String, PrivacyRoute> = state
            .routes
            .drain(..)
            .map(|route| (route.route_id.clone(), route))
            .collect();

        let mut changes_applied = Vec::new();
        let mut errors = Vec::new();

        for action in &diff.actions {
            match action {
                StateAction::Create { resource, config } => {
                    let route: PrivacyRoute = simd_json::serde::from_owned_value(config.clone())
                        .context("deserialize route create")?;
                    routes_by_id.insert(resource.clone(), route);
                    changes_applied.push(format!("created privacy route {}", resource));
                }
                StateAction::Modify { resource, changes } => {
                    let route: PrivacyRoute = simd_json::serde::from_owned_value(changes.clone())
                        .context("deserialize route modify")?;
                    routes_by_id.insert(resource.clone(), route);
                    changes_applied.push(format!("updated privacy route {}", resource));
                }
                StateAction::Delete { resource } => {
                    routes_by_id.remove(resource);
                    changes_applied.push(format!("deleted privacy route {}", resource));
                }
                StateAction::NoOp { .. } => {}
            }
        }

        state.routes = routes_by_id.into_values().collect();
        state.routes.sort_by(|a, b| a.route_id.cmp(&b.route_id));

        if let Err(e) = self.save_store(&state).await {
            errors.push(e.to_string());
        }

        Ok(ApplyResult {
            success: errors.is_empty(),
            changes_applied,
            errors,
            checkpoint: None,
        })
    }

    async fn verify_state(&self, _desired: &Value) -> Result<bool> {
        Ok(true)
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        let current = simd_json::json!(null);
        Ok(Checkpoint {
            id: format!("privacy-routes-{}", chrono::Utc::now().timestamp()),
            plugin: self.name().to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: current,
            backend_checkpoint: None,
        })
    }

    async fn rollback(&self, checkpoint: &Checkpoint) -> Result<()> {
        let state: PrivacyRoutesState =
            simd_json::serde::from_owned_value(checkpoint.state_snapshot.clone())?;
        self.save_store(&state).await
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

pub(crate) fn privacy_routes_schema() -> PluginSchema {
    let route_fields = {
        let mut fields = HashMap::new();
        fields.insert(
            "name".to_string(),
            FieldSchema {
                field_type: FieldType::String,
                required: true,
                description: "Stable route object identifier".to_string(),
                default: None,
                example: Some(json!(
                    "4f5e7f1a2d3c4b5a6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5"
                )),
                constraints: Vec::new(),
                read_only: true,
                read_only_when: None,
            },
        );
        fields.insert(
            "route_id".to_string(),
            FieldSchema {
                field_type: FieldType::String,
                required: true,
                description: "Derived route ID from WireGuard public key and shared secret"
                    .to_string(),
                default: None,
                example: Some(json!(
                    "4f5e7f1a2d3c4b5a6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5"
                )),
                constraints: Vec::new(),
                read_only: true,
                read_only_when: None,
            },
        );
        fields.insert(
            "user_id".to_string(),
            FieldSchema {
                field_type: FieldType::String,
                required: true,
                description: "Internal privacy user identifier".to_string(),
                default: None,
                example: Some(json!("550e8400-e29b-41d4-a716-446655440000")),
                constraints: Vec::new(),
                read_only: true,
                read_only_when: None,
            },
        );
        fields.insert(
            "email".to_string(),
            FieldSchema {
                field_type: FieldType::String,
                required: true,
                description: "User email for audit and publication context".to_string(),
                default: None,
                example: Some(json!("user@example.com")),
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        );
        fields.insert(
            "wireguard_public_key".to_string(),
            FieldSchema {
                field_type: FieldType::String,
                required: true,
                description: "WireGuard public key backing this route identity".to_string(),
                default: None,
                example: Some(json!("P8c9Kjnv4B3r6C4+J4Q6VQ2sY4bXn4XWz0P2r5s6t7U=")),
                constraints: Vec::new(),
                read_only: true,
                read_only_when: None,
            },
        );
        fields.insert(
            "assigned_ip".to_string(),
            FieldSchema {
                field_type: FieldType::String,
                required: true,
                description: "Assigned WireGuard tunnel address".to_string(),
                default: None,
                example: Some(json!("10.100.0.2/32")),
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        );
        fields.insert(
            "selector_ip".to_string(),
            FieldSchema {
                field_type: FieldType::String,
                required: true,
                description: "Packet-visible selector used for OpenFlow matching".to_string(),
                default: None,
                example: Some(json!("10.100.0.2")),
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        );
        fields.insert(
            "container_name".to_string(),
            FieldSchema {
                field_type: FieldType::String,
                required: false,
                description: "Associated Incus instance name".to_string(),
                default: None,
                example: Some(json!("privacy-user-550e8400")),
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        );
        fields.insert(
            "ingress_port".to_string(),
            FieldSchema {
                field_type: FieldType::String,
                required: true,
                description: "Shared OVS ingress port for route matching".to_string(),
                default: Some(json!("ovsbr0-sock")),
                example: Some(json!("ovsbr0-sock")),
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        );
        fields.insert(
            "next_hop".to_string(),
            FieldSchema {
                field_type: FieldType::String,
                required: true,
                description: "First logical next hop for this route".to_string(),
                default: Some(json!("gbr_wg")),
                example: Some(json!("gbr_wg")),
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        );
        fields.insert(
            "enabled".to_string(),
            FieldSchema {
                field_type: FieldType::Boolean,
                required: true,
                description: "Whether this route should be active".to_string(),
                default: Some(json!(true)),
                example: Some(json!(true)),
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        );
        fields.insert(
            "created_at".to_string(),
            FieldSchema {
                field_type: FieldType::String,
                required: true,
                description: "Creation timestamp".to_string(),
                default: None,
                example: Some(json!("2026-01-01T00:00:00Z")),
                constraints: Vec::new(),
                read_only: true,
                read_only_when: None,
            },
        );
        fields.insert(
            "updated_at".to_string(),
            FieldSchema {
                field_type: FieldType::String,
                required: true,
                description: "Last update timestamp".to_string(),
                default: None,
                example: Some(json!("2026-01-01T00:05:00Z")),
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        );
        fields
    };

    use super::plugin_scaffold_helpers::{method_decl_from_schemars_with_output, EmptyInput};
    use op_state_store::SideEffect;

    #[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
    pub struct ListRoutesOutput {
        pub routes: Vec<PrivacyRoute>,
    }
    #[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
    pub struct DeleteRouteInput {
        pub route_id: String,
    }

    let methods = {
        let mut methods = HashMap::new();
        methods.insert(
            "list_routes".to_string(),
            method_decl_from_schemars_with_output::<EmptyInput, ListRoutesOutput>(
                "list_routes",
                SideEffect::Read,
                true,
                "privacy_routes.read",
                "obs.network.plugin.privacy-routes.routes.list@v1",
            ),
        );
        methods.insert(
            "delete_route".to_string(),
            method_decl_from_schemars_with_output::<DeleteRouteInput, ListRoutesOutput>(
                "delete_route",
                SideEffect::Mutation,
                true,
                "privacy_routes.write",
                "mut.network.plugin.privacy-routes.route.delete@v1",
            ),
        );
        methods
    };

    PluginSchema::builder("privacy_routes")
        .category("network")
        .version("1.0.0")
        .description("Per-user privacy route objects keyed by WireGuard identity")
        .dependency("wireguard")
        .dependency("privacy_router")
        .array_field(
            "routes",
            FieldType::Object(route_fields),
            true,
            "Published privacy route objects",
        )
        .methods(methods)
        .example(json!({
            "routes": [
                {
                    "name": "4f5e7f1a2d3c4b5a6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5",
                    "route_id": "4f5e7f1a2d3c4b5a6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5",
                    "user_id": "550e8400-e29b-41d4-a716-446655440000",
                    "email": "user@example.com",
                    "wireguard_public_key": "P8c9Kjnv4B3r6C4+J4Q6VQ2sY4bXn4XWz0P2r5s6t7U=",
                    "assigned_ip": "10.100.0.2/32",
                    "selector_ip": "10.100.0.2",
                    "container_name": "privacy-user-550e8400",
                    "ingress_port": "ovsbr0-sock",
                    "next_hop": "gbr_wg",
                    "enabled": true,
                    "created_at": "2026-01-01T00:00:00Z",
                    "updated_at": "2026-01-01T00:00:00Z"
                }
            ]
        }))
        .capability(op_state_store::CapabilityDecl {
            id: "privacy_routes.read".to_string(),
            description: "Grants: list_routes.".to_string(),
        })
        .capability(op_state_store::CapabilityDecl {
            id: "privacy_routes.write".to_string(),
            description: "Grants: delete_route.".to_string(),
        })
        .build()
}

/// Dispatch a `privacy_routes` schema method. Called from `op-grpc-bridge`'s
/// `MutationEngine::dispatch_method_call`.
pub async fn dispatch_privacy_routes_method(
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    let plugin = PrivacyRoutesPlugin::default();
    match method {
        "list_routes" => {
            let state = plugin.load_store().await?;
            Ok(serde_json::json!({ "routes": state.routes }))
        }
        "delete_route" => {
            let route_id = args
                .get("route_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("route_id is required"))?;
            let mut state = plugin.load_store().await?;
            state.routes.retain(|r| r.route_id != route_id);
            plugin.save_store(&state).await?;
            Ok(serde_json::json!({ "routes": state.routes }))
        }
        other => Err(anyhow::anyhow!("unknown privacy_routes method: {}", other)),
    }
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new("privacy_routes", |_ctx| std::sync::Arc::new(PrivacyRoutesPlugin::default()))
}
