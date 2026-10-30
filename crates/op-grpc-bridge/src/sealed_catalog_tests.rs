use super::*;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};

fn schema(id: &str, version: &str) -> op_state_store::PluginSchema {
    op_state_store::PluginSchema::builder(id)
        .version(version)
        .method(op_state_store::MethodDecl {
            name: "Read".into(),
            args: simd_json::json!({"type":"object","properties":{}}),
            returns: Some(
                simd_json::json!({"type":"object","properties":{"value":{"type":"string"}}}),
            ),
            side_effect: op_state_store::SideEffect::Read,
            idempotent: true,
            required_capability: Some("fixture.read".into()),
            subid: "obs.service.fixture.read@v1".into(),
        })
        .build()
}

fn seal(dir: &Path, schema: &op_state_store::PluginSchema) -> op_blob::PluginObjectBlob {
    let blob = op_blob::blobify_plugin_schema(&schema.name, schema.clone());
    op_blob::BlobStore::open(dir).unwrap().write(&blob).unwrap();
    blob
}

fn fingerprint(dir: &Path) -> BTreeMap<String, (u64, u64, i64, i64, Vec<u8>)> {
    fs::read_dir(dir)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            let metadata = entry.metadata().unwrap();
            (
                entry.file_name().to_str().unwrap().to_owned(),
                (
                    metadata.ino(),
                    metadata.len(),
                    metadata.mtime(),
                    metadata.mtime_nsec(),
                    fs::read(entry.path()).unwrap(),
                ),
            )
        })
        .collect()
}

#[test]
fn pinned_reader_ignores_orphans_and_preserves_original_sections_without_writes() {
    let dir = tempfile::tempdir().unwrap();
    let pinned = seal(dir.path(), &schema("chat_router", "1.0.0"));
    let orphan = op_blob::blobify_plugin_schema("cozo", schema("cozo", "1.0.0"));
    orphan.write_to_dir(dir.path()).unwrap(); // producer fixture without manifest commit
    let before = fingerprint(dir.path());
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o500)).unwrap();
    let loaded = read_manifest_pinned_catalog(dir.path()).unwrap();
    assert_eq!(
        loaded.plugins.keys().cloned().collect::<Vec<_>>(),
        vec!["chat_router"]
    );
    let loaded = &loaded.plugins["chat_router"];
    assert_eq!(loaded.schema_hash, pinned.manifest.schema_hash);
    assert_eq!(
        loaded.blob.schema_json.as_bytes(),
        pinned.schema_json.as_bytes()
    );
    assert_eq!(loaded.blob.descriptor_set, pinned.descriptor_set);
    assert_eq!(loaded.blob.meta_json, pinned.meta_json);
    assert!(op_blob::catalog::read_plugin_state_store_schema(dir.path(), "cozo").is_none());
    assert_eq!(
        op_blob::catalog::read_plugin_state_store_schema(dir.path(), "chat_router")
            .unwrap()
            .version,
        "1.0.0"
    );
    assert_eq!(fingerprint(dir.path()), before);
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn absent_manifest_and_directory_do_not_create_existence_or_paths() {
    let dir = tempfile::tempdir().unwrap();
    let absent = dir.path().join("not-created");
    assert!(read_manifest_pinned_catalog(&absent).is_err());
    assert!(!absent.exists());
    let orphan = op_blob::blobify_plugin_schema("chat_router", schema("chat_router", "1.0.0"));
    orphan.write_to_dir(dir.path()).unwrap();
    let before = fingerprint(dir.path());
    assert!(read_manifest_pinned_schema_document(dir.path(), "chat_router").is_err());
    assert!(op_blob::catalog::read_plugin_state_store_schema(dir.path(), "chat_router").is_none());
    assert!(read_manifest_pinned_catalog(dir.path()).is_err());
    assert_eq!(fingerprint(dir.path()), before);
}

#[test]
fn malformed_headers_hashes_identity_and_descriptors_fail_closed() {
    for defect in ["header", "hash", "identity", "descriptor", "missing"] {
        let dir = tempfile::tempdir().unwrap();
        let mut blob = seal(dir.path(), &schema("chat_router", "1.0.0"));
        let path = dir.path().join(blob.file_name());
        match defect {
            "header" => {
                let mut bytes = fs::read(&path).unwrap();
                bytes[0] = 0;
                fs::write(&path, bytes).unwrap();
            }
            "hash" => {
                let mut bytes = fs::read(&path).unwrap();
                bytes[16] ^= 1;
                fs::write(&path, bytes).unwrap();
            }
            "identity" => {
                blob.manifest.plugin_id = "foreign".into();
                fs::write(&path, blob.seal()).unwrap();
            }
            "descriptor" => {
                blob.descriptor_set = vec![0xff];
                fs::write(&path, blob.seal()).unwrap();
            }
            "missing" => fs::remove_file(&path).unwrap(),
            _ => unreachable!(),
        }
        assert!(
            read_manifest_pinned_catalog(dir.path()).is_err(),
            "accepted {defect}"
        );
        if defect != "descriptor" {
            assert!(
                op_blob::catalog::read_plugin_state_store_schema(dir.path(), "chat_router")
                    .is_none(),
                "schema helper accepted {defect}"
            );
        }
    }
}

