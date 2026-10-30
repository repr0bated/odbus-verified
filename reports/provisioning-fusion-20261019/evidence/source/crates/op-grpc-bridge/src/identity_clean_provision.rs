//! Fresh same-key adoption of an externally prepared shared-seed container.
//! The owner verifies native/storage state and performs only an atomic identity commit.
use super::*;
use anyhow::{Result, ensure};
use op_cozo_store::{CleanProvisionReceipt, IdentityCleanProvision};
use op_plugins::state_plugins::{identity_sled::SledBtrfsDevice, incus::IncusRootRepairBackend};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Component, Path},
};

#[path = "identity_clean_provision/storage.rs"]
mod storage;

const PLAN_DIR: &str = "/var/lib/op-dbus/identity-provision-plans";
const PARTITIONED_POOL: &str = "/var/lib/incus/storage-pools/3tched-pool";
const PARTITIONED_POOL_FSID: &str = "a1ab7b42-12a8-437c-8f65-d0469892c8dd";
const MEMBERS: [&str; 7] = [
    "@",
    "@agent_memory",
    "@agent_rules",
    "@agent_shared",
    "@agent_skills",
    "@agent_soul",
    "@agent_user",
];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MemberPin {
    id: u64,
    uuid: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PartitionPin {
    partuuid: String,
    start_sector: u64,
    sectors: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PartitionedOverlay {
    disk_guid: String,
    root_partition: PartitionPin,
    upgrade_partition: PartitionPin,
    upgrade_fsid: String,
    host_upgrade_fsid: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CleanPlan {
    version: u8,
    session_id: String,
    wireguard_pubkey: String,
    expected_revision: u64,
    expected_revision_operation_id: String,
    binding: SledBtrfsDevice,
    seed_device: String,
    seed_fsid: String,
    seed_generation: u64,
    leaf_inode: u64,
    leaf_size: u64,
    members: BTreeMap<String, MemberPin>,
    persist_inode: u64,
    native_root: String,
    pool_name: String,
    pool_source: String,
    instance_etag: String,
    instance: JsonValue,
    previous_instance_etag: String,
    previous_instance: JsonValue,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    partitioned_overlay: Option<PartitionedOverlay>,
}

fn canonical(path: &str) -> bool {
    Path::new(path).is_absolute()
        && Path::new(path)
            .components()
            .all(|p| matches!(p, Component::RootDir | Component::Normal(_)))
}
fn native_uuid(instance: &JsonValue) -> Result<&str> {
    let value = instance["config"]["volatile.uuid"]
        .as_str()
        .context("native UUID missing")?;
    ensure!(
        uuid::Uuid::parse_str(value)?.to_string() == value,
        "native UUID is noncanonical"
    );
    Ok(value)
}
fn canonical_key(key: &str) -> Result<()> {
    let decoded = base64::engine::general_purpose::STANDARD.decode(key)?;
    ensure!(
        decoded.len() == 32 && base64::engine::general_purpose::STANDARD.encode(decoded) == key,
        "noncanonical public key"
    );
    Ok(())
}

impl CleanPlan {
    fn native_name(&self) -> Result<&str> {
        self.instance["name"].as_str().context("clean native name missing")
    }
    fn validate(&self) -> Result<()> {
        let native_name = self.native_name()?;
        ensure!(native_name == CONTROL_PLANE_CHATBOT_CONTAINER || native_name == RESERVED_CHATBOT_CONTAINER
            || op_cozo_store::is_numbered_native_name(native_name),
            "invalid clean native name");
        canonical_key(&self.wireguard_pubkey)?;
        ensure!(
            matches!(self.version, 2 | 3 | 4)
                && self.session_id
                    == op_identity::session::derive_session_id(&self.wireguard_pubkey)
                && control_plane_chatbot_session_id().as_deref() == Some(self.session_id.as_str())
                && configured_chatbot_key().as_deref() == Some(self.wireguard_pubkey.as_str()),
            "clean adoption is only for the configured chatbot key"
        );
        if self.version == 4 {
            let layout = self.partitioned_overlay.as_ref().context("partitioned overlay pins absent")?;
            let root = &layout.root_partition;
            let upgrade = &layout.upgrade_partition;
            ensure!(self.binding.device_path == format!("{PARTITIONED_POOL}/{native_name}.img")
                && self.binding.mount_point == format!("{PARTITIONED_POOL}/.mounts/{native_name}/root")
                && root.start_sector == 2048 && root.sectors >= 2_097_152
                && root.start_sector.checked_add(root.sectors) == Some(upgrade.start_sector)
                && upgrade.sectors >= 2_097_152
                && upgrade.start_sector.checked_add(upgrade.sectors)
                    == self.leaf_size.checked_div(512).and_then(|sectors| sectors.checked_sub(2048))
                && self.leaf_size % 512 == 0,
                "partition geometry or external mount paths differ");
            let mut uuids = HashSet::new();
            for value in [&layout.disk_guid, &root.partuuid, &upgrade.partuuid, &layout.upgrade_fsid,
                &layout.host_upgrade_fsid, &self.seed_fsid, &self.binding.btrfs_uuid] {
                ensure!(uuid::Uuid::parse_str(value)?.to_string() == *value && uuids.insert(value),
                    "partition or filesystem identities are invalid or reused");
            }
            ensure!(layout.host_upgrade_fsid == "7ac6c955-4028-439c-80b4-c55a647abcb4",
                "host upgrade identity differs");
        } else {
            ensure!(self.partitioned_overlay.is_none(), "legacy plan includes partitioned layout");
        }
        ensure!(
            self.binding.cozo_id == self.session_id
                && self.binding.attached
                && self.leaf_inode > 0
                && self.leaf_size >= 1024 * 1024 * 1024
                && self.persist_inode > 256
                && self.seed_generation > 0
                && !self.instance_etag.is_empty(),
            "invalid clean storage/native provenance"
        );
        for path in [
            &self.seed_device,
            &self.binding.device_path,
            &self.binding.mount_point,
            &self.native_root,
            &self.pool_source,
        ] {
            ensure!(canonical(path), "noncanonical clean plan path");
        }
        ensure!(
            self.seed_device.starts_with("/dev/")
                && !self.binding.device_path.starts_with("/dev/")
                && uuid::Uuid::parse_str(&self.seed_fsid)?.to_string() == self.seed_fsid
                && uuid::Uuid::parse_str(&self.binding.btrfs_uuid)?.to_string()
                    == self.binding.btrfs_uuid
                && self.seed_fsid != self.binding.btrfs_uuid,
            "invalid shared seed/leaf identities"
        );
        ensure!(
            self.members.len() == 7 && MEMBERS.iter().all(|name| self.members.contains_key(*name)),
            "clean plan requires exactly seven inherited siblings"
        );
        let mut ids = HashSet::new();
        let mut uuids = HashSet::new();
        for pin in self.members.values() {
            ensure!(
                pin.id >= 256
                    && uuid::Uuid::parse_str(&pin.uuid)?.to_string() == pin.uuid
                    && ids.insert(pin.id)
                    && uuids.insert(&pin.uuid),
                "invalid inherited member identities"
            );
        }
        ensure!(
            self.pool_name == "identity-roots"
                && self.native_root
                    == format!(
                        "/var/lib/incus/storage-pools/{}/containers/{}/rootfs",
                        self.pool_name, native_name
                    )
                && (self.version != 4 || self.pool_source == "/var/lib/incus-storage/identity-roots")
                && self.instance["type"] == "container"
                && self.instance["status"] == "Stopped"
                && self.instance["status_code"] == 102,
            "clean plan requires the stopped fresh native container"
        );
        native_uuid(&self.instance)?;
        if self.version == 2 {
            ensure!(
                !self.previous_instance_etag.is_empty()
                    && self.previous_instance["type"] == "container"
                    && self.previous_instance["status"] == "Stopped"
                    && self.previous_instance["name"] != self.instance["name"]
                    && native_uuid(&self.instance)? != native_uuid(&self.previous_instance)?,
                "replacement requires distinct stopped native containers"
            );
        } else {
            ensure!(
                self.previous_instance.is_null() && self.previous_instance_etag.is_empty(),
                "post-deletion provision must not import the previous native record"
            );
        }
        ensure!(
            self.instance["profiles"] == serde_json::json!(["3tched-lungs"]),
            "unexpected fresh native profiles"
        );
        for config_field in ["config", "expanded_config"] {
            let config = self.instance[config_field]
                .as_object()
                .context("native config absent")?;
            ensure!(
                config
                    .get("security.privileged")
                    .and_then(JsonValue::as_str)
                    == Some("false")
                    && config.get("security.nesting").and_then(JsonValue::as_str) == Some("false")
                    && config
                        .get("security.protection.shift")
                        .and_then(JsonValue::as_str)
                        == Some("true")
                    && config.get("boot.autostart").and_then(JsonValue::as_str) == Some("false")
                    && config
                        .get("security.syscalls.deny_default")
                        .and_then(JsonValue::as_str)
                        == Some("true")
                    && config
                        .get("user.opdbus.session_id")
                        .and_then(JsonValue::as_str)
                        == Some(self.session_id.as_str())
                    && config
                        .get("user.opdbus.wireguard_public_key")
                        .and_then(JsonValue::as_str)
                        == Some(self.wireguard_pubkey.as_str())
                    && config.get("raw.lxc").and_then(JsonValue::as_str)
                        == Some(format!("lxc.uts.name = {}", self.session_id).as_str())
                    && !config.contains_key("raw.seccomp")
                    && !config.contains_key("security.syscalls.allow")
                    && !config.contains_key("security.syscalls.whitelist")
                    && config
                        .keys()
                        .all(|key| !key.starts_with("raw.") || key == "raw.lxc"),
                "fresh native identity/security policy differs"
            );
            let deny = config
                .get("security.syscalls.deny")
                .and_then(JsonValue::as_str)
                .context("native storage protection absent")?;
            let expected: std::collections::BTreeSet<_> = [
                "ioctl errno 1 [1,0x4008941a,SCMP_CMP_MASKED_EQ,0xffffffff]",
                "ioctl errno 1 [1,0x5000940f,SCMP_CMP_MASKED_EQ,0xffffffff]",
                "ioctl errno 1 [1,0x5000943f,SCMP_CMP_MASKED_EQ,0xffffffff]",
            ]
            .into_iter()
            .collect();
            ensure!(
                deny.lines()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .collect::<std::collections::BTreeSet<_>>()
                    == expected,
                "native storage protection differs"
            );
        }
        for field in ["devices", "expanded_devices"] {
            let devices = self.instance[field]
                .as_object()
                .context("native disks absent")?;
            ensure!(
                devices.iter().all(|(name, d)| d["type"] == "disk"
                    || (self.version == 4 && name == "fuse"
                        && *d == serde_json::json!({"type":"unix-char","source":"/dev/fuse","path":"/dev/fuse"})))
                    && devices["root"]["path"] == "/"
                    && devices["root"]["pool"] == self.pool_name,
                "fresh native requires disk-only devices and pinned root"
            );
            if self.version == 4 {
                ensure!(devices.get("fuse") == Some(&serde_json::json!(
                    {"type":"unix-char","source":"/dev/fuse","path":"/dev/fuse"}))
                    && !devices.contains_key("3tchedfs") && !devices.contains_key("system-bus"),
                    "partitioned guest must own its FUSE mount without a host bus bind");
            }
        }
        Ok(())
    }
    fn validate_expected(&self, expected: &IdentityRevision) -> Result<()> {
        self.validate()?;
        ensure!(
            expected.session_id == self.session_id
                && expected.revision == self.expected_revision
                && expected.operation_id == self.expected_revision_operation_id,
            "clean plan revision changed"
        );
        if self.version == 3 {
            ensure!(
                deleted_for_provision(expected),
                "fresh provision requires the exact explicit deletion tombstone"
            );
            return Ok(());
        }
        if self.version == 4 {
            ensure!(fresh_for_provision(expected) || deleted_for_provision(expected),
                "partitioned adoption requires a fresh key or explicit deletion tombstone");
            return Ok(());
        }
        let old = expected
            .record
            .as_ref()
            .context("clean replacement requires existing identity")?;
        let old_instance: IncusInstance = serde_json::from_str(&old.instance_json)?;
        ensure!(
            expected.session_id == self.session_id
                && expected.revision == self.expected_revision
                && expected.operation_id == self.expected_revision_operation_id
                && old.wireguard_pubkey == self.wireguard_pubkey
                && old_instance.name
                    == self.previous_instance["name"]
                        .as_str()
                        .context("old native name missing")?
                && old_instance
                    .config
                    .as_ref()
                    .and_then(|c| c.get("volatile.uuid"))
                    .map(String::as_str)
                    == Some(native_uuid(&self.previous_instance)?)
                && old.btrfs_device_json != serde_json::to_string(&self.binding)?,
            "clean plan does not match previous authoritative binding/revision"
        );
        if let Some(device) = revision_sled(expected)?.and_then(|sled| sled.btrfs_device) {
            ensure!(
                device.device_path != self.binding.device_path
                    && device.mount_point != self.binding.mount_point
                    && device.btrfs_uuid != self.binding.btrfs_uuid,
                "clean adoption cannot reuse old leaf storage"
            );
        }
        Ok(())
    }
}

fn fresh_for_provision(expected: &IdentityRevision) -> bool {
    expected.revision == 0 && expected.operation_id.is_empty()
        && expected.record.is_none() && expected.genesis.is_none()
}

fn deleted_for_provision(expected: &IdentityRevision) -> bool {
    expected.revision > 1
        && expected.record.is_none()
        && expected.genesis.is_none()
        && expected.operation_id
            == format!(
                "delete-provision-{}-{}",
                expected.session_id,
                expected.revision - 1
            )
}

fn parsed_native(raw: &JsonValue) -> Result<IncusInstance> {
    IncusPlugin::instance_from_native_metadata(raw.clone())
}

pub(super) fn binding_matches(pinned: &str, current: &str) -> bool {
    op_cozo_store::clean_provision_instance_binding_matches(pinned, current)
}
async fn receipt(store: &IdentitySledStore, sid: &str) -> Result<Option<CleanProvisionReceipt>> {
    let cozo = store.cozo.clone();
    let sid = sid.to_owned();
    Ok(tokio::task::spawn_blocking(move || cozo.read_clean_provision(&sid)).await??)
}
async fn native_proof(
    plan: &CleanPlan,
    strict_stopped: bool,
) -> Result<(storage::StorageGuard, IncusInstance)> {
    let native = IncusRootRepairBackend::local();
    let (instance, etag) = native.instance(plan.native_name()?).await?;
    if strict_stopped {
        ensure!(
            instance == plan.instance && etag == plan.instance_etag,
            "fresh native definition/ETag changed"
        );
        if plan.version == 2 {
            let (old, old_etag) = native
                .instance(
                    plan.previous_instance["name"]
                        .as_str()
                        .context("old native name absent")?,
                )
                .await?;
            ensure!(
                old == plan.previous_instance && old_etag == plan.previous_instance_etag,
                "retained old native definition/ETag changed"
            );
        }
    } else {
        ensure!(
            binding_matches(
                &serde_json::to_string(&parsed_native(&plan.instance)?)?,
                &serde_json::to_string(&parsed_native(&instance)?)?
            ),
            "clean native storage/UUID/config changed"
        );
    }
    let pool = native
        .pool(&plan.pool_name)
        .await?
        .context("native pool absent")?;
    ensure!(
        pool["driver"] == "dir"
            && pool["status"] == "Created"
            && pool["config"]["source"] == plan.pool_source,
        "native root pool changed"
    );
    let parsed = parsed_native(&instance)?;
    let plan = plan.clone();
    let guard = tokio::task::spawn_blocking(move || storage::verify(&plan)).await??;
    Ok((guard, parsed))
}

pub(super) struct CleanRootGuard {
    pub should_start: bool,
    _storage: storage::StorageGuard,
}

pub(super) async fn guard_start(
    store: &IdentitySledStore,
    revision: &IdentityRevision,
    sled: &ContainerIdentitySled,
) -> Result<Option<CleanRootGuard>> {
    let Some(receipt) = receipt(store, &sled.session_id).await? else {
        return Ok(None);
    };
    let plan: CleanPlan = serde_json::from_str(&receipt.provenance_json)?;
    plan.validate()?;
    ensure!(
        revision.revision >= receipt.committed_revision
            && sled.wireguard_pubkey == plan.wireguard_pubkey
            && sled.btrfs_device.as_ref().is_some_and(|device| {
                let mut device = device.clone();
                device.attached = true;
                device == plan.binding
            })
            && sled.instance.as_ref().is_some_and(|i| binding_matches(
                &receipt.instance_json,
                &serde_json::to_string(i).unwrap_or_default()
            )),
        "clean receipt does not match authoritative binding"
    );
    let (guard, native) = native_proof(&plan, false).await?;
    let should_start = root_repair::decide_seeded_native_start(sled.active, &native.status)?;
    Ok(Some(CleanRootGuard {
        should_start,
        _storage: guard,
    }))
}

pub(super) async fn observe_power(
    engine: &MutationEngine,
    store: &IdentitySledStore,
    revision: &IdentityRevision,
    sled: &ContainerIdentitySled,
    status: &str,
) -> Result<Option<IncusInstance>> {
    let Some(_guard) = guard_start(store, revision, sled).await? else {
        return Ok(None);
    };
    let snapshot = IncusRootRepairBackend::local()
        .default_project_state()
        .await?;
    let instance = snapshot
        .instances
        .iter()
        .find(|instance| sled.instance.as_ref().is_some_and(|stored| instance.name == stored.name))
        .context("clean native power readback absent")?
        .clone();
    ensure!(
        instance.status == status && identity_instance_matches(sled, &instance),
        "clean native power readback differs"
    );
    crate::incus_observer::publish_repair_readback_locked(engine, &snapshot).await?;
    Ok(Some(instance))
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn provision(
    engine: &MutationEngine,
    store: &IdentitySledStore,
    actor: VerifiedIdentityActor<'_>,
    input: seed_provision::ProvisionInput,
    seal: seed_provision::ProvisionSealPin,
    _request_operation: &str,
    preflight: bool,
) -> Result<JsonValue> {
    canonical_key(&input.wireguard_pubkey)?;
    let sid = op_identity::session::derive_session_id(&input.wireguard_pubkey);
    ensure!(
        control_plane_chatbot_session_id().as_deref() == Some(sid.as_str())
            && configured_chatbot_key().as_deref() == Some(input.wireguard_pubkey.as_str()),
        "clean provision requires configured chatbot key"
    );
    let _lifecycle = store.lifecycle_lock(&sid).await.lock_owned().await;
    let _host = engine.lock_host_observation().await;
    seed_provision::admit(
        engine,
        store,
        actor.actor_id,
        actor.session_id,
        actor.genesis,
        &seal,
    )
    .await?;
    seed_provision::target_key_is_registered(&input.wireguard_pubkey).await?;
    let expected = load_revision(store, &sid).await?;
    let old = revision_sled(&expected)?;
    ensure!(
        old.as_ref()
            .is_some_and(|old| old.wireguard_pubkey == input.wireguard_pubkey)
            || deleted_for_provision(&expected) || fresh_for_provision(&expected),
        "clean provision requires the existing key or its explicit deletion tombstone"
    );
    if preflight {
        preflight_native_identity_effect().await?;
        let Some(old) = old else {
            seed_provision::admit(
                engine,
                store,
                actor.actor_id,
                actor.session_id,
                actor.genesis,
                &seal,
            )
            .await?;
            return Ok(
                serde_json::json!({"session_id":sid,"wireguard_pubkey":input.wireguard_pubkey,"expected_revision":expected.revision,"expected_revision_operation_id":expected.operation_id,
                "previous_instance":null,"previous_instance_etag":"","previous_native_instance":null}),
            );
        };
        let stored_instance = old
            .instance
            .context("previous native instance binding absent")?;
        let (native, etag) = delete_provision::retirement_native(&stored_instance)
            .await?
            .unwrap_or((JsonValue::Null, String::new()));
        seed_provision::admit(
            engine,
            store,
            actor.actor_id,
            actor.session_id,
            actor.genesis,
            &seal,
        )
        .await?;
        return Ok(
            serde_json::json!({"session_id":sid,"wireguard_pubkey":input.wireguard_pubkey,"expected_revision":expected.revision,"expected_revision_operation_id":expected.operation_id,
            "previous_instance":stored_instance,"previous_instance_etag":etag,"previous_native_instance":native}),
        );
    }
    ensure!(
        root_repair::valid_digest(&input.plan_sha256),
        "invalid adoption digest"
    );
    let digest = input.plan_sha256.clone();
    let read_sid = sid.clone();
    let bytes = tokio::task::spawn_blocking(move || {
        root_repair::trusted_plan_bytes_at(Path::new(PLAN_DIR), &read_sid, &digest, 0)
    })
    .await??;
    let plan: CleanPlan = serde_json::from_slice(&bytes)?;
    plan.validate()?;
    let operation = format!("clean-provision-{}", input.plan_sha256);
    if let Some(receipt) = receipt(store, &sid).await?
        && receipt.plan_sha256 == input.plan_sha256
    {
        ensure!(
            receipt.provenance_json == serde_json::to_string(&plan)?,
            "replay plan changed"
        );
        let old = old.context("clean replay record missing")?;
        guard_start(store, &expected, &old)
            .await?
            .context("clean replay storage receipt missing")?;
        seed_provision::admit(
            engine,
            store,
            actor.actor_id,
            actor.session_id,
            actor.genesis,
            &seal,
        )
        .await?;
        recover_identity_startup_with_store(engine, store).await?;
        return Ok(
            serde_json::json!({"identity":old,"plan_sha256":input.plan_sha256,"operation_id":receipt.operation_id,"committed_revision":receipt.committed_revision,"replayed":true}),
        );
    }
    plan.validate_expected(&expected)?;
    preflight_native_identity_effect().await?;
    let (_storage, instance) = native_proof(&plan, true).await?;
    let timestamp = now();
    let mut fresh: ContainerIdentitySled = serde_json::from_value(
        serde_json::json!({"session_id":sid,"wireguard_pubkey":input.wireguard_pubkey}),
    )?;
    fresh.schema_version = RECORD_FORMAT;
    fresh.principal_kind = Some("service".to_string());
    fresh.btrfs_device = Some(plan.binding.clone());
    fresh.instance = Some(instance);
    fresh.session_started_at = timestamp;
    fresh.last_seen_at = timestamp;
    let stamp = engine
        .mint_identity_genesis_stamp(&sid, &fresh.wireguard_pubkey, 0)
        .await?;
    apply_genesis_stamp(&mut fresh, &stamp)?;
    validate_sled(&fresh)?;
    ensure!(
        load_revision(store, &sid).await? == expected,
        "identity changed during external adoption proof"
    );
    seed_provision::admit(
        engine,
        store,
        actor.actor_id,
        actor.session_id,
        actor.genesis,
        &seal,
    )
    .await?;
    let (_final_storage, _) = native_proof(&plan, true).await?;
    let cozo = store.cozo.clone();
    let operation_for_commit = operation.clone();
    let plan_hash = input.plan_sha256.clone();
    let record = sled_to_record(&fresh)?;
    let genesis = sled_to_genesis(&fresh);
    let provenance = serde_json::to_string(&plan)?;
    let value = fresh.clone();
    let publication = engine
        .commit_identity_intent(operation.clone(), move |_| {
            let committed = cozo.apply_identity_intent(
                &sid,
                &operation_for_commit,
                IdentityIntent::CleanProvision(Box::new(IdentityCleanProvision {
                    expected,
                    record,
                    genesis,
                    plan_sha256: plan_hash,
                    provenance_json: provenance,
                })),
            )?;
            let state = cozo.read_identity_recovery_state(MAX_EVENTS_IN_STATE)?;
            Ok((
                Some(state_value(&state)?),
                DurableResult {
                    value: (value, committed.current.revision),
                    replayed: committed.replayed,
                },
            ))
        })
        .await?;
    let (identity, committed_revision) = publication_value(publication)?;
    Ok(
        serde_json::json!({"identity":identity,"plan_sha256":input.plan_sha256,"operation_id":operation,"committed_revision":committed_revision,"replayed":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clean_binding_allows_power_but_pins_uuid_devices_and_policy() {
        let before = serde_json::json!({"name":"3tchedChatbot","status":"Stopped","status_code":102,"last_used_at":"old",
            "config":{"volatile.uuid":"native-id","volatile.idmap.next":"prepared","security.privileged":"false"},"expanded_config":{},"devices":[{"name":"root"}],"profiles":["3tched-lungs"]});
        let mut running = before.clone();
        running["status"] = "Running".into();
        running["status_code"] = 103.into();
        running["last_used_at"] = "new".into();
        running["config"]["volatile.idmap.current"] = "prepared".into();
        assert!(binding_matches(&before.to_string(), &running.to_string()));
        for (key, value) in [
            ("volatile.uuid", "foreign"),
            ("security.privileged", "true"),
        ] {
            let mut changed = running.clone();
            changed["config"][key] = value.into();
            assert!(!binding_matches(&before.to_string(), &changed.to_string()));
        }
        running["profiles"] = serde_json::json!(["default"]);
        assert!(!binding_matches(&before.to_string(), &running.to_string()));
        assert!(!binding_matches("{}", "{}"));
    }
}
