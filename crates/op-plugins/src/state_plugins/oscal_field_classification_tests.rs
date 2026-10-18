//! Disposable source fixtures only. Owner deferred execution until deployment.
use super::*;
use serde_json::json;

fn pin() -> FieldSourcePin {
    FieldSourcePin {
        plugin_id: "fixture".into(),
        schema_version: "1.0.0".into(),
        source: FieldSource::Candidate {
            candidate_digest: "a".repeat(64),
            producer_id: "fixture-producer".into(),
            producer_digest: "b".repeat(64),
        },
    }
}

fn schema(args: Value) -> Value {
    json!({"name":"fixture","version":"1.0.0","fields":{},"methods":{
        "canonical_key":{"name":"TransportName","args":args,"returns":{"type":"integer"}}
    },"signals":[{"name":"Changed","payload":{"type":"integer"}}],
    "example":{"private-default-key":"must-not-appear"},"description":"private-doc-must-not-appear"})
}

fn input_root() -> FieldRoot {
    FieldRoot::MethodInput {
        method_key: "canonical_key".into(),
    }
}

fn rule(root: FieldRoot, class_id: &str, include_map_keys: bool) -> FieldRule {
    FieldRule {
        id: "fixture-rule".into(),
        revision: 1,
        source: pin(),
        selector: (root != FieldRoot::DocumentMetadata).then(Vec::new),
        root,
        schema_pointer: String::new(),
        scope: FieldRuleScope::Subtree,
        include_map_keys,
        class_id: class_id.into(),
    }
}

fn rules(bindings: Vec<FieldRule>) -> ReviewedFieldRules {
    ReviewedFieldRules {
        revision: 1,
        vocabulary_revision: 1,
        classes: vec![
            FieldClass {
                id: "reviewed-ordinary".into(),
                revision: 1,
                minimum_protection: ProtectionFloor::None,
            },
            FieldClass {
                id: "reviewed-private".into(),
                revision: 1,
                minimum_protection: ProtectionFloor::Private,
            },
            FieldClass {
                id: "reviewed-secret".into(),
                revision: 1,
                minimum_protection: ProtectionFloor::Secret,
            },
        ],
        rules: bindings,
    }
}

fn run(schema: Value, rules: Option<&ReviewedFieldRules>) -> FieldAssessment {
    assess_candidate(pin(), &schema.to_string(), rules.map(|r| (r, 1))).unwrap()
}

#[test]
fn absent_authority_does_not_bootstrap_or_leak_metadata() {
    let report = run(schema(json!({"type":"integer"})), None);
    assert_eq!(report.authority, FieldAuthority::Unavailable);
    assert!(report.traversal_complete);
    assert!(!report.boundary_coverage_complete);
    assert!(!report.interior_inspection_complete);
    assert!(!report.grants_disclosure);
    assert!(report.roots.contains(&FieldRoot::DocumentMetadata));
    let output = serde_json::to_string(&report).unwrap();
    assert!(!output.contains("must-not-appear"));
    assert!(!output.contains("private-default-key"));
}

#[test]
fn map_values_cannot_implicitly_classify_dynamic_keys() {
    let contract = schema(json!({"type":"object","additionalProperties":{"type":"integer"}}));
    let policy = rules(vec![rule(input_root(), "reviewed-private", false)]);
    let report = run(contract.clone(), Some(&policy));
    let keys = report
        .findings
        .iter()
        .find(|f| f.root == input_root() && f.selector == Some(vec![PayloadStep::EachMapKey]))
        .unwrap();
    assert_eq!(keys.resolution, FieldResolution::Uncovered);
    let values = report
        .findings
        .iter()
        .find(|f| f.root == input_root() && f.selector == Some(vec![PayloadStep::EachMapValue]))
        .unwrap();
    assert_eq!(values.resolution, FieldResolution::Resolved);
    let policy = rules(vec![rule(input_root(), "reviewed-private", true)]);
    let report = run(contract, Some(&policy));
    assert_eq!(
        report
            .findings
            .iter()
            .find(|f| f.root == input_root() && f.selector == Some(vec![PayloadStep::EachMapKey]))
            .unwrap()
            .resolution,
        FieldResolution::RestrictedOpaque
    );
}

