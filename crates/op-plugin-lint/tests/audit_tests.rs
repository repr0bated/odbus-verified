use op_plugin_lint::{
    audit_source, audit_source_with_coverage, resolve_introspect_target, CoverageInputs, Severity,
};
use std::path::PathBuf;

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read_to_string(path).expect("fixture readable")
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn good_plugin_has_no_failures() {
    let report = audit_source("good_plugin.rs", &fixture("good_plugin.rs")).unwrap();
    let fails: Vec<_> = report
        .findings
        .iter()
        .filter(|f| f.severity == Severity::Fail)
        .collect();
    assert!(
        fails.is_empty(),
        "unexpected FAILs on good fixture:\n{}",
        report.to_markdown()
    );
    assert!(report.ok());
}

#[test]
fn bad_plugin_flags_core_contract_gaps() {
    let report = audit_source("bad_plugin.rs", &fixture("bad_plugin.rs")).unwrap();
    assert!(!report.ok());

    let codes: Vec<&str> = report.findings.iter().map(|f| f.code.as_str()).collect();
    // Must catch the high-signal gaps from PLUGIN-RENDER-CONTRACT.md
    for required in [
        "missing_x_oscal_category",
        "missing_field_subid",
        "missing_inventory_submit",
        "deprecated_method_helper",
        "invalid_subid",
    ] {
        assert!(
            codes.contains(&required),
            "expected code `{required}` in findings {codes:?}\n{}",
            report.to_markdown()
        );
    }
}

#[test]
fn bad_plugin_hints_port_range_enhancement() {
    let report = audit_source("bad_plugin.rs", &fixture("bad_plugin.rs")).unwrap();
    let codes: Vec<&str> = report.findings.iter().map(|f| f.code.as_str()).collect();
    assert!(
        codes.contains(&"suggest_port_range"),
        "expected port-range enhancement hint; got {codes:?}"
    );
}

#[test]
fn introspect_file_flags_sdk_elements_missing_from_plugin() {
    let target = fixture_path("demo_instance.json");
    let coverage = resolve_introspect_target(target.to_str().unwrap()).unwrap();
    assert!(coverage.instance_json.is_some());

    let report =
        audit_source_with_coverage("good_plugin.rs", &fixture("good_plugin.rs"), &coverage)
            .unwrap();
    let codes: Vec<&str> = report.findings.iter().map(|f| f.code.as_str()).collect();
    assert!(
        codes.contains(&"gadget_missing_element"),
        "expected missing SDK elements vs good_plugin; got {codes:?}\n{}",
        report.to_markdown()
    );
    let msgs: String = report
        .findings
        .iter()
        .filter(|f| f.code == "gadget_missing_element")
        .map(|f| f.message.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        msgs.contains("sdk_only_surface") || msgs.contains("extra_sdk_field"),
        "expected sdk_only_surface or extra_sdk_field in missing elements:\n{msgs}"
    );
}

#[test]
fn coverage_noop_without_introspect() {
    let report = audit_source_with_coverage(
        "good_plugin.rs",
        &fixture("good_plugin.rs"),
        &CoverageInputs::default(),
    )
    .unwrap();
    assert!(report.ok());
}
