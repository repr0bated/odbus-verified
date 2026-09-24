// Run only under the private namespace launcher; never against live Incus.
// Seal the three actual schemas used by this fixture. The release catalog has
// a separate deployment gate and is not represented by this test catalog.
async fn private_native_fixture(
    byte: u8,
) -> (
    Arc<MutationEngine>,
    TempShm,
    ContainerIdentitySled,
    tempfile::TempDir,
    Option<std::ffi::OsString>,
    crate::human_principal_dispatch::tests::TempCozo,
) {
    use op_state::StatePlugin;

    let registry = crate::human_principal_dispatch::tests::temp_cozo();
    let key = pk(byte);
    let shm = crate::human_principal_dispatch::tests::temp_shm_with_schemas(&[
        op_plugins::state_plugins::human_principal::HumanPrincipalPlugin::new()
            .schema()
            .unwrap(),
        op_plugins::state_plugins::identity_sled::IdentitySledPlugin::new()
            .schema()
            .unwrap(),
        IncusPlugin::new().schema().unwrap(),
    ]);
    let engine = test_engine();
    engine
        .install_identity_sled_store_for_test(IdentitySledStore::new(
            CozoGraphShuttle::new_in_memory().unwrap(),
        ))
        .unwrap();
    crate::human_principal_dispatch::dispatch_human_principal_method(
        "register_key",
        &serde_json::json!({"human_pubkey":key,"display_alias":format!("private-{byte}")}),
    )
    .await
    .unwrap();
    let identity = write_identity_term_with_scope(&engine, &key, 0, None)
        .await
        .unwrap();
    set_session_actor_class(&engine, &identity.session_id, "human")
        .await
        .unwrap();
    let identity = stored_session(&engine, &identity.session_id)
        .await
        .unwrap()
        .unwrap();
    let principal = op_identity::session::derive_principal_id(&key);
    let grants_dir = tempfile::tempdir().unwrap();
    let grants_path = grants_dir.path().join("capability-grants.json");
    std::fs::write(
        &grants_path,
        serde_json::to_vec(
            &serde_json::json!({principal: {"capabilities": ["identity_sled.read"]}}),
        )
        .unwrap(),
    )
    .unwrap();
    let previous = std::env::var_os("OP_GRANTS_PATH");
    std::env::set_var("OP_GRANTS_PATH", &grants_path);
    (engine, shm, identity, grants_dir, previous, registry)
}

#[derive(Clone, Copy)]
enum PrivateProvisionOutcome {
    Committed,
    NativeDrift,
    ContaminatedSeed,
}

