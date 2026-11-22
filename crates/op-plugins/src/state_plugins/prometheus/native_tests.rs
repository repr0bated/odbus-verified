use super::super::*;
use super::*;
use crate::state_plugins::common::oscal::validate_subid;
use crate::state_plugins::openapi_http::test_support::{assert_lossless, fixture, serve};
use std::collections::BTreeSet;

fn returns_validator(method: &str) -> jsonschema::Validator {
    let schema = prometheus_schema();
    let returns = serde_json::to_value(schema.methods[method].returns.as_ref().unwrap()).unwrap();
    jsonschema::validator_for(&returns).unwrap_or_else(|e| panic!("{method} returns schema: {e}"))
}

#[test]
fn methods_are_exactly_the_generated_operations() {
    let schema = prometheus_schema();
    let declared: BTreeSet<&str> = schema.methods.keys().map(String::as_str).collect();
    let generated: BTreeSet<&str> = api::OPERATIONS.iter().map(|op| op.method).collect();
    assert_eq!(declared, generated);
    assert!(api::OPERATIONS.iter().all(|op| op.http_method == "GET"));
    for (name, decl) in &schema.methods {
        assert_eq!(decl.side_effect, SideEffect::Read, "{name}");
        assert!(decl.idempotent, "{name}");
        assert_eq!(
            decl.required_capability.as_deref(),
            Some(READ_CAPABILITY),
            "{name}"
        );
        validate_subid(&decl.subid).unwrap_or_else(|e| panic!("{name}: {e}"));
    }
    assert!(schema.capabilities.contains_key(READ_CAPABILITY));
    schema.validate_public_state_policy().unwrap();
}

#[test]
fn method_schemas_are_valid_json_schema_and_stable() {
    let first = prometheus_schema();
    let second = prometheus_schema();
    for (name, decl) in &first.methods {
        let args = serde_json::to_value(&decl.args).unwrap();
        let returns = serde_json::to_value(decl.returns.as_ref().unwrap()).unwrap();
        jsonschema::validator_for(&args).unwrap_or_else(|e| panic!("{name} args: {e}"));
        jsonschema::validator_for(&returns).unwrap_or_else(|e| panic!("{name} returns: {e}"));
        let again = &second.methods[name];
        assert_eq!(
            args,
            serde_json::to_value(&again.args).unwrap(),
            "{name} args drifted"
        );
        assert_eq!(
            returns,
            serde_json::to_value(again.returns.as_ref().unwrap()).unwrap(),
            "{name} returns drifted"
        );
    }
    // Typed inputs come from the spec's parameters, e.g. query requires `query`.
    let query_args = serde_json::to_value(&first.methods["query"].args).unwrap();
    let validator = jsonschema::validator_for(&query_args).unwrap();
    assert!(validator.is_valid(&serde_json::json!({"query": "up"})));
    assert!(!validator.is_valid(&serde_json::json!({})));
    assert!(!validator.is_valid(&serde_json::json!({"query": "up", "url": "http://elsewhere"})));
    assert_eq!(api::UPSTREAM_TAG, "v3.16.0");
}

/// Live 3.16.0 responses from antix1 decode into the generated types without
/// loss and validate against the published method `returns` schemas.
#[test]
fn live_fixtures_round_trip_through_generated_types() {
    macro_rules! check {
        ($method:literal, $output:ty, [$($fixture:literal),+]) => {$(
            let upstream = fixture("prometheus", $fixture);
            assert!(returns_validator($method).is_valid(&upstream), "{} fails {} returns", $fixture, $method);
            let decoded: $output = serde_json::from_value(upstream.clone())
                .unwrap_or_else(|e| panic!("{}: {e}", $fixture));
            assert_lossless($fixture, &upstream, &serde_json::to_value(&decoded).unwrap());
        )+};
    }
    check!(
        "query",
        api::QueryOutput,
        [
            "query_vector",
            "query_scalar",
            "query_string",
            "query_histogram"
        ]
    );
    check!("query_range", api::QueryRangeOutput, ["query_range"]);
    check!("series", api::SeriesOutput, ["series"]);
    check!("labels", api::LabelsOutput, ["labels"]);
    check!("label_values", api::LabelValuesOutput, ["label_values"]);
    check!("targets", api::TargetsOutput, ["targets"]);
    check!("scrape_pools", api::ScrapePoolsOutput, ["scrape_pools"]);
    check!("rules", api::RulesOutput, ["rules"]);
    check!("alerts", api::AlertsOutput, ["alerts"]);
    check!("metadata", api::MetadataOutput, ["metadata"]);
    check!("build_info", api::BuildInfoOutput, ["build_info"]);
    check!("runtime_info", api::RuntimeInfoOutput, ["runtime_info"]);
    check!("tsdb_status", api::TsdbStatusOutput, ["tsdb_status"]);

    let error: api::Error = serde_json::from_value(fixture("prometheus", "query_error")).unwrap();
    assert_eq!(error.error_type, "bad_data");
}

