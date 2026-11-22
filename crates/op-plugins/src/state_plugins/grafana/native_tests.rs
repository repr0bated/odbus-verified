use super::super::*;
use super::*;
use crate::state_plugins::common::oscal::validate_subid;
use crate::state_plugins::openapi_http::test_support::{assert_lossless, fixture, serve};
use std::collections::BTreeSet;

fn returns_validator(method: &str) -> jsonschema::Validator {
    let schema = grafana_schema();
    let returns = serde_json::to_value(schema.methods[method].returns.as_ref().unwrap()).unwrap();
    jsonschema::validator_for(&returns).unwrap_or_else(|e| panic!("{method} returns schema: {e}"))
}

#[test]
fn methods_are_exactly_the_generated_operations() {
    let schema = grafana_schema();
    let declared: BTreeSet<&str> = schema.methods.keys().map(String::as_str).collect();
    let generated: BTreeSet<&str> = api::OPERATIONS.iter().map(|op| op.method).collect();
    assert_eq!(declared, generated);
    assert!(api::OPERATIONS.iter().all(|op| op.http_method == "GET"));
    for (name, decl) in &schema.methods {
        assert_eq!(decl.side_effect, SideEffect::Read, "{name}");
        assert_eq!(
            decl.required_capability.as_deref(),
            Some(READ_CAPABILITY),
            "{name}"
        );
        validate_subid(&decl.subid).unwrap_or_else(|e| panic!("{name}: {e}"));
        let args = serde_json::to_value(&decl.args).unwrap();
        jsonschema::validator_for(&args).unwrap_or_else(|e| panic!("{name} args: {e}"));
        let again = serde_json::to_value(&grafana_schema().methods[name].args).unwrap();
        assert_eq!(args, again, "{name} args drifted");
    }
    schema.validate_public_state_policy().unwrap();
    assert_eq!(api::UPSTREAM_TAG, "v13.3.1");
}

#[test]
fn live_fixtures_round_trip_through_generated_types() {
    macro_rules! check {
        ($method:literal, $output:ty, $fixture:literal) => {
            let upstream = fixture("grafana", $fixture);
            assert!(
                returns_validator($method).is_valid(&upstream),
                "{} fails {} returns",
                $fixture,
                $method
            );
            let decoded: $output = serde_json::from_value(upstream.clone())
                .unwrap_or_else(|e| panic!("{}: {e}", $fixture));
            assert_lossless(
                $fixture,
                &upstream,
                &serde_json::to_value(&decoded).unwrap(),
            );
        };
    }
    check!("health", api::HealthOutput, "health");
    check!("search", api::SearchOutput, "search");
    check!(
        "get_dashboard_by_uid",
        api::GetDashboardByUidOutput,
        "dashboard_by_uid"
    );
    check!(
        "get_dashboard_tags",
        api::GetDashboardTagsOutput,
        "dashboard_tags"
    );
    check!(
        "list_datasources",
        api::ListDatasourcesOutput,
        "datasources"
    );
    check!(
        "get_datasource_by_uid",
        api::GetDatasourceByUidOutput,
        "datasource_by_uid"
    );
    check!(
        "check_datasource_health",
        api::CheckDatasourceHealthOutput,
        "datasource_health"
    );
    check!("list_folders", api::ListFoldersOutput, "folders");
    check!(
        "get_folder_by_uid",
        api::GetFolderByUidOutput,
        "folder_by_uid"
    );
    check!("get_current_org", api::GetCurrentOrgOutput, "current_org");
    check!("get_annotations", api::GetAnnotationsOutput, "annotations");
    check!("list_alert_rules", api::ListAlertRulesOutput, "alert_rules");
    let health: api::HealthOutput = serde_json::from_value(fixture("grafana", "health")).unwrap();
    let state = summarize("http://100.96.0.5:3000/api", &health);
    assert_eq!(state.endpoint, "http://100.96.0.5:3000");
    assert_eq!(state.version.as_deref(), Some("13.3.1"));
}

#[tokio::test]
async fn dispatch_encodes_paths_clamps_limits_and_types_errors() {
    let (base, mut seen) = serve(|target| match target.split('?').next().unwrap() {
        "/api/health" => (200, fixture("grafana", "health").to_string()),
        "/api/datasources/uid/prometheus/health" => {
            (200, fixture("grafana", "datasource_health").to_string())
        }
        "/api/search" => (200, fixture("grafana", "search").to_string()),
        _ => (404, fixture("grafana", "error_not_found").to_string()),
    })
    .await;
    let endpoint = endpoint_with(
        &base,
        Some("viewer-token".into()),
        Duration::from_secs(5),
        MAX_RESPONSE_BYTES,
    )
    .unwrap();
    for (method, args) in [
        ("health", serde_json::json!({})),
        (
            "check_datasource_health",
            serde_json::json!({"uid": "prometheus"}),
        ),
        (
            "search",
            serde_json::json!({"query": "node", "limit": 50000}),
        ),
    ] {
        let output = dispatch_with_endpoint(&endpoint, method, &args)
            .await
            .unwrap_or_else(|e| panic!("{method}: {e}"));
        assert!(
            returns_validator(method).is_valid(&output),
            "{method} output"
        );
    }
    let mut targets = Vec::new();
    while let Ok(target) = seen.try_recv() {
        targets.push(target);
    }
    assert!(
        targets
            .iter()
            .any(|t| t.starts_with("/api/search?") && t.contains("limit=1000")),
        "{targets:?}"
    );

    let err = dispatch_with_endpoint(
        &endpoint,
        "get_dashboard_by_uid",
        &serde_json::json!({"uid": "missing"}),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, UpstreamCallError::Api { status: 404, .. }),
        "{err}"
    );
    let err = dispatch_with_endpoint(
        &endpoint,
        "get_folder_by_uid",
        &serde_json::json!({"folder_uid": "../admin"}),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, UpstreamCallError::InvalidInput { .. }),
        "{err}"
    );
    let err = dispatch_with_endpoint(&endpoint, "create_dashboard", &serde_json::json!({}))
        .await
        .unwrap_err();
    assert!(matches!(err, UpstreamCallError::UnknownMethod { .. }));
    assert!(matches!(
        enforce_result_limits(&serde_json::json!(vec![0; MAX_ITEMS + 1])),
        Err(UpstreamCallError::ResultLimit { .. })
    ));
}
