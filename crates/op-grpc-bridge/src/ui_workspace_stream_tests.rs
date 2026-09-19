use super::*;
use crate::{
    grpc_server::OperationGrpcServer,
    human_principal_dispatch::tests::{
        pk, register, temp_cozo, temp_shm_with_schemas, test_engine, TempCozo, TempShm,
    },
    identity_sled_dispatch::{tests::write_identity, IdentitySledStore},
    proto::{state_sync_server::StateSync, ChangeType},
};
use futures::StreamExt;
use op_plugins::state_plugins::{
    gemma_brain::{
        workspace::{UiAccessContext, UiWorkspaceMethods},
        GemmaBrainPlugin,
    },
    human_principal::HumanPrincipalPlugin,
    identity_sled::IdentitySledPlugin,
    json_render::JsonRenderPlugin,
    runtime_documents::RuntimeDocuments,
    web_ui::WebUiPlugin,
};
use op_state::StatePlugin;
use prost::Message;
use serde_json::json;
use tonic::Response;

struct GrantsEnvironment(Option<std::ffi::OsString>);
impl Drop for GrantsEnvironment {
    fn drop(&mut self) {
        match self.0.take() {
            Some(path) => std::env::set_var("OP_GRANTS_PATH", path),
            None => std::env::remove_var("OP_GRANTS_PATH"),
        }
    }
}

struct Fixture {
    engine: Arc<MutationEngine>,
    server: OperationGrpcServer,
    first: HumanPrincipalIdentity,
    second: HumanPrincipalIdentity,
    documents: Arc<RuntimeDocuments>,
    catalog: std::path::PathBuf,
    grants: std::path::PathBuf,
    _environment: GrantsEnvironment,
    _root: tempfile::TempDir,
    _shm: TempShm,
    _cozo: TempCozo,
}

impl Fixture {
    async fn new() -> Self {
        // Reuse the established process-wide principal and SHM guards. No new
        // parallel environment mutex and no caller-selected live paths.
        let cozo = temp_cozo();
        let shm = temp_shm_with_schemas(&[
            HumanPrincipalPlugin::new().schema().unwrap(),
            IdentitySledPlugin::new().schema().unwrap(),
            GemmaBrainPlugin::new().schema().unwrap(),
            JsonRenderPlugin::new().schema().unwrap(),
            WebUiPlugin::new().schema().unwrap(),
            op_plugins::state_plugins::schema_renderer::SchemaRendererPlugin::new()
                .schema()
                .unwrap(),
        ]);
        let engine = test_engine();
        engine
            .install_identity_sled_store_for_test(IdentitySledStore::new(
                op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap(),
            ))
            .unwrap();
        let root = tempfile::tempdir().unwrap();
        let documents = Arc::new(RuntimeDocuments::new(
            op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap(),
        ));
        let private = op_core::projection_shm::PrivateProjectionDirectory::prepare(
            &root.path().join("private"),
        )
        .unwrap();
        let catalog = crate::sealed_schema_reader::blob_catalog_dir_from_env();
        engine
            .configure_ui_workspace(private, documents.clone(), catalog.clone())
            .unwrap();
        let mut identities = Vec::new();
        for (byte, name) in [(0xC1, "workspace-first"), (0xC2, "workspace-second")] {
            let key = pk(byte);
            register(&key, name).await.unwrap();
            let sled = write_identity(&engine, &key).await.unwrap();
            identities.push(HumanPrincipalIdentity {
                principal_id: op_identity::session::derive_principal_id(&key),
                human_pubkey: key,
                session_id: sled.session_id,
                session_genesis: sled.genesis.unwrap(),
                expires_at: 0,
            });
        }
        let grants = root.path().join("grants.json");
        let environment = GrantsEnvironment(std::env::var_os("OP_GRANTS_PATH"));
        std::env::set_var("OP_GRANTS_PATH", &grants);
        let first = identities.remove(0);
        let second = identities.remove(0);
        let fixture = Self {
            server: OperationGrpcServer::new(engine.clone()),
            engine,
            first,
            second,
            documents,
            catalog,
            grants,
            _environment: environment,
            _root: root,
            _shm: shm,
            _cozo: cozo,
        };
        fixture.grant_first(true);
        fixture
    }

    fn grant_first(&self, read: bool) {
        let caps = if read {
            vec!["gemma.read", "gemma.invoke"]
        } else {
            vec!["gemma.invoke"]
        };
        std::fs::write(
            &self.grants,
            serde_json::to_vec(&json!({
                (self.first.principal_id.clone()): {"capabilities":caps},
                (self.second.principal_id.clone()): {"capabilities":["gemma.read", "gemma.invoke"]},
            }))
            .unwrap(),
        )
        .unwrap();
    }

