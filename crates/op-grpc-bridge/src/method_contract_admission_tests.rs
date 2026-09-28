use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

use op_state::StatePlugin;
use op_state_store::{PluginSchema, SideEffect};

const ID: &str = "contract_fixture";
const METHOD: &str = "Write";
const CAPABILITY: &str = "fixture.write";

fn schema() -> PluginSchema {
    PluginSchema::builder(ID)
        .version("2.0.0")
        .method(MethodDecl {
            name: METHOD.into(),
            args: simd_json::json!({
                "type":"object", "properties":{"value":{"type":"string"}},
                "required":["value"], "additionalProperties":false
            }),
            returns: Some(simd_json::json!({
                "type":"object", "properties":{"revision":{"type":"integer", "minimum":0}},
                "required":["revision"], "additionalProperties":false
            })),
            side_effect: SideEffect::Mutation,
            idempotent: false,
            required_capability: Some(CAPABILITY.into()),
            subid: "mut.service.fixture.write@v2".into(),
        })
        .build()
}

fn evidence(schema: &PluginSchema) -> CompiledContracts {
    BTreeMap::from([(
        schema.name.clone(),
        CompiledPluginContract {
            version: schema.version.clone(),
            fields: schema.fields.clone(),
            methods: schema
                .methods
                .iter()
                .map(|(name, declaration)| (name.clone(), declaration.clone()))
                .collect(),
        },
    )])
}

fn seal(dir: &Path, schema: &PluginSchema) -> op_blob::PluginObjectBlob {
    let blob = op_blob::blobify_plugin_schema(&schema.name, schema.clone());
    op_blob::BlobStore::open(dir).unwrap().write(&blob).unwrap();
    blob
}

// The injectable catalog and evidence enter the same pinned-reader/comparison
// path as production. The counter represents the first backend effect.
fn dispatch_fixture(
    dir: &Path,
    evidence: &CompiledContracts,
    capability: Option<&str>,
    effects: &AtomicUsize,
) -> Result<MethodDecl> {
    let admitted = admit_fixture_method(dir, ID, METHOD, capability, evidence)?;
    effects.fetch_add(1, Ordering::SeqCst);
    Ok(admitted)
}

#[test]
fn matching_pinned_contract_admits_exact_declaration_once() {
    let dir = tempfile::tempdir().unwrap();
    let schema = schema();
    seal(dir.path(), &schema);
    let effects = AtomicUsize::new(0);
    let admitted =
        dispatch_fixture(dir.path(), &evidence(&schema), Some(CAPABILITY), &effects).unwrap();
    assert_eq!(admitted, schema.methods[METHOD]);
    assert_eq!(effects.load(Ordering::SeqCst), 1);
}

#[test]
fn every_method_contract_component_and_plugin_version_blocks_before_effects() {
    for defect in [
        "version",
        "name",
        "arguments",
        "old_ack_returns",
        "absent_returns",
        "side_effect",
        "idempotent",
        "required_capability",
        "subid",
        "removed_method",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let compiled_schema = schema();
        let mut sealed_schema = compiled_schema.clone();
        let declaration = sealed_schema.methods.get_mut(METHOD).unwrap();
        match defect {
            "version" => sealed_schema.version = "1.0.0".into(),
            "name" => declaration.name = "PreviousWrite".into(),
            "arguments" => declaration.args = simd_json::json!({"type":"object","properties":{}}),
            "old_ack_returns" => {
                declaration.returns = Some(simd_json::json!({
                    "type":"object", "properties":{"ok":{"type":"boolean"}}
                }))
            }
            "absent_returns" => declaration.returns = None,
            "side_effect" => declaration.side_effect = SideEffect::Read,
            "idempotent" => declaration.idempotent = true,
            "required_capability" => declaration.required_capability = Some("fixture.admin".into()),
            "subid" => declaration.subid = "mut.service.fixture.write@v1".into(),
            "removed_method" => {
                sealed_schema.methods.remove(METHOD);
            }
            _ => unreachable!(),
        }
        seal(dir.path(), &sealed_schema);
        let effects = AtomicUsize::new(0);
        let result = dispatch_fixture(
            dir.path(),
            &evidence(&compiled_schema),
            Some(CAPABILITY),
            &effects,
        );
        assert!(result.is_err(), "accepted {defect}");
        assert_eq!(effects.load(Ordering::SeqCst), 0, "effect after {defect}");
    }
}

