//! Canonical flow admission, native-owner receipts and committed child state.
use super::*;
use crate::human_principal_dispatch::tests::{
    pk, register, temp_cozo, temp_shm_with_schemas, test_engine,
};
use crate::identity_sled_dispatch::{tests::write_identity, IdentitySledStore};
use crate::obfuscation_methods::{FlowOwner, ObfuscationMethods};
use anyhow::Result;
use op_plugins::state_plugins::{
    human_principal::HumanPrincipalPlugin, identity_sled::IdentitySledPlugin, openflow::FlowEntry,
    openflow_obfuscation::OpenFlowObfuscationPlugin, runtime_documents::RuntimeDocuments,
};
use op_state::StatePlugin;
use serde_json::{json, Value};

#[derive(Default)]
struct Switch(Mutex<Vec<FlowEntry>>);

#[async_trait::async_trait]
impl FlowOwner for Switch {
    async fn matches(&self, _bridge: &str, flow: &FlowEntry, absent: bool) -> Result<bool> {
        Ok(self.0.lock().await.contains(flow) != absent)
    }
    async fn exchange(
        &self,
        _bridge: &str,
        flow: &FlowEntry,
        expected: Option<&FlowEntry>,
        remove: bool,
    ) -> Result<()> {
        let mut current = self.0.lock().await;
        if let Some(expected) = expected {
            anyhow::ensure!(current.contains(expected), "native comparison conflict");
            current.retain(|entry| entry != expected);
        } else {
            anyhow::ensure!(!current.contains(flow), "native entry already exists");
        }
        if !remove {
            current.push(flow.clone());
        }
        Ok(())
    }
}

struct GrantsEnvironment(Option<std::ffi::OsString>);
impl Drop for GrantsEnvironment {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            std::env::set_var("OP_GRANTS_PATH", path);
        } else {
            std::env::remove_var("OP_GRANTS_PATH");
        }
    }
}

#[tokio::test]
async fn flow_methods_commit_verified_children_and_reject_revoked_identity() {
    let _cozo = temp_cozo();
    let schema = OpenFlowObfuscationPlugin::new(Default::default())
        .schema()
        .unwrap();
    let _shm = temp_shm_with_schemas(&[
        HumanPrincipalPlugin::new().schema().unwrap(),
        IdentitySledPlugin::new().schema().unwrap(),
        schema.clone(),
    ]);
    let engine = test_engine();
    engine
        .install_identity_sled_store_for_test(IdentitySledStore::new(
            op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap(),
        ))
        .unwrap();
    let owner = Arc::new(Switch::default());
    assert!(engine
        .obfuscation_methods
        .set(Arc::new(ObfuscationMethods::new(
            Arc::new(RuntimeDocuments::new(
                op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap()
            )),
            owner.clone(),
        )))
        .is_ok());
    let public_key = pk(0xD5);
    register(&public_key, "flow-fixture").await.unwrap();
    let identity = write_identity(&engine, &public_key).await.unwrap();
    let principal = op_identity::session::derive_principal_id(&public_key);
    let root = tempfile::tempdir().unwrap();
    let grant_path = root.path().join("grants.json");
    let _environment = GrantsEnvironment(std::env::var_os("OP_GRANTS_PATH"));
    std::env::set_var("OP_GRANTS_PATH", &grant_path);
    let capabilities = schema
        .methods
        .values()
        .filter_map(|m| m.required_capability.clone())
        .collect::<Vec<_>>();
    std::fs::write(
        &grant_path,
        json!({(principal.clone()): {"capabilities":capabilities}}).to_string(),
    )
    .unwrap();

    let mut ids = Vec::new();
    for (revision, priority) in [(0, 123), (1, 124)] {
        let input = json!({"bridge":"fixture0","expected_revision":revision,"ttl":64,"flow":{"table":7,"priority":priority,"match_fields":{"ip":"","in_port":"3"},"actions":[{"type":"output","port":"4"}]}});
        let response = engine
            .dispatch_method_call_with_identity(
                "openflow_obfuscation",
                "obfuscate_flow",
                &input.to_string(),
                schema.methods["obfuscate_flow"]
                    .required_capability
                    .as_deref(),
                &principal,
                Some(&identity.session_id),
                identity.genesis.as_deref(),
            )
            .await
            .unwrap();
        let output = &response["result"];
        let contract =
            serde_json::to_value(schema.methods["obfuscate_flow"].returns.as_ref().unwrap())
                .unwrap();
        jsonschema::Validator::new(&contract)
            .unwrap()
            .validate(output)
            .unwrap();
        ids.push(output["flow_id"].as_str().unwrap().to_owned());
        let state: Value = serde_json::from_slice(
            &op_core::projection_shm::read_projection_bytes("openflow_obfuscation").unwrap(),
        )
        .unwrap();
        assert_eq!(state["revision"], revision + 1);
        assert_eq!(state["flows"][&ids[revision as usize]], output["record"]);
        assert_eq!(
            state["flows"].as_object().unwrap().len(),
            revision as usize + 1
        );
    }
    let remove = json!({"bridge":"fixture0","flow_id":ids[0],"expected_revision":2});
    let before = op_core::projection_shm::read_projection_bytes("openflow_obfuscation").unwrap();
    std::fs::write(&grant_path, "{}").unwrap();
    assert!(engine
        .dispatch_method_call_with_identity(
            "openflow_obfuscation",
            "deobfuscate_flow",
            &remove.to_string(),
            schema.methods["deobfuscate_flow"]
                .required_capability
                .as_deref(),
            &principal,
            Some(&identity.session_id),
            identity.genesis.as_deref(),
        )
        .await
        .is_err());
    assert_eq!(owner.0.lock().await.len(), 2);
    assert_eq!(
        op_core::projection_shm::read_projection_bytes("openflow_obfuscation").unwrap(),
        before
    );
    std::fs::write(
        &grant_path,
        json!({(principal.clone()): {"capabilities":capabilities}}).to_string(),
    )
    .unwrap();
    let response = engine
        .dispatch_method_call_with_identity(
            "openflow_obfuscation",
            "deobfuscate_flow",
            &remove.to_string(),
            schema.methods["deobfuscate_flow"]
                .required_capability
                .as_deref(),
            &principal,
            Some(&identity.session_id),
            identity.genesis.as_deref(),
        )
        .await
        .unwrap();
    let contract =
        serde_json::to_value(schema.methods["deobfuscate_flow"].returns.as_ref().unwrap()).unwrap();
    jsonschema::Validator::new(&contract)
        .unwrap()
        .validate(&response["result"])
        .unwrap();
    let state: Value = serde_json::from_slice(
        &op_core::projection_shm::read_projection_bytes("openflow_obfuscation").unwrap(),
    )
    .unwrap();
    assert_eq!(state["revision"], 3);
    assert_eq!(state["flows"].as_object().unwrap().len(), 1);
    assert!(state["flows"].get(&ids[0]).is_none());
    assert!(state["flows"].get(&ids[1]).is_some());
    assert_eq!(owner.0.lock().await.len(), 1);
}