#[test]
fn state_summary_comes_from_generated_types() {
    let build: api::BuildInfoOutput =
        serde_json::from_value(fixture("prometheus", "build_info")).unwrap();
    let runtime: api::RuntimeInfoOutput =
        serde_json::from_value(fixture("prometheus", "runtime_info")).unwrap();
    let targets: api::TargetsOutput =
        serde_json::from_value(fixture("prometheus", "targets")).unwrap();
    let state = summarize("http://127.0.0.1:9090/api/v1", &build, &runtime, &targets);
    assert_eq!(state.endpoint, "http://127.0.0.1:9090");
    assert_eq!(state.version.as_deref(), Some("3.16.0"));
    assert_eq!(state.targets_total, 4);
    assert_eq!(
        state.targets_up + state.targets_down + state.targets_unknown,
        4
    );
    let schema = prometheus_schema();
    let public = schema
        .project_public_state(&simd_json::serde::to_owned_value(&state).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(public["targets_total"], 4u64);
}

#[test]
fn limits_are_enforced_as_typed_errors() {
    let series = |n: usize, points: usize| {
        serde_json::json!({"status": "success", "data": {"resultType": "matrix", "result":
            (0..n).map(|i| serde_json::json!({"metric": {"i": i.to_string()},
                "values": vec![serde_json::json!([1, "1"]); points]})).collect::<Vec<_>>()}})
    };
    assert!(enforce_result_limits("query_range", &series(10, 10)).is_ok());
    assert!(matches!(
        enforce_result_limits("query_range", &series(MAX_SERIES + 1, 1)),
        Err(UpstreamCallError::ResultLimit { unit: "series", .. })
    ));
    assert!(matches!(
        enforce_result_limits("query_range", &series(2, MAX_POINTS / 2 + 1)),
        Err(UpstreamCallError::ResultLimit { unit: "points", .. })
    ));
    let labels = serde_json::json!({"status": "success", "data": vec!["x"; MAX_SERIES + 1]});
    assert!(matches!(
        enforce_result_limits("labels", &labels),
        Err(UpstreamCallError::ResultLimit { unit: "items", .. })
    ));

    let mut args = serde_json::json!({"query": "up"});
    tighten_arguments("query", &mut args);
    assert_eq!(args["limit"], MAX_SERIES as i64);
    assert_eq!(args["timeout"], DEFAULT_QUERY_TIMEOUT);
    let mut args = serde_json::json!({"query": "up", "limit": 5, "timeout": "1s"});
    tighten_arguments("query", &mut args);
    assert_eq!(args["limit"], 5);
    assert_eq!(args["timeout"], "1s");
    let mut args = serde_json::json!({"limit": 0});
    tighten_arguments("labels", &mut args);
    assert_eq!(args["limit"], MAX_SERIES as i64);
}

#[tokio::test]
async fn response_size_and_timeout_bounds_are_typed() {
    let (base, _) = serve(|target| {
        if target.contains("/status/buildinfo") {
            (
                200,
                format!(
                    "{{\"status\":\"success\",\"data\":\"{}\"}}",
                    "x".repeat(4096)
                ),
            )
        } else {
            (0, String::new()) // never answers
        }
    })
    .await;
    let small = endpoint_with(&base, Duration::from_millis(300), 1024).unwrap();
    let err = dispatch_with_endpoint(&small, "build_info", &serde_json::json!({}))
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            UpstreamCallError::ResponseTooLarge {
                limit_bytes: 1024,
                ..
            }
        ),
        "{err}"
    );
    let err = dispatch_with_endpoint(&small, "alerts", &serde_json::json!({}))
        .await
        .unwrap_err();
    assert!(matches!(err, UpstreamCallError::Timeout { .. }), "{err}");
}

