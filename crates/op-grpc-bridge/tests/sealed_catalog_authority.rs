//! Startup consumes sealed plugins without writing or rebuilding their contracts.
use op_grpc_bridge::mutation_engine::MutationEngine;
use op_state_store::{ChainConfig, EventChain, PluginSchema};
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::test]
async fn startup_restores_observations_without_publishing_schema_examples_or_resealing() {
    struct Restore(Vec<(&'static str, Option<std::ffi::OsString>)>);
    impl Drop for Restore {
        fn drop(&mut self) {
            for (key, value) in &self.0 {
                match value {
                    // FIXME: Audit that the environment access only happens in single-threaded code.
                    Some(value) => unsafe { std::env::set_var(key, value) },
                    // FIXME: Audit that the environment access only happens in single-threaded code.
                    None => unsafe { std::env::remove_var(key) },
                }
            }
        }
    }
    let root = tempfile::tempdir().unwrap();
    let catalog = root.path().join("blobs");
    let _restore = Restore(
        [
            "OP_BLOB_CATALOG_DIR",
            "OP_SHM_STATE_DIR",
            "DBUS_SESSION_BUS_ADDRESS",
        ]
        .into_iter()
        .map(|key| (key, std::env::var_os(key)))
        .collect(),
    );
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("OP_BLOB_CATALOG_DIR", &catalog) };
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("OP_SHM_STATE_DIR", root.path().join("state")) };
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe {
        std::env::set_var(
            "DBUS_SESSION_BUS_ADDRESS",
            format!("unix:path={}/absent-bus", root.path().display()),
        )
    };
    let schema = PluginSchema::builder("fixture_uncompiled_plugin")
        .string_field("status", false, "Observed status")
        .example(simd_json::json!({"status":"example-is-not-observed"}))
        .build();
    let blob = op_blob::blobify_plugin_schema("fixture_uncompiled_plugin", schema);
    let mut sealer = op_blob::catalog::BlobStore::open(&catalog).unwrap();
    sealer.write(&blob).unwrap();
    let manifest = catalog.join(op_blob::catalog::MANIFEST_FILENAME);
    let before = std::fs::read(&manifest).unwrap();
    let files_before = std::fs::read_dir(&catalog).unwrap().count();
    let engine = MutationEngine::new(
        Arc::new(RwLock::new(EventChain::new(ChainConfig::default()))),
        Arc::new(op_network::rovs_proxy::OvsdbDbusClient::new()),
    );
    assert_eq!(engine.seed_missing_plugin_projections().await.unwrap(), 0);
    assert!(engine
        .get_state("fixture_uncompiled_plugin")
        .await
        .is_none());
    assert!(!root
        .path()
        .join("state/fixture_uncompiled_plugin.json")
        .exists());
    assert!(!root.path().join("state/.manifest.json").exists());
    assert!(engine.get_state("tched_router").await.is_none());
    assert_eq!(engine.seed_missing_plugin_projections().await.unwrap(), 0);
    assert_eq!(std::fs::read(manifest).unwrap(), before);
    assert_eq!(std::fs::read_dir(&catalog).unwrap().count(), files_before);

    // Mutable values recover from their durable store, while the original
    // catalog remains the only authority for which plugins may be projected.
    let documents = Arc::new(
        op_plugins::state_plugins::runtime_documents::RuntimeDocuments::new(
            op_cozo_store::CozoGraphShuttle::new_persistent(root.path().join("documents")).unwrap(),
        ),
    );
    let config = serde_json::json!({"configs":{"chosen_model":"provider/model","enabled":false}});
    let committed = config.clone();
    documents.update("config", |_| Ok(committed)).await.unwrap();
    documents
        .update("endpoint", |_| {
            Ok(serde_json::json!({"endpoints":["fixture"]}))
        })
        .await
        .unwrap();
    let sockets =
        op_plugins::state_plugins::shared_unix_socket_methods::SharedUnixSocketMethods::new(
            documents.clone(),
            root.path().join("fixture.sock"),
        )
        .unwrap();
    let registration = sockets
        .dispatch(
            "create_unix_socket",
            &serde_json::json!({
                "name":"fixture", "ports":[8081, 8080],
            }),
        )
        .await
        .unwrap();
    // Unsealed rows cannot materialize plugins.
    assert_eq!(
        engine
            .seed_plugin_projections_with_documents(Some(documents.clone()))
            .await
            .unwrap(),
        0
    );
    assert!(engine.get_state("config").await.is_none());
    assert!(engine.get_state("endpoint").await.is_none());
    assert!(engine.get_state("shared_unix_socket").await.is_none());
    sealer
        .write(&op_blob::blobify_plugin_schema(
            "config",
            PluginSchema::builder("config").build(),
        ))
        .unwrap();
    sealer
        .write(&op_blob::blobify_plugin_schema(
            "shared_unix_socket",
            PluginSchema::builder("shared_unix_socket").build(),
        ))
        .unwrap();
    let manifest = catalog.join(op_blob::catalog::MANIFEST_FILENAME);
    let pinned = std::fs::read(&manifest).unwrap();
    op_core::projection_shm::update_projection_in(
        root.path().join("state").to_str().unwrap(),
        "config",
        |_| Ok(Some(br#"{"configs":{"chosen_model":"stale"}}"#.to_vec())),
    )
    .unwrap();
    assert_eq!(
        engine
            .seed_plugin_projections_with_documents(Some(documents.clone()))
            .await
            .unwrap(),
        2
    );
    let restored =
        serde_json::to_value(engine.get_state("shared_unix_socket").await.unwrap()).unwrap();
    assert_eq!(restored["registrations"][0], registration["registration"]);
    assert_eq!(
        restored["registrations"][0]["ports"],
        serde_json::json!([8080, 8081])
    );
    assert_eq!(
        serde_json::to_value(engine.get_state("config").await.unwrap()).unwrap(),
        config
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(
            &std::fs::read(root.path().join("state/config.json")).unwrap()
        )
        .unwrap(),
        config
    );
    assert_eq!(
        engine
            .seed_plugin_projections_with_documents(Some(documents.clone()))
            .await
            .unwrap(),
        0
    );
    assert!(engine.get_state("endpoint").await.is_none());
    assert_eq!(std::fs::read(&manifest).unwrap(), pinned);
    documents
        .update("config", |_| Ok(serde_json::json!({"configs":"corrupt"})))
        .await
        .unwrap();
    assert!(engine
        .seed_plugin_projections_with_documents(Some(documents))
        .await
        .is_err());
    assert_eq!(
        serde_json::to_value(engine.get_state("config").await.unwrap()).unwrap(),
        config
    );
    assert_eq!(std::fs::read(manifest).unwrap(), pinned);
}