#[test]
fn structural_parser_rejects_overflow_overlap_and_wrong_pin() {
    let blob = op_blob::blobify_plugin_schema("chat_router", schema("chat_router", "1.0.0"));
    let bytes = blob.seal();
    assert!(op_blob::BlobRef::from_manifest_pinned(&bytes, &"f".repeat(64)).is_err());
    let mut overflow = bytes.clone();
    overflow[72..80].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(op_blob::BlobRef::from_manifest_pinned(&overflow, &blob.manifest.schema_hash).is_err());
    let mut overlapping = bytes.clone();
    overlapping[96..104].copy_from_slice(&bytes[72..80]);
    assert!(
        op_blob::BlobRef::from_manifest_pinned(&overlapping, &blob.manifest.schema_hash).is_err()
    );
    // The trusted-reader constructor compares stored identity without hashing
    // payload bytes. Integrity checking remains available for untrusted imports.
    let mut changed_header = bytes.clone();
    changed_header[16..48].fill(0xab);
    assert!(op_blob::BlobRef::from_manifest_pinned(&changed_header, &"ab".repeat(32)).is_ok());
    assert!(op_blob::BlobRef::new(&changed_header).is_err());
}

#[tokio::test]
async fn reflection_reseal_missing_manifest_and_corrupt_pin_replace_advertisements() {
    let dir = tempfile::tempdir().unwrap();
    let catalog = crate::dynamic_reflection::ActiveReflectionCatalog::with_dir(dir.path().into());
    let first = seal(dir.path(), &schema("chat_router", "1.0.0"));
    assert_eq!(catalog.sync_from_shm().await, 1);
    assert!(
        catalog
            .list_services()
            .await
            .iter()
            .any(|name| name == "operation.plugin.v1.ChatRouterPluginMethods")
    );
    let second = seal(dir.path(), &schema("chat_router", "2.0.0"));
    assert_ne!(first.manifest.schema_hash, second.manifest.schema_hash);
    assert_eq!(catalog.sync_from_shm().await, 1);
    assert_eq!(
        read_manifest_pinned_catalog(dir.path()).unwrap().plugins["chat_router"]
            .schema
            .version,
        "2.0.0"
    );
    let manifest_path = dir.path().join(op_blob::catalog::MANIFEST_FILENAME);
    let manifest = fs::read(&manifest_path).unwrap();
    fs::remove_file(&manifest_path).unwrap();
    assert_eq!(catalog.sync_from_shm().await, 0);
    assert_eq!(
        catalog.list_services().await,
        vec!["grpc.reflection.v1.ServerReflection"]
    );
    fs::write(&manifest_path, manifest).unwrap();
    assert_eq!(catalog.sync_from_shm().await, 1);
    fs::write(dir.path().join(second.file_name()), b"corrupt").unwrap();
    assert_eq!(catalog.sync_from_shm().await, 0);
    assert_eq!(
        catalog.list_services().await,
        vec!["grpc.reflection.v1.ServerReflection"]
    );
}

#[tokio::test]
async fn sealed_method_metadata_keeps_exact_descriptor_bytes_and_replaces_old_methods() {
    let dir = tempfile::tempdir().unwrap();
    let original = seal(dir.path(), &schema("chat_router", "1.0.0"));
    let loaded = read_manifest_pinned_schema_document(dir.path(), "chat_router").unwrap();
    let registry = crate::plugin_grpc_gen::PerMethodGrpcServices::new();
    registry
        .register_sealed_services(
            "chat_router".into(),
            &loaded.schema,
            &loaded.blob.descriptor_set,
        )
        .await
        .unwrap();
    assert_eq!(
        registry.frozen_descriptor_set_bytes(),
        original.descriptor_set
    );
    let mut replacement = schema("chat_router", "2.0.0");
    replacement.methods.clear();
    let replacement = seal(dir.path(), &replacement);
    let loaded = read_manifest_pinned_schema_document(dir.path(), "chat_router").unwrap();
    registry
        .register_sealed_services(
            "chat_router".into(),
            &loaded.schema,
            &loaded.blob.descriptor_set,
        )
        .await
        .unwrap();
    assert!(
        registry
            .get_services_for_plugin("chat_router")
            .await
            .is_empty()
    );
    assert_eq!(
        registry.frozen_descriptor_set_bytes(),
        replacement.descriptor_set
    );
}