#[test]
fn inherited_coverage_is_root_specific_and_keeps_canonical_method_keys() {
    let policy = rules(vec![rule(input_root(), "reviewed-private", true)]);
    let report = run(
        schema(json!({"type":"array","items":{"type":["integer","null"]}})),
        Some(&policy),
    );
    assert!(
        report
            .findings
            .iter()
            .filter(|f| f.root == input_root())
            .all(|f| f.resolution == FieldResolution::Resolved)
    );
    assert!(
        report
            .findings
            .iter()
            .filter(|f| f.root != input_root())
            .all(|f| f.resolution == FieldResolution::Uncovered)
    );
    assert!(
        !serde_json::to_string(&report)
            .unwrap()
            .contains("TransportName")
    );
}

#[test]
fn local_reference_carries_reference_site_protection_and_escaped_selector() {
    let contract = schema(json!({"type":"object","additionalProperties":false,
        "properties":{"a/b~c":{"$ref":"#/$defs/value","x-secret":true}},
        "$defs":{"value":{"type":"string"}}}));
    let policy = rules(vec![rule(input_root(), "reviewed-ordinary", true)]);
    let report = run(contract, Some(&policy));
    let reference = report
        .findings
        .iter()
        .find(|f| !f.reference_chain.is_empty())
        .unwrap();
    assert_eq!(reference.protection_floor, ProtectionFloor::Secret);
    assert_eq!(reference.resolution, FieldResolution::Conflict);
    assert_eq!(
        reference.selector,
        Some(vec![PayloadStep::Property {
            name: "a/b~c".into()
        }])
    );
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.schema_pointer == "/properties/a~1b~0c")
    );
}

#[test]
fn stale_candidate_and_candidate_as_runtime_source_fail_closed() {
    let mut binding = rule(input_root(), "reviewed-private", true);
    binding.source.source = FieldSource::Candidate {
        candidate_digest: "c".repeat(64),
        producer_id: "fixture-producer".into(),
        producer_digest: "b".repeat(64),
    };
    let policy = rules(vec![binding]);
    let contract = schema(json!({"type":"integer"}));
    let report = run(contract.clone(), Some(&policy));
    assert_eq!(
        report
            .findings
            .iter()
            .find(|f| f.root == input_root())
            .unwrap()
            .resolution,
        FieldResolution::Stale
    );
    assert!(assess_sealed(pin(), &contract.to_string(), Some((&policy, 1))).is_err());
    let mut sealed = pin();
    sealed.source = FieldSource::Sealed {
        catalog_hash: "d".repeat(64),
        schema_hash: "e".repeat(64),
    };
    assert!(assess_candidate(sealed, &contract.to_string(), None).is_err());
}

#[test]
fn opaque_boundary_can_only_resolve_as_restricted_with_unknown_interior() {
    let policy = rules(vec![rule(input_root(), "reviewed-private", true)]);
    let report = run(schema(json!({})), Some(&policy));
    let root = report
        .findings
        .iter()
        .find(|f| f.root == input_root())
        .unwrap();
    assert_eq!(root.resolution, FieldResolution::RestrictedOpaque);
    assert!(root.opaque_interior);
    assert!(!report.interior_inspection_complete);
    let permissive = rules(vec![rule(input_root(), "reviewed-ordinary", true)]);
    assert_eq!(
        run(schema(json!({})), Some(&permissive))
            .findings
            .iter()
            .find(|f| f.root == input_root())
            .unwrap()
            .resolution,
        FieldResolution::Uncovered
    );
}

#[test]
fn unsupported_keywords_and_recursive_refs_never_report_complete() {
    let policy = rules(vec![rule(input_root(), "reviewed-secret", true)]);
    for contract in [
        json!({"type":"object","patternProperties":{"secret.*":{"type":"string"}}}),
        json!({"$ref":"#/$defs/loop","$defs":{"loop":{"$ref":"#/$defs/loop"}}}),
        json!({"$ref":"https://must-not-fetch.invalid/schema"}),
        json!({"type":17}),
        json!({"type":"array","items":17}),
        json!({"enum":[]}),
    ] {
        let report = run(schema(contract), Some(&policy));
        assert!(!report.traversal_complete);
        assert!(!report.boundary_coverage_complete);
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.resolution == FieldResolution::Unsupported)
        );
    }
}