#[test]
fn compiled_evidence_cannot_create_missing_or_unpinned_plugins() {
    for absent in [
        "directory",
        "manifest",
        "membership",
        "blob",
        "corrupt_blob",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let schema = schema();
        let catalog = dir.path().join("catalog");
        if absent != "directory" {
            std::fs::create_dir(&catalog).unwrap();
            let blob = if absent == "manifest" {
                let blob = op_blob::blobify_plugin_schema(ID, schema.clone());
                blob.write_to_dir(&catalog).unwrap();
                blob
            } else {
                seal(&catalog, &schema)
            };
            match absent {
                "membership" => {
                    let manifest_path = catalog.join(op_blob::catalog::MANIFEST_FILENAME);
                    let mut manifest: serde_json::Value =
                        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
                    manifest["plugins"] = serde_json::json!({});
                    std::fs::write(manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
                }
                "blob" => std::fs::remove_file(catalog.join(blob.file_name())).unwrap(),
                "corrupt_blob" => {
                    std::fs::write(catalog.join(blob.file_name()), b"corrupt").unwrap()
                }
                _ => {}
            }
        }
        let effects = AtomicUsize::new(0);
        assert!(
            dispatch_fixture(&catalog, &evidence(&schema), Some(CAPABILITY), &effects).is_err()
        );
        assert_eq!(effects.load(Ordering::SeqCst), 0, "effect after {absent}");
        if absent == "directory" {
            assert!(!catalog.exists());
        }
    }
}

#[test]
fn sealed_methods_without_exact_compiled_evidence_are_not_dispatched() {
    let dir = tempfile::tempdir().unwrap();
    let schema = schema();
    seal(dir.path(), &schema);
    for missing in ["plugin", "method", "case_alias"] {
        let mut compiled = evidence(&schema);
        match missing {
            "plugin" => compiled.clear(),
            "method" => compiled.get_mut(ID).unwrap().methods.clear(),
            "case_alias" => {
                let methods = &mut compiled.get_mut(ID).unwrap().methods;
                let method = methods.remove(METHOD).unwrap();
                methods.insert(METHOD.to_ascii_lowercase(), method);
            }
            _ => unreachable!(),
        }
        let effects = AtomicUsize::new(0);
        assert!(dispatch_fixture(dir.path(), &compiled, Some(CAPABILITY), &effects).is_err());
        assert_eq!(effects.load(Ordering::SeqCst), 0, "effect after {missing}");
    }
}

#[test]
fn exercised_capability_must_match_same_current_pin_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let old = schema();
    seal(dir.path(), &old);
    // Model a reseal between outer admission and backend dispatch. The binary
    // already supports the new contract, but the old capability is insufficient.
    let mut current = old.clone();
    current.methods.get_mut(METHOD).unwrap().required_capability = Some("fixture.admin".into());
    seal(dir.path(), &current);
    let compiled = evidence(&current);
    for capability in [
        None,
        Some(CAPABILITY),
        Some("fixture.admin.extra"),
        Some("*"),
    ] {
        let effects = AtomicUsize::new(0);
        let error = dispatch_fixture(dir.path(), &compiled, capability, &effects).unwrap_err();
        assert!(error.to_string().contains("exercised capability"));
        assert_eq!(effects.load(Ordering::SeqCst), 0);
    }
    let effects = AtomicUsize::new(0);
    dispatch_fixture(dir.path(), &compiled, Some("fixture.admin"), &effects).unwrap();
    assert_eq!(effects.load(Ordering::SeqCst), 1);
}

#[test]
fn cron_mutation_rejects_read_grant_before_gateway_effect() {
    let dir = tempfile::tempdir().unwrap();
    let schema = op_plugins::state_plugins::cron::CronPlugin::new()
        .schema()
        .unwrap();
    seal(dir.path(), &schema);
    let effects = AtomicUsize::new(0);
    let result = admit_fixture_method(
        dir.path(),
        "cron",
        "create_job",
        Some("cron.read"),
        &evidence(&schema),
    );
    if result.is_ok() {
        effects.fetch_add(1, Ordering::SeqCst);
    }
    assert!(result.is_err());
    assert_eq!(effects.load(Ordering::SeqCst), 0);
}

