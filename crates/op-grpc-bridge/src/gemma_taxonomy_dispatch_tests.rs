use crate::{
    human_principal_dispatch::tests::{
        pk, register, temp_cozo, temp_shm_with_schemas, test_engine,
    },
    identity_sled_dispatch::{tests::write_identity, IdentitySledStore},
    mutation_engine::MutationEngine,
    oracle_assertion::HumanPrincipalIdentity,
};
use op_plugins::state_plugins::{
    gemma_brain::{workspace::UiAccessContext, GemmaBrainPlugin},
    human_principal::HumanPrincipalPlugin,
    identity_sled::IdentitySledPlugin,
    json_render::JsonRenderPlugin,
    runtime_documents::RuntimeDocuments,
};
use op_state::StatePlugin;
use op_state_store::PluginSchema;
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc};

struct GrantsEnvironment(Option<std::ffi::OsString>);
impl Drop for GrantsEnvironment {
    fn drop(&mut self) {
        match self.0.take() {
            // FIXME: Audit that the environment access only happens in single-threaded code.
            Some(path) => unsafe { std::env::set_var("OP_GRANTS_PATH", path) },
            // FIXME: Audit that the environment access only happens in single-threaded code.
            None => unsafe { std::env::remove_var("OP_GRANTS_PATH") },
        }
    }
}

struct Fixture {
    engine: Arc<MutationEngine>,
    schema: PluginSchema,
    private: op_core::projection_shm::PrivateProjectionDirectory,
    first: HumanPrincipalIdentity,
    second: HumanPrincipalIdentity,
    grants: PathBuf,
    _environment: GrantsEnvironment,
    _root: tempfile::TempDir,
    _shm: crate::human_principal_dispatch::tests::TempShm,
    _cozo: crate::human_principal_dispatch::tests::TempCozo,
}