    fn request<T>(
        &self,
        body: T,
        identity: &HumanPrincipalIdentity,
        capability: Option<&str>,
    ) -> Request<T> {
        let mut request = Request::new(body);
        request.extensions_mut().insert(identity.clone());
        if let Some(capability) = capability {
            request
                .metadata_mut()
                .insert(DECLARED_CAPABILITY_HEADER, capability.parse().unwrap());
        }
        request
    }

    fn subscription(
        &self,
        paths: Vec<String>,
        identity: &HumanPrincipalIdentity,
    ) -> Request<SubscribeRequest> {
        self.request(
            SubscribeRequest {
                plugin_ids: vec![PLUGIN.into()],
                path_patterns: paths,
                include_initial_state: true,
                include_schema: false,
                ..Default::default()
            },
            identity,
            Some(UI_WORKSPACE_READ_CAPABILITY),
        )
    }

    async fn mutation(
        &self,
        identity: &HumanPrincipalIdentity,
        method: &str,
        args: Value,
    ) -> Value {
        self.engine
            .dispatch_method_call_with_identity(
                PLUGIN,
                method,
                &args.to_string(),
                Some("gemma.invoke"),
                &identity.principal_id,
                Some(&identity.session_id),
                Some(&identity.session_genesis),
            )
            .await
            .unwrap_or_else(|error| {
                panic!("UI workspace fixture mutation '{method}' failed: {error:#}")
            })["result"]
            .clone()
    }

    async fn activate(&self, identity: &HumanPrincipalIdentity) {
        self.mutation(identity, "recover_ui_workspace", json!({}))
            .await;
    }

    async fn get(
        &self,
        identity: &HumanPrincipalIdentity,
        path: &str,
    ) -> Result<GetStateResponse, Status> {
        self.server
            .get_state(self.request(
                GetStateRequest {
                    plugin_id: PLUGIN.into(),
                    object_path: path.into(),
                },
                identity,
                Some(UI_WORKSPACE_READ_CAPABILITY),
            ))
            .await
            .map(Response::into_inner)
    }

    fn durable_methods(&self) -> UiWorkspaceMethods {
        let renderer = crate::sealed_schema_reader::read_manifest_pinned_schema_document(
            &self.catalog,
            "json_render",
        )
        .unwrap();
        UiWorkspaceMethods::from_manifest_pinned_renderer(
            self.documents.clone(),
            Arc::<str>::from(renderer.blob.schema_json.clone()),
            renderer.schema_hash,
        )
        .unwrap()
    }

    async fn web_ui(
        &self,
        identity: &HumanPrincipalIdentity,
        method: &str,
        args: Value,
    ) -> anyhow::Result<Value> {
        let schema = WebUiPlugin::new().schema().unwrap();
        let declaration = &schema.methods[method];
        let capability = schema.resolve_method_capability(declaration);
        let response = self
            .engine
            .dispatch_method_call_with_identity(
                "web_ui",
                method,
                &args.to_string(),
                Some(capability.capability()),
                &identity.principal_id,
                Some(&identity.session_id),
                Some(&identity.session_genesis),
            )
            .await?;
        let result = response["result"].clone();
        jsonschema::Validator::new(&serde_json::to_value(
            declaration.returns.as_ref().unwrap(),
        )?)
        .map_err(|error| anyhow::anyhow!("invalid web_ui output schema: {error}"))?
        .validate(&result)
        .map_err(|error| anyhow::anyhow!("invalid web_ui result: {error}"))?;
        Ok(result)
    }
}

fn path(suffix: &str) -> String {
    format!("/org/opdbus/v1/plugins/{PLUGIN}/{suffix}")
}

