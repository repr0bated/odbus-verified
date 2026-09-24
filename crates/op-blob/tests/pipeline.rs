//! End-to-end pipeline tests implementing the whitepaper's "Required Tests"
//! and the spec's reflection-coverage gate (Task 5.1):
//! struct -> PluginSchema -> blob -> zero-copy read -> catalog -> btrfs image.

use op_blob::blob::{blobify, blobify_with_identity, BlobRef};
use op_blob::catalog::ActiveReflectionCatalog;
use op_blob::demo::{unix_socket_schema, wireguard_schema};
use op_blob::descriptor::decode_file_summaries;
use op_blob::identity::WgKeypair;
use std::path::PathBuf;

fn tmp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "op-blob-test-{tag}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

// --- Blob tests ------------------------------------------------------------

#[test]
fn blob_contains_canonical_schema_bytes_and_stable_hash() {
    let schema = unix_socket_schema();
    let a = blobify(&schema);
    let b = blobify(&schema);
    // Same schema -> same hash, same sealed bytes (deterministic identity).
    assert_eq!(a.manifest.schema_hash, b.manifest.schema_hash);
    assert_eq!(a.seal(), b.seal());
    assert_eq!(a.schema_json, schema.canonical_json());
    assert_eq!(a.manifest.schema_hash, schema.schema_hash_hex());
}

#[test]
fn blob_dbus_and_grpc_identity_match_plugin() {
    let blob = blobify(&unix_socket_schema());
    assert_eq!(blob.manifest.plugin_id, "unix_socket");
    assert_eq!(
        blob.manifest.dbus.object_path,
        "/org/opdbus/v1/plugins/unix_socket"
    );
    assert_eq!(blob.manifest.dbus.interface_name, "org.opdbus.v1.PluginV1");
    assert!(blob
        .manifest
        .grpc
        .services
        .contains(&"operation.method.unix_socket.bind.BindService".to_string()));
}

#[test]
fn blob_methods_match_plugin_schema_methods() {
    let schema = wireguard_schema();
    let blob = blobify(&schema);
    assert_eq!(blob.manifest.methods.len(), schema.methods.len());
    for m in &blob.manifest.methods {
        let decl = schema
            .methods
            .get(&m.schema_name)
            .expect("method in schema");
        assert_eq!(m.subid, decl.subid);
        assert_eq!(m.required_capability, decl.required_capability);
        assert_eq!(m.idempotent, decl.idempotent);
    }
}

#[test]
fn sealed_blob_reads_back_zero_copy_and_verified() {
    let schema = unix_socket_schema();
    let blob = blobify(&schema);
    let bytes = blob.seal();

    let r = BlobRef::new(&bytes).expect("valid blob");
    // Zero-copy: returned slices point inside the sealed image.
    let range = bytes.as_ptr() as usize..bytes.as_ptr() as usize + bytes.len();
    assert!(range.contains(&(r.schema_json().as_ptr() as usize)));
    assert!(range.contains(&(r.descriptor_set().as_ptr() as usize)));

    // The schema reconstructed from the blob is byte-identical contract-wise.
    let round = r.plugin_schema().unwrap();
    assert!(op_blob::schema_diffs(&schema, &round).is_empty());

    // Corruption is detected (schema hash guard).
    let mut bad = bytes.clone();
    let schema_pos = bad
        .windows(12)
        .position(|w| w == b"\"unix_socket")
        .expect("schema text present");
    bad[schema_pos + 1] ^= 0x20;
    assert!(BlobRef::new(&bad).is_err());
}

#[test]
fn identity_is_wireguard_keypair_with_account_persistent_session() {
    let kp = WgKeypair::generate();
    let blob = blobify_with_identity(&wireguard_schema(), Some(kp.public_identity()));
    let bytes = blob.seal();
    let m = BlobRef::new(&bytes).unwrap().manifest().unwrap();
    let id = m.identity.expect("identity in manifest");
    assert_eq!(id.wg_public_key, kp.public_base64());
    // Session derives from the keypair and never expires with the account.
    assert_eq!(id.session, kp.public_identity().session);
    // The private key must never appear in the sealed artifact.
    let text = String::from_utf8_lossy(&bytes);
    assert!(!text.contains(&kp.private_base64()));
}

// --- Reflection coverage (Task 5.1 analogue) --------------------------------

