//! Isolated catalogs for tests entering the real compiled/sealed admission gate.
#![allow(dead_code)]
use std::sync::{Mutex, MutexGuard};

pub struct CatalogScope {
    _guard: MutexGuard<'static, ()>,
    _root: tempfile::TempDir,
    environment: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

impl CatalogScope {
    pub fn new(schemas: &[op_state_store::PluginSchema]) -> Self {
        static ENVIRONMENT: Mutex<()> = Mutex::new(());
        let guard = ENVIRONMENT
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let root = tempfile::tempdir().unwrap();
        let environment = [
            "OP_BLOB_CATALOG_DIR",
            "OP_SHM_STATE_DIR",
            "DBUS_SESSION_BUS_ADDRESS",
            "OP_PLUGIN_RUNTIME_COZO_PATH",
            "OP_IDENTITY_PROJECTION_GROUP",
        ]
        .into_iter()
        .map(|key| (key, std::env::var_os(key)))
        .collect();
        let catalog = root.path().join("blobs");
        let mut sealer = op_blob::BlobStore::open(&catalog).unwrap();
        for schema in schemas {
            sealer
                .write(&op_blob::blobify_plugin_schema(
                    &schema.name,
                    schema.clone(),
                ))
                .unwrap();
        }
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("OP_BLOB_CATALOG_DIR", catalog) };
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("OP_SHM_STATE_DIR", root.path().join("state")) };
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe {
            std::env::set_var(
                "OP_IDENTITY_PROJECTION_GROUP",
                nix::unistd::Group::from_gid(nix::unistd::Gid::current())
                    .unwrap()
                    .expect("test process group exists")
                    .name,
            )
        };
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe {
            std::env::set_var(
                "OP_PLUGIN_RUNTIME_COZO_PATH",
                root.path().join("runtime-documents"),
            )
        };
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe {
            std::env::set_var(
                "DBUS_SESSION_BUS_ADDRESS",
                format!("unix:path={}/absent-bus", root.path().display()),
            )
        };
        Self {
            _guard: guard,
            _root: root,
            environment,
        }
    }

    pub fn root(&self) -> &std::path::Path {
        self._root.path()
    }
}

impl Drop for CatalogScope {
    fn drop(&mut self) {
        for (key, value) in &self.environment {
            match value {
                // FIXME: Audit that the environment access only happens in single-threaded code.
                Some(value) => unsafe { std::env::set_var(key, value) },
                // FIXME: Audit that the environment access only happens in single-threaded code.
                None => unsafe { std::env::remove_var(key) },
            }
        }
    }
}

/// Seed the result of an isolated identity-container provision before asking
/// MutationEngine to recover and mint its SID1. This mirrors the production
/// typed Cozo lifecycle boundary without invoking Incus in a test.
pub fn provision_identity_record(
    cozo: &op_cozo_store::CozoGraphShuttle,
    root: &std::path::Path,
    wireguard_pubkey: &str,
) -> anyhow::Result<()> {
    use op_cozo_store::{IdentityCompareExchange, IdentityIntent, IdentitySledRecord};
    use op_plugins::state_plugins::{
        identity_sled::{ContainerIdentitySled, SledBtrfsDevice, RECORD_FORMAT},
        incus::IncusInstance,
    };

    let session_id = op_identity::session::derive_session_id(wireguard_pubkey);
    let now = chrono::Utc::now().timestamp();
    let btrfs_root = root.join("identity-btrfs").join(&session_id);
    std::fs::create_dir_all(&btrfs_root)?;
    let sled = ContainerIdentitySled {
        session_id: session_id.clone(),
        wireguard_pubkey: wireguard_pubkey.to_owned(),
        interface: String::new(),
        peer_ip: None,
        mutation_index: 0,
        genesis: None,
        trace_id: String::new(),
        schema_version: RECORD_FORMAT,
        vector_id: String::new(),
        sealed_id: None,
        principal_kind: Some("human".to_owned()),
        btrfs_device: Some(SledBtrfsDevice {
            device_path: btrfs_root.join("fstorage.img").display().to_string(),
            mount_point: btrfs_root.join("mount").display().to_string(),
            btrfs_uuid: "00000000-0000-0000-0000-000000000001".to_owned(),
            cozo_id: session_id.clone(),
            attached: false,
        }),
        instance: Some(IncusInstance {
            name: session_id.clone(),
            status: "Stopped".to_owned(),
            instance_type: "container".to_owned(),
            profiles: vec!["identity".to_owned()],
            config: Some(std::collections::HashMap::from([(
                "boot.autostart".to_owned(),
                "false".to_owned(),
            )])),
            ..Default::default()
        }),
        session_started_at: now,
        last_seen_at: now,
        active: false,
        expires_at: None,
        arrival_timestamp: 0,
        chain_head_at_arrival: String::new(),
        catalog_hash_at_arrival: String::new(),
        head_timestamp_at_arrival: 0,
    };
    let expected = cozo.read_identity_revision(&session_id)?;
    cozo.apply_identity_intent(
        &session_id,
        &format!("fixture-provision-{}", uuid::Uuid::new_v4().simple()),
        IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
            expected,
            record: Some(IdentitySledRecord {
                session_id: sled.session_id.clone(),
                wireguard_pubkey: sled.wireguard_pubkey.clone(),
                interface: sled.interface.clone(),
                peer_ip: sled.peer_ip.clone().unwrap_or_default(),
                mutation_index: 0,
                session_genesis: String::new(),
                trace_id: String::new(),
                schema_version: i64::from(RECORD_FORMAT),
                vector_id: sled.vector_id.clone(),
                sealed_id: String::new(),
                principal_kind: sled.principal_kind.clone(),
                btrfs_device_json: serde_json::to_string(
                    sled.btrfs_device.as_ref().expect("fixture btrfs device"),
                )?,
                instance_json: serde_json::to_string(
                    sled.instance.as_ref().expect("fixture identity instance"),
                )?,
                session_started_at: sled.session_started_at,
                last_seen_at: sled.last_seen_at,
                active: false,
                expires_at: 0,
            }),
            genesis: None,
        })),
    )?;
    Ok(())
}