#[test]
fn no_capability_is_an_exact_contract_not_an_implicit_grant() {
    let dir = tempfile::tempdir().unwrap();
    let mut schema = schema();
    schema.methods.get_mut(METHOD).unwrap().required_capability = None;
    seal(dir.path(), &schema);
    let compiled = evidence(&schema);
    let effects = AtomicUsize::new(0);
    assert!(dispatch_fixture(dir.path(), &compiled, Some(CAPABILITY), &effects).is_err());
    assert_eq!(effects.load(Ordering::SeqCst), 0);
    dispatch_fixture(dir.path(), &compiled, None, &effects).unwrap();
    assert_eq!(effects.load(Ordering::SeqCst), 1);
}

#[test]
fn non_method_catalog_metadata_does_not_replace_compatibility_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let compiled_schema = schema();
    let mut pinned_schema = compiled_schema.clone();
    pinned_schema.description = "Sealer environment may shape non-method state metadata".into();
    seal(dir.path(), &pinned_schema);
    let effects = AtomicUsize::new(0);
    dispatch_fixture(
        dir.path(),
        &evidence(&compiled_schema),
        Some(CAPABILITY),
        &effects,
    )
    .unwrap();
    assert_eq!(effects.load(Ordering::SeqCst), 1);
}

#[test]
fn generated_artifact_contains_full_declarations_keyed_by_canonical_method() {
    let contracts = compiled_contracts().unwrap();
    assert!(!contracts.is_empty());
    for (id, plugin) in contracts {
        assert!(!id.is_empty());
        assert!(!plugin.version.is_empty());
        assert!(!plugin.methods.is_empty());
        for (canonical_method, declaration) in &plugin.methods {
            assert!(!canonical_method.is_empty());
            assert!(!declaration.name.is_empty());
            assert!(!declaration.subid.is_empty());
        }
    }
    // These contracts distinguish a real backend response from the old Ack.
    assert!(contracts["unix_socket"].methods["bind"].returns.is_some());
    assert!(
        contracts["embedding_model"].methods["embed"]
            .returns
            .is_some()
    );
}

#[test]
fn current_pin_revalidates_constraints_before_effects_without_echoing_inputs() {
    let dir = tempfile::tempdir().unwrap();
    let mut current = schema();
    current.methods.get_mut(METHOD).unwrap().args = simd_json::json!({
        "type":"object", "properties":{
            "value":{"type":"string", "maxLength":4},
            "enabled":{"type":"boolean"},
            "revision":{"type":"integer", "minimum":0}
        },
        "required":["value","enabled","revision"], "additionalProperties":false
    });
    seal(dir.path(), &current);
    let declaration = admit_fixture_method(
        dir.path(),
        ID,
        METHOD,
        Some(CAPABILITY),
        &evidence(&current),
    )
    .unwrap();
    let effects = AtomicUsize::new(0);
    let dispatch = |arguments: serde_json::Value| -> Result<()> {
        validate_admitted_arguments(&declaration, &arguments)?;
        effects.fetch_add(1, Ordering::SeqCst);
        Ok(())
    };
    let secret = "private-provider-credential";
    let error =
        dispatch(serde_json::json!({"value":secret,"enabled":false,"revision":0})).unwrap_err();
    assert!(!error.to_string().contains(secret));
    for arguments in [
        serde_json::json!({"value":"okay","revision":0}),
        serde_json::json!({"value":"okay","enabled":false}),
        serde_json::json!({"value":"okay","enabled":false,"revision":-1}),
        serde_json::json!({"value":"okay","enabled":false,"revision":0,"ignored":true}),
    ] {
        assert!(dispatch(arguments).is_err());
    }
    assert_eq!(effects.load(Ordering::SeqCst), 0);
    dispatch(serde_json::json!({"value":"okay","enabled":false,"revision":0})).unwrap();
    assert_eq!(effects.load(Ordering::SeqCst), 1);
}