impl Fixture {
    async fn new() -> Self {
        let cozo = temp_cozo();
        let schema = GemmaBrainPlugin::new().schema().unwrap();
        let shm = temp_shm_with_schemas(&[
            HumanPrincipalPlugin::new().schema().unwrap(),
            IdentitySledPlugin::new().schema().unwrap(),
            schema.clone(),
            JsonRenderPlugin::new().schema().unwrap(),
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
        let private = op_core::projection_shm::PrivateProjectionDirectory::prepare(
            &root.path().join("private"),
        )
        .unwrap();
        engine
            .configure_ui_workspace(
                private.clone(),
                Arc::new(RuntimeDocuments::new(
                    op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap(),
                )),
                crate::sealed_schema_reader::blob_catalog_dir_from_env(),
            )
            .unwrap();
        let mut identities = Vec::new();
        for (byte, name) in [(0xDA, "taxonomy-first"), (0xDB, "taxonomy-second")] {
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
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("OP_GRANTS_PATH", &grants) };
        let fixture = Self {
            engine,
            schema,
            private,
            first: identities.remove(0),
            second: identities.remove(0),
            grants,
            _environment: environment,
            _root: root,
            _shm: shm,
            _cozo: cozo,
        };
        fixture.write_grants();
        fixture
    }

    fn write_grants(&self) {
        std::fs::write(
            &self.grants,
            serde_json::to_vec(&json!({
                (self.first.principal_id.clone()):{"capabilities":["gemma.invoke","gemma.read"]},
                (self.second.principal_id.clone()):{"capabilities":["gemma.invoke","gemma.read"]}
            }))
            .unwrap(),
        )
        .unwrap();
    }

    async fn call(
        &self,
        identity: &HumanPrincipalIdentity,
        method: &str,
        args: Value,
    ) -> anyhow::Result<Value> {
        let declaration = &self.schema.methods[method];
        let capability = declaration.required_capability.as_deref().unwrap();
        let response = self
            .engine
            .dispatch_method_call_with_identity(
                "gemma_brain",
                method,
                &args.to_string(),
                Some(capability),
                &identity.principal_id,
                Some(&identity.session_id),
                Some(&identity.session_genesis),
            )
            .await?;
        let output = response["result"].clone();
        let output_schema =
            serde_json::to_value(declaration.returns.as_ref().expect("typed Gemma output"))?;
        let validator = jsonschema::Validator::new(&output_schema)
            .map_err(|error| anyhow::anyhow!("invalid Gemma output schema: {error}"))?;
        validator
            .validate(&output)
            .map_err(|_| anyhow::anyhow!("Gemma output does not satisfy its declared schema"))?;
        Ok(output)
    }

    fn committed(&self, identity: &HumanPrincipalIdentity) -> Vec<u8> {
        let access = UiAccessContext {
            principal_id: identity.principal_id.clone(),
            session_id: identity.session_id.clone(),
            session_genesis: Some(identity.session_genesis.clone()),
        };
        let key = access.owner_key().unwrap();
        let mut snapshot = self
            .private
            .open_child(key.namespace_component())
            .unwrap()
            .expect("private owner projection")
            .read_snapshot(&["gemma_brain".into()])
            .unwrap();
        snapshot
            .states
            .remove("gemma_brain")
            .expect("committed Gemma state")
    }
}

fn tag(id: &str, keyword: &str, perspective: &str) -> Value {
    json!({
        "id":id,
        "label":id,
        "description":"fixture taxonomy",
        "keywords":[keyword],
        "perspectives":[perspective]
    })
}

#[tokio::test]
async fn all_five_taxonomy_methods_are_admitted_scoped_and_schema_validated() {
    let fixture = Fixture::new().await;
    let registered = fixture
        .call(
            &fixture.first,
            "register_tag",
            json!({"expected_taxonomy_revision":0,"tag":tag("deploy","release","operational")}),
        )
        .await
        .unwrap();
    assert_eq!(registered["taxonomy_revision"], 1);
    let classified = fixture
        .call(
            &fixture.first,
            "classify",
            json!({"text":"release the service","limit":8}),
        )
        .await
        .unwrap();
    assert_eq!(classified["classification"], "deploy");
    let tags = fixture
        .call(&fixture.first, "list_tags", json!({}))
        .await
        .unwrap();
    assert_eq!(tags["tags"][0]["id"], "deploy");
    let perspectives = fixture
        .call(&fixture.first, "list_perspectives", json!({}))
        .await
        .unwrap();
    assert_eq!(perspectives["perspectives"][0]["id"], "operational");
    let intent = fixture
        .call(
            &fixture.first,
            "analyze_intent",
            json!({"text":"release now","limit":8}),
        )
        .await
        .unwrap();
    assert_eq!(intent["intent"], "deploy");
    assert_eq!(intent["perspectives"], json!(["operational"]));

    let first_before = fixture.committed(&fixture.first);
    let first_state: Value = serde_json::from_slice(&first_before).unwrap();
    assert_eq!(first_state["taxonomy_revision"], 1);
    assert_eq!(
        first_state["tags"]["deploy"]["keywords"],
        json!(["release"])
    );
    assert!(fixture
        .call(
            &fixture.first,
            "register_tag",
            json!({"expected_taxonomy_revision":0,"tag":tag("stale","bad","other")}),
        )
        .await
        .is_err());
    assert_eq!(fixture.committed(&fixture.first), first_before);
    let tags_after_conflict = fixture
        .call(&fixture.first, "list_tags", json!({}))
        .await
        .unwrap();
    assert_eq!(tags_after_conflict["taxonomy_revision"], 1);
    assert_eq!(tags_after_conflict["tags"], tags["tags"]);

    fixture
        .call(
            &fixture.second,
            "register_tag",
            json!({"expected_taxonomy_revision":0,"tag":tag("review","audit","analytical")}),
        )
        .await
        .unwrap();
    let second_tags = fixture
        .call(&fixture.second, "list_tags", json!({}))
        .await
        .unwrap();
    assert_eq!(second_tags["tags"][0]["id"], "review");
    assert_ne!(
        fixture.committed(&fixture.first),
        fixture.committed(&fixture.second)
    );

    let declaration = &fixture.schema.methods["list_tags"];
    assert!(fixture
        .engine
        .dispatch_method_call_with_identity(
            "gemma_brain",
            "list_tags",
            "{}",
            declaration.required_capability.as_deref(),
            &fixture.first.principal_id,
            Some(&fixture.second.session_id),
            Some(&fixture.first.session_genesis),
        )
        .await
        .is_err());
    assert!(fixture
        .engine
        .dispatch_method_call_with_identity(
            "gemma_brain",
            "list_tags",
            "{}",
            declaration.required_capability.as_deref(),
            &fixture.first.principal_id,
            Some(&fixture.first.session_id),
            Some(&fixture.second.session_genesis),
        )
        .await
        .is_err());
}
