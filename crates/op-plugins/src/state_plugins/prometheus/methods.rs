//! Prometheus HTTP API v1 method adapter for the `prometheus` plugin.
//!
//! Endpoint resolution, bounds and one dispatch arm per generated
//! operation. Inputs and outputs are the generated `api_types`; every call
//! is bounded and failures are typed [`UpstreamCallError`]s.

use super::api_types as api;
use super::{PLUGIN_NAME, PrometheusState};
use crate::state_plugins::openapi_http::{UpstreamCallError, UpstreamEndpoint, call_typed};
use anyhow::Result;
use std::time::Duration;

const DEFAULT_PROMETHEUS_URL: &str = "http://127.0.0.1:9090";
const API_PREFIX: &str = "/api/v1";

/// Whole-call bound (connect, headers and body).
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// PromQL evaluation timeout sent when the caller gives none (below REQUEST_TIMEOUT).
pub const DEFAULT_QUERY_TIMEOUT: &str = "25s";
/// Largest accepted response body.
pub const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
/// Most series / label names / label values / metric names one call may return.
pub const MAX_SERIES: usize = 10_000;
/// Most samples (float + histogram points) one range query may return.
pub const MAX_POINTS: usize = 250_000;

/// Methods whose upstream operation accepts a `limit` parameter.
const LIMITED_METHODS: &[&str] = &[
    "query",
    "query_range",
    "series",
    "labels",
    "label_values",
    "metadata",
];

fn endpoint() -> Result<UpstreamEndpoint, UpstreamCallError> {
    endpoint_with(&prometheus_url(), REQUEST_TIMEOUT, MAX_RESPONSE_BYTES)
}

fn endpoint_with(
    base: &str,
    timeout: Duration,
    max_response_bytes: usize,
) -> Result<UpstreamEndpoint, UpstreamCallError> {
    UpstreamEndpoint::new(
        PLUGIN_NAME,
        base,
        API_PREFIX,
        None,
        timeout,
        max_response_bytes,
    )
}

pub(super) fn prometheus_url() -> String {
    std::env::var("PROMETHEUS_URL")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_PROMETHEUS_URL.to_string())
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

// =============================================================================
// BOUNDS
// =============================================================================

/// Clamp `limit` into `1..=MAX_SERIES` (upstream treats 0/absent as unlimited)
/// and give PromQL evaluation a timeout below the request bound.
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
            MAX_SERIES as i64
        } else {
            requested.min(MAX_SERIES as i64)
        };
        map.insert("limit".into(), clamped.into());
    }
    if matches!(method, "query" | "query_range") && map.get("timeout").is_none_or(|v| v.is_null()) {
        map.insert("timeout".into(), DEFAULT_QUERY_TIMEOUT.into());
    }
}

fn limit_error(unit: &'static str, limit: usize, observed: usize) -> UpstreamCallError {
    UpstreamCallError::ResultLimit {
        service: PLUGIN_NAME,
        unit,
        limit,
        observed,
    }
}

/// Enforce series/point bounds on the raw body before typed decoding.
pub fn enforce_result_limits(
    method: &str,
    body: &serde_json::Value,
) -> Result<(), UpstreamCallError> {
    let data = &body["data"];
    match method {
        "query" | "query_range" => {
            let Some(result) = data["result"].as_array() else {
                return Ok(());
            };
            if data["resultType"] == "scalar" || data["resultType"] == "string" {
                return Ok(());
            }
            if result.len() > MAX_SERIES {
                return Err(limit_error("series", MAX_SERIES, result.len()));
            }
            let points: usize = result
                .iter()
                .map(|series| {
                    ["values", "histograms"]
                        .iter()
                        .filter_map(|key| series[*key].as_array())
                        .map(Vec::len)
                        .sum::<usize>()
                        .max(1)
                })
                .sum();
            if points > MAX_POINTS {
                return Err(limit_error("points", MAX_POINTS, points));
            }
        }
        "series" | "labels" | "label_values" => {
            if let Some(items) = data.as_array().filter(|items| items.len() > MAX_SERIES) {
                return Err(limit_error("items", MAX_SERIES, items.len()));
            }
        }
        "metadata" => {
            if let Some(metrics) = data.as_object().filter(|m| m.len() > MAX_SERIES) {
                return Err(limit_error("metrics", MAX_SERIES, metrics.len()));
            }
        }
        _ => {}
    }
    Ok(())
}

