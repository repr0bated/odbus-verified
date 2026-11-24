//! Prometheus HTTP API v1 plugin — read-only.
//!
//! Every method input and output is a type generated from the official
//! Prometheus OpenAPI document pinned at `prometheus/prometheus` tag v3.16.0
//! (`web/api/v1/testdata/openapi_3.1_golden.yaml`, the same document the
//! server serves at `/api/v1/openapi.yaml`). See `prometheus/openapi/` (vendored
//! spec, provenance, selection, errata) and the generated
//! `prometheus/api_types.rs`; nothing here restates an API shape.
//! Reference: <https://prometheus.io/docs/prometheus/latest/querying/api/>.
//!
//! The server root comes from `PROMETHEUS_URL` (deploy-time environment,
//! default `http://127.0.0.1:9090`), never from method input. Calls are bounded
//! by a timeout, a response size, and series/point limits; violations are
//! typed `UpstreamCallError`s (see `methods`).

use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;
use anyhow::Result;
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{PluginSchema, SideEffect};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;

#[path = "prometheus/api_types.rs"]
pub mod api_types;
#[path = "prometheus/methods.rs"]
pub mod methods;

use api_types as api;
pub use methods::{current_state, dispatch_prometheus_method, enforce_result_limits};

// =============================================================================
// PLUGIN ENTRY: identity and typed schema seed
// =============================================================================

const PLUGIN_NAME: &str = "prometheus";
const PLUGIN_VERSION: &str = "1.0.0";
const PLUGIN_CATEGORY: &str = "service";
const PLUGIN_DESCRIPTION: &str = "Prometheus HTTP API v1 (read-only): PromQL queries, series, labels, targets, rules, alerts, metadata and server status";
const PLUGIN_DISPLAY_NAME: &str = "GB.Prometheus";
const READ_CAPABILITY: &str = "prometheus.read";

/// Observed Prometheus server summary. Every value is read from generated
/// response types (`PrometheusVersion`, `RuntimeInfo`, `TargetDiscovery`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.service.plugin.prometheus.schema@v1"))]
#[schemars(extend("x-oscal-category" = "service"))]
pub struct PrometheusState {
    /// Configured server root (`PROMETHEUS_URL`).
    pub endpoint: String,
    /// `status/buildinfo` version.
    #[serde(default)]
    pub version: Option<String>,
    /// `status/buildinfo` revision.
    #[serde(default)]
    pub revision: Option<String>,
    /// `status/runtimeinfo` startTime.
    #[serde(default)]
    pub start_time: Option<String>,
    /// `status/runtimeinfo` storageRetention.
    #[serde(default)]
    pub storage_retention: Option<String>,
    /// `status/runtimeinfo` reloadConfigSuccess.
    #[serde(default)]
    pub reload_config_success: Option<bool>,
    /// Active scrape targets.
    pub targets_total: u64,
    /// Active targets whose health is `up`.
    pub targets_up: u64,
    /// Active targets whose health is `down`.
    pub targets_down: u64,
    /// Active targets whose health is neither `up` nor `down`.
    pub targets_unknown: u64,
}

// =============================================================================
// PLUGIN BODY: upstream-backed behavior only
// =============================================================================

pub struct PrometheusPlugin;

impl Default for PrometheusPlugin {
    fn default() -> Self {
        Self
    }
}

impl PrometheusPlugin {
    pub fn new() -> Self {
        Self
    }

    pub(crate) fn exemplar_state() -> PrometheusState {
        PrometheusState {
            endpoint: methods::prometheus_url(),
            ..PrometheusState::default()
        }
    }
}

#[async_trait]
impl StatePlugin for PrometheusPlugin {
    fn name(&self) -> &str {
        PLUGIN_NAME
    }

    fn version(&self) -> &str {
        PLUGIN_VERSION
    }

    fn schema(&self) -> Option<PluginSchema> {
        let mut schema = prometheus_schema();
        super::common::oscal::ensure_category_metadata_fields(&mut schema);
        Some(schema.subsidiary(|capability| matches!(
            capability,
            "prometheus.read"
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
            &format!("obs.service.prometheus.{subject}.get@v1"),
        ),
    );
}

pub(crate) fn prometheus_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(PrometheusState))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        PLUGIN_NAME,
        PLUGIN_VERSION,
        PLUGIN_DESCRIPTION,
        &root,
    );
    schema.category = PLUGIN_CATEGORY.to_string();
    if let Ok(defaults) = simd_json::serde::to_owned_value(PrometheusPlugin::exemplar_state()) {
        super::schemars_adapter::apply_state_defaults(&mut schema, &defaults);
    }
    schema.display_name = Some(PLUGIN_DISPLAY_NAME.to_string());
    schema.subids.insert(
        "__schema__".to_string(),
        "sch.service.plugin.prometheus.schema@v1".to_string(),
    );

    read_method::<api::QueryInput, api::QueryOutput>(&mut schema, "query", "query");
    read_method::<api::QueryRangeInput, api::QueryRangeOutput>(
        &mut schema,
        "query_range",
        "query-range",
    );
    read_method::<api::SeriesInput, api::SeriesOutput>(&mut schema, "series", "series");
    read_method::<api::LabelsInput, api::LabelsOutput>(&mut schema, "labels", "labels");
    read_method::<api::LabelValuesInput, api::LabelValuesOutput>(
        &mut schema,
        "label_values",
        "label-values",
    );
    read_method::<api::TargetsInput, api::TargetsOutput>(&mut schema, "targets", "targets");
    read_method::<api::ScrapePoolsInput, api::ScrapePoolsOutput>(
        &mut schema,
        "scrape_pools",
        "scrape-pools",
    );
    read_method::<api::RulesInput, api::RulesOutput>(&mut schema, "rules", "rules");
    read_method::<api::AlertsInput, api::AlertsOutput>(&mut schema, "alerts", "alerts");
    read_method::<api::MetadataInput, api::MetadataOutput>(&mut schema, "metadata", "metadata");
    read_method::<api::BuildInfoInput, api::BuildInfoOutput>(
        &mut schema,
        "build_info",
        "build-info",
    );
    read_method::<api::RuntimeInfoInput, api::RuntimeInfoOutput>(
        &mut schema,
        "runtime_info",
        "runtime-info",
    );
    read_method::<api::TsdbStatusInput, api::TsdbStatusOutput>(
        &mut schema,
        "tsdb_status",
        "tsdb-status",
    );

    let granted: Vec<&str> = api::OPERATIONS.iter().map(|op| op.method).collect();
    schema.capabilities.insert(
        READ_CAPABILITY.to_string(),
        op_state_store::CapabilityDecl {
            id: READ_CAPABILITY.to_string(),
            description: format!("Grants: {}.", granted.join(", ")),
        },
    );

    // Shared readers see only the server summary; query results stay with
    // the caller that holds the read capability.
    schema.public_state.paths = vec![
        "/endpoint".to_string(),
        "/version".to_string(),
        "/revision".to_string(),
        "/start_time".to_string(),
        "/storage_retention".to_string(),
        "/reload_config_success".to_string(),
        "/targets_total".to_string(),
        "/targets_up".to_string(),
        "/targets_down".to_string(),
        "/targets_unknown".to_string(),
    ];

    schema
}

inventory::submit! {
    crate::default_registry::PluginReg::new(PLUGIN_NAME, |_ctx| std::sync::Arc::new(PrometheusPlugin::new()))
}