#[tokio::test]
async fn gallery_facades_read_only_the_verified_owners_committed_workspace() {
    let fixture = Fixture::new().await;
    fixture.activate(&fixture.first).await;
    fixture.activate(&fixture.second).await;
    let created = fixture
        .mutation(
            &fixture.first,
            "create_ui_spec",
            json!({
                "spec":canvas("private gallery value"), "tags":[]
            }),
        )
        .await;
    let spec_id = created["spec"]["id"].as_str().unwrap();
    let grants = |first: &[&str], second: &[&str]| {
        std::fs::write(
            &fixture.grants,
            serde_json::to_vec(&json!({
                (fixture.first.principal_id.clone()): {"capabilities":first},
                (fixture.second.principal_id.clone()): {"capabilities":second},
            }))
            .unwrap(),
        )
        .unwrap();
    };
    grants(
        &["schema_renderer.read", "gemma.read"],
        &["schema_renderer.read", "gemma.read"],
    );
    let call = |identity: &HumanPrincipalIdentity, method: &str, args: Value| {
        let engine = fixture.engine.clone();
        let identity = identity.clone();
        let method = method.to_owned();
        async move {
            engine
                .dispatch_method_call_with_identity(
                    "schema_renderer",
                    &method,
                    &args.to_string(),
                    Some("schema_renderer.read"),
                    &identity.principal_id,
                    Some(&identity.session_id),
                    Some(&identity.session_genesis),
                )
                .await
        }
    };
    let listed = call(&fixture.first, "list_galleries", json!({}))
        .await
        .unwrap();
    assert_eq!(listed["result"]["galleries"][0]["rendering_count"], 1);
    let other = call(&fixture.second, "list_galleries", json!({}))
        .await
        .unwrap();
    // No committed workspace means no gallery for this owner.
    assert_eq!(other["result"]["galleries"], json!([]));
    let args = json!({"gallery_id":"workspace", "rendering_id":spec_id});
    let owned = call(&fixture.first, "get_rendering", args.clone())
        .await
        .unwrap();
    assert_eq!(
        owned["result"]["rendering"]["spec"],
        canvas("private gallery value")
    );
    assert_eq!(owned["result"]["render_admitted"], true);
    let other = call(&fixture.second, "get_rendering", args.clone())
        .await
        .unwrap();
    assert_eq!(other["result"]["rendering"], Value::Null);
    assert_eq!(other["result"]["render_admitted"], false);
    grants(&["schema_renderer.read"], &["gemma.read"]);
    assert!(call(&fixture.first, "get_rendering", args.clone())
        .await
        .unwrap_err()
        .to_string()
        .contains("nested Gemma"));
    assert!(call(&fixture.second, "get_rendering", args.clone())
        .await
        .is_err());
    grants(&["schema_renderer.read", "gemma.read"], &[]);
    // A durable-only deletion must not change the gallery's committed SHM view.
    fixture
        .durable_methods()
        .delete_spec(
            &UiAccessContext {
                principal_id: fixture.first.principal_id.clone(),
                session_id: fixture.first.session_id.clone(),
                session_genesis: Some(fixture.first.session_genesis.clone()),
            },
            spec_id,
            1,
        )
        .await
        .unwrap();
    let committed = call(&fixture.first, "get_rendering", args).await.unwrap();
    assert_eq!(committed["result"]["rendering"]["id"], spec_id);
}
fn canvas(text: &str) -> Value {
    json!({"root":"root", "elements":{"root":{"type":"text","props":{"text":text,"className":null},"children":[]}}})
}

