use super::*;
use serde_json::json;

fn context() -> UiAccessContext {
    UiAccessContext {
        principal_id: "principal-a".into(),
        session_id: "session-a".into(),
        session_genesis: None,
    }
}
fn spec(text: &str) -> Value {
    json!({"root":"root","elements":{"root":{"type":"text","props":{"text":text,"className":null},"children":[]}}})
}
fn event_spec() -> Value {
    json!({"root":"tile","state":{"size":"small"},"elements":{"tile":{
        "type":"autoTileItem","props":{"size":"small","contentType":null,"tileId":"tile","className":null},"children":[],
        "on":{"resize":{"action":"setState","params":{"statePath":"/size","value":"large"}}}
    }}})
}
fn renderer(documents: Arc<RuntimeDocuments>, changed: bool) -> UiWorkspaceMethods {
    let mut schema = super::super::super::json_render::json_render_schema();
    if changed {
        schema
            .description
            .push_str("; next authored contract revision");
    }
    let blob = op_blob::blobify_plugin_schema("json_render", schema);
    let bytes = blob.seal();
    let parsed =
        op_blob::BlobRef::from_manifest_pinned(&bytes, &blob.manifest.schema_hash).unwrap();
    UiWorkspaceMethods::from_manifest_pinned_renderer(
        documents,
        Arc::from(parsed.schema_json()),
        blob.manifest.schema_hash.clone(),
    )
    .unwrap()
}
fn documents() -> Arc<RuntimeDocuments> {
    Arc::new(RuntimeDocuments::new(
        op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap(),
    ))
}