#[tokio::test]
async fn every_operation_dispatches_typed_and_encodes_parameters() {
    let (base, mut seen) = serve(|target| {
        let path = target.split('?').next().unwrap();
        let name = match path {
            "/api/v1/query" => "query_vector",
            "/api/v1/query_range" => "query_range",
            "/api/v1/series" => "series",
            "/api/v1/labels" => "labels",
            "/api/v1/label/job/values" => "label_values",
            "/api/v1/targets" => "targets",
            "/api/v1/scrape_pools" => "scrape_pools",
            "/api/v1/rules" => "rules",
            "/api/v1/alerts" => "alerts",
            "/api/v1/metadata" => "metadata",
            "/api/v1/status/buildinfo" => "build_info",
            "/api/v1/status/runtimeinfo" => "runtime_info",
            "/api/v1/status/tsdb" => "tsdb_status",
            _ => return (404, "{}".into()),
        };
        (200, fixture("prometheus", name).to_string())
    })
    .await;
    let endpoint = endpoint_with(&base, Duration::from_secs(5), MAX_RESPONSE_BYTES).unwrap();
    let args = |method: &str| match method {
        "query" => serde_json::json!({"query": "up"}),
        "query_range" => {
            serde_json::json!({"query": "node_load1", "start": 1, "end": 61, "step": "15s"})
        }
        "series" => serde_json::json!({"match[]": ["up", "node_load1"]}),
        "label_values" => serde_json::json!({"name": "job"}),
        _ => serde_json::json!({}),
    };
    for op in api::OPERATIONS {
        let output = dispatch_with_endpoint(&endpoint, op.method, &args(op.method))
            .await
            .unwrap_or_else(|e| panic!("{}: {e}", op.method));
        assert!(
            returns_validator(op.method).is_valid(&output),
            "{} output",
            op.method
        );
    }
    let mut targets = Vec::new();
    while let Ok(target) = seen.try_recv() {
        targets.push(target);
    }
    assert!(
        targets.iter().any(|t| t.starts_with("/api/v1/series?")
            && t.contains("match%5B%5D=up&match%5B%5D=node_load1"))
    );
    assert!(
        targets
            .iter()
            .any(|t| t.starts_with("/api/v1/query?") && t.contains("timeout=25s"))
    );

    let unknown = dispatch_with_endpoint(&endpoint, "delete_series", &serde_json::json!({})).await;
    assert!(matches!(
        unknown,
        Err(UpstreamCallError::UnknownMethod { .. })
    ));
    let traversal = dispatch_with_endpoint(
        &endpoint,
        "label_values",
        &serde_json::json!({"name": ".."}),
    )
    .await;
    assert!(matches!(
        traversal,
        Err(UpstreamCallError::InvalidInput { .. })
    ));
    let smuggled = dispatch_with_endpoint(
        &endpoint,
        "query",
        &serde_json::json!({"query": "up", "base": "http://x"}),
    )
    .await;
    assert!(matches!(
        smuggled,
        Err(UpstreamCallError::InvalidInput { .. })
    ));
}

#[tokio::test]
async fn upstream_errors_are_typed_with_the_generated_error_schema() {
    let (base, _) = serve(|_| (400, fixture("prometheus", "query_error").to_string())).await;
    let endpoint = endpoint_with(&base, Duration::from_secs(5), MAX_RESPONSE_BYTES).unwrap();
    let err = dispatch_with_endpoint(&endpoint, "query", &serde_json::json!({"query": "sum(("}))
        .await
        .unwrap_err();
    match err {
        UpstreamCallError::Api {
            status, error_type, ..
        } => {
            assert_eq!(status, 400);
            assert_eq!(error_type, "bad_data");
        }
        other => panic!("{other}"),
    }
}

#[test]
fn endpoint_rejects_credentials_query_and_fragment() {
    for bad in [
        "http://user:pass@127.0.0.1:9090",
        "http://127.0.0.1:9090/?x=1",
        "http://127.0.0.1:9090/#f",
        "file:///etc/passwd",
    ] {
        assert!(
            endpoint_with(bad, REQUEST_TIMEOUT, MAX_RESPONSE_BYTES).is_err(),
            "{bad}"
        );
    }
    let ok = endpoint_with(DEFAULT_PROMETHEUS_URL, REQUEST_TIMEOUT, MAX_RESPONSE_BYTES).unwrap();
    assert_eq!(ok.base_url(), "http://127.0.0.1:9090/api/v1");
}
