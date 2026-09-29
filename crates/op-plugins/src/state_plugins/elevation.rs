//! elevation — per-task sudo elevation for protected operations.
//!
//! Some operations change what every user gets: the Control Plane model and
//! the router's provider profiles, and the system-wide UI catalog. Each of
//! these is a *protected task*. Besides the caller's verified identity (the
//! stored SID1 session) and exact grant, a protected task needs a one-time
//! elevation ticket for that exact call: the caller re-enters their current
//! sudo password through `authorize_task`, which binds the ticket to the
//! target plugin, method and canonical arguments. The ticket is consumed by
//! the call it names and expires shortly after; nothing stays unlocked.
//!
//! Protected tasks carry polkit-style action ids so a polkit-backed authority
//! can later replace the password check without touching the protected
//! methods; `deploy/polkit/org.opdbus.v1.policy` declares the same ids for
//! local D-Bus callers. This plugin is the schema of record and owns the
//! protected-task policy; MutationEngine enforces it at its single dispatch
//! choke point (op-grpc-bridge `task_elevation`).

use anyhow::{ensure, Result};
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{PluginSchema, SideEffect};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use simd_json::OwnedValue as Value;

use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;

pub const PLUGIN_ID: &str = "elevation";
const PLUGIN_VERSION: &str = "1.0.0";
pub const AUTHORIZE_CAPABILITY: &str = "elevation.authorize";
pub const READ_CAPABILITY: &str = "elevation.read";
/// Seconds an unused ticket stays valid.
pub const TICKET_TTL_SECONDS: i64 = 60;
pub const MAX_PASSWORD_BYTES: usize = 1024;

pub const CONTROL_PLANE_CONFIGURE: &str = "org.opdbus.v1.control-plane.configure";
pub const ROUTER_PROVIDERS_CONFIGURE: &str = "org.opdbus.v1.router.providers.configure";
pub const ROUTER_CONFIG_MIGRATE: &str = "org.opdbus.v1.router.config.migrate";
pub const CATALOG_PROMOTE: &str = "org.opdbus.v1.catalog.promote";
pub const CATALOG_RETIRE: &str = "org.opdbus.v1.catalog.retire";

/// One protected task class.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-oscal-subid" = "sch.service.elevation.protected-action.schema@v1"))]
pub struct ProtectedAction {
    /// polkit-style action id.
    pub action_id: String,
    pub plugin: String,
    pub methods: Vec<String>,
    pub description: String,
}

pub fn protected_actions() -> Vec<ProtectedAction> {
    let router = |action: &str, methods: &[&str], description: &str| ProtectedAction {
        action_id: action.into(),
        plugin: "tched_router".into(),
        methods: methods.iter().map(|m| m.to_string()).collect(),
        description: description.into(),
    };
    vec![
        router(
            CONTROL_PLANE_CONFIGURE,
            &["config_set", "config_patch", "PatchConfig", "config_init"],
            "Change the Control Plane agent's configuration, including its model.",
        ),
        router(
            ROUTER_PROVIDERS_CONFIGURE,
            &["config_set", "config_patch", "PatchConfig", "config_init"],
            "Change the router's provider profiles, their models and endpoints.",
        ),
        router(
            ROUTER_CONFIG_MIGRATE,
            &["config_migrate"],
            "Migrate the router configuration.",
        ),
        ProtectedAction {
            action_id: CATALOG_PROMOTE.into(),
            plugin: "json_render".into(),
            methods: vec!["promote_candidate".into()],
            description: "Promote a gallery candidate into the system-wide UI catalog.".into(),
        },
        ProtectedAction {
            action_id: CATALOG_RETIRE.into(),
            plugin: "json_render".into(),
            methods: vec!["retire_catalog_entry".into()],
            description: "Retire an entry from the system-wide UI catalog.".into(),
        },
    ]
}