#[test]
fn reflection_descriptor_covers_every_method_with_typed_messages() {
    for schema in [unix_socket_schema(), wireguard_schema()] {
        let blob = blobify(&schema);
        let files = decode_file_summaries(&blob.descriptor_set).unwrap();
        let method_files = files
            .iter()
            .filter(|file| file.package != "google.protobuf")
            .collect::<Vec<_>>();
        assert_eq!(method_files.len(), schema.methods.len());
        assert!(files.len() <= method_files.len() + 1);
        for f in method_files {
            // Per-method Input/Output records remain typed. Dynamic/nullable
            // fields import the standard JSON well-known descriptor once.
            assert_eq!(f.messages.len(), 2, "{}: Input + Output", f.name);
            assert_eq!(f.services.len(), 1);
            assert!(f
                .package
                .starts_with(&format!("operation.method.{}.", blob.manifest.plugin_id)));
        }
    }
}

// --- Catalog tests -----------------------------------------------------------

#[test]
fn catalog_insert_lists_service_and_remove_unlists() {
    let dir = tmp_dir("catalog");
    let mut cat = ActiveReflectionCatalog::open(&dir).unwrap();
    assert!(cat.list_services().is_empty()); // inactive => not advertised

    let blob = blobify(&unix_socket_schema());
    cat.upsert_blob(&blob).unwrap();
    assert!(cat
        .list_services()
        .contains(&"operation.method.unix_socket.bind.BindService".to_string()));

    // file_by_filename / symbol lookup serve descriptor bytes.
    let file = cat
        .file_by_filename("operation/method/unix_socket/bind.proto")
        .expect("descriptor file bytes");
    assert!(!file.is_empty());
    assert!(cat
        .symbol_by_name("operation.method.unix_socket.bind.BindService")
        .is_some());

    // Reload from disk: persistence layout works.
    let cat2 = ActiveReflectionCatalog::open(&dir).unwrap();
    assert_eq!(cat2.active_plugins(), vec!["unix_socket".to_string()]);

    let mut cat = cat2;
    assert!(cat.remove_blob("unix_socket").unwrap());
    assert!(cat.list_services().is_empty());
    assert!(cat.blob_paths().is_empty());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn catalog_upsert_replaces_stale_hash_versions() {
    let dir = tmp_dir("stale");
    let mut cat = ActiveReflectionCatalog::open(&dir).unwrap();
    let mut schema = unix_socket_schema();
    cat.upsert_blob(&blobify(&schema)).unwrap();
    schema.version = "1.1.0".to_string(); // new contract -> new hash
    cat.upsert_blob(&blobify(&schema)).unwrap();

    let blobs: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "blob"))
        .collect();
    assert_eq!(blobs.len(), 1, "stale hash versions must be removed");
    std::fs::remove_dir_all(&dir).ok();
}

// --- Btrfs filesystem in the blob artifact -----------------------------------

#[test]
fn seals_catalog_into_btrfs_filesystem_image() {
    if op_blob::btrfs::which_mkfs().is_none() {
        eprintln!("mkfs.btrfs not found; skipping btrfs seal test");
        return;
    }
    let dir = tmp_dir("btrfs");
    let mut cat = ActiveReflectionCatalog::open(&dir).unwrap();
    let kp = WgKeypair::generate();
    cat.upsert_blob(&blobify_with_identity(
        &unix_socket_schema(),
        Some(kp.public_identity()),
    ))
    .unwrap();
    cat.upsert_blob(&blobify_with_identity(
        &wireguard_schema(),
        Some(kp.public_identity()),
    ))
    .unwrap();

    let image = dir.join("opdbus-blobs.btrfs");
    let report = op_blob::btrfs::seal_blobs_into_btrfs_image(&cat.blob_paths(), &image).unwrap();
    assert_eq!(report.blob_count, 2);
    assert!(report.subvolumes);
    assert!(op_blob::btrfs::verify_btrfs_image(&image).unwrap());
    let roots = std::process::Command::new("btrfs")
        .args(["inspect-internal", "dump-tree", "-t", "root"])
        .arg(&image)
        .output()
        .unwrap();
    assert!(
        roots.status.success(),
        "{}",
        String::from_utf8_lossy(&roots.stderr)
    );
    let roots = String::from_utf8_lossy(&roots.stdout);
    // Root-tree backrefs, unlike ordinary directory entries, prove that the
    // two plugin paths inside the unmounted image are actual subvolumes.
    assert!(roots.contains("root backref") && roots.contains("name unix_socket"));
    assert!(roots.contains("root backref") && roots.contains("name wireguard"));
    // --shrink keeps the at-rest artifact small.
    assert!(report.image_bytes < 512 * 1024 * 1024);
    std::fs::remove_dir_all(&dir).ok();
}
