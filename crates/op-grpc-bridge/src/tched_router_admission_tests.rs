use super::*;
use crate::human_principal_dispatch::tests::{
    pk, register, temp_cozo, temp_shm_with_schemas, test_engine,
};
use crate::identity_sled_dispatch::{tests::write_identity, IdentitySledStore};
use op_plugins::state_plugins::{
    human_principal::HumanPrincipalPlugin, identity_sled::IdentitySledPlugin,
    tched_router::TchedRouterPlugin,
};
use op_state::StatePlugin;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

struct RestoreGrants(Option<std::ffi::OsString>);
impl Drop for RestoreGrants {
    fn drop(&mut self) {
        match self.0.take() {
            Some(value) => std::env::set_var("OP_GRANTS_PATH", value),
            None => std::env::remove_var("OP_GRANTS_PATH"),
        }
    }
}

/// Runs the actual final admission while queued on the actual runtime guard.
/// The counter represents the first possible native effect after admission.
#[tokio::test]
async fn queued_calls_reject_reseal_and_grant_withdrawal_before_effect() {
    let _cozo = temp_cozo();
    let schema = TchedRouterPlugin::new().schema().unwrap();
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
    let key = pk(0xE3);
    register(&key, "queued-router-fixture").await.unwrap();
    let sled = write_identity(&engine, &key).await.unwrap();
    let actor = op_identity::session::derive_principal_id(&key);
    let capability = schema.methods["SetModel"]
        .required_capability
        .clone()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let grants = directory.path().join("grants.json");
    let _environment = RestoreGrants(std::env::var_os("OP_GRANTS_PATH"));
    std::env::set_var("OP_GRANTS_PATH", &grants);
    let write_grants = |allowed: bool| {
        std::fs::write(&grants, serde_json::to_vec(&serde_json::json!({
            (actor.clone()): {"capabilities": if allowed { vec![capability.clone()] } else { vec![] }}
        })).unwrap()).unwrap();
    };
    let caller = || RouterCaller {
        actor: actor.clone(),
        capability: Some(capability.clone()),
        session: Some(sled.session_id.clone()),
        genesis: sled.genesis.clone(),
    };
    let effects = Arc::new(AtomicUsize::new(0));
    for scenario in ["grant", "method", "metadata", "identity"] {
        write_grants(true);
        let catalog = crate::sealed_schema_reader::blob_catalog_dir_from_env();
        let mut sealer = op_blob::BlobStore::open(&catalog).unwrap();
        sealer
            .write(&op_blob::blobify_plugin_schema(
                "tched_router",
                schema.clone(),
            ))
            .unwrap();
        // Prove the original identity, exact grant and full pinned schema pass.
        drop(
            admit_guarded(&engine, &schema, "SetModel", &caller())
                .await
                .unwrap(),
        );
        let guard = crate::tched_router_runtime::TchedRouterRuntimeClient::selection_guard().await;
        let queued_engine = engine.clone();
        let queued_schema = schema.clone();
        let queued_caller = caller();
        let counter = effects.clone();
        let (started, ready) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            started.send(()).unwrap();
            let _admitted =
                admit_guarded(&queued_engine, &queued_schema, "SetModel", &queued_caller).await?;
            counter.fetch_add(1, Ordering::SeqCst);
            Ok::<_, anyhow::Error>(())
        });
        ready.await.unwrap();
        match scenario {
            "grant" => write_grants(false),
            "identity" => {
                crate::human_principal_dispatch::tests::revoke(&key)
                    .await
                    .unwrap();
            }
            "method" => {
                let mut changed = schema.clone();
                changed
                    .methods
                    .get_mut("SetModel")
                    .unwrap()
                    .required_capability = Some("changed.required.grant".into());
                sealer
                    .write(&op_blob::blobify_plugin_schema("tched_router", changed))
                    .unwrap();
            }
            _ => {
                // Even matching compiled fields/methods cannot substitute a
                // new catalog pin for the schema originally admitted.
                let mut changed = schema.clone();
                changed.description.push_str(" changed while queued");
                sealer
                    .write(&op_blob::blobify_plugin_schema("tched_router", changed))
                    .unwrap();
            }
        }
        drop(guard);
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(5), task)
                .await
                .unwrap()
                .unwrap()
                .is_err()
        );
        assert_eq!(effects.load(Ordering::SeqCst), 0);
    }
}