/// Which protected configuration area a dotted config path or a JSON Pointer
/// reaches. A path that names a parent of a protected area (the root,
/// `agents`, `providers`) reaches it too.
fn config_area(path: &str, pointer: bool) -> Option<&'static str> {
    let segments: Vec<&str> = if pointer {
        path.strip_prefix('/')
            .unwrap_or(path)
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect()
    } else {
        path.split('.')
            .filter(|segment| !segment.is_empty())
            .collect()
    };
    match segments.as_slice() {
        [] => Some(CONTROL_PLANE_CONFIGURE),
        ["agents"] | ["agents", "control_plane", ..] => Some(CONTROL_PLANE_CONFIGURE),
        ["providers"] | ["providers", "models", ..] => Some(ROUTER_PROVIDERS_CONFIGURE),
        _ => None,
    }
}

/// The protected action a call performs, if any. `args` are the call's
/// admitted arguments; a malformed shape is treated as protected so it cannot
/// slip past the check.
pub fn protected_action(plugin: &str, method: &str, args: &JsonValue) -> Option<&'static str> {
    match (plugin, method) {
        ("json_render", "promote_candidate") => Some(CATALOG_PROMOTE),
        ("json_render", "retire_catalog_entry") => Some(CATALOG_RETIRE),
        ("tched_router", "config_migrate") => Some(ROUTER_CONFIG_MIGRATE),
        ("tched_router", "config_set") => match args.get("path").and_then(JsonValue::as_str) {
            Some(path) => config_area(path, false),
            None => Some(CONTROL_PLANE_CONFIGURE),
        },
        ("tched_router", "config_init") => match args.get("section") {
            None | Some(JsonValue::Null) => Some(CONTROL_PLANE_CONFIGURE),
            Some(JsonValue::String(section)) => config_area(section, false),
            Some(_) => Some(CONTROL_PLANE_CONFIGURE),
        },
        ("tched_router", "config_patch" | "PatchConfig") => {
            let Some(operations) = args.get("operations").and_then(JsonValue::as_array) else {
                return Some(CONTROL_PLANE_CONFIGURE);
            };
            let mut found = None;
            for operation in operations {
                let area = match operation.get("path").and_then(JsonValue::as_str) {
                    Some(path) => config_area(path, true),
                    None => Some(CONTROL_PLANE_CONFIGURE),
                };
                match (found, area) {
                    (_, None) => {}
                    (None, Some(area)) => found = Some(area),
                    // One ticket covers the call; control-plane wins the label.
                    (Some(_), Some(CONTROL_PLANE_CONFIGURE)) => {
                        found = Some(CONTROL_PLANE_CONFIGURE)
                    }
                    (Some(_), Some(_)) => {}
                }
            }
            found
        }
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AuthorizeTaskInput {
    /// The protected call this ticket is for: its plugin, method and the
    /// exact arguments it will be sent with.
    pub plugin: String,
    pub method: String,
    pub args: JsonValue,
    /// The caller's current sudo password. Checked once and discarded.
    #[schemars(extend("x-secret" = true))]
    pub password: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AuthorizeTaskOutput {
    pub action_id: String,
    /// RFC 3339 time after which the unused ticket is void.
    pub expires_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListProtectedActionsInput {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListProtectedActionsOutput {
    pub actions: Vec<ProtectedAction>,
    pub ticket_ttl_seconds: i64,
}

pub fn validate_authorize_input(input: &AuthorizeTaskInput) -> Result<&'static str> {
    ensure!(
        !input.password.is_empty() && input.password.len() <= MAX_PASSWORD_BYTES,
        "password must be 1 to {MAX_PASSWORD_BYTES} bytes"
    );
    ensure!(input.args.is_object(), "task arguments must be an object");
    protected_action(&input.plugin, &input.method, &input.args).ok_or_else(|| {
        anyhow::anyhow!(
            "{}.{} with these arguments is not a protected task",
            input.plugin,
            input.method
        )
    })
}

/// The protected-task policy, sealed in this plugin's blob.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.service.plugin.elevation.schema@v1"))]
#[schemars(extend("x-oscal-category" = "service"))]
pub struct ElevationState {
    #[schemars(extend("x-oscal-subid" = "obs.service.elevation.protected-actions@v1"))]
    pub protected_actions: Vec<ProtectedAction>,
    #[schemars(extend("x-oscal-subid" = "obs.service.elevation.ticket-ttl@v1"))]
    pub ticket_ttl_seconds: i64,
}

impl Default for ElevationState {
    fn default() -> Self {
        Self {
            protected_actions: protected_actions(),
            ticket_ttl_seconds: TICKET_TTL_SECONDS,
        }
    }
}

pub fn elevation_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(ElevationState))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        PLUGIN_ID,
        PLUGIN_VERSION,
        "Per-task sudo elevation for protected operations",
        &root,
    );
    schema.category = "service".to_string();
    schema.display_name = Some("Task Elevation".to_string());
    let state = simd_json::serde::to_owned_value(&ElevationState::default())
        .expect("ElevationState default serializes");
    super::schemars_adapter::apply_state_defaults(&mut schema, &state);
    schema.methods.insert(
        "authorize_task".to_string(),
        method_decl_from_schemars_with_output::<AuthorizeTaskInput, AuthorizeTaskOutput>(
            "authorize_task",
            SideEffect::Mutation,
            false,
            AUTHORIZE_CAPABILITY,
            "mut.service.elevation.task.authorize@v1",
        ),
    );
    schema.methods.insert(
        "list_protected_actions".to_string(),
        method_decl_from_schemars_with_output::<
            ListProtectedActionsInput,
            ListProtectedActionsOutput,
        >(
            "list_protected_actions",
            SideEffect::Read,
            true,
            READ_CAPABILITY,
            "obs.service.elevation.actions.list@v1",
        ),
    );
    schema.capabilities.insert(
        AUTHORIZE_CAPABILITY.to_string(),
        op_state_store::CapabilityDecl {
            id: AUTHORIZE_CAPABILITY.to_string(),
            description: "Grants: authorize_task (one protected call, with the caller's current sudo password).".to_string(),
        },
    );
    schema.capabilities.insert(
        READ_CAPABILITY.to_string(),
        op_state_store::CapabilityDecl {
            id: READ_CAPABILITY.to_string(),
            description: "Grants: list_protected_actions.".to_string(),
        },
    );
    schema
}

