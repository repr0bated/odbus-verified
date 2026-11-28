//! Grafana HTTP API method adapter for the `grafana` plugin.
//!
//! Endpoint resolution, bounds and one dispatch arm per generated
//! operation. Inputs and outputs are the generated `api_types`; every call
//! is bounded and failures are typed [`UpstreamCallError`]s.

use super::api_types as api;
use super::{GrafanaState, PLUGIN_NAME};
use crate::state_plugins::openapi_http::{UpstreamCallError, UpstreamEndpoint, call_typed};
use anyhow::Result;
use std::time::Duration;

const DEFAULT_GRAFANA_URL: &str = "http://100.96.0.5:3000";
const API_PREFIX: &str = "/api";

/// Whole-call bound (connect, headers and body).
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// Largest accepted response body (a full dashboard model is ~250 KiB).
pub const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
/// Most items (search hits, folders, annotations, rules, data sources) per call.
pub const MAX_ITEMS: usize = 1_000;

/// Methods whose upstream operation accepts a `limit` parameter.
const LIMITED_METHODS: &[&str] = &["search", "list_folders", "get_annotations"];

fn endpoint() -> Result<UpstreamEndpoint, UpstreamCallError> {
    endpoint_with(
        &grafana_url(),
        std::env::var("GRAFANA_API_TOKEN").ok(),
        REQUEST_TIMEOUT,
        MAX_RESPONSE_BYTES,
    )
}

fn endpoint_with(
    base: &str,
    token: Option<String>,
    timeout: Duration,
    max_response_bytes: usize,
) -> Result<UpstreamEndpoint, UpstreamCallError> {
    UpstreamEndpoint::new(
        PLUGIN_NAME,
        base,
        API_PREFIX,
        token,
        timeout,
        max_response_bytes,
    )
}

pub(super) fn grafana_url() -> String {
    std::env::var("GRAFANA_URL")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_GRAFANA_URL.to_string())
}

fn operation(method: &str) -> Result<&'static api::Operation, UpstreamCallError> {
    api::OPERATIONS
        .iter()
        .find(|op| op.method == method)
        .ok_or_else(|| UpstreamCallError::UnknownMethod {
            service: PLUGIN_NAME,
            method: method.to_string(),
        })
}

/// Clamp `limit` into `1..=MAX_ITEMS`.
fn tighten_arguments(method: &str, args: &mut serde_json::Value) {
    let Some(map) = args.as_object_mut() else {
        return;
    };
    if LIMITED_METHODS.contains(&method) {
        let requested = map
            .get("limit")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
        let clamped = if requested <= 0 {
            MAX_ITEMS as i64
        } else {
            requested.min(MAX_ITEMS as i64)
        };
        map.insert("limit".into(), clamped.into());
    }
}

/// Enforce the item bound on list-shaped bodies before typed decoding.
pub fn enforce_result_limits(body: &serde_json::Value) -> Result<(), UpstreamCallError> {
    match body.as_array() {
        Some(items) if items.len() > MAX_ITEMS => Err(UpstreamCallError::ResultLimit {
            service: PLUGIN_NAME,
            unit: "items",
            limit: MAX_ITEMS,
            observed: items.len(),
        }),
        _ => Ok(()),
    }
}

/// Type an upstream error body with the generated `ErrorResponseBody` schema.
fn typed_api_error(error: UpstreamCallError) -> UpstreamCallError {
    match error {
        UpstreamCallError::Upstream {
            service,
            status,
            body,
        } => match serde_json::from_value::<api::ErrorResponseBody>(body.clone()) {
            Ok(api_error) => UpstreamCallError::Api {
                service,
                status,
                error_type: api_error.status.or(api_error.error).unwrap_or_default(),
                message: api_error.message,
            },
            Err(_) => UpstreamCallError::Upstream {
                service,
                status,
                body,
            },
        },
        other => other,
    }
}

pub async fn dispatch_grafana_method(
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    Ok(dispatch_with_endpoint(&endpoint()?, method, args).await?)
}

async fn dispatch_with_endpoint(
    endpoint: &UpstreamEndpoint,
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value, UpstreamCallError> {
    let op = operation(method)?;
    macro_rules! typed {
        ($input:ty, $output:ty) => {
            call_typed::<$input, $output>(
                endpoint,
                method,
                op.path,
                op.output,
                args,
                |typed| tighten_arguments(method, typed),
                enforce_result_limits,
            )
            .await
        };
    }
    let result = match method {
        "health" => typed!(api::HealthInput, api::HealthOutput),
        "search" => typed!(api::SearchInput, api::SearchOutput),
        "get_dashboard_by_uid" => typed!(api::GetDashboardByUidInput, api::GetDashboardByUidOutput),
        "get_dashboard_tags" => typed!(api::GetDashboardTagsInput, api::GetDashboardTagsOutput),
        "list_datasources" => typed!(api::ListDatasourcesInput, api::ListDatasourcesOutput),
        "get_datasource_by_uid" => {
            typed!(api::GetDatasourceByUidInput, api::GetDatasourceByUidOutput)
        }
        "check_datasource_health" => {
            typed!(
                api::CheckDatasourceHealthInput,
                api::CheckDatasourceHealthOutput
            )
        }
        "list_folders" => typed!(api::ListFoldersInput, api::ListFoldersOutput),
        "get_folder_by_uid" => typed!(api::GetFolderByUidInput, api::GetFolderByUidOutput),
        "get_current_org" => typed!(api::GetCurrentOrgInput, api::GetCurrentOrgOutput),
        "get_annotations" => typed!(api::GetAnnotationsInput, api::GetAnnotationsOutput),
        "list_alert_rules" => typed!(api::ListAlertRulesInput, api::ListAlertRulesOutput),
        _ => Err(UpstreamCallError::UnknownMethod {
            service: PLUGIN_NAME,
            method: method.to_string(),
        }),
    };
    result.map_err(typed_api_error)
}

/// Read the live server summary (unauthenticated `/api/health`).
pub async fn current_state() -> Result<GrafanaState> {
    Ok(current_state_with(&endpoint()?).await?)
}

async fn current_state_with(
    endpoint: &UpstreamEndpoint,
) -> Result<GrafanaState, UpstreamCallError> {
    let health = dispatch_with_endpoint(endpoint, "health", &serde_json::json!({})).await?;
    let health: api::HealthOutput =
        serde_json::from_value(health).map_err(|error| UpstreamCallError::Contract {
            service: PLUGIN_NAME,
            output: "HealthOutput",
            message: error.to_string(),
        })?;
    Ok(summarize(endpoint.base_url(), &health))
}

fn summarize(endpoint: &str, health: &api::HealthOutput) -> GrafanaState {
    GrafanaState {
        endpoint: endpoint
            .trim_end_matches('/')
            .trim_end_matches(API_PREFIX)
            .to_string(),
        version: health.version.clone(),
        commit: health.commit.clone(),
        database: health.database.clone(),
    }
}

#[cfg(test)]
#[path = "native_tests.rs"]
mod native_grafana_tests;
