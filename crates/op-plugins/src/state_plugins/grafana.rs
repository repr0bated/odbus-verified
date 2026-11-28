//! Grafana HTTP API plugin — read-only.
//!
//! Every method input and output is a type generated from Grafana's official
//! OpenAPI document pinned at `grafana/grafana` tag v13.3.1
//! (`public/openapi3.json`). See `grafana/openapi/` (vendored spec,
//! provenance, selection, errata) and the generated `grafana/api_types.rs`;
//! nothing here restates an API shape.
//! Reference: <https://grafana.com/docs/grafana/latest/developers/http_api/>.
//!
//! The server root comes from `GRAFANA_URL` (deploy-time environment, default
//! `http://100.96.0.5:3000`, see deploy/monitoring) and an optional
//! `GRAFANA_API_TOKEN` (a Viewer service-account token, sent as a bearer
//! token); neither is ever taken from method input. Calls are bounded by a
//! timeout, a response size and an item limit; violations are typed
//! `UpstreamCallError`s (see `methods`).

use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;
use anyhow::Result;
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{PluginSchema, SideEffect};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;

#[path = "grafana/api_types.rs"]
pub mod api_types;
#[path = "grafana/methods.rs"]
pub mod methods;

use api_types as api;
pub use methods::{current_state, dispatch_grafana_method, enforce_result_limits};

// =============================================================================
// PLUGIN ENTRY: identity and typed schema seed
// =============================================================================

const PLUGIN_NAME: &str = "grafana";
const PLUGIN_VERSION: &str = "1.0.0";
const PLUGIN_CATEGORY: &str = "service";
const PLUGIN_DESCRIPTION: &str = "Grafana HTTP API (read-only): health, dashboards, folders, data sources, annotations and alert rules";
const PLUGIN_DISPLAY_NAME: &str = "GB.Grafana";
const READ_CAPABILITY: &str = "grafana.read";

/// Observed Grafana server summary, read from the generated `HealthResponse`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.service.plugin.grafana.schema@v1"))]
#[schemars(extend("x-oscal-category" = "service"))]
pub struct GrafanaState {
    /// Configured server root (`GRAFANA_URL`).
    pub endpoint: String,
    /// `/api/health` version.
    #[serde(default)]
    pub version: Option<String>,
    /// `/api/health` commit.
    #[serde(default)]
    pub commit: Option<String>,
    /// `/api/health` database status (`ok` when healthy).
    #[serde(default)]
    pub database: Option<String>,
}

// =============================================================================
// PLUGIN BODY: upstream-backed behavior only
// =============================================================================

pub struct GrafanaPlugin;

impl Default for GrafanaPlugin {
    fn default() -> Self {
        Self
    }
}

impl GrafanaPlugin {
    pub fn new() -> Self {
        Self
    }

    pub(crate) fn exemplar_state() -> GrafanaState {
        GrafanaState {
            endpoint: methods::grafana_url(),
            ..GrafanaState::default()
        }
    }
}

#[async_trait]
impl StatePlugin for GrafanaPlugin {
    fn name(&self) -> &str {
        PLUGIN_NAME
    }

    fn version(&self) -> &str {
        PLUGIN_VERSION
    }

    fn schema(&self) -> Option<PluginSchema> {
        let mut schema = grafana_schema();
        super::common::oscal::ensure_category_metadata_fields(&mut schema);
        Some(schema.subsidiary(|capability| matches!(
            capability,
            "grafana.read"
        )))
    }

    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        Ok(StateDiff {
            plugin: PLUGIN_NAME.to_string(),
            actions: vec![],
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: String::new(),
                desired_hash: String::new(),
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
            plugin: PLUGIN_NAME.to_string(),
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
            supports_rollback: false,
            supports_checkpoints: false,
            supports_verification: true,
            atomic_operations: false,
        }
    }
}

// =============================================================================
// PLUGIN EXIT: publish the single PluginSchema contract
// =============================================================================

fn read_method<I: JsonSchema, O: JsonSchema>(
    schema: &mut PluginSchema,
    method: &str,
    subject: &str,
) {
    schema.methods.insert(
        method.to_string(),
        method_decl_from_schemars_with_output::<I, O>(
            method,
            SideEffect::Read,
            true,
            READ_CAPABILITY,
            &format!("obs.service.grafana.{subject}.get@v1"),
        ),
    );
}

pub(crate) fn grafana_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(GrafanaState))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        PLUGIN_NAME,
        PLUGIN_VERSION,
        PLUGIN_DESCRIPTION,
        &root,
    );
    schema.category = PLUGIN_CATEGORY.to_string();
    if let Ok(defaults) = simd_json::serde::to_owned_value(GrafanaPlugin::exemplar_state()) {
        super::schemars_adapter::apply_state_defaults(&mut schema, &defaults);
    }
    schema.display_name = Some(PLUGIN_DISPLAY_NAME.to_string());
    schema.subids.insert(
        "__schema__".to_string(),
        "sch.service.plugin.grafana.schema@v1".to_string(),
    );

    read_method::<api::HealthInput, api::HealthOutput>(&mut schema, "health", "health");
    read_method::<api::SearchInput, api::SearchOutput>(&mut schema, "search", "search");
    read_method::<api::GetDashboardByUidInput, api::GetDashboardByUidOutput>(
        &mut schema,
        "get_dashboard_by_uid",
        "dashboard",
    );
    read_method::<api::GetDashboardTagsInput, api::GetDashboardTagsOutput>(
        &mut schema,
        "get_dashboard_tags",
        "dashboard-tags",
    );
    read_method::<api::ListDatasourcesInput, api::ListDatasourcesOutput>(
        &mut schema,
        "list_datasources",
        "datasources",
    );
    read_method::<api::GetDatasourceByUidInput, api::GetDatasourceByUidOutput>(
        &mut schema,
        "get_datasource_by_uid",
        "datasource",
    );
    read_method::<api::CheckDatasourceHealthInput, api::CheckDatasourceHealthOutput>(
        &mut schema,
        "check_datasource_health",
        "datasource-health",
    );
    read_method::<api::ListFoldersInput, api::ListFoldersOutput>(
        &mut schema,
        "list_folders",
        "folders",
    );
    read_method::<api::GetFolderByUidInput, api::GetFolderByUidOutput>(
        &mut schema,
        "get_folder_by_uid",
        "folder",
    );
    read_method::<api::GetCurrentOrgInput, api::GetCurrentOrgOutput>(
        &mut schema,
        "get_current_org",
        "org",
    );
    read_method::<api::GetAnnotationsInput, api::GetAnnotationsOutput>(
        &mut schema,
        "get_annotations",
        "annotations",
    );
    read_method::<api::ListAlertRulesInput, api::ListAlertRulesOutput>(
        &mut schema,
        "list_alert_rules",
        "alert-rules",
    );

    let granted: Vec<&str> = api::OPERATIONS.iter().map(|op| op.method).collect();
    schema.capabilities.insert(
        READ_CAPABILITY.to_string(),
        op_state_store::CapabilityDecl {
            id: READ_CAPABILITY.to_string(),
            description: format!("Grants: {}.", granted.join(", ")),
        },
    );
    schema.public_state.paths = vec![
        "/endpoint".to_string(),
        "/version".to_string(),
        "/commit".to_string(),
        "/database".to_string(),
    ];
    schema
}

inventory::submit! {
    crate::default_registry::PluginReg::new(PLUGIN_NAME, |_ctx| std::sync::Arc::new(GrafanaPlugin::new()))
}