#[test]
fn broad_schema_hits_a_bounded_limit_without_complete_coverage() {
    let properties: serde_json::Map<String, Value> = (0..MAX_NODES + 1)
        .map(|i| (format!("field_{i}"), json!({"type":"integer"})))
        .collect();
    let report = run(
        schema(json!({"type":"object","properties":properties,"additionalProperties":false})),
        None,
    );
    assert!(!report.traversal_complete);
    assert!(!report.limits_hit.is_empty());
    assert!(report.findings.len() <= MAX_NODES);
    assert!(serde_json::to_vec(&report).unwrap().len() <= MAX_REPORT_BYTES);
}

#[test]
fn protection_loss_between_exact_and_finite_representations_is_detected() {
    let mut before = schema(json!({"type":"integer"}));
    before["fields"] =
        json!({"credential":{"field_type":{"json_schema":{"type":"string","x-secret":true}}}});
    let mut after = before.clone();
    after["fields"]["credential"]["field_type"] = json!("string");
    let before = run(before, None);
    let after = run(after, None);
    let losses = protection_losses(&before, &after).unwrap();
    assert!(losses.iter().any(|loss| loss.root == FieldRoot::State
        && loss.selector
            == Some(vec![PayloadStep::Property {
                name: "credential".into()
            }])
        && loss.required_floor == ProtectionFloor::Secret));
}

#[test]
fn conflicting_alternatives_do_not_select_a_permissive_first_branch() {
    let contract = schema(json!({"oneOf":[{"type":"integer"},{"type":"string"}]}));
    let mut one = rule(input_root(), "reviewed-ordinary", true);
    one.id = "ordinary-branch".into();
    one.schema_pointer = "/oneOf/0".into();
    let mut two = rule(input_root(), "reviewed-private", true);
    two.id = "private-branch".into();
    two.schema_pointer = "/oneOf/1".into();
    let policy = rules(vec![one, two]);
    let report = run(contract, Some(&policy));
    assert!(
        report
            .findings
            .iter()
            .filter(|f| f.root == input_root() && !f.branch_chain.is_empty())
            .all(|f| f.resolution == FieldResolution::Conflict)
    );
    assert!(!report.boundary_coverage_complete);
}

#[test]
fn assessment_contract_matches_actual_output_without_untyped_fields() {
    let report = run(schema(json!({"type":"integer"})), None);
    let contract = serde_json::to_value(schemars::schema_for!(FieldAssessment)).unwrap();
    let validator = jsonschema::Validator::new(&contract).unwrap();
    validator
        .validate(&serde_json::to_value(&report).unwrap())
        .unwrap();
    assert!(
        serde_json::from_value::<AssessFieldsInput>(json!({"plugin_id":"fixture","rules":[]}))
            .is_err()
    );
}

#[test]
fn exact_key_rule_requires_explicit_key_scope_and_stale_orphans_are_reported() {
    let contract = schema(json!({"type":"object","additionalProperties":{"type":"integer"}}));
    let mut key = rule(input_root(), "reviewed-private", false);
    key.scope = FieldRuleScope::Exact;
    key.selector = Some(vec![PayloadStep::EachMapKey]);
    let mut stale = rule(input_root(), "reviewed-private", true);
    stale.id = "old-field".into();
    stale.schema_pointer = "/properties/removed".into();
    stale.source.schema_version = "0.9.0".into();
    let report = run(contract, Some(&rules(vec![key, stale])));
    assert_eq!(
        report
            .findings
            .iter()
            .find(|f| f.root == input_root() && f.selector == Some(vec![PayloadStep::EachMapKey]))
            .unwrap()
            .resolution,
        FieldResolution::Uncovered
    );
    assert_eq!(report.rule_issues[0].kind, FieldRuleIssueKind::StaleSource);
    assert!(!report.boundary_coverage_complete);
}

#[test]
fn counting_writer_bounds_escaped_strings_without_allocating_the_report() {
    let value = "\n".repeat(1_000);
    assert_eq!(encoded_size(&value, 2_002), Some(2_002));
    assert_eq!(encoded_size(&value, 2_001), None);
}