#[test]
fn registration_requires_pinned_existence_and_exact_schema_without_writing() {
    let dir = tempfile::tempdir().unwrap();
    let supplied = schema("chat_router", "1.0.0");
    assert!(read_registration_document(dir.path(), "chat_router", &supplied).is_err());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    seal(dir.path(), &supplied);
    let before = fingerprint(dir.path());
    assert!(read_registration_document(dir.path(), "chat_router", &supplied).is_ok());
    let changed = schema("chat_router", "2.0.0");
    assert!(read_registration_document(dir.path(), "chat_router", &changed).is_err());
    assert!(read_registration_document(dir.path(), "cozo", &schema("cozo", "1.0.0")).is_err());
    assert_eq!(fingerprint(dir.path()), before);
}

#[tokio::test]
async fn real_dns_schema_keeps_canonical_keys_and_explicit_transport_names() {
    let dir = tempfile::tempdir().unwrap();
    let dns = op_plugins::state_plugins::dnsresolver::DnsResolverPlugin::new();
    let schema = op_state::StatePlugin::schema(&dns).unwrap();
    assert_eq!(schema.methods["resolve_hostname"].name, "ResolveHostname");
    let original = seal(dir.path(), &schema);
    let before = fingerprint(dir.path());
    let pinned = read_registration_document(dir.path(), "dnsresolver", &schema).unwrap();
    assert_eq!(pinned.blob.descriptor_set, original.descriptor_set);
    assert_eq!(
        pinned
            .blob
            .manifest
            .methods
            .iter()
            .map(|method| method.schema_name.clone())
            .collect::<BTreeSet<_>>(),
        schema.methods.keys().cloned().collect::<BTreeSet<_>>()
    );
    let method = pinned
        .blob
        .manifest
        .methods
        .iter()
        .find(|method| method.schema_name == "resolve_hostname")
        .unwrap();
    assert_eq!(method.grpc_name, "ResolveHostname");
    assert_eq!(
        method.grpc_path,
        "/operation.method.dnsresolver.resolve_hostname.ResolveHostnameService/ResolveHostname"
    );
    let reflection = crate::per_plugin_reflection::PerMethodReflectionRegistry::new();
    let meta = reflection
        .register_sealed("dnsresolver".into(), method, schema.version.clone())
        .await;
    assert_eq!(meta.method_name, "resolve_hostname");
    assert_eq!(meta.method_path, "dnsresolver.resolve_hostname");
    assert_eq!(meta.service_name, method.grpc_service);
    let services = crate::plugin_grpc_gen::PerMethodGrpcServices::new();
    let registered = services
        .register_sealed_services(
            "dnsresolver".into(),
            &pinned.schema,
            &pinned.blob.descriptor_set,
        )
        .await
        .unwrap();
    assert_eq!(
        registered
            .iter()
            .map(|service| service.method_name.clone())
            .collect::<BTreeSet<_>>(),
        schema.methods.keys().cloned().collect::<BTreeSet<_>>()
    );
    assert_eq!(
        services.frozen_descriptor_set_bytes(),
        original.descriptor_set
    );
    assert_eq!(fingerprint(dir.path()), before);
}

#[tokio::test]
async fn historical_transport_name_manifest_requires_explicit_reseal_without_reader_aliases() {
    let dir = tempfile::tempdir().unwrap();
    let dns = op_plugins::state_plugins::dnsresolver::DnsResolverPlugin::new();
    let schema = op_state::StatePlugin::schema(&dns).unwrap();
    let mut historical = seal(dir.path(), &schema);
    let catalog = crate::dynamic_reflection::ActiveReflectionCatalog::with_dir(dir.path().into());
    assert_eq!(catalog.sync_from_shm().await, 1);
    assert!(
        catalog
            .list_services()
            .await
            .iter()
            .any(|service| service == "operation.plugin.v1.DnsresolverPluginMethods")
    );
    let method = historical
        .manifest
        .methods
        .iter_mut()
        .find(|method| method.schema_name == "resolve_hostname")
        .unwrap();
    method.schema_name = "ResolveHostname".into();
    // A local producer fixture reproduces the old manifest; the reader never
    // rewrites its schema, stored hash, descriptor bytes, or method identity.
    fs::write(dir.path().join(historical.file_name()), historical.seal()).unwrap();
    let before = fingerprint(dir.path());
    let error = read_manifest_pinned_schema_document(dir.path(), "dnsresolver")
        .err()
        .expect("historical noncanonical key must fail closed");
    assert!(error.to_string().contains("explicit reseal required"));
    assert_eq!(catalog.sync_from_shm().await, 0);
    assert_eq!(
        catalog.list_services().await,
        vec!["grpc.reflection.v1.ServerReflection"]
    );
    assert_eq!(fingerprint(dir.path()), before);
}