/// Type an upstream error body with the generated `Error` schema.
fn typed_api_error(error: UpstreamCallError) -> UpstreamCallError {
    match error {
        UpstreamCallError::Upstream {
            service,
            status,
            body,
        } => match serde_json::from_value::<api::Error>(body.clone()) {
            Ok(api_error) => UpstreamCallError::Api {
                service,
                status,
                error_type: api_error.error_type,
                message: api_error.error,
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

// =============================================================================
// DISPATCH: one arm per generated operation
// =============================================================================

pub async fn dispatch_prometheus_method(
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
                |body| enforce_result_limits(method, body),
            )
            .await
        };
    }
    let result = match method {
        "query" => typed!(api::QueryInput, api::QueryOutput),
        "query_range" => typed!(api::QueryRangeInput, api::QueryRangeOutput),
        "series" => typed!(api::SeriesInput, api::SeriesOutput),
        "labels" => typed!(api::LabelsInput, api::LabelsOutput),
        "label_values" => typed!(api::LabelValuesInput, api::LabelValuesOutput),
        "targets" => typed!(api::TargetsInput, api::TargetsOutput),
        "scrape_pools" => typed!(api::ScrapePoolsInput, api::ScrapePoolsOutput),
        "rules" => typed!(api::RulesInput, api::RulesOutput),
        "alerts" => typed!(api::AlertsInput, api::AlertsOutput),
        "metadata" => typed!(api::MetadataInput, api::MetadataOutput),
        "build_info" => typed!(api::BuildInfoInput, api::BuildInfoOutput),
        "runtime_info" => typed!(api::RuntimeInfoInput, api::RuntimeInfoOutput),
        "tsdb_status" => typed!(api::TsdbStatusInput, api::TsdbStatusOutput),
        _ => Err(UpstreamCallError::UnknownMethod {
            service: PLUGIN_NAME,
            method: method.to_string(),
        }),
    };
    result.map_err(typed_api_error)
}

/// Read the live server summary for publication into the SHM projection.
pub async fn current_state() -> Result<PrometheusState> {
    Ok(current_state_with(&endpoint()?).await?)
}

async fn current_state_with(
    endpoint: &UpstreamEndpoint,
) -> Result<PrometheusState, UpstreamCallError> {
    let empty = serde_json::json!({});
    let build: api::BuildInfoOutput = decode(
        dispatch_with_endpoint(endpoint, "build_info", &empty).await?,
        "BuildInfoOutput",
    )?;
    let runtime: api::RuntimeInfoOutput = decode(
        dispatch_with_endpoint(endpoint, "runtime_info", &empty).await?,
        "RuntimeInfoOutput",
    )?;
    let targets: api::TargetsOutput = decode(
        dispatch_with_endpoint(endpoint, "targets", &serde_json::json!({"state": "active"}))
            .await?,
        "TargetsOutput",
    )?;
    Ok(summarize(endpoint.base_url(), &build, &runtime, &targets))
}

fn decode<T: serde::de::DeserializeOwned>(
    value: serde_json::Value,
    output: &'static str,
) -> Result<T, UpstreamCallError> {
    serde_json::from_value(value).map_err(|error| UpstreamCallError::Contract {
        service: PLUGIN_NAME,
        output,
        message: error.to_string(),
    })
}

fn summarize(
    endpoint: &str,
    build: &api::BuildInfoOutput,
    runtime: &api::RuntimeInfoOutput,
    targets: &api::TargetsOutput,
) -> PrometheusState {
    let active = &targets.data.active_targets;
    let count = |health: &str| active.iter().filter(|t| t.health == health).count() as u64;
    let (up, down) = (count("up"), count("down"));
    PrometheusState {
        endpoint: endpoint
            .trim_end_matches('/')
            .trim_end_matches(API_PREFIX)
            .to_string(),
        version: Some(build.data.version.clone()),
        revision: Some(build.data.revision.clone()),
        start_time: Some(runtime.data.start_time.clone()),
        storage_retention: Some(runtime.data.storage_retention.clone()),
        reload_config_success: Some(runtime.data.reload_config_success),
        targets_total: active.len() as u64,
        targets_up: up,
        targets_down: down,
        targets_unknown: active.len() as u64 - up - down,
    }
}

#[cfg(test)]
#[path = "native_tests.rs"]
mod native_prometheus_tests;