async fn run_seeded_canonical_provision_private_fixture(expected: PrivateProvisionOutcome) {
    use sha2::{Digest, Sha256};
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let _lock = refresh_test_lock().lock().await;
    let (engine, _shm, actor, _grants_dir, previous_grants, _registry) =
        private_native_fixture(0x91).await;
    let actor_id = op_identity::session::derive_principal_id(&actor.wireguard_pubkey);
    let target_key = pk(0x92);
    crate::human_principal_dispatch::dispatch_human_principal_method(
        "register_key",
        &serde_json::json!({"human_pubkey": target_key, "display_alias":"private-seeded-target"}),
    )
    .await
    .unwrap();
    let target_sid = op_identity::session::derive_session_id(&target_key);
    let grants = std::env::var("OP_GRANTS_PATH").unwrap();
    let env = |name| std::env::var(name).expect("private fixture environment missing");
    let seed = env("OP_IDENTITY_TEST_SEED_IMAGE");
    let seed_sha = env("OP_IDENTITY_TEST_SEED_SHA256");
    let seed_fsid = env("OP_IDENTITY_TEST_SEED_FSID");
    let leaf = env("OP_IDENTITY_TEST_LEAF_IMAGE");
    let mount = env("OP_IDENTITY_TEST_MOUNT_POINT");
    let pool = env("OP_IDENTITY_TEST_POOL_NAME");
    let image = env("OP_IDENTITY_TEST_IMAGE_FINGERPRINT");
    let plan_dir = env("OP_IDENTITY_TEST_PLAN_DIR");
    let persist = format!("{mount}/persist");
    let instance = serde_json::json!({
        "name":target_sid,"status":"Stopped","type":"container",
        "image":image,"storage_pool":pool,"profiles":["identity"],
        "config":{"boot.autostart":"false","security.privileged":"false","security.nesting":"false"},
        "expanded_config":{"boot.autostart":"false","security.privileged":"false","security.nesting":"false"},
        "devices":[
            {"name":"root","device":{"type":"disk","path":"/","pool":pool}},
            {"name":"persist","device":{"type":"disk","path":"/opt/run-mounts/persist","source":persist,"shift":"true"}}
        ],
        "expanded_devices":[
            {"name":"root","device":{"type":"disk","path":"/","pool":pool}},
            {"name":"persist","device":{"type":"disk","path":"/opt/run-mounts/persist","source":persist,"shift":"true"}}
        ]
    });
    let plan = serde_json::json!({
        "version":1,"session_id":target_sid,"wireguard_pubkey":target_key,
        "expected_revision":0,"expected_revision_operation_id":"",
        "seed_image_path":seed,"seed_sha256":seed_sha,"seed_fsid":seed_fsid,
        "new_leaf_image_path":leaf,"leaf_size_bytes":1073741824u64,
        "mount_point":mount,"pool_name":pool,"image_fingerprint":image,
        "instance":instance
    });
    let bytes = serde_json::to_vec(&plan).unwrap();
    let digest = hex::encode(Sha256::digest(&bytes));
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(format!("{plan_dir}/{target_sid}.json"))
        .unwrap()
        .write_all(&bytes)
        .unwrap();
    assert!(
        dispatch_identity_sled_method_for_verified_session(
            &engine,
            "provision_container",
            &serde_json::json!({"wireguard_pubkey":target_key,"plan_sha256":digest}),
            &actor_id,
            Some(&actor.session_id),
            actor.genesis.as_deref(),
        )
        .await
        .is_err(),
        "provision without exact write grant was admitted"
    );
    let store = engine.identity_sled_store().unwrap();
    assert!(
        store
            .cozo
            .read_seed_provision_journal(&target_sid)
            .unwrap()
            .is_none(),
        "no-grant provision entered native intent"
    );
    assert!(
        !std::path::Path::new(&leaf).exists(),
        "no-grant provision created leaf"
    );
    std::fs::write(
        &grants,
        serde_json::to_vec(
            &serde_json::json!({actor_id.clone(): {"capabilities": ["identity_sled.write"]}}),
        )
        .unwrap(),
    )
    .unwrap();
    let result = dispatch_identity_sled_method_for_verified_session(
        &engine,
        "provision_container",
        &serde_json::json!({"wireguard_pubkey":target_key,"plan_sha256":digest}),
        &actor_id,
        Some(&actor.session_id),
        actor.genesis.as_deref(),
    )
    .await;
    let journal = store
        .cozo
        .read_seed_provision_journal(&target_sid)
        .unwrap()
        .unwrap();
    let durable = load_revision(store, &target_sid).await.unwrap();
    match expected {
        PrivateProvisionOutcome::NativeDrift => {
            assert!(result.is_err(), "native profile drift was accepted");
            assert_eq!(journal.phase, "instance-create-intent");
            let cause = format!("{:#}", result.as_ref().unwrap_err());
            assert!(
                cause.contains("native create added or changed unpinned configuration"),
                "native drift test failed before the intended readback gate: {cause}"
            );
            assert!(
                durable.record.is_none(),
                "uncertain native create minted target identity"
            );
        }
        PrivateProvisionOutcome::ContaminatedSeed => {
            assert!(result.is_err(), "foreign container in seed was accepted");
            assert_eq!(journal.phase, "leaf-intent");
            let cause = format!("{:#}", result.as_ref().unwrap_err());
            assert!(
                cause.contains("core seed contains a pre-existing container or other volume"),
                "contaminated seed test missed the clean-core refusal: {cause}"
            );
            assert!(
                durable.record.is_none(),
                "foreign seed minted target identity"
            );
            assert!(
                !std::path::Path::new(&leaf).exists(),
                "foreign seed created native leaf"
            );
            assert!(
                !std::path::Path::new(&leaf)
                    .with_extension("provision.jsonl")
                    .exists(),
                "foreign seed created helper journal"
            );
        }
        PrivateProvisionOutcome::Committed => {
            let output =
                result.unwrap_or_else(|error| panic!("canonical provision failed: {error:#}"));
            let identity: ContainerIdentitySled =
                serde_json::from_value(output["identity"].clone()).unwrap();
            assert!(
                identity.is_anchored(),
                "canonical owner did not anchor target identity"
            );
            assert!(identity
                .sealed_id
                .as_deref()
                .is_some_and(|value| value.starts_with("sid1:")));
            let binding = identity.btrfs_device.as_ref().unwrap();
            assert_eq!(binding.device_path, leaf);
            assert_eq!(binding.mount_point, mount);
            assert_eq!(binding.cozo_id, target_sid);
            assert!(binding.attached);
            assert_eq!(journal.phase, "identity-committed");
            assert!(durable.record.is_some());
            let native = op_plugins::state_plugins::incus::IncusRootRepairBackend::local();
            assert!(matches!(
                native
                    .root_subvolume_state(&pool, &format!("{mount}/pool"), &target_sid)
                    .await
                    .unwrap(),
                op_network::incus_root_subvolume::RootSubvolumeState::Present
            ));
        }
    }
    restore_grants_path(previous_grants);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires root-only private Incus/Btrfs/catalog namespace fixture"]
async fn seeded_canonical_provision_private_fixture() {
    run_seeded_canonical_provision_private_fixture(PrivateProvisionOutcome::Committed).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires root-only private Incus/Btrfs/catalog namespace fixture"]
async fn seeded_canonical_provision_native_drift_private_fixture() {
    run_seeded_canonical_provision_private_fixture(PrivateProvisionOutcome::NativeDrift).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires root-only contaminated-seed private fixture"]
async fn seeded_canonical_provision_contaminated_private_fixture() {
    run_seeded_canonical_provision_private_fixture(PrivateProvisionOutcome::ContaminatedSeed).await;
}

// The matching shell launcher creates only private Incus, Btrfs and SHM state.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires root-only private Incus/Btrfs/catalog namespace fixture"]
async fn root_repair_canonical_private_fixture() {
    use op_plugins::state_plugins::identity_sled::SledBtrfsDevice;
    use sha2::{Digest, Sha256};
    use std::io::{Read, Write};
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

    let _lock = refresh_test_lock().lock().await;
    let (engine, _shm, original, _grants_dir, previous_grants, _registry) =
        private_native_fixture(0x93).await;
    assert_eq!(
        original.session_id, "4fc4f83c-aa8e-0dd1-77f8-3f9fa09bf021",
        "private fixture session derivation changed"
    );
    let env = |name| std::env::var(name).expect("private fixture environment missing");
    let seed = env("OP_IDENTITY_TEST_SEED_IMAGE");
    let seed_sha = env("OP_IDENTITY_TEST_SEED_SHA256");
    let seed_fsid = env("OP_IDENTITY_TEST_SEED_FSID");
    let leaf = env("OP_IDENTITY_TEST_LEAF_IMAGE");
    let mount = env("OP_IDENTITY_TEST_MOUNT_POINT");
    let image = env("OP_IDENTITY_TEST_IMAGE_FINGERPRINT");
    let old_image = env("OP_IDENTITY_TEST_OLD_IMAGE");
    let old_mount = env("OP_IDENTITY_TEST_OLD_MOUNT");
    let old_fsid = env("OP_IDENTITY_TEST_OLD_FSID");
    let old_sha256 = env("OP_IDENTITY_TEST_OLD_SHA256");
    let repair_dir = env("OP_IDENTITY_TEST_REPAIR_PLAN_DIR");
    let original_stub = std::path::PathBuf::from(format!(
        "/var/lib/incus/storage-pools/sourcepool/containers/{}",
        original.session_id
    ));
    let original_backup = original_stub.join("backup.yaml");
    let stub_meta = std::fs::symlink_metadata(&original_stub).unwrap();
    let backup_meta = std::fs::symlink_metadata(&original_backup).unwrap();
    assert!(stub_meta.is_dir() && stub_meta.mode() & 0o7777 == 0o100);
    assert!(backup_meta.is_file() && backup_meta.mode() & 0o7777 == 0o400);
    let original_backup_sha256 = hex::encode(Sha256::digest(std::fs::read(&original_backup).unwrap()));
    let actor_id = op_identity::session::derive_principal_id(&original.wireguard_pubkey);
    let grants = std::env::var("OP_GRANTS_PATH").unwrap();
    std::fs::write(
        &grants,
        serde_json::to_vec(&serde_json::json!({
            actor_id.clone(): {"capabilities": [
                "identity_sled.read", "cap.identity.sled.root_repair@v1"
            ]}
        }))
        .unwrap(),
    )
    .unwrap();

    let native = op_plugins::state_plugins::incus::IncusRootRepairBackend::local();
    let snapshot = native.default_project_state().await.unwrap();
    let native_instance = snapshot
        .instances
        .into_iter()
        .find(|instance| instance.name == original.session_id)
        .expect("private stopped source instance absent");
    let mut with_original_binding = original.clone();
    with_original_binding.instance = Some(native_instance);
    with_original_binding.btrfs_device = Some(SledBtrfsDevice {
        device_path: old_image.clone(),
        mount_point: old_mount.clone(),
        btrfs_uuid: old_fsid.clone(),
        cozo_id: old_fsid.clone(),
        attached: true,
    });
    let store = engine.identity_sled_store().unwrap();
    let revision = load_revision(store, &original.session_id).await.unwrap();
    let anchored = commit_sled_update(
        &engine,
        store,
        opid("private-original-binding"),
        revision,
        with_original_binding.clone(),
        with_original_binding,
    )
    .await
    .unwrap();
    assert!(
        anchored.is_anchored(),
        "private original identity is not anchored"
    );

    // Compose the target from the clean pinned seed. Drop the creation guard
    // before the repair owner acquires the same leaf lock, retaining only the
    // native mount and loop mappings inside this disposable namespace.
    let (sprout_fsid, target_pool_source) = tokio::task::spawn_blocking({
        let (seed, seed_sha, seed_fsid, image, leaf, mount) = (
            seed.clone(),
            seed_sha.clone(),
            seed_fsid.clone(),
            image.clone(),
            leaf.clone(),
            mount.clone(),
        );
        move || {
            let descriptor = op_network::seeded_root::SeededProvisionDescriptor {
                seed_image: std::path::Path::new(&seed),
                seed_sha256: &seed_sha,
                seed_fsid: uuid::Uuid::parse_str(&seed_fsid)?,
                image_fingerprint: &image,
                new_leaf_image: std::path::Path::new(&leaf),
                leaf_size_bytes: 1024 * 1024 * 1024,
                mount_point: std::path::Path::new(&mount),
            };
            let mut guard = op_network::seeded_root::provision_seeded_leaf(&descriptor)?;
            guard.prepare_persist_subvolume("pool")?;
            let fsid = guard.receipt.sprout_fsid.to_string();
            drop(guard);
            Ok::<_, anyhow::Error>((fsid, format!("{mount}/pool")))
        }
    })
    .await
    .unwrap()
    .unwrap();

    let preflight = dispatch_identity_sled_method_for_verified_session(
        &engine,
        "inspect_own_root_repair_preflight",
        &serde_json::json!({}),
        &actor_id,
        Some(&original.session_id),
        anchored.genesis.as_deref(),
    )
    .await
    .unwrap();
    assert_eq!(preflight["root_subvolume"], "missing");
    assert_eq!(preflight["source_pool"], "sourcepool");
    let selected = serde_json::json!({
        "device_path": leaf, "mount_point": mount, "btrfs_uuid": sprout_fsid,
        "cozo_id": old_fsid
    });
    let original_binding = serde_json::json!({
        "device_path": old_image, "mount_point": old_mount, "btrfs_uuid": old_fsid,
        "cozo_id": old_fsid
    });
    let plan = serde_json::json!({
        "version": 3,
        "session_id": original.session_id,
        "expected_session_genesis": anchored.genesis.as_deref().unwrap(),
        "expected_revision": preflight["revision"],
        "expected_revision_operation_id": preflight["revision_operation_id"],
        "expected_btrfs_device": original_binding,
        "expected_instance_etag": preflight["instance_etag"],
        "expected_instance_uuid": preflight["instance_uuid"],
        "expected_first_start_idmap_sha256": preflight["expected_first_start_idmap_sha256"],
        "image_fingerprint": image,
        "source_pool": preflight["source_pool"],
        "source_pool_source": preflight["source_pool_source"],
        "target_pool": "seedpool",
        "target_pool_source": target_pool_source,
        "target_persist_source": format!("{mount}/persist"),
        "target_persist_shift": "true",
        "selected_btrfs_device": selected,
        "seed_image_path": seed,
        "seed_sha256": seed_sha,
        "seed_fsid": seed_fsid,
        "leaf_image_path": leaf,
        "leaf_fsid": sprout_fsid,
        "mount_point": mount
    });
    let bytes = serde_json::to_vec(&plan).unwrap();
    let digest = hex::encode(Sha256::digest(&bytes));
    let plan_path = format!("{repair_dir}/{}.json", original.session_id);
    let mut stale = plan.clone();
    stale["expected_revision"] = serde_json::json!(preflight["revision"]
        .as_u64()
        .unwrap()
        .checked_add(1)
        .unwrap());
    let stale_bytes = serde_json::to_vec(&stale).unwrap();
    let stale_digest = hex::encode(Sha256::digest(&stale_bytes));
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&plan_path)
        .unwrap()
        .write_all(&stale_bytes)
        .unwrap();
    let unchanged_revision = load_revision(store, &original.session_id).await.unwrap();
    assert!(
        dispatch_identity_sled_method_for_verified_session(
            &engine,
            "repair_own_root",
            &serde_json::json!({"plan_sha256": stale_digest}),
            &actor_id,
            Some(&original.session_id),
            anchored.genesis.as_deref(),
        )
        .await
        .is_err(),
        "stale trusted revision was accepted"
    );
    assert!(
        store
            .cozo
            .read_root_repair_journal(&original.session_id)
            .unwrap()
            .is_none(),
        "stale preflight entered native repair"
    );
    assert!(
        load_revision(store, &original.session_id).await.unwrap() == unchanged_revision,
        "stale preflight changed durable identity"
    );
    std::fs::write(&plan_path, &bytes).unwrap();
    let result = dispatch_identity_sled_method_for_verified_session(
        &engine,
        "repair_own_root",
        &serde_json::json!({"plan_sha256": digest}),
        &actor_id,
        Some(&original.session_id),
        anchored.genesis.as_deref(),
    )
    .await
    .unwrap();
    assert_eq!(result["status"], "root_and_persist_moved");
    let after = stored_session(&engine, &original.session_id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        after.sealed_id == original.sealed_id,
        "repair changed original SID1"
    );
    assert!(
        after.genesis == original.genesis,
        "repair changed original genesis"
    );
    assert!(
        after
            .btrfs_device
            .as_ref()
            .is_some_and(|device| device.cozo_id == old_fsid),
        "repair changed original Cozo binding key"
    );
    let journal = store
        .cozo
        .read_root_repair_journal(&original.session_id)
        .unwrap()
        .unwrap();
    assert_eq!(journal.phase, "identity-committed");
    let receipt: serde_json::Value = serde_json::from_str(&journal.stub_receipt_json).unwrap();
    assert_eq!(receipt["status"], "quarantined");
    assert_eq!(receipt["session_id"], original.session_id);
    assert_eq!(receipt["plan_sha256"], digest);
    assert_eq!(receipt["operation_id"], journal.operation_id);
    assert_eq!(receipt["original_stub_inode"], stub_meta.ino());
    assert_eq!(receipt["backup_inode"], backup_meta.ino());
    assert_eq!(receipt["backup_sha256"], original_backup_sha256);
    assert_eq!(receipt["original_stub_mode"], 0o100);
    let quarantined = std::path::PathBuf::from(receipt["path"].as_str().unwrap());
    assert_eq!(
        quarantined,
        std::path::Path::new(preflight["source_pool_source"].as_str().unwrap()).join(format!(
            ".opdbus-root-{}-{}",
            original.session_id, journal.operation_id
        ))
    );
    let preserved_stub = std::fs::symlink_metadata(&quarantined).unwrap();
    assert_eq!(preserved_stub.ino(), stub_meta.ino());
    assert_eq!(preserved_stub.mode() & 0o7777, 0o700);
    assert_eq!(
        std::fs::symlink_metadata(quarantined.join("backup.yaml"))
            .unwrap()
            .ino(),
        backup_meta.ino()
    );
    assert_eq!(
        hex::encode(Sha256::digest(
            std::fs::read(quarantined.join("backup.yaml")).unwrap()
        )),
        original_backup_sha256,
        "original Incus backup metadata changed"
    );
    assert!(matches!(
        native
            .root_subvolume_state(
                "sourcepool",
                preflight["source_pool_source"].as_str().unwrap(),
                &original.session_id
            )
            .await
            .unwrap(),
        op_network::incus_root_subvolume::RootSubvolumeState::Missing
    ));
    assert!(matches!(
        native
            .root_subvolume_state("seedpool", &target_pool_source, &original.session_id)
            .await
            .unwrap(),
        op_network::incus_root_subvolume::RootSubvolumeState::Present
    ));
    let (repaired, _) = native.instance(&original.session_id).await.unwrap();
    assert_eq!(
        repaired["config"]["volatile.uuid"],
        preflight["instance_uuid"]
    );
    assert_eq!(repaired["status"], "Stopped");
    assert_eq!(repaired["devices"]["root"]["pool"], "seedpool");
    assert_eq!(
        repaired["devices"]["persist"]["source"],
        format!("{mount}/persist")
    );
    assert_eq!(repaired["devices"]["persist"]["shift"], "true");
    op_network::btrfs::verify_dedicated_mount(
        std::path::Path::new(&old_image),
        std::path::Path::new(&old_mount),
        &old_fsid,
    )
    .unwrap();
    assert!(
        std::fs::read_dir(&old_mount).unwrap().next().is_none(),
        "old persisted filesystem was modified"
    );
    let mut old_hash = Sha256::new();
    let mut old_file = std::fs::File::open(&old_image).unwrap();
    let mut chunk = [0u8; 1024 * 1024];
    loop {
        let read = old_file.read(&mut chunk).unwrap();
        if read == 0 {
            break;
        }
        old_hash.update(&chunk[..read]);
    }
    assert_eq!(
        hex::encode(old_hash.finalize()),
        old_sha256,
        "old persistent image bytes changed"
    );
    restore_grants_path(previous_grants);
}