#[tokio::test(flavor = "multi_thread")]
async fn web_ui_render_and_update_use_committed_owner_state_and_preserve_siblings() {
    let fixture = Fixture::new().await;
    fixture.activate(&fixture.first).await;
    fixture.activate(&fixture.second).await;
    let created = fixture
        .mutation(
            &fixture.first,
            "create_ui_spec",
            json!({"spec":canvas("first"),"tags":[]}),
        )
        .await;
    let sibling = fixture
        .mutation(
            &fixture.first,
            "create_ui_spec",
            json!({"spec":canvas("sibling stays"),"tags":[]}),
        )
        .await;
    let other = fixture
        .mutation(
            &fixture.second,
            "create_ui_spec",
            json!({"spec":canvas("other owner"),"tags":[]}),
        )
        .await;
    let id = created["spec"]["id"].as_str().unwrap();
    let sibling_id = sibling["spec"]["id"].as_str().unwrap();
    let other_id = other["spec"]["id"].as_str().unwrap();
    let rendered = fixture
        .web_ui(&fixture.first, "render_page", json!({"spec_id":id}))
        .await
        .unwrap();
    assert_eq!(rendered["spec"], created["spec"]);
    assert_eq!(rendered["render_admitted"], true);
    let hidden = fixture
        .web_ui(&fixture.second, "render_page", json!({"spec_id":id}))
        .await
        .unwrap();
    assert_eq!(hidden, json!({"spec":null,"render_admitted":false}));

    let args = json!({"spec_id":id,"expected_revision":1,"element_id":"root",
        "properties":{"text":"changed","className":null}});
    let updated = fixture
        .web_ui(&fixture.first, "update_component", args.clone())
        .await
        .unwrap();
    assert_eq!(updated["spec"]["revision"], 2);
    assert_eq!(updated["spec"]["spec"], canvas("changed"));
    let readback = fixture
        .web_ui(&fixture.first, "render_page", json!({"spec_id":id}))
        .await
        .unwrap();
    assert_eq!(readback["spec"], updated["spec"]);
    assert_eq!(
        fixture
            .web_ui(&fixture.first, "render_page", json!({"spec_id":sibling_id}))
            .await
            .unwrap()["spec"],
        sibling["spec"]
    );
    assert_eq!(
        fixture
            .web_ui(&fixture.second, "render_page", json!({"spec_id":other_id}))
            .await
            .unwrap()["spec"],
        other["spec"]
    );
    let committed = fixture.get(&fixture.first, &path("specs")).await.unwrap();
    let before = json_value(committed.value.as_ref().unwrap());
    assert_eq!(before[id]["spec"], canvas("changed"));
    assert_eq!(before[sibling_id]["spec"], canvas("sibling stays"));
    assert_eq!(before.as_object().unwrap().len(), 2);
    assert!(fixture
        .web_ui(&fixture.first, "update_component", args.clone())
        .await
        .is_err());
    assert!(fixture
        .web_ui(&fixture.second, "update_component", args)
        .await
        .is_err());
    let after = fixture.get(&fixture.first, &path("specs")).await.unwrap();
    assert_eq!(json_value(after.value.as_ref().unwrap()), before);

    // A durable owner write alone is insufficient: render_page must continue
    // to return the last MutationEngine publication, including its revision.
    fixture
        .durable_methods()
        .dispatch_mutation(
            &UiAccessContext {
                principal_id: fixture.first.principal_id.clone(),
                session_id: fixture.first.session_id.clone(),
                session_genesis: Some(fixture.first.session_genesis.clone()),
            },
            "update_ui_spec",
            &json!({"spec_id":id,"expected_revision":2,"element_id":"root",
            "properties":{"text":"durable but unpublished","className":null}}),
            None,
        )
        .await
        .unwrap();
    let still_committed = fixture
        .web_ui(&fixture.first, "render_page", json!({"spec_id":id}))
        .await
        .unwrap();
    assert_eq!(still_committed, readback);
    fixture.grant_first(false);
    assert!(fixture
        .web_ui(&fixture.first, "render_page", json!({"spec_id":id}))
        .await
        .is_err());
    assert!(fixture.engine.get_state("gemma_brain").await.is_none());
}
fn event_canvas() -> Value {
    json!({
        "root":"tile",
        "state":{"size":"small"},
        "elements":{"tile":{
            "type":"autoTileItem",
            "props":{"size":"small","contentType":null,"tileId":"tile","className":null},
            "children":[],
            "on":{"resize":{"action":"setState","params":{"statePath":"/size","value":"large"}}}
        }}
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn web_ui_handle_event_commits_pinned_set_state_to_owners_private_shm() {
    let fixture = Fixture::new().await;
    fixture.activate(&fixture.first).await;
    fixture.activate(&fixture.second).await;
    let created = fixture
        .mutation(
            &fixture.first,
            "create_ui_spec",
            json!({"spec":event_canvas(),"tags":[]}),
        )
        .await;
    let spec_id = created["spec"]["id"].as_str().unwrap().to_owned();
    let arguments = json!({
        "spec_id":spec_id,
        "expected_revision":1,
        "element_id":"tile",
        "event":"resize"
    });
    let schema = WebUiPlugin::new().schema().unwrap();
    let declaration = &schema.methods["handle_event"];
    jsonschema::Validator::new(&serde_json::to_value(&declaration.args).unwrap())
        .unwrap()
        .validate(&arguments)
        .unwrap();

    let response = fixture
        .engine
        .dispatch_method_call_with_identity(
            "web_ui",
            "handle_event",
            &arguments.to_string(),
            Some("gemma.invoke"),
            &fixture.first.principal_id,
            Some(&fixture.first.session_id),
            Some(&fixture.first.session_genesis),
        )
        .await
        .unwrap();
    let result = &response["result"];
    jsonschema::Validator::new(
        &serde_json::to_value(declaration.returns.as_ref().unwrap()).unwrap(),
    )
    .unwrap()
    .validate(result)
    .unwrap();
    assert_eq!(result["action"], "setState");
    assert_eq!(result["event"], "resize");
    assert_eq!(result["spec"]["revision"], 2);
    assert_eq!(result["spec"]["spec"]["state"]["size"], "large");

    // A later durable-only update must not become StateSync authority before
    // the owner publishes it to the private projection.
    let durable_only = fixture
        .durable_methods()
        .handle_event(
            &UiAccessContext {
                principal_id: fixture.first.principal_id.clone(),
                session_id: fixture.first.session_id.clone(),
                session_genesis: Some(fixture.first.session_genesis.clone()),
            },
            &spec_id,
            2,
            "tile",
            "resize",
        )
        .await
        .unwrap();
    assert_eq!(durable_only.document_revision, 3);
    let committed = fixture.get(&fixture.first, &path("specs")).await.unwrap();
    let committed = json_value(committed.value.as_ref().unwrap());
    assert_eq!(committed[spec_id.as_str()]["revision"].as_f64(), Some(2.0));
    assert_eq!(
        committed[spec_id.as_str()]["spec"]["state"]["size"],
        "large"
    );
    assert!(fixture.engine.get_state("gemma_brain").await.is_none());
    assert_eq!(
        fixture
            .get(&fixture.second, &path("specs"))
            .await
            .unwrap_err()
            .code(),
        tonic::Code::NotFound
    );

    let stale = fixture
        .engine
        .dispatch_method_call_with_identity(
            "web_ui",
            "handle_event",
            &arguments.to_string(),
            Some("gemma.invoke"),
            &fixture.first.principal_id,
            Some(&fixture.first.session_id),
            Some(&fixture.first.session_genesis),
        )
        .await
        .unwrap_err();
    assert!(format!("{stale:#}").contains("stale UI spec revision"));
    let mut forged = fixture.first.clone();
    forged.session_id = fixture.second.session_id.clone();
    assert!(fixture
        .engine
        .dispatch_method_call_with_identity(
            "web_ui",
            "handle_event",
            &json!({
                "spec_id":spec_id,
                "expected_revision":2,
                "element_id":"tile",
                "event":"resize"
            })
            .to_string(),
            Some("gemma.invoke"),
            &forged.principal_id,
            Some(&forged.session_id),
            Some(&forged.session_genesis),
        )
        .await
        .is_err());
    let still_committed = fixture.get(&fixture.first, &path("specs")).await.unwrap();
    assert_eq!(
        json_value(still_committed.value.as_ref().unwrap())[spec_id.as_str()]["revision"].as_f64(),
        Some(2.0)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn queued_web_ui_event_rechecks_original_facade_seal_before_effect() {
    let fixture = Fixture::new().await;
    fixture.activate(&fixture.first).await;
    let created = fixture
        .mutation(
            &fixture.first,
            "create_ui_spec",
            json!({"spec":event_canvas(),"tags":[]}),
        )
        .await;
    let spec_id = created["spec"]["id"].as_str().unwrap().to_owned();
    let owner = UiAccessContext {
        principal_id: fixture.first.principal_id.clone(),
        session_id: fixture.first.session_id.clone(),
        session_genesis: Some(fixture.first.session_genesis.clone()),
    }
    .owner_key()
    .unwrap();
    let gate = fixture
        .engine
        .ui_workspace()
        .unwrap()
        .lock_owner_for_test(&owner)
        .await
        .unwrap();
    let engine = fixture.engine.clone();
    let identity = fixture.first.clone();
    let arguments = json!({
        "spec_id":spec_id,
        "expected_revision":1,
        "element_id":"tile",
        "event":"resize"
    })
    .to_string();
    let call = tokio::spawn(async move {
        engine
            .dispatch_method_call_with_identity(
                "web_ui",
                "handle_event",
                &arguments,
                Some("gemma.invoke"),
                &identity.principal_id,
                Some(&identity.session_id),
                Some(&identity.session_genesis),
            )
            .await
    });
    gate.wait_until_queued().await;

    let mut changed = WebUiPlugin::new().schema().unwrap();
    changed
        .description
        .push_str("; deliberately changed after facade admission");
    op_blob::BlobStore::open(&fixture.catalog)
        .unwrap()
        .write(&op_blob::blobify_plugin_schema("web_ui", changed))
        .unwrap();
    drop(gate);
    let error = call.await.unwrap().unwrap_err();
    assert!(
        format!("{error:#}").contains("web_ui")
            || format!("{error:#}").contains("facade")
            || format!("{error:#}").contains("seal")
    );

    let committed = fixture.get(&fixture.first, &path("specs")).await.unwrap();
    let committed = json_value(committed.value.as_ref().unwrap());
    assert_eq!(committed[spec_id.as_str()]["revision"].as_f64(), Some(1.0));
    assert_eq!(
        committed[spec_id.as_str()]["spec"]["state"]["size"],
        "small"
    );
}
fn json_value(value: &prost_types::Value) -> Value {
    use prost_types::value::Kind;
    match value.kind.as_ref().unwrap() {
        Kind::NullValue(_) => Value::Null,
        Kind::BoolValue(value) => json!(value),
        Kind::NumberValue(value) => json!(value),
        Kind::StringValue(value) => json!(value),
        Kind::ListValue(value) => Value::Array(value.values.iter().map(json_value).collect()),
        Kind::StructValue(value) => Value::Object(
            value
                .fields
                .iter()
                .map(|(key, value)| (key.clone(), json_value(value)))
                .collect(),
        ),
    }
}
async fn frame(stream: &mut WorkspaceStream) -> Result<StateChange, Status> {
    tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .expect("bounded stream progress")
        .expect("stream frame")
}

#[tokio::test(flavor = "multi_thread")]
async fn real_statesync_handlers_gate_owners_read_only_committed_children_and_rehydrate_lag() {
    let fixture = Fixture::new().await;
    let paths = vec![path("specs")];
    // Missing verified extension, missing/duplicate/wrong capability and a
    // valid grant without explicit activation all fail at the actual handler.
    assert_eq!(
        fixture
            .server
            .subscribe(Request::new(SubscribeRequest {
                path_patterns: paths.clone(),
                ..Default::default()
            }))
            .await
            .err()
            .unwrap()
            .code(),
        tonic::Code::Unauthenticated
    );
    let mut missing = fixture.subscription(paths.clone(), &fixture.first);
    missing.metadata_mut().remove(DECLARED_CAPABILITY_HEADER);
    assert_eq!(
        fixture
            .server
            .subscribe(missing)
            .await
            .err()
            .unwrap()
            .code(),
        tonic::Code::PermissionDenied
    );
    let mut duplicate = fixture.subscription(paths.clone(), &fixture.first);
    duplicate.metadata_mut().append(
        DECLARED_CAPABILITY_HEADER,
        UI_WORKSPACE_READ_CAPABILITY.parse().unwrap(),
    );
    assert_eq!(
        fixture
            .server
            .subscribe(duplicate)
            .await
            .err()
            .unwrap()
            .code(),
        tonic::Code::PermissionDenied
    );
    let mut wrong = fixture.subscription(paths.clone(), &fixture.first);
    wrong
        .metadata_mut()
        .insert(DECLARED_CAPABILITY_HEADER, "gemma.invoke".parse().unwrap());
    assert_eq!(
        fixture.server.subscribe(wrong).await.err().unwrap().code(),
        tonic::Code::PermissionDenied
    );
    assert_eq!(
        fixture
            .server
            .subscribe(fixture.subscription(paths.clone(), &fixture.first))
            .await
            .err()
            .unwrap()
            .code(),
        tonic::Code::FailedPrecondition
    );
    let mut forged = fixture.first.clone();
    forged.session_id = fixture.second.session_id.clone();
    assert_eq!(
        fixture
            .get(&forged, &path("specs"))
            .await
            .unwrap_err()
            .code(),
        tonic::Code::Unauthenticated
    );
    assert_eq!(
        fixture
            .server
            .subscribe(fixture.subscription(
                vec![
                    path("specs"),
                    crate::state_stream::plugin_object_path(PLUGIN)
                ],
                &fixture.first
            ))
            .await
            .err()
            .unwrap()
            .code(),
        tonic::Code::InvalidArgument
    );
    assert_eq!(
        fixture
            .server
            .subscribe(fixture.subscription(vec![path("specs/*")], &fixture.first))
            .await
            .err()
            .unwrap()
            .code(),
        tonic::Code::InvalidArgument
    );

    fixture.activate(&fixture.first).await;
    fixture.activate(&fixture.second).await;
    let mut public_wakes = fixture.engine.change_tx().subscribe();
    let first = fixture
        .mutation(
            &fixture.first,
            "create_ui_spec",
            json!({"spec":canvas("PRIVATE_ORIGINAL"),"tags":[]}),
        )
        .await;
    let id = first["spec"]["id"].as_str().unwrap().to_owned();
    let sibling = fixture
        .mutation(
            &fixture.first,
            "create_ui_spec",
            json!({"spec":canvas("PRIVATE_SIBLING"),"tags":[]}),
        )
        .await;
    let sibling_id = sibling["spec"]["id"].as_str().unwrap().to_owned();
    assert!(fixture.engine.get_state(PLUGIN).await.is_none());
    while let Ok(wake) = public_wakes.try_recv() {
        let encoded = format!("{wake:?}");
        assert!(!encoded.contains("PRIVATE_ORIGINAL") && !encoded.contains("PRIVATE_SIBLING"));
    }

    // Neither a newer durable row nor a poisoned RAM cache is stream authority.
    let access = UiAccessContext {
        principal_id: fixture.first.principal_id.clone(),
        session_id: fixture.first.session_id.clone(),
        session_genesis: Some(fixture.first.session_genesis.clone()),
    };
    fixture
        .durable_methods()
        .create_spec(
            &access,
            "durable-only",
            canvas("PRIVATE_UNCOMMITTED"),
            vec![],
        )
        .await
        .unwrap();
    fixture
        .engine
        .update_state_cache(
            PLUGIN.into(),
            simd_json::json!({"specs":{"ram-only":{"secret":"PRIVATE_RAM"}}}),
        )
        .await;
    let selected = fixture.get(&fixture.first, &path("specs")).await.unwrap();
    let selected_json = json_value(selected.value.as_ref().unwrap());
    assert_eq!(selected_json.as_object().unwrap().len(), 2);
    assert!(!selected_json.to_string().contains("PRIVATE_UNCOMMITTED"));
    assert!(!selected_json.to_string().contains("PRIVATE_RAM"));
    assert!(selected.state.is_some());
    assert_eq!(
        fixture
            .get(&fixture.second, &path("specs"))
            .await
            .unwrap_err()
            .code(),
        tonic::Code::NotFound
    );

    let selected_path = path(&format!("specs/{id}/spec/elements/root/props"));
    let mut stream = fixture
        .server
        .subscribe(fixture.subscription(vec![selected_path.clone()], &fixture.first))
        .await
        .unwrap()
        .into_inner();
    let initial = frame(&mut stream).await.unwrap();
    assert_eq!(initial.frame_kind, StateFrameKind::InitialState as i32);
    assert_eq!(initial.object_path, selected_path);
    assert_eq!(
        json_value(initial.new_value.as_ref().unwrap())["text"],
        "PRIVATE_ORIGINAL"
    );
    assert!(initial.state_generation > 0);
    assert!(!initial.schema_hash.is_empty() && !initial.catalog_hash.is_empty());
    assert!(initial.actor_id.is_empty() && initial.event_id == 0 && initial.event_hash.is_empty());

    // The additive Value preserves descendant type and null presence. Struct
    // stays absent for nonobjects; no envelope or JSON-string substitution.
    for (suffix, expected) in [
        (
            format!("specs/{id}/spec/elements/root/props/text"),
            json!("PRIVATE_ORIGINAL"),
        ),
        (
            format!("specs/{id}/spec/elements/root/props/className"),
            Value::Null,
        ),
        (format!("specs/{id}/spec/elements/root/children"), json!([])),
    ] {
        let response = fixture.get(&fixture.first, &path(&suffix)).await.unwrap();
        assert!(response.state.is_none());
        let wire = GetStateResponse::decode(response.encode_to_vec().as_slice()).unwrap();
        assert_eq!(json_value(wire.value.as_ref().unwrap()), expected);
    }

    // More complete commits than the owner ring can retain force a SHM read;
    // deletion still produces a tombstone even though the trigger is lag.
    let mut revision = 1;
    for n in 0..35 {
        let changed = fixture
            .mutation(
                &fixture.first,
                "update_ui_spec",
                json!({"spec_id":id,"expected_revision":revision,
            "element_id":"root","properties":{
                "text":format!("private-update-{n}"), "className":null}}),
            )
            .await;
        revision = changed["spec"]["revision"].as_u64().unwrap();
    }
    fixture
        .mutation(
            &fixture.first,
            "delete_ui_spec",
            json!({"spec_id":id,"expected_revision":revision}),
        )
        .await;
    let removed = frame(&mut stream).await.unwrap();
    assert_eq!(removed.frame_kind, StateFrameKind::InitialState as i32);
    assert_eq!(removed.change_type, ChangeType::ObjectRemoved as i32);
    assert_eq!(removed.object_path, selected_path);
    assert!(removed.new_value.is_none() && removed.old_value.is_none());
    assert!(removed.state_generation > initial.state_generation);
    assert_eq!(
        fixture
            .get(
                &fixture.first,
                &path(&format!("specs/{sibling_id}/spec/elements/root/props/text"))
            )
            .await
            .unwrap()
            .value
            .map(|value| json_value(&value)),
        Some(json!("PRIVATE_SIBLING"))
    );

    // Bare plugin/public selection never upgrades into a private namespace.
    let public = fixture
        .server
        .get_state(fixture.request(
            GetStateRequest {
                plugin_id: PLUGIN.into(),
                object_path: String::new(),
            },
            &fixture.first,
            None,
        ))
        .await;
    assert_eq!(public.unwrap_err().code(), tonic::Code::NotFound);
    let mut public_stream = fixture
        .server
        .subscribe(fixture.request(
            SubscribeRequest {
                plugin_ids: vec![PLUGIN.into()],
                include_initial_state: true,
                include_schema: true,
                ..Default::default()
            },
            &fixture.first,
            None,
        ))
        .await
        .unwrap()
        .into_inner();
    let schema = frame(&mut public_stream).await.unwrap();
    assert_eq!(schema.change_type, ChangeType::SchemaMigration as i32);
    assert!(!format!("{schema:?}").contains("PRIVATE_SIBLING"));
    assert!(
        tokio::time::timeout(Duration::from_millis(30), public_stream.next())
            .await
            .is_err()
    );

    // A frame batch is suspended at each yield. Removing the current grant
    // stops the next private frame, even when it was prepared before revocation.
    let mut revoked = fixture
        .server
        .subscribe(fixture.subscription(vec![path("jobs"), path("specs")], &fixture.first))
        .await
        .unwrap()
        .into_inner();
    frame(&mut revoked).await.unwrap();
    fixture.grant_first(false);
    assert_eq!(
        frame(&mut revoked).await.unwrap_err().code(),
        tonic::Code::PermissionDenied
    );
    assert_eq!(
        fixture
            .get(&fixture.first, &path("specs"))
            .await
            .unwrap_err()
            .code(),
        tonic::Code::PermissionDenied
    );
    fixture.grant_first(true);

    // The heartbeat uses the last committed observation and only revalidates
    // access. Deliberately break the isolated private file after opening the
    // ticket: a state poll here would fail instead of emitting metadata.
    let heartbeat_request = fixture.subscription(vec![path("jobs")], &fixture.first);
    let (publisher, ticket, access_check) =
        open(&fixture.engine, &heartbeat_request, fixture.first.clone())
            .await
            .unwrap();
    let heartbeat_generation = ticket.initial.generation;
    let mut heartbeat_only = build_stream(
        fixture.engine.clone(),
        publisher,
        ticket,
        access_check,
        WorkspaceSelection::from_subscription(&[PLUGIN.into()], &[path("jobs")])
            .unwrap()
            .unwrap(),
        false,
        false,
        Duration::from_millis(1),
    );
    let owner_key = access.owner_key().unwrap();
    let private_file = fixture
        ._root
        .path()
        .join("private")
        .join(owner_key.namespace_component())
        .join("gemma_brain.json");
    let committed_bytes = std::fs::read(&private_file).unwrap();
    std::fs::write(&private_file, b"invalid isolated fixture JSON").unwrap();
    let heartbeat = frame(&mut heartbeat_only).await.unwrap();
    assert_eq!(heartbeat.frame_kind, StateFrameKind::Heartbeat as i32);
    assert_eq!(heartbeat.state_generation, heartbeat_generation);
    assert!(heartbeat.new_value.is_none());
    std::fs::write(&private_file, committed_bytes).unwrap();
    fixture.grant_first(false);
    assert_eq!(
        frame(&mut heartbeat_only).await.unwrap_err().code(),
        tonic::Code::PermissionDenied
    );
    fixture.grant_first(true);

    let mut withdrawn = fixture
        .server
        .subscribe(fixture.subscription(vec![path("jobs"), path("specs")], &fixture.first))
        .await
        .unwrap()
        .into_inner();
    frame(&mut withdrawn).await.unwrap();
    op_blob::BlobStore::open(&fixture.catalog)
        .unwrap()
        .remove_blob("json_render")
        .unwrap();
    assert_eq!(
        frame(&mut withdrawn).await.unwrap_err().code(),
        tonic::Code::PermissionDenied
    );
    let renderer_schema = JsonRenderPlugin::new().schema().unwrap();
    op_blob::BlobStore::open(&fixture.catalog)
        .unwrap()
        .write(&op_blob::blobify_plugin_schema(
            "json_render",
            renderer_schema,
        ))
        .unwrap();
    let mut revoked_identity = fixture
        .server
        .subscribe(fixture.subscription(vec![path("jobs"), path("specs")], &fixture.first))
        .await
        .unwrap()
        .into_inner();
    frame(&mut revoked_identity).await.unwrap();
    crate::human_principal_dispatch::tests::revoke(&fixture.first.human_pubkey)
        .await
        .unwrap();
    assert_eq!(
        frame(&mut revoked_identity).await.unwrap_err().code(),
        tonic::Code::Unauthenticated
    );
}

#[test]
fn exact_selection_and_wire_preserve_false_zero_null_arrays_and_deleted_descendants() {
    let tree = json!({"jobs":{"one":{"zero":0,"flag":false,"nullable":null,"items":[false,{"value":"deep"}]}},"specs":{}});
    let paths = vec![
        path("jobs/one/zero"),
        path("jobs/one/flag"),
        path("jobs/one/nullable"),
        path("jobs/one/items"),
        path("jobs/one/items/1/value"),
    ];
    let current = StateSnapshot {
        generation: u64::MAX - 1,
        catalog_hash: "stored-catalog".into(),
        plugins: [(
            PLUGIN.into(),
            PluginSnapshot {
                schema_hash: "stored-schema".into(),
                schema: json!({}),
                state: Some(tree.clone()),
            },
        )]
        .into_iter()
        .collect(),
    };
    let frames = state_snapshot_frames_with_views(
        &current,
        None,
        false,
        true,
        StateFrameKind::InitialState,
        &paths,
        false,
        exact_views,
    )
    .unwrap();
    assert_eq!(frames.len(), 5);
    for frame in frames {
        let decoded = StateChange::decode(frame.encode_to_vec().as_slice()).unwrap();
        assert_eq!(decoded.state_generation, u64::MAX - 1); // typed metadata, not Value numeric fidelity
        let actual = json_value(decoded.new_value.as_ref().unwrap());
        let expected = value_at_path(&tree, &decoded.object_path).unwrap().unwrap();
        if expected.is_number() {
            assert_eq!(actual.as_f64(), expected.as_f64());
        } else {
            assert_eq!(&actual, expected);
        }
    }
    let mut after = current.clone();
    after.generation = u64::MAX;
    after.plugins.get_mut(PLUGIN).unwrap().state = Some(json!({"jobs":{},"specs":{}}));
    let deleted = state_snapshot_frames_with_views(
        &after,
        Some(&current),
        false,
        true,
        StateFrameKind::InitialState,
        &paths,
        true,
        exact_views,
    )
    .unwrap();
    assert_eq!(deleted.len(), 5);
    assert!(deleted.iter().all(
        |frame| frame.change_type == ChangeType::ObjectRemoved as i32 && frame.new_value.is_none()
    ));
    assert!(value_at_path(&tree, &path("jobs/one/items/01")).is_err());
    assert!(exact_views(PLUGIN, &tree, &[path("jobs/*")]).is_err());
}