#[tokio::test]
async fn production_profile_crud_is_revision_checked_and_does_not_return_siblings() {
    let service = renderer(documents(), false);
    let owner = context();
    let first = service
        .dispatch_mutation(
            &owner,
            "create_ui_spec",
            &json!({"spec":spec("first"),"tags":[]}),
            Some("spec-a"),
        )
        .await
        .unwrap();
    service
        .create_spec(&owner, "spec-b", spec("PRIVATE_SIBLING"), vec![])
        .await
        .unwrap();
    let changed = service.dispatch_mutation(&owner, "update_ui_spec", &json!({"spec_id":"spec-a","expected_revision":1,"element_id":"root","properties":{"text":"changed","className":null}}), None).await.unwrap();
    assert_eq!(changed.projection.specs["spec-a"].revision, 2);
    assert_eq!(
        changed.projection.specs["spec-b"].spec,
        spec("PRIVATE_SIBLING")
    );
    assert!(!changed
        .method_result()
        .unwrap()
        .to_string()
        .contains("PRIVATE_SIBLING"));
    assert!(service
        .update_spec_properties(
            &owner,
            "spec-a",
            1,
            "root",
            json!({"text":"stale","className":null})
        )
        .await
        .is_err());
    let listed = service
        .list_specs(
            &owner,
            UiListSpecsInput {
                after: None,
                limit: 1,
            },
        )
        .await
        .unwrap();
    assert_eq!(listed.specs[0].record.id, "spec-a");
    assert_eq!(listed.next_cursor.as_deref(), Some("spec-a"));
    let deleted = service.delete_spec(&owner, "spec-a", 2).await.unwrap();
    assert!(!deleted.projection.specs.contains_key("spec-a"));
    assert!(deleted.projection.specs.contains_key("spec-b"));
    assert_eq!(first.document_revision, 1);
    assert!(service
        .snapshot(&UiAccessContext {
            principal_id: "principal-b".into(),
            session_id: owner.session_id.clone(),
            session_genesis: None,
        })
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn invalid_active_props_and_forged_owner_fail_before_any_domain_write() {
    let service = renderer(documents(), false);
    let owner = context();
    let mut invalid = spec("value");
    invalid["elements"]["root"]["props"]
        .as_object_mut()
        .unwrap()
        .remove("className");
    assert!(service
        .create_spec(&owner, "spec", invalid, vec![])
        .await
        .is_err());
    assert!(service
        .dispatch_mutation(
            &owner,
            "create_ui_spec",
            &json!({"spec":spec("value"),"principal_id":"other"}),
            Some("spec")
        )
        .await
        .is_err());
    assert!(service.snapshot(&owner).await.unwrap().is_none());
}

#[tokio::test]
async fn pinned_event_binding_commits_exact_state_with_revision_cas() {
    let service = renderer(documents(), false);
    let owner = context();
    service
        .create_spec(&owner, "event", event_spec(), vec![])
        .await
        .unwrap();
    let changed = service
        .dispatch_mutation(
            &owner,
            "handle_ui_event",
            &json!({
                "spec_id":"event","expected_revision":1,"element_id":"tile","event":"resize"
            }),
            None,
        )
        .await
        .unwrap();
    assert_eq!(changed.projection.specs["event"].revision, 2);
    assert_eq!(
        changed.projection.specs["event"].spec["state"]["size"],
        "large"
    );
    let result = changed.method_result().unwrap();
    assert_eq!(result["action"], "setState");
    assert_eq!(result["event"], "resize");
    assert!(service
        .dispatch_mutation(
            &owner,
            "handle_ui_event",
            &json!({
                "spec_id":"event","expected_revision":1,"element_id":"tile","event":"resize"
            }),
            None
        )
        .await
        .is_err());
    assert!(service
        .dispatch_mutation(
            &owner,
            "handle_ui_event",
            &json!({
                "spec_id":"event","expected_revision":2,"element_id":"tile","event":"delete"
            }),
            None
        )
        .await
        .is_err());
    assert_eq!(
        service
            .get_spec(&owner, "event")
            .await
            .unwrap()
            .unwrap()
            .revision,
        2
    );
}

#[tokio::test]
async fn empty_rebind_checks_exact_document_revision_and_never_relabels_artifacts() {
    let documents = documents();
    let old = renderer(documents.clone(), false);
    let current = renderer(documents, true);
    let owner = context();
    old.create_spec(&owner, "spec", spec("retained"), vec![])
        .await
        .unwrap();
    assert!(current
        .rebind_renderer(&owner, 1, old.catalog_hash())
        .await
        .is_err());
    assert_eq!(
        current
            .get_spec(&owner, "spec")
            .await
            .unwrap()
            .unwrap()
            .catalog_hash,
        old.catalog_hash()
    );
    let empty = current.delete_spec(&owner, "spec", 1).await.unwrap();
    assert!(current
        .rebind_renderer(&owner, empty.document_revision - 1, old.catalog_hash())
        .await
        .is_err());
    let (left, right) = tokio::join!(
        current.rebind_renderer(&owner, empty.document_revision, old.catalog_hash()),
        current.rebind_renderer(&owner, empty.document_revision, old.catalog_hash())
    );
    assert_ne!(left.is_ok(), right.is_ok());
    let after = current.recover_snapshot(&owner).await.unwrap().unwrap();
    assert_eq!(after.document_revision, empty.document_revision + 1);
    assert_eq!(after.projection.catalog_hash, current.catalog_hash());
    current
        .create_spec(&owner, "new", spec("new profile"), vec![])
        .await
        .unwrap();
    assert!(old
        .create_spec(&owner, "old", spec("old profile"), vec![])
        .await
        .is_err());
}

#[tokio::test]
async fn history_requires_explicit_terminal_deletion_before_rebind() {
    let documents = documents();
    let old = renderer(documents.clone(), false);
    let current = renderer(documents, true);
    let owner = context();
    old.create_job(&owner, "job", &"a".repeat(64))
        .await
        .unwrap();
    assert!(current.delete_job(&owner, "job", 1).await.is_err());
    let failed = current
        .fail_job(&owner, "job", 1, UiJobFailureCode::CatalogChanged)
        .await
        .unwrap();
    assert!(current
        .rebind_renderer(&owner, failed.document_revision, old.catalog_hash())
        .await
        .is_err());
    let removed = current.delete_job(&owner, "job", 2).await.unwrap();
    assert!(removed.projection.jobs.is_empty());
    let rebound = current
        .rebind_renderer(&owner, removed.document_revision, old.catalog_hash())
        .await
        .unwrap();
    assert!(matches!(
        rebound.receipt,
        UiMutationReceipt::RendererRebound { .. }
    ));
}

#[tokio::test]
async fn provider_independent_record_and_revision_survive_private_store_reopen() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("documents");
    let revision;
    {
        let docs = Arc::new(RuntimeDocuments::new(
            op_cozo_store::CozoGraphShuttle::new_persistent_private(path.clone()).unwrap(),
        ));
        let service = renderer(docs, false);
        revision = service
            .create_spec(&context(), "saved", spec("saved"), vec![])
            .await
            .unwrap()
            .document_revision;
    }
    let docs = Arc::new(RuntimeDocuments::new(
        op_cozo_store::CozoGraphShuttle::new_persistent_private(path).unwrap(),
    ));
    let service = renderer(docs, false);
    let recovered = service.recover_snapshot(&context()).await.unwrap().unwrap();
    assert_eq!(recovered.document_revision, revision);
    assert_eq!(recovered.projection.specs["saved"].spec, spec("saved"));
}

#[test]
fn canonical_workspace_schema_owns_private_children_and_all_method_dtos() {
    let schema = super::super::gemma_brain_schema();
    assert_eq!(schema.version, "4.0.0");
    for field in ["document_revision", "catalog_hash", "jobs", "specs"] {
        assert!(schema.fields.contains_key(field));
    }
    assert!(schema
        .public_state
        .paths
        .iter()
        .all(|path| !path.starts_with("/jobs") && !path.starts_with("/specs")));
    for method in [
        "get_ui_workspace",
        "get_ui_spec",
        "get_ui_job",
        "list_ui_specs",
        "create_ui_spec",
        "update_ui_spec",
        "delete_ui_spec",
        "delete_ui_job",
        "rebind_ui_renderer",
        "recover_ui_workspace",
    ] {
        let declaration = &schema.methods[method];
        assert!(declaration.returns.is_some());
        assert!(schema
            .capabilities
            .contains_key(declaration.required_capability.as_ref().unwrap()));
    }
    let input = serde_json::to_value(&schema.methods["create_ui_spec"].args).unwrap();
    assert!(jsonschema::is_valid(&input, &json!({"spec":spec("valid")})));
    assert!(!jsonschema::is_valid(
        &input,
        &json!({"spec":spec("valid"),"principal_id":"other"})
    ));
}

#[tokio::test]
async fn malformed_private_domain_values_do_not_escape_in_error_chains() {
    let documents = documents();
    let service = renderer(documents.clone(), false);
    let owner = context();
    service
        .create_job(&owner, "job", &"a".repeat(64))
        .await
        .unwrap();
    let id = Owner::new(&owner).unwrap().document_id;
    documents
        .update(&id, |old| {
            let mut value = old.unwrap();
            value["jobs"]["job"]["status"] = json!("PRIVATE_INVALID_VARIANT");
            Ok(value)
        })
        .await
        .unwrap();
    let error = service.recover_snapshot(&owner).await.unwrap_err();
    assert!(!format!("{error:#?} {error:#}").contains("PRIVATE_INVALID_VARIANT"));
}

#[tokio::test]
async fn pure_committed_reads_ignore_newer_durable_rows_and_validate_flat_envelope() {
    let documents = documents();
    let service = renderer(documents.clone(), false);
    let owner = context();
    let first = service
        .create_spec(&owner, "first", spec("committed"), vec![])
        .await
        .unwrap();
    let committed = UiWorkspaceRecovery::from_plugin_state(&first.plugin_state().unwrap()).unwrap();
    assert_eq!(committed.document_revision, first.document_revision);
    assert!(first.plugin_state().unwrap().get("projection").is_none());
    service
        .create_spec(&owner, "later", spec("DURABLE_UNCOMMITTED"), vec![])
        .await
        .unwrap();
    let view = service
        .dispatch_committed_read(
            "list_ui_specs",
            &json!({"limit":1}),
            Some(committed.clone()),
        )
        .unwrap();
    assert_eq!(view["specs"].as_array().unwrap().len(), 1);
    assert_eq!(view["specs"][0]["record"]["id"], "first");
    assert_eq!(view["next_cursor"], Value::Null);
    assert!(view["specs"][0]["render_admitted"].as_bool().unwrap());
    assert!(service
        .dispatch_committed_read(
            "get_ui_spec",
            &json!({"spec_id":"later"}),
            Some(committed.clone())
        )
        .unwrap()["spec"]
        .is_null());
    assert!(service
        .dispatch_committed_read(
            "get_ui_job",
            &json!({"job_id":"absent"}),
            Some(committed.clone())
        )
        .unwrap()["job"]
        .is_null());
    assert!(service
        .dispatch_committed_read("get_ui_workspace", &json!({}), None)
        .unwrap()["workspace"]
        .is_null());
    for (method, args) in [
        (
            "get_ui_spec",
            json!({"spec_id":"first","principal_id":"FORGED"}),
        ),
        ("list_ui_specs", json!({"limit":201})),
        ("get_ui_job", json!({"job_id":"../bad"})),
    ] {
        assert!(service
            .dispatch_committed_read(method, &args, Some(committed.clone()))
            .is_err());
    }
    let next = renderer(documents, true);
    assert!(!next
        .dispatch_committed_read(
            "get_ui_spec",
            &json!({"spec_id":"first"}),
            Some(committed.clone())
        )
        .unwrap()["render_admitted"]
        .as_bool()
        .unwrap());
    let mut forged = first.plugin_state().unwrap();
    forged["principal_id"] = json!("FORGED");
    assert!(UiWorkspaceRecovery::from_plugin_state(&forged).is_err());
    let mut malformed = first.plugin_state().unwrap();
    malformed["document_revision"] = json!(0);
    assert!(UiWorkspaceRecovery::from_plugin_state(&malformed).is_err());
}

#[test]
fn workspace_state_retains_stable_map_keys_with_exact_numeric_nullable_record_contracts() {
    let schema = super::super::gemma_brain_schema();
    for name in ["jobs", "specs"] {
        let op_state_store::FieldType::Map(value) = &schema.fields[name].field_type else {
            panic!("stable typed map lost")
        };
        let op_state_store::FieldType::JsonSchema(value) = value.as_ref() else {
            panic!("exact record contract lost")
        };
        let value = serde_json::to_value(value.as_ref()).unwrap();
        assert_eq!(value["properties"]["revision"]["format"], "uint64");
    }
    let state = UiWorkspaceRecovery {
        document_revision: 1,
        projection: UiWorkspaceProjection {
            catalog_hash: "a".repeat(64),
            jobs: BTreeMap::new(),
            specs: BTreeMap::new(),
            tags: BTreeMap::new(),
            taxonomy_revision: 0,
        },
    }
    .plugin_state()
    .unwrap();
    let state = simd_json::serde::to_owned_value(state).unwrap();
    assert!(schema.validate(&state).valid);
    schema.validate_exact_schema_fields(&state).unwrap();
}