pub struct ElevationPlugin;

impl ElevationPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ElevationPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StatePlugin for ElevationPlugin {
    fn name(&self) -> &str {
        PLUGIN_ID
    }

    fn version(&self) -> &str {
        PLUGIN_VERSION
    }

    fn schema(&self) -> Option<PluginSchema> {
        let mut schema = elevation_schema();
        super::common::oscal::ensure_category_metadata_fields(&mut schema);
        Some(schema)
    }

    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        Ok(StateDiff {
            plugin: PLUGIN_ID.to_string(),
            actions: vec![],
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: "schema".to_string(),
                desired_hash: "schema".to_string(),
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
            plugin: PLUGIN_ID.to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: simd_json::serde::to_owned_value(ElevationState::default())?,
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

inventory::submit! {
    crate::default_registry::PluginReg::new(PLUGIN_ID, |_ctx| std::sync::Arc::new(ElevationPlugin::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_blob::subid::validate_subid;
    use serde_json::json;

    #[test]
    fn control_plane_and_provider_changes_are_protected() {
        let set = |path: &str| {
            protected_action(
                "tched_router",
                "config_set",
                &json!({"path": path, "value": "x"}),
            )
        };
        assert_eq!(
            set("agents.control_plane.model_provider"),
            Some(CONTROL_PLANE_CONFIGURE)
        );
        assert_eq!(set("agents"), Some(CONTROL_PLANE_CONFIGURE));
        assert_eq!(set(""), Some(CONTROL_PLANE_CONFIGURE));
        assert_eq!(
            set("providers.models.opencode.go_control_plane.model"),
            Some(ROUTER_PROVIDERS_CONFIGURE)
        );
        assert_eq!(set("providers"), Some(ROUTER_PROVIDERS_CONFIGURE));
        assert_eq!(set("agents.dashboard.model_provider"), None);
        assert_eq!(set("gateway.request_timeout_secs"), None);
        assert_eq!(
            protected_action("tched_router", "config_set", &json!({"value": 1})),
            Some(CONTROL_PLANE_CONFIGURE),
            "a malformed call is treated as protected"
        );
    }

    #[test]
    fn patches_are_protected_when_any_operation_reaches_an_area() {
        let patch = |ops: JsonValue| {
            protected_action("tched_router", "PatchConfig", &json!({"operations": ops}))
        };
        assert_eq!(
            patch(
                json!([{"op": "replace", "path": "/agents/dashboard/model_provider", "value": "x"}])
            ),
            None
        );
        assert_eq!(
            patch(json!([
                {"op": "replace", "path": "/agents/dashboard/model_provider", "value": "x"},
                {"op": "replace", "path": "/providers/models/opencode/go_x/model", "value": "y"}
            ])),
            Some(ROUTER_PROVIDERS_CONFIGURE)
        );
        assert_eq!(
            patch(json!([{"op": "add", "path": "/agents", "value": {}}])),
            Some(CONTROL_PLANE_CONFIGURE)
        );
        assert_eq!(
            protected_action(
                "tched_router",
                "config_patch",
                &json!({"operations": [{"op": "remove", "path": "/agents/control_plane"}]})
            ),
            Some(CONTROL_PLANE_CONFIGURE)
        );
        assert_eq!(
            protected_action("tched_router", "config_init", &json!({})),
            Some(CONTROL_PLANE_CONFIGURE)
        );
        assert_eq!(
            protected_action(
                "tched_router",
                "config_init",
                &json!({"section": "gateway"})
            ),
            None
        );
        assert_eq!(
            protected_action("tched_router", "config_migrate", &json!({})),
            Some(ROUTER_CONFIG_MIGRATE)
        );
        assert_eq!(
            protected_action("tched_router", "SetSelection", &json!({})),
            None
        );
    }

    #[test]
    fn catalog_changes_are_protected_and_reads_are_not() {
        assert_eq!(
            protected_action("json_render", "promote_candidate", &json!({})),
            Some(CATALOG_PROMOTE)
        );
        assert_eq!(
            protected_action("json_render", "retire_catalog_entry", &json!({})),
            Some(CATALOG_RETIRE)
        );
        assert_eq!(
            protected_action("json_render", "save_candidate", &json!({})),
            None
        );
        assert_eq!(
            protected_action("json_render", "list_catalog", &json!({})),
            None
        );
    }

    #[test]
    fn authorize_input_names_a_protected_task_and_a_password() {
        let input = AuthorizeTaskInput {
            plugin: "tched_router".into(),
            method: "config_set".into(),
            args: json!({"path": "agents.control_plane.model_provider", "value": "opencode.go_x"}),
            password: "secret".into(),
        };
        assert_eq!(
            validate_authorize_input(&input).unwrap(),
            CONTROL_PLANE_CONFIGURE
        );
        let mut empty = input.clone();
        empty.password.clear();
        assert!(validate_authorize_input(&empty).is_err());
        let mut unprotected = input.clone();
        unprotected.args = json!({"path": "gateway.port", "value": 1});
        assert!(validate_authorize_input(&unprotected).is_err());
    }

    #[test]
    fn schema_declares_methods_grants_and_policy() {
        let schema = elevation_schema();
        assert_eq!(
            schema.methods["authorize_task"]
                .required_capability
                .as_deref(),
            Some(AUTHORIZE_CAPABILITY)
        );
        assert_eq!(
            schema.methods["list_protected_actions"]
                .required_capability
                .as_deref(),
            Some(READ_CAPABILITY)
        );
        assert!(schema.fields.contains_key("protected_actions"));
        let actions = protected_actions();
        for action in &actions {
            assert!(action.action_id.starts_with("org.opdbus.v1."));
        }
    }

    #[test]
    fn all_subids_are_valid() {
        let raw = serde_json::to_value(schemars::schema_for!(ElevationState)).unwrap();
        let mut stack = vec![&raw];
        while let Some(node) = stack.pop() {
            if let Some(subid) = node.get("x-oscal-subid").and_then(|v| v.as_str()) {
                validate_subid(subid).unwrap();
            }
            if let Some(object) = node.as_object() {
                stack.extend(object.values());
            }
        }
        for method in elevation_schema().methods.values() {
            validate_subid(&method.subid).unwrap();
        }
    }
}
