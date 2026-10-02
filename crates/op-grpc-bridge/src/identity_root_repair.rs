//! Own-session, digest-pinned repair of a missing Incus root. This is an
//! isolated proposal: no grant or trusted plan is installed by this module.
//! Native uncertainty is journaled in the existing private Cozo identity
//! ledger and never treated as a successful identity publication.

use super::*;
use anyhow::{Context, Result};
use async_trait::async_trait;
use base64::Engine;
use op_plugins::state_plugins::identity_sled::SledBtrfsDevice;
use op_plugins::state_plugins::incus::{IncusRootRepairBackend, IncusState};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::ffi::CString;
use std::fs::{self, File};
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};

const REPAIR_CAPABILITY: &str = "cap.identity.sled.root_repair@v1";
const PLAN_DIR: &str = "/var/lib/op-dbus/identity-repair-plans";
const MAX_PLAN_BYTES: u64 = 32 * 1024;

pub(super) struct RepairSealPin {
    schema_hash: String,
    blob: op_blob::PluginObjectBlob,
}

pub(super) fn capture_repair_seal() -> Result<RepairSealPin> {
    crate::method_contract_admission::admit_compiled_method(
        "identity_sled",
        "repair_own_root",
        Some(REPAIR_CAPABILITY),
    )?;
    let pinned = strict_identity_contract()?;
    Ok(RepairSealPin {
        schema_hash: pinned.schema_hash,
        blob: pinned.blob,
    })
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RepairOwnRootInput {
    plan_sha256: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RootRepairPlan {
    version: u8,
    session_id: String,
    expected_session_genesis: String,
    expected_revision: u64,
    expected_revision_operation_id: String,
    expected_btrfs_device: Option<PlanBtrfsBinding>,
    expected_instance_etag: String,
    expected_instance_uuid: String,
    expected_first_start_idmap_sha256: String,
    image_fingerprint: String,
    source_pool: String,
    source_pool_source: String,
    target_pool: String,
    target_pool_source: String,
    target_persist_source: String,
    target_persist_shift: String,
    selected_btrfs_device: PlanBtrfsBinding,
    seed_image_path: String,
    seed_sha256: String,
    seed_fsid: String,
    leaf_image_path: String,
    leaf_fsid: String,
    mount_point: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanBtrfsBinding {
    device_path: String,
    mount_point: String,
    btrfs_uuid: String,
    cozo_id: String,
}

impl PlanBtrfsBinding {
    fn matches(&self, actual: &SledBtrfsDevice) -> bool {
        self.device_path == actual.device_path
            && self.mount_point == actual.mount_point
            && self.btrfs_uuid == actual.btrfs_uuid
            && self.cozo_id == actual.cozo_id
    }
}

impl RootRepairPlan {
    fn validate_identity_anchor(&self, sled: &ContainerIdentitySled, selected: bool) -> Result<()> {
        let pinned = if selected {
            Some(&self.selected_btrfs_device)
        } else {
            self.expected_btrfs_device.as_ref()
        };
        anyhow::ensure!(
            sled.is_anchored()
                && sled.session_id == self.session_id
                && sled.genesis.as_deref() == Some(self.expected_session_genesis.as_str())
                && pinned
                    .zip(sled.btrfs_device.as_ref())
                    .is_some_and(|(pinned, actual)| pinned.matches(actual)),
            "current identity anchor or Btrfs binding differs from trusted plan"
        );
        Ok(())
    }

    fn validate_structure(&self) -> Result<()> {
        anyhow::ensure!(self.version == 3, "unsupported root repair plan version");
        anyhow::ensure!(
            uuid::Uuid::parse_str(&self.expected_instance_uuid)
                .is_ok_and(|uuid| uuid.to_string() == self.expected_instance_uuid)
                && uuid::Uuid::parse_str(&self.seed_fsid).is_ok()
                && uuid::Uuid::parse_str(&self.leaf_fsid).is_ok(),
            "root repair plan has invalid native UUIDs"
        );
        anyhow::ensure!(
            valid_digest(&self.image_fingerprint)
                && valid_digest(&self.seed_sha256)
                && valid_digest(&self.expected_first_start_idmap_sha256),
            "root repair plan has invalid provenance digests"
        );
        for name in [&self.source_pool, &self.target_pool] {
            op_plugins::state_plugins::incus::validate_root_repair_pool_name(name)?;
        }
        anyhow::ensure!(
            self.source_pool != self.target_pool,
            "root repair target pool must be dedicated"
        );
        anyhow::ensure!(
            Path::new(&self.source_pool_source).is_absolute()
                && self.source_pool_source != self.target_pool_source
                && Path::new(&self.source_pool_source)
                    .components()
                    .all(|part| matches!(part, Component::RootDir | Component::Normal(_))),
            "root repair source pool path is not pinned and normalized"
        );
        let mount = Path::new(&self.mount_point);
        let source = Path::new(&self.target_pool_source);
        let child = source.strip_prefix(mount)?;
        let persist = Path::new(&self.target_persist_source);
        let old = self
            .expected_btrfs_device
            .as_ref()
            .context("trusted plan lacks original fstorage binding")?;
        anyhow::ensure!(
            mount.is_absolute()
                && source.is_absolute()
                && matches!(
                    child.components().collect::<Vec<_>>().as_slice(),
                    [Component::Normal(_)]
                )
                && persist == mount.join("persist")
                && self.target_persist_shift == "true"
                && source != persist
                && Path::new(&old.device_path).is_absolute()
                && Path::new(&old.mount_point).is_absolute()
                && uuid::Uuid::parse_str(&old.btrfs_uuid).is_ok()
                && self.selected_btrfs_device.device_path == self.leaf_image_path
                && self.selected_btrfs_device.mount_point == self.mount_point
                && self.selected_btrfs_device.btrfs_uuid == self.leaf_fsid
                && self.selected_btrfs_device.cozo_id == old.cozo_id,
            "root repair storage layout or Btrfs binding differs from trusted plan"
        );
        for path in [&self.seed_image_path, &self.leaf_image_path] {
            anyhow::ensure!(
                Path::new(path).is_absolute(),
                "root repair image path is relative"
            );
        }
        Ok(())
    }

    fn validate(
        &self,
        own_session: &str,
        revision: &IdentityRevision,
        sled: &ContainerIdentitySled,
    ) -> Result<()> {
        self.validate_structure()?;
        self.validate_identity_anchor(sled, false)?;
        anyhow::ensure!(
            self.session_id == own_session
                && revision.session_id == own_session
                && self.expected_revision == revision.revision
                && self.expected_revision_operation_id == revision.operation_id,
            "root repair plan does not match the current identity revision"
        );
        Ok(())
    }

    fn validate_completed_journal(
        &self,
        journal: &op_cozo_store::RootRepairJournal,
        revision: &IdentityRevision,
        sled: &ContainerIdentitySled,
    ) -> Result<()> {
        anyhow::ensure!(
            self.version == 3
                && journal.version == 1
                && journal.phase == "identity-committed"
                && journal.expected_revision == self.expected_revision
                && journal.expected_revision_operation_id == self.expected_revision_operation_id
                && journal.committed_revision > journal.expected_revision
                && revision.revision >= journal.committed_revision,
            "identity root repair journal does not certify this plan"
        );
        self.validate_structure()?;
        self.validate_identity_anchor(sled, true)
    }
}

pub(super) fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub(super) fn trusted_plan_bytes_at(
    directory: &Path,
    session_id: &str,
    digest: &str,
    owner_uid: u32,
) -> Result<Vec<u8>> {
    anyhow::ensure!(
        valid_digest(digest),
        "plan_sha256 must be 64 lowercase hex digits"
    );
    let parsed = uuid::Uuid::parse_str(session_id).context("invalid own session ID")?;
    anyhow::ensure!(
        parsed.to_string() == session_id,
        "noncanonical own session ID"
    );
    anyhow::ensure!(directory.is_absolute(), "plan directory is not absolute");
    // Walk with held directory descriptors. Checking path strings first and
    // reopening the final pathname would permit ancestor replacement races.
    let root = CString::new("/")?;
    let root_fd = unsafe {
        nix::libc::open(
            root.as_ptr(),
            nix::libc::O_RDONLY
                | nix::libc::O_DIRECTORY
                | nix::libc::O_CLOEXEC
                | nix::libc::O_NOFOLLOW,
        )
    };
    anyhow::ensure!(root_fd >= 0, "cannot open trusted plan root");
    let mut dir = unsafe { File::from_raw_fd(root_fd) };
    for component in directory.components() {
        let Component::Normal(segment) = component else {
            anyhow::ensure!(
                matches!(component, Component::RootDir),
                "unsafe plan directory component"
            );
            continue;
        };
        let name = CString::new(segment.as_encoded_bytes())?;
        let fd = unsafe {
            nix::libc::openat(
                dir.as_raw_fd(),
                name.as_ptr(),
                nix::libc::O_RDONLY
                    | nix::libc::O_DIRECTORY
                    | nix::libc::O_CLOEXEC
                    | nix::libc::O_NOFOLLOW,
            )
        };
        anyhow::ensure!(fd >= 0, "trusted plan ancestor is absent or unsafe");
        let next = unsafe { File::from_raw_fd(fd) };
        let meta = next.metadata()?;
        anyhow::ensure!(
            meta.is_dir() && meta.uid() == owner_uid && meta.permissions().mode() & 0o022 == 0,
            "trusted plan ancestor is not owner-controlled"
        );
        dir = next;
    }
    let dir_meta = dir.metadata()?;
    anyhow::ensure!(
        dir_meta.permissions().mode() & 0o077 == 0,
        "root repair plan directory is not private"
    );
    let filename = CString::new(format!("{session_id}.json"))?;
    let fd = unsafe {
        nix::libc::openat(
            dir.as_raw_fd(),
            filename.as_ptr(),
            nix::libc::O_RDONLY | nix::libc::O_CLOEXEC | nix::libc::O_NOFOLLOW,
        )
    };
    anyhow::ensure!(fd >= 0, "root repair plan absent");
    let mut file = unsafe { File::from_raw_fd(fd) };
    let meta = file.metadata()?;
    anyhow::ensure!(
        meta.is_file()
            && meta.uid() == owner_uid
            && meta.permissions().mode() & 0o077 == 0
            && meta.len() <= MAX_PLAN_BYTES,
        "root repair plan is not a private bounded regular file"
    );
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    file.by_ref()
        .take(MAX_PLAN_BYTES + 1)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_PLAN_BYTES,
        "root repair plan is oversized"
    );
    let actual = hex::encode(Sha256::digest(&bytes));
    anyhow::ensure!(actual == digest, "root repair plan digest mismatch");
    Ok(bytes)
}

fn trusted_plan_at(
    directory: &Path,
    session_id: &str,
    digest: &str,
    owner_uid: u32,
) -> Result<RootRepairPlan> {
    serde_json::from_slice(&trusted_plan_bytes_at(
        directory, session_id, digest, owner_uid,
    )?)
    .context("invalid root repair plan")
}

#[async_trait]
trait NativeRootRepair: Send + Sync {
    async fn instance(&self, name: &str) -> Result<(JsonValue, String)>;
    async fn pool(&self, name: &str) -> Result<Option<JsonValue>>;
    async fn root_volume_present(&self, pool: &str, name: &str) -> Result<bool>;
    async fn root_subvolume_state(
        &self,
        pool: &str,
        source: &str,
        name: &str,
    ) -> Result<op_network::incus_root_subvolume::RootSubvolumeState>;
    async fn prepare_missing_root_for_rebuild(
        &self,
        pool: &str,
        source: &str,
        name: &str,
        operation_id: &str,
    ) -> Result<op_network::incus_root_subvolume::MissingRootPreparation>;
    async fn recover_existing_pool(&self, name: &str, source: &str) -> Result<()>;
    async fn rebuild_from_image(&self, name: &str, fingerprint: &str) -> Result<()>;
    async fn redirect_persist_source(
        &self,
        name: &str,
        old: &str,
        new: &str,
        etag: &str,
    ) -> Result<()>;
    async fn move_to_pool(&self, name: &str, pool: &str) -> Result<()>;
    async fn default_project_state(&self) -> Result<IncusState>;
}

#[async_trait]
impl NativeRootRepair for IncusRootRepairBackend {
    async fn instance(&self, name: &str) -> Result<(JsonValue, String)> {
        self.instance(name).await
    }
    async fn pool(&self, name: &str) -> Result<Option<JsonValue>> {
        self.pool(name).await
    }
    async fn root_volume_present(&self, pool: &str, name: &str) -> Result<bool> {
        self.root_volume_present(pool, name).await
    }
    async fn root_subvolume_state(
        &self,
        pool: &str,
        source: &str,
        name: &str,
    ) -> Result<op_network::incus_root_subvolume::RootSubvolumeState> {
        self.root_subvolume_state(pool, source, name).await
    }
    async fn prepare_missing_root_for_rebuild(
        &self,
        pool: &str,
        source: &str,
        name: &str,
        operation_id: &str,
    ) -> Result<op_network::incus_root_subvolume::MissingRootPreparation> {
        self.prepare_missing_root_for_rebuild(pool, source, name, operation_id)
            .await
    }
    async fn recover_existing_pool(&self, name: &str, source: &str) -> Result<()> {
        self.recover_existing_pool(name, source).await
    }
    async fn rebuild_from_image(&self, name: &str, fingerprint: &str) -> Result<()> {
        self.rebuild_from_image(name, fingerprint).await
    }
    async fn redirect_persist_source(
        &self,
        name: &str,
        old: &str,
        new: &str,
        etag: &str,
    ) -> Result<()> {
        self.redirect_persist_source(name, old, new, etag).await
    }
    async fn move_to_pool(&self, name: &str, pool: &str) -> Result<()> {
        self.move_to_pool(name, pool).await
    }
    async fn default_project_state(&self) -> Result<IncusState> {
        self.default_project_state().await
    }
}

struct RepairReconciliationRequired {
    operation_id: String,
    phase: String,
    _private_cause: anyhow::Error,
}

impl std::fmt::Display for RepairReconciliationRequired {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "root repair {} requires reconciliation after {}",
            self.operation_id, self.phase
        )
    }
}

impl std::fmt::Debug for RepairReconciliationRequired {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}
impl std::error::Error for RepairReconciliationRequired {
    #[cfg(test)]
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self._private_cause.as_ref())
    }
}

fn pending(operation_id: &str, phase: &str, error: impl Into<anyhow::Error>) -> anyhow::Error {
    RepairReconciliationRequired {
        operation_id: operation_id.into(),
        phase: phase.into(),
        _private_cause: error.into(),
    }
    .into()
}

pub(super) fn owned_task_join_error(
    operation_id: &str,
    error: tokio::task::JoinError,
) -> anyhow::Error {
    pending(operation_id, "owned-task-join", error)
}

async fn phase(
    store: &IdentitySledStore,
    session_id: &str,
    digest: &str,
    operation_id: &str,
    name: &'static str,
    expected_identity: &IdentityRevision,
) -> Result<()> {
    let cozo = store.cozo.clone();
    let own_session = session_id.to_string();
    let own_digest = digest.to_string();
    let own_operation = operation_id.to_string();
    let expected_identity = expected_identity.clone();
    tokio::task::spawn_blocking(move || {
        cozo.append_root_repair_phase(
            &own_session,
            &own_digest,
            &own_operation,
            name,
            &expected_identity,
            now(),
        )
    })
    .await??;
    Ok(())
}

async fn stub_absent_phase(
    store: &IdentitySledStore,
    session_id: &str,
    digest: &str,
    operation_id: &str,
    expected_identity: &IdentityRevision,
    receipt_json: &str,
) -> Result<()> {
    let cozo = store.cozo.clone();
    let session_id = session_id.to_owned();
    let digest = digest.to_owned();
    let operation_id = operation_id.to_owned();
    let expected_identity = expected_identity.clone();
    let receipt_json = receipt_json.to_owned();
    tokio::task::spawn_blocking(move || {
        let row = cozo.append_root_repair_stub_absent(
            &session_id,
            &digest,
            &operation_id,
            &expected_identity,
            now(),
            &receipt_json,
        )?;
        anyhow::ensure!(
            row.phase == "stub-absent" && row.stub_receipt_json == receipt_json,
            "durable stub receipt differs from native readback"
        );
        Ok::<(), anyhow::Error>(())
    })
    .await??;
    Ok(())
}

fn native_stub_receipt_json(
    native: &op_network::incus_root_subvolume::MissingRootPreparation,
    session_id: &str,
    plan_sha256: &str,
    operation_id: &str,
) -> Result<String> {
    use op_network::incus_root_subvolume::MissingRootPreparation;
    let receipt = match native {
        MissingRootPreparation::AlreadyAbsent => serde_json::json!({
            "status":"already_absent", "session_id":session_id,
            "plan_sha256":plan_sha256, "operation_id":operation_id,
        }),
        MissingRootPreparation::Quarantined(stub) => serde_json::json!({
            "status":"quarantined", "session_id":session_id,
            "plan_sha256":plan_sha256, "operation_id":operation_id,
            "path":stub.path, "original_stub_inode":stub.original_stub_inode,
            "backup_inode":stub.backup_inode, "backup_sha256":stub.backup_sha256,
            "original_stub_mode":stub.original_stub_mode,
        }),
    };
    Ok(serde_json::to_string(&receipt)?)
}

fn verify_original_instance(instance: &JsonValue, etag: &str, plan: &RootRepairPlan) -> Result<()> {
    anyhow::ensure!(
        etag == plan.expected_instance_etag
            && instance["name"] == plan.session_id
            && instance["status"] == "Stopped"
            && instance["type"] == "container"
            && instance["config"]["volatile.uuid"] == plan.expected_instance_uuid
            && instance["config"]["volatile.base_image"] == plan.image_fingerprint
            && instance["config"]["boot.autostart"] == "false"
            && instance["devices"]["root"]["pool"] == plan.source_pool
            && instance["expanded_devices"]["root"]["pool"] == plan.source_pool
            && instance["devices"]["persist"]["source"]
                == plan
                    .expected_btrfs_device
                    .as_ref()
                    .context("original persistence binding absent")?
                    .mount_point
            && instance["devices"]["persist"].get("shift").is_none()
            && instance["expanded_devices"]["persist"]["source"]
                == plan
                    .expected_btrfs_device
                    .as_ref()
                    .context("original persistence binding absent")?
                    .mount_point
            && instance["expanded_devices"]["persist"]
                .get("shift")
                .is_none(),
        "native stopped identity differs from trusted repair plan"
    );
    let expanded = instance["expanded_devices"]
        .as_object()
        .context("native identity has no expanded device readback")?;
    anyhow::ensure!(
        !expanded.values().any(|device| device["type"] == "nic"),
        "identity has an expanded NIC"
    );
    anyhow::ensure!(
        original_last_state_idmap_digest(instance)? == plan.expected_first_start_idmap_sha256,
        "native original idmap differs from trusted repair plan"
    );
    Ok(())
}

fn original_last_state_idmap_digest(instance: &JsonValue) -> Result<String> {
    let current = instance["config"]["volatile.last_state.idmap"]
        .as_str()
        .context("native original last-state idmap is absent")?;
    let expanded = instance["expanded_config"]["volatile.last_state.idmap"]
        .as_str()
        .context("native expanded last-state idmap is absent")?;
    anyhow::ensure!(
        current == expanded
            && serde_json::from_str::<JsonValue>(current).is_ok_and(|value| value.is_array()),
        "native original last-state idmap is inconsistent"
    );
    Ok(hex::encode(Sha256::digest(current.as_bytes())))
}

fn verify_device_map(before: &JsonValue, after: &JsonValue, field: &str) -> Result<()> {
    let mut before_devices = before[field]
        .as_object()
        .context("missing original native devices")?
        .clone();
    let mut after_devices = after[field]
        .as_object()
        .context("missing repaired native devices")?
        .clone();
    let mut before_root = before_devices
        .remove("root")
        .context("missing original root disk")?;
    let mut after_root = after_devices
        .remove("root")
        .context("missing repaired root disk")?;
    let before_root = before_root
        .as_object_mut()
        .context("invalid original root disk")?;
    let after_root = after_root
        .as_object_mut()
        .context("invalid repaired root disk")?;
    before_root.remove("pool");
    after_root.remove("pool");
    anyhow::ensure!(
        before_devices == after_devices && before_root == after_root,
        "native disk options or non-root bind devices changed during root repair"
    );
    Ok(())
}

fn verify_persist_device_transition(
    before: &JsonValue,
    after: &JsonValue,
    field: &str,
    old_source: &str,
    new_source: &str,
) -> Result<()> {
    let mut before_devices = before[field]
        .as_object()
        .context("missing original devices")?
        .clone();
    let mut after_devices = after[field]
        .as_object()
        .context("missing updated devices")?
        .clone();
    let mut before_persist = before_devices
        .remove("persist")
        .context("original persist disk absent")?;
    let mut after_persist = after_devices
        .remove("persist")
        .context("updated persist disk absent")?;
    let before_persist = before_persist
        .as_object_mut()
        .context("original persist disk malformed")?;
    let after_persist = after_persist
        .as_object_mut()
        .context("updated persist disk malformed")?;
    anyhow::ensure!(
        before_persist.get("type").and_then(|value| value.as_str()) == Some("disk")
            && after_persist.get("type").and_then(|value| value.as_str()) == Some("disk")
            && before_persist
                .get("source")
                .and_then(|value| value.as_str())
                == Some(old_source)
            && after_persist.get("source").and_then(|value| value.as_str()) == Some(new_source)
            && !before_persist.contains_key("shift")
            && after_persist.get("shift").and_then(|value| value.as_str()) == Some("true"),
        "native persist source differs from trusted transition"
    );
    before_persist.remove("source");
    after_persist.remove("source");
    after_persist.remove("shift");
    anyhow::ensure!(
        before_devices == after_devices && before_persist == after_persist,
        "persist update changed another native device option"
    );
    Ok(())
}

fn verify_persist_redirect(
    before: &JsonValue,
    after: &JsonValue,
    plan: &RootRepairPlan,
) -> Result<()> {
    let old = plan
        .expected_btrfs_device
        .as_ref()
        .context("original persistence binding absent")?;
    anyhow::ensure!(
        before["name"] == after["name"]
            && after["name"] == plan.session_id
            && after["status"] == "Stopped"
            && before["type"] == after["type"]
            && after["type"] == "container"
            && before["config"] == after["config"]
            && before["expanded_config"] == after["expanded_config"]
            && before["profiles"] == after["profiles"]
            && after["config"]["volatile.uuid"] == plan.expected_instance_uuid
            && after["config"]["volatile.base_image"] == plan.image_fingerprint
            && after["config"]["boot.autostart"] == "false",
        "persist update changed native identity metadata"
    );
    for field in ["devices", "expanded_devices"] {
        verify_persist_device_transition(
            before,
            after,
            field,
            &old.mount_point,
            &plan.target_persist_source,
        )?;
    }
    Ok(())
}

fn verify_after_effect(
    before: &JsonValue,
    after: &JsonValue,
    pool: &str,
    plan: &RootRepairPlan,
) -> Result<()> {
    anyhow::ensure!(
        after["name"] == plan.session_id
            && after["status"] == "Stopped"
            && after["type"] == "container"
            && after["config"]["volatile.uuid"] == plan.expected_instance_uuid
            && after["config"]["volatile.base_image"] == plan.image_fingerprint
            && after["config"]["boot.autostart"] == "false"
            && after["devices"]["root"]["pool"] == pool
            && after["expanded_devices"]["root"]["pool"] == pool
            && after["config"] == before["config"]
            && after["expanded_config"] == before["expanded_config"]
            && after["profiles"] == before["profiles"],
        "native identity metadata changed during root repair"
    );
    verify_device_map(before, after, "devices")?;
    verify_device_map(before, after, "expanded_devices")?;
    anyhow::ensure!(
        !after["expanded_devices"]
            .as_object()
            .context("native expanded devices disappeared")?
            .values()
            .any(|device| device["type"] == "nic"),
        "native identity gained an expanded NIC"
    );
    Ok(())
}

/// A stopped same-image rebuild clears two Incus runtime idmap fields and
/// may restore the `create` template marker on a previously started CT. The
/// subsequent persist update and pool move retain strict full equality
/// against their immediate native readbacks.
fn verify_after_rebuild(
    before: &JsonValue,
    after: &JsonValue,
    pool: &str,
    plan: &RootRepairPlan,
) -> Result<()> {
    let mut expected = before.clone();
    for field in ["config", "expanded_config"] {
        let config = expected[field]
            .as_object_mut()
            .context("native pre-rebuild configuration is malformed")?;
        config.remove("volatile.idmap.next");
        config.remove("volatile.last_state.idmap");
        if !config.contains_key("volatile.apply_template")
            && after[field]["volatile.apply_template"] == "create"
        {
            config.insert("volatile.apply_template".into(), "create".into());
        }
    }
    verify_after_effect(&expected, after, pool, plan)
}

/// Incus restores the original idmap bookkeeping when migrating the stopped
/// rebuilt CT and may issue a new cloud-init instance UUID. All other native
/// metadata still passes the strict after-effect comparison.
fn verify_after_move(
    original: &JsonValue,
    before: &JsonValue,
    after: &JsonValue,
    pool: &str,
    plan: &RootRepairPlan,
) -> Result<()> {
    let mut expected = before.clone();
    for key in ["volatile.idmap.next", "volatile.last_state.idmap"] {
        anyhow::ensure!(
            original["config"][key].is_string()
                && original["config"][key] == original["expanded_config"][key],
            "original idmap bookkeeping is inconsistent"
        );
    }
    for field in ["config", "expanded_config"] {
        let config = expected[field]
            .as_object_mut()
            .context("native pre-move configuration is malformed")?;
        for key in ["volatile.idmap.next", "volatile.last_state.idmap"] {
            if !config.contains_key(key) {
                config.insert(key.into(), original[field][key].clone());
            }
        }
        let prior_cloud_id = original[field]["volatile.cloud-init.instance-id"]
            .as_str()
            .context("original native cloud-init instance ID is absent")?;
        let moved_cloud_id = after[field]["volatile.cloud-init.instance-id"]
            .as_str()
            .context("moved native cloud-init instance ID is absent")?;
        anyhow::ensure!(
            before[field]["volatile.cloud-init.instance-id"] == prior_cloud_id
                && uuid::Uuid::parse_str(prior_cloud_id)
                    .is_ok_and(|uuid| uuid.to_string() == prior_cloud_id)
                && uuid::Uuid::parse_str(moved_cloud_id)
                    .is_ok_and(|uuid| uuid.to_string() == moved_cloud_id),
            "native move cloud-init ID is not a canonical provider UUID"
        );
        config.insert(
            "volatile.cloud-init.instance-id".into(),
            moved_cloud_id.into(),
        );
    }
    anyhow::ensure!(
        after["config"]["volatile.cloud-init.instance-id"]
            == after["expanded_config"]["volatile.cloud-init.instance-id"],
        "moved native cloud-init IDs disagree"
    );
    verify_after_effect(&expected, after, pool, plan)
}

pub(super) fn projected_devices(
    instance: &IncusInstance,
    expanded: bool,
) -> Result<BTreeMap<String, JsonValue>> {
    let devices = if expanded {
        &instance.expanded_devices
    } else {
        &instance.devices
    };
    let mut mapped = BTreeMap::new();
    for named in devices {
        anyhow::ensure!(
            mapped
                .insert(named.name.clone(), serde_json::to_value(&named.device)?)
                .is_none(),
            "native instance has duplicate device names"
        );
    }
    Ok(mapped)
}

pub(super) fn verify_native_runtime_config(
    before: &Option<std::collections::HashMap<String, String>>,
    after: &Option<std::collections::HashMap<String, String>>,
    native_status: &str,
) -> Result<()> {
    verify_native_runtime_config_with_repair_pin(before, after, native_status, None)
}

fn verify_native_runtime_config_with_repair_pin(
    before: &Option<std::collections::HashMap<String, String>>,
    after: &Option<std::collections::HashMap<String, String>>,
    native_status: &str,
    repair_idmap_sha256: Option<&str>,
) -> Result<()> {
    let mut before = before.clone().unwrap_or_default();
    let mut after = after.clone().unwrap_or_default();
    let before_idmap = before.remove("volatile.idmap.current");
    let after_idmap = after.remove("volatile.idmap.current");
    let before_power = before.remove("volatile.last_state.power");
    let after_power = after.remove("volatile.last_state.power");
    let before_ready = before.remove("volatile.last_state.ready");
    let after_ready = after.remove("volatile.last_state.ready");
    let before_template = before.remove("volatile.apply_template");
    let after_template = after.remove("volatile.apply_template");
    anyhow::ensure!(
        before == after,
        "native identity configuration changed outside proven runtime keys"
    );
    anyhow::ensure!(
        after_idmap.as_ref().is_none_or(|value| !value.is_empty())
            && (before_idmap.is_some()
                || after_idmap.as_ref().is_none_or(|value| {
                    after.get("volatile.idmap.next") == Some(value)
                        || (!after.contains_key("volatile.idmap.next")
                            && repair_idmap_sha256.is_some_and(|digest| {
                                hex::encode(Sha256::digest(value.as_bytes())) == digest
                            }))
                }))
            && (before_idmap.is_none() || before_idmap == after_idmap),
        "native idmap changed after its first start"
    );
    anyhow::ensure!(
        after_ready.as_deref().is_none_or(|value| value == "false")
            && (before_ready.is_none() || before_ready == after_ready),
        "native ready state changed unexpectedly"
    );
    anyhow::ensure!(
        after_template.is_none() || before_template == after_template,
        "native template setting appeared or changed"
    );
    anyhow::ensure!(
        after_power.as_deref().is_none_or(|value| value
            == match native_status {
                "Running" => "RUNNING",
                "Stopped" => "STOPPED",
                _ => "",
            })
            && (before_power == after_power || after_power.is_some()),
        "native power marker disappeared or is empty"
    );
    Ok(())
}

fn verify_start_native_projection(
    pinned: &IncusInstance,
    current: &IncusInstance,
    plan: &RootRepairPlan,
    native_status: &str,
) -> Result<()> {
    anyhow::ensure!(
        pinned.name == plan.session_id
            && current.name == plan.session_id
            && current.status == native_status
            && current.instance_type == "container"
            && current.profiles == pinned.profiles
            && current
                .config
                .as_ref()
                .and_then(|config| config.get("boot.autostart"))
                .is_some_and(|value| value == "false"),
        "parked native identity differs from committed Cozo readback"
    );
    verify_native_runtime_config_with_repair_pin(
        &pinned.config,
        &current.config,
        native_status,
        Some(&plan.expected_first_start_idmap_sha256),
    )?;
    verify_native_runtime_config_with_repair_pin(
        &pinned.expanded_config,
        &current.expanded_config,
        native_status,
        Some(&plan.expected_first_start_idmap_sha256),
    )?;
    anyhow::ensure!(
        current
            .config
            .as_ref()
            .and_then(|config| config.get("volatile.uuid"))
            .is_some_and(|value| value == &plan.expected_instance_uuid)
            && current
                .config
                .as_ref()
                .and_then(|config| config.get("volatile.base_image"))
                .is_some_and(|value| value == &plan.image_fingerprint),
        "native instance UUID or base image differs from trusted plan"
    );
    for expanded in [false, true] {
        let pinned_devices = projected_devices(pinned, expanded)?;
        let current_devices = projected_devices(current, expanded)?;
        anyhow::ensure!(
            pinned_devices == current_devices,
            "native device layout differs from committed Cozo readback"
        );
        anyhow::ensure!(
            current_devices
                .get("root")
                .is_some_and(|root| root["pool"] == plan.target_pool),
            "native root disk is not on the pinned seed pool"
        );
        anyhow::ensure!(
            current_devices
                .get("persist")
                .is_some_and(|persist| persist["type"] == "disk"
                    && persist["source"] == plan.target_persist_source
                    && persist["shift"] == "true"),
            "native persist disk is not on the pinned sprout subvolume"
        );
        anyhow::ensure!(
            !current_devices
                .values()
                .any(|device| device["type"] == "nic"),
            "native identity has a network interface"
        );
    }
    Ok(())
}

pub(crate) struct RootStartGuard {
    pub should_start: bool,
    pub(super) _mount: op_network::seeded_root::SeededRootMount,
}

pub(super) fn decide_seeded_native_start(sled_active: bool, native_status: &str) -> Result<bool> {
    match native_status {
        "Stopped" => Ok(true),
        "Running" if sled_active => Ok(false),
        _ => anyhow::bail!("seeded native power state conflicts with identity policy"),
    }
}

fn recheck_own_admission(actor_id: &str, original: &RepairSealPin) -> Result<()> {
    crate::method_contract_admission::admit_compiled_method(
        "identity_sled",
        "repair_own_root",
        Some(REPAIR_CAPABILITY),
    )?;
    let current = strict_identity_contract()?;
    anyhow::ensure!(
        current.schema_hash == original.schema_hash
            && current.blob.manifest == original.blob.manifest
            && current.blob.schema_json == original.blob.schema_json
            && current.blob.descriptor_set == original.blob.descriptor_set
            && current.blob.meta_json == original.blob.meta_json,
        "identity repair method seal changed while queued"
    );
    anyhow::ensure!(
        crate::interceptor::load_capability_grants(actor_id)
            .iter()
            .any(|capability| capability == REPAIR_CAPABILITY),
        "root repair requires a current exact-principal grant"
    );
    Ok(())
}

pub(super) async fn verify_current_repair_sid1(
    engine: &MutationEngine,
    sled: &ContainerIdentitySled,
    actor_id: &str,
    session_genesis: &str,
) -> Result<()> {
    let sealed = sled
        .sealed_id
        .as_deref()
        .context("own SID1 credential is absent")?;
    let encoded = sealed
        .strip_prefix(op_identity::sealed_id::INLINE_PREFIX)
        .context("own SID1 credential is not authored inline")?;
    let wire = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(encoded)
        .context("own SID1 encoding is invalid")?;
    // This existing verifier opens the authored wire and matches the current
    // authoritative projection, including its term. It never mints a SID1.
    let claims = super::authenticate_sid1_wire(engine, &wire, "dbus")
        .await
        .map_err(anyhow::Error::msg)?;
    anyhow::ensure!(
        claims.principal_id == actor_id
            && claims.session_id == sled.session_id
            && claims.session_genesis == session_genesis
            && claims.wireguard_pubkey == sled.wireguard_pubkey,
        "queued repair caller no longer owns the current SID1 session"
    );
    anyhow::ensure!(
        crate::human_principal_dispatch::resolve_key_for_assertion(&sled.wireguard_pubkey)
            .await
            .map_err(|_| anyhow::anyhow!("identity principal registry is unavailable"))?
            .is_some_and(|record| record.revoked_at == 0 && record.principal_id == actor_id),
        "queued repair principal is revoked or unregistered"
    );
    Ok(())
}

fn descriptor_for_plan(
    plan: &RootRepairPlan,
) -> Result<op_network::seeded_root::SeededRootDescriptor<'_>> {
    let mount_point = Path::new(&plan.mount_point);
    let pool_subvolume = Path::new(&plan.target_pool_source)
        .strip_prefix(mount_point)?
        .to_str()
        .context("invalid pool subvolume name")?
        .trim_start_matches('/');
    Ok(op_network::seeded_root::SeededRootDescriptor {
        seed_image: Path::new(&plan.seed_image_path),
        seed_sha256: &plan.seed_sha256,
        seed_fsid: uuid::Uuid::parse_str(&plan.seed_fsid)?,
        leaf_image: Path::new(&plan.leaf_image_path),
        sprout_fsid: uuid::Uuid::parse_str(&plan.leaf_fsid)?,
        mount_point,
        pool_subvolume,
    })
}

async fn ensure_mounted_plan(
    plan: &RootRepairPlan,
) -> Result<op_network::seeded_root::SeededRootMount> {
    let plan = plan.clone();
    tokio::task::spawn_blocking(move || {
        let descriptor = descriptor_for_plan(&plan)?;
        let mount = op_network::seeded_root::ensure_seeded_root_mounted(&descriptor)?;
        anyhow::ensure!(
            mount.pool_path == Path::new(&plan.target_pool_source),
            "mounted pool path differs from trusted plan"
        );
        Ok::<_, anyhow::Error>(mount)
    })
    .await?
}

async fn reverify_mounted_plan(
    plan: &RootRepairPlan,
    mount: op_network::seeded_root::SeededRootMount,
) -> Result<op_network::seeded_root::SeededRootMount> {
    let plan = plan.clone();
    tokio::task::spawn_blocking(move || {
        let descriptor = descriptor_for_plan(&plan)?;
        mount.revalidate(&descriptor)?;
        Ok::<_, anyhow::Error>(mount)
    })
    .await?
}

/// One-shot clean-core check before Incus imports the target pool. A normal
/// repaired container makes the pool nonempty, so start/boot paths never call
/// this predicate.
async fn verify_clean_core_plan(
    plan: &RootRepairPlan,
    mount: op_network::seeded_root::SeededRootMount,
) -> Result<op_network::seeded_root::SeededRootMount> {
    let plan = plan.clone();
    tokio::task::spawn_blocking(move || {
        let descriptor = descriptor_for_plan(&plan)?;
        mount.verify_clean_core(&descriptor, &plan.image_fingerprint)?;
        Ok::<_, anyhow::Error>(mount)
    })
    .await?
}

async fn verify_old_persist_empty(plan: &RootRepairPlan) -> Result<()> {
    let old = plan
        .expected_btrfs_device
        .clone()
        .context("original persistence binding is absent")?;
    tokio::task::spawn_blocking(move || {
        anyhow::ensure!(
            fs::read_dir(&old.mount_point)?
                .next()
                .transpose()?
                .is_none(),
            "original fstorage is not empty; migration requires explicit review"
        );
        Ok::<(), anyhow::Error>(())
    })
    .await??;
    Ok(())
}

async fn verify_selected_persist(
    plan: &RootRepairPlan,
    mount: op_network::seeded_root::SeededRootMount,
    require_empty: bool,
) -> Result<op_network::seeded_root::SeededRootMount> {
    let plan = plan.clone();
    tokio::task::spawn_blocking(move || {
        let descriptor = descriptor_for_plan(&plan)?;
        let path = mount.verify_persist_subvolume(&descriptor, "persist", require_empty)?;
        anyhow::ensure!(
            path == Path::new(&plan.target_persist_source),
            "persist subvolume differs from trusted plan"
        );
        Ok::<_, anyhow::Error>(mount)
    })
    .await?
}

async fn install_boot_descriptor(
    plan: &RootRepairPlan,
    session_id: &str,
    mount: op_network::seeded_root::SeededRootMount,
) -> Result<op_network::seeded_root::SeededRootMount> {
    let plan = plan.clone();
    let session_id = session_id.to_string();
    tokio::task::spawn_blocking(move || {
        let descriptor = descriptor_for_plan(&plan)?;
        op_network::seeded_boot::install_verified(&session_id, &descriptor, &mount)?;
        Ok::<_, anyhow::Error>(mount)
    })
    .await?
}

/// Called only under the owning session lifecycle lock. No journal means a
/// legacy identity and leaves its existing activation behavior unchanged.
/// Any partial repair row is a durable start denial until reconciliation.
pub(super) async fn guard_own_root_start(
    store: &IdentitySledStore,
    revision: &IdentityRevision,
    sled: &ContainerIdentitySled,
) -> Result<Option<RootStartGuard>> {
    guard_own_root_start_with_backend(
        store,
        revision,
        sled,
        Arc::new(IncusRootRepairBackend::local()),
        PathBuf::from(PLAN_DIR),
        0,
    )
    .await
}

async fn guard_own_root_start_with_backend(
    store: &IdentitySledStore,
    revision: &IdentityRevision,
    sled: &ContainerIdentitySled,
    native: Arc<dyn NativeRootRepair>,
    plan_dir: PathBuf,
    plan_owner_uid: u32,
) -> Result<Option<RootStartGuard>> {
    let cozo = store.cozo.clone();
    let session_id = sled.session_id.clone();
    let journal =
        tokio::task::spawn_blocking(move || cozo.read_root_repair_journal(&session_id)).await??;
    let Some(journal) = journal else {
        return Ok(None);
    };
    anyhow::ensure!(
        journal.phase == "identity-committed",
        "identity root repair is incomplete; start denied"
    );
    preflight_native_identity_effect().await?;
    let directory = plan_dir.clone();
    let session_id = sled.session_id.clone();
    let digest = journal.plan_sha256.clone();
    let plan = tokio::task::spawn_blocking(move || {
        trusted_plan_at(&directory, &session_id, &digest, plan_owner_uid)
    })
    .await??;
    plan.validate_completed_journal(&journal, revision, sled)?;
    let pinned_instance = sled
        .instance
        .as_ref()
        .context("committed native identity is absent")?;
    let mut mounted = ensure_mounted_plan(&plan).await?;
    mounted = reverify_mounted_plan(&plan, mounted).await?;
    mounted = verify_selected_persist(&plan, mounted, false).await?;
    let pool = native
        .pool(&plan.target_pool)
        .await?
        .context("seeded identity pool is absent")?;
    anyhow::ensure!(
        pool["name"] == plan.target_pool
            && pool["driver"] == "btrfs"
            && pool["config"]["source"] == plan.target_pool_source
            && pool["status"] == "Created",
        "native identity pool source differs from trusted plan"
    );
    let (current, etag) = native.instance(&plan.session_id).await?;
    let native_status = current["status"]
        .as_str()
        .context("native identity status is absent")?;
    let should_start = decide_seeded_native_start(sled.active, native_status)?;
    anyhow::ensure!(
        !etag.is_empty()
            && current["name"] == plan.session_id
            && current["type"] == "container"
            && current["config"]["volatile.uuid"] == plan.expected_instance_uuid
            && current["config"]["volatile.base_image"] == plan.image_fingerprint
            && current["config"]["boot.autostart"] == "false"
            && current["devices"]["root"]["pool"] == plan.target_pool
            && current["expanded_devices"]["root"]["pool"] == plan.target_pool
            && current["devices"]["persist"]["source"] == plan.target_persist_source
            && current["expanded_devices"]["persist"]["source"] == plan.target_persist_source
            && current["devices"]["persist"]["shift"] == "true"
            && current["expanded_devices"]["persist"]["shift"] == "true",
        "native identity is not the pinned seeded container"
    );
    anyhow::ensure!(
        native_root_present(native.as_ref(), &plan, &plan.target_pool).await?,
        "seeded identity root subvolume is absent"
    );
    anyhow::ensure!(
        native
            .root_volume_present(&plan.target_pool, &plan.session_id)
            .await?,
        "seeded identity root volume metadata is absent"
    );
    let state = native.default_project_state().await?;
    let projected = state
        .instances
        .iter()
        .find(|instance| instance.name == plan.session_id)
        .context("native project readback lacks identity")?;
    verify_start_native_projection(pinned_instance, projected, &plan, native_status)?;
    mounted = verify_selected_persist(&plan, mounted, false).await?;
    Ok(Some(RootStartGuard {
        should_start,
        _mount: mounted,
    }))
}

/// Return the complete native Incus instance after an owned power effect.
/// The caller holds the session lifecycle and host observation locks. A stop
/// may use this without remounting; a start must retain RootStartGuard too.
pub(super) async fn observe_owned_power_effect(
    engine: &MutationEngine,
    store: &IdentitySledStore,
    revision: &IdentityRevision,
    sled: &ContainerIdentitySled,
    expected_status: &str,
) -> Result<Option<IncusInstance>> {
    let cozo = store.cozo.clone();
    let session = sled.session_id.clone();
    let journal =
        tokio::task::spawn_blocking(move || cozo.read_root_repair_journal(&session)).await??;
    let Some(journal) = journal else {
        return Ok(None);
    };
    anyhow::ensure!(
        journal.phase == "identity-committed",
        "identity root repair is incomplete; native stop requires reconciliation"
    );
    let directory = PathBuf::from(PLAN_DIR);
    let session = sled.session_id.clone();
    let digest = journal.plan_sha256.clone();
    let plan =
        tokio::task::spawn_blocking(move || trusted_plan_at(&directory, &session, &digest, 0))
            .await??;
    plan.validate_completed_journal(&journal, revision, sled)?;
    let pinned = sled
        .instance
        .as_ref()
        .context("committed native identity is absent")?;
    let native = IncusRootRepairBackend::local();
    let snapshot = native.default_project_state().await?;
    let current = snapshot
        .instances
        .iter()
        .find(|instance| instance.name == sled.session_id)
        .context("owned native power readback lacks identity")?;
    verify_start_native_projection(pinned, current, &plan, expected_status)?;
    let observed = current.clone();
    crate::incus_observer::publish_repair_readback_locked(engine, &snapshot).await?;
    Ok(Some(observed))
}

pub(super) async fn repair_journal_incomplete(
    store: &IdentitySledStore,
    session_id: &str,
) -> Result<bool> {
    let cozo = store.cozo.clone();
    let session = session_id.to_string();
    let row =
        tokio::task::spawn_blocking(move || cozo.read_root_repair_journal(&session)).await??;
    Ok(row.is_some_and(|journal| journal.phase != "identity-committed"))
}

async fn pre_effect_instance(
    native: &dyn NativeRootRepair,
    plan: &RootRepairPlan,
    original: &JsonValue,
    pool: &str,
    root_present: bool,
) -> Result<String> {
    if pool == plan.source_pool {
        verify_source_pool(native, plan).await?;
    }
    let (current, etag) = native.instance(&plan.session_id).await?;
    anyhow::ensure!(!etag.is_empty(), "native instance has no conditional ETag");
    verify_after_effect(original, &current, pool, plan)?;
    anyhow::ensure!(
        native_root_present(native, plan, pool).await? == root_present,
        "native root subvolume state changed before effect"
    );
    if pool == plan.source_pool {
        anyhow::ensure!(
            native.root_volume_present(pool, &plan.session_id).await?,
            "original Incus root volume metadata disappeared"
        );
    }
    Ok(etag)
}

async fn native_root_present(
    native: &dyn NativeRootRepair,
    plan: &RootRepairPlan,
    pool: &str,
) -> Result<bool> {
    let source = if pool == plan.source_pool {
        &plan.source_pool_source
    } else if pool == plan.target_pool {
        &plan.target_pool_source
    } else {
        anyhow::bail!("root pool is outside the trusted plan");
    };
    Ok(matches!(
        native
            .root_subvolume_state(pool, source, &plan.session_id)
            .await?,
        op_network::incus_root_subvolume::RootSubvolumeState::Present
    ))
}

async fn verify_source_pool(native: &dyn NativeRootRepair, plan: &RootRepairPlan) -> Result<()> {
    let pool = native
        .pool(&plan.source_pool)
        .await?
        .context("original Incus pool is absent")?;
    anyhow::ensure!(
        pool["name"] == plan.source_pool
            && pool["driver"] == "btrfs"
            && pool["config"]["source"] == plan.source_pool_source
            && pool["status"] == "Created",
        "original Incus pool source or availability differs from trusted plan"
    );
    Ok(())
}

struct NativeRootPreflight {
    etag: String,
    instance_uuid: String,
    first_start_idmap_sha256: String,
    image_fingerprint: String,
    source_pool: String,
    source_pool_source: String,
}

fn admit_preflight_read(
    actor_id: &str,
) -> Result<crate::sealed_schema_reader::ManifestPinnedSchemaDocument> {
    const READ_CAPABILITY: &str = "identity_sled.read";
    crate::method_contract_admission::admit_compiled_method(
        "identity_sled",
        "inspect_own_root_repair_preflight",
        Some(READ_CAPABILITY),
    )?;
    anyhow::ensure!(
        crate::interceptor::load_capability_grants(actor_id).contains(READ_CAPABILITY),
        "own root repair preflight requires a current exact-principal read grant"
    );
    strict_identity_contract()
}

async fn verify_original_fstorage_empty(old: SledBtrfsDevice) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        anyhow::ensure!(
            fs::read_dir(&old.mount_point)?
                .next()
                .transpose()?
                .is_none(),
            "original fstorage is populated; repair needs an explicit migration plan"
        );
        Ok::<(), anyhow::Error>(())
    })
    .await??;
    Ok(())
}

async fn inspect_native_missing_root(
    native: &IncusRootRepairBackend,
    session_id: &str,
    old: &SledBtrfsDevice,
) -> Result<NativeRootPreflight> {
    let (instance, etag) = native.instance(session_id).await?;
    let source_pool = instance["devices"]["root"]["pool"]
        .as_str()
        .context("native root pool is absent")?;
    op_plugins::state_plugins::incus::validate_root_repair_pool_name(source_pool)?;
    let instance_uuid = instance["config"]["volatile.uuid"]
        .as_str()
        .context("native instance UUID is absent")?;
    let image_fingerprint = instance["config"]["volatile.base_image"]
        .as_str()
        .context("native base image fingerprint is absent")?;
    anyhow::ensure!(
        !etag.is_empty()
            && instance["name"] == session_id
            && instance["type"] == "container"
            && instance["status"] == "Stopped"
            && instance["config"]["boot.autostart"] == "false"
            && uuid::Uuid::parse_str(instance_uuid)
                .is_ok_and(|uuid| uuid.to_string() == instance_uuid)
            && valid_digest(image_fingerprint)
            && instance["expanded_devices"]["root"]["pool"] == source_pool
            && instance["devices"]["persist"]["source"] == old.mount_point
            && instance["expanded_devices"]["persist"]["source"] == old.mount_point
            && instance["devices"]["persist"].get("shift").is_none()
            && instance["expanded_devices"]["persist"]
                .get("shift")
                .is_none(),
        "native stopped identity differs from the original Cozo binding"
    );
    let expanded = instance["expanded_devices"]
        .as_object()
        .context("native expanded devices are absent")?;
    anyhow::ensure!(
        !expanded.values().any(|device| device["type"] == "nic"),
        "native identity has an expanded NIC"
    );
    let first_start_idmap_sha256 = original_last_state_idmap_digest(&instance)?;
    let pool = native
        .pool(source_pool)
        .await?
        .context("native source pool is absent")?;
    let source = pool["config"]["source"]
        .as_str()
        .context("native source pool path is absent")?;
    anyhow::ensure!(
        pool["name"] == source_pool && pool["driver"] == "btrfs" && pool["status"] == "Created",
        "native source pool is unavailable or changed"
    );
    anyhow::ensure!(
        native.root_volume_present(source_pool, session_id).await?,
        "original Incus volume metadata is absent"
    );
    anyhow::ensure!(
        matches!(
            native
                .root_subvolume_state(source_pool, source, session_id)
                .await?,
            op_network::incus_root_subvolume::RootSubvolumeState::Missing
        ),
        "native root subvolume is present; repair is not indicated"
    );
    Ok(NativeRootPreflight {
        etag,
        instance_uuid: instance_uuid.to_owned(),
        first_start_idmap_sha256,
        image_fingerprint: image_fingerprint.to_owned(),
        source_pool: source_pool.to_owned(),
        source_pool_source: source.to_owned(),
    })
}

/// Return the exact current Cozo revision and original storage binding needed
/// to prepare a root-owned plan. The verified caller can inspect only its own
/// parked identity. This read never authors a plan, changes native state, or
/// returns a credential; repair independently rechecks every precondition.
pub(super) async fn inspect_own_root_repair_preflight(
    engine: &MutationEngine,
    store: &IdentitySledStore,
    actor_id: &str,
    session_id: &str,
    session_genesis: &str,
) -> Result<JsonValue> {
    let initial_seal = admit_preflight_read(actor_id)?;
    let lifecycle = store.lifecycle_lock(session_id).await;
    let _session_guard = lifecycle.lock_owned().await;
    let _observation = engine.lock_host_observation().await;
    let current_actor = engine
        .session_context_for_actor(actor_id)
        .await
        .context("own root repair preflight has no current authenticated caller")?;
    anyhow::ensure!(
        current_actor.session_id == session_id && current_actor.genesis_hex == session_genesis,
        "own root repair preflight caller changed while queued"
    );
    let revision = load_revision(store, session_id).await?;
    let sled = revision_sled(&revision)?.context("own identity is absent")?;
    anyhow::ensure!(
        sled.is_anchored()
            && sled.session_id == session_id
            && sled.genesis.as_deref() == Some(session_genesis)
            && op_identity::session::derive_session_id(&sled.wireguard_pubkey) == session_id
            && op_identity::session::derive_principal_id(&sled.wireguard_pubkey) == actor_id
            && !sled.active
            && sled.instance.as_ref().is_some_and(|instance| {
                instance.name == session_id && instance.instance_type == "container"
            }),
        "own root repair preflight requires the parked, anchored container owner"
    );
    let old = sled
        .btrfs_device
        .clone()
        .context("original Cozo Btrfs binding is absent")?;
    anyhow::ensure!(
        old.attached
            && !old.device_path.is_empty()
            && !old.mount_point.is_empty()
            && !old.btrfs_uuid.is_empty()
            && !old.cozo_id.is_empty(),
        "original Cozo Btrfs binding is incomplete"
    );
    verify_current_repair_sid1(engine, &sled, actor_id, session_genesis).await?;
    let cozo = store.cozo.clone();
    let own_session = session_id.to_owned();
    tokio::task::spawn_blocking(move || {
        anyhow::ensure!(
            cozo.read_root_repair_journal(&own_session)?.is_none()
                && cozo.read_seed_provision_journal(&own_session)?.is_none(),
            "existing identity storage journal requires reconciliation"
        );
        Ok::<(), anyhow::Error>(())
    })
    .await??;
    verify_original_fstorage_empty(old.clone()).await?;
    preflight_native_identity_effect().await?;
    let native = IncusRootRepairBackend::local();
    let native = inspect_native_missing_root(&native, session_id, &old).await?;
    admit_preflight_read(actor_id)?;
    same_identity_contract(&initial_seal)?;
    verify_current_repair_sid1(engine, &sled, actor_id, session_genesis).await?;
    anyhow::ensure!(
        load_revision(store, session_id).await? == revision,
        "own identity revision changed during repair preflight"
    );
    Ok(serde_json::json!({
        "session_id": session_id,
        "genesis": session_genesis,
        "revision": revision.revision,
        "revision_operation_id": revision.operation_id,
        "btrfs_device": old,
        "instance_etag": native.etag,
        "instance_uuid": native.instance_uuid,
        "expected_first_start_idmap_sha256": native.first_start_idmap_sha256,
        "image_fingerprint": native.image_fingerprint,
        "source_pool": native.source_pool,
        "source_pool_source": native.source_pool_source,
        "native_status": "Stopped",
        "root_subvolume": "missing"
    }))
}

pub(super) async fn repair_own_root(
    engine: &MutationEngine,
    store: &IdentitySledStore,
    actor: VerifiedIdentityActor<'_>,
    input: RepairOwnRootInput,
    original_seal: RepairSealPin,
    operation_id: String,
) -> Result<JsonValue> {
    let VerifiedIdentityActor {
        actor_id,
        session_id,
        genesis: session_genesis,
    } = actor;
    repair_own_root_with_backend(RepairRequest {
        engine,
        store,
        actor_id,
        session_id,
        session_genesis,
        input,
        original_seal: &original_seal,
        operation_id,
        native: Arc::new(IncusRootRepairBackend::local()),
        plan_dir: PathBuf::from(PLAN_DIR),
        plan_owner_uid: 0,
    })
    .await
}

struct RepairRequest<'a> {
    engine: &'a MutationEngine,
    store: &'a IdentitySledStore,
    actor_id: &'a str,
    session_id: &'a str,
    session_genesis: &'a str,
    input: RepairOwnRootInput,
    original_seal: &'a RepairSealPin,
    operation_id: String,
    native: Arc<dyn NativeRootRepair>,
    plan_dir: PathBuf,
    plan_owner_uid: u32,
}

struct RepairExecution<'a> {
    engine: &'a MutationEngine,
    store: &'a IdentitySledStore,
    actor_id: &'a str,
    session_id: &'a str,
    session_genesis: &'a str,
    digest: &'a str,
    original_seal: &'a RepairSealPin,
    operation_id: &'a str,
    native: &'a dyn NativeRootRepair,
    plan: &'a RootRepairPlan,
    expected: &'a IdentityRevision,
    active_phase: &'static str,
}

impl RepairExecution<'_> {
    async fn enter(&mut self, name: &'static str) -> Result<()> {
        self.active_phase = name;
        phase(
            self.store,
            self.session_id,
            self.digest,
            self.operation_id,
            name,
            self.expected,
        )
        .await
        .map_err(|error| pending(self.operation_id, name, error))
    }

    async fn admit(&self) -> Result<()> {
        recheck_own_admission(self.actor_id, self.original_seal)?;
        anyhow::ensure!(
            &load_revision(self.store, self.session_id).await? == self.expected,
            "identity revision changed before native repair effect"
        );
        let sled =
            revision_sled(self.expected)?.context("own identity disappeared while repairing")?;
        verify_current_repair_sid1(self.engine, &sled, self.actor_id, self.session_genesis).await
    }

    async fn publish_native(&self) -> Result<IncusState> {
        let snapshot = self.native.default_project_state().await?;
        crate::incus_observer::publish_repair_readback_locked(self.engine, &snapshot).await?;
        Ok(snapshot)
    }

    async fn pre_effect(
        &self,
        baseline: &JsonValue,
        pool: &str,
        root_present: bool,
    ) -> Result<String> {
        let native_pool = self
            .native
            .pool(pool)
            .await?
            .context("native source pool is absent")?;
        anyhow::ensure!(
            native_pool["status"] == "Created",
            "native source pool is unavailable"
        );
        pre_effect_instance(self.native, self.plan, baseline, pool, root_present).await
    }

    async fn import_pool(
        &mut self,
        mount: op_network::seeded_root::SeededRootMount,
        baseline: &JsonValue,
    ) -> Result<op_network::seeded_root::SeededRootMount> {
        self.admit().await?;
        self.enter("pool-import-intent").await?;
        let mount = reverify_mounted_plan(self.plan, mount).await?;
        let mount = verify_clean_core_plan(self.plan, mount).await?;
        let etag = self
            .pre_effect(baseline, &self.plan.source_pool, false)
            .await?;
        anyhow::ensure!(
            etag == self.plan.expected_instance_etag,
            "native instance ETag changed before pool import"
        );
        anyhow::ensure!(
            self.native.pool(&self.plan.target_pool).await?.is_none(),
            "target pool appeared before import"
        );
        self.admit().await?;
        self.native
            .recover_existing_pool(&self.plan.target_pool, &self.plan.target_pool_source)
            .await?;
        let pool = self
            .native
            .pool(&self.plan.target_pool)
            .await?
            .context("recovered pool is absent")?;
        anyhow::ensure!(
            pool["driver"] == "btrfs"
                && pool["config"]["source"] == self.plan.target_pool_source
                && pool["status"] == "Created",
            "recovered pool source or availability differs from plan"
        );
        self.publish_native().await?;
        self.enter("pool-ready").await?;
        Ok(mount)
    }

    async fn quarantine_stub(
        &mut self,
        mount: op_network::seeded_root::SeededRootMount,
        baseline: &JsonValue,
    ) -> Result<op_network::seeded_root::SeededRootMount> {
        self.admit().await?;
        self.enter("stub-quarantine-intent").await?;
        let mount = reverify_mounted_plan(self.plan, mount).await?;
        let etag = self
            .pre_effect(baseline, &self.plan.source_pool, false)
            .await?;
        anyhow::ensure!(
            etag == self.plan.expected_instance_etag,
            "native instance changed before original-stub quarantine"
        );
        self.admit().await?;
        let native_receipt = self
            .native
            .prepare_missing_root_for_rebuild(
                &self.plan.source_pool,
                &self.plan.source_pool_source,
                self.session_id,
                self.operation_id,
            )
            .await?;
        let receipt = native_stub_receipt_json(
            &native_receipt,
            self.session_id,
            self.digest,
            self.operation_id,
        )?;
        self.active_phase = "stub-absent";
        stub_absent_phase(
            self.store,
            self.session_id,
            self.digest,
            self.operation_id,
            self.expected,
            &receipt,
        )
        .await
        .map_err(|error| pending(self.operation_id, "stub-absent", error))?;
        Ok(mount)
    }

    async fn rebuild_root(
        &mut self,
        mount: op_network::seeded_root::SeededRootMount,
        baseline: &JsonValue,
    ) -> Result<(op_network::seeded_root::SeededRootMount, JsonValue)> {
        self.admit().await?;
        self.enter("rebuild-intent").await?;
        let mount = reverify_mounted_plan(self.plan, mount).await?;
        let etag = self
            .pre_effect(baseline, &self.plan.source_pool, false)
            .await?;
        anyhow::ensure!(
            etag == self.plan.expected_instance_etag,
            "native instance ETag changed before rebuild"
        );
        self.admit().await?;
        // Incus 7.4 ignores If-Match on rebuild POST; observed ETag is only a
        // drift check under the owner and host locks, not native CAS.
        self.native
            .rebuild_from_image(self.session_id, &self.plan.image_fingerprint)
            .await?;
        let (rebuilt, _) = self.native.instance(self.session_id).await?;
        verify_after_rebuild(baseline, &rebuilt, &self.plan.source_pool, self.plan)?;
        anyhow::ensure!(
            native_root_present(self.native, self.plan, &self.plan.source_pool).await?,
            "rebuilt root volume is absent"
        );
        self.publish_native().await?;
        self.enter("rebuilt").await?;
        Ok((mount, rebuilt))
    }

    async fn redirect_persist(
        &mut self,
        mount: op_network::seeded_root::SeededRootMount,
        rebuilt: &JsonValue,
    ) -> Result<(op_network::seeded_root::SeededRootMount, JsonValue)> {
        self.admit().await?;
        self.enter("persist-intent").await?;
        let mut mount = verify_selected_persist(self.plan, mount, true).await?;
        verify_old_persist_empty(self.plan).await?;
        let etag = self
            .pre_effect(rebuilt, &self.plan.source_pool, true)
            .await?;
        let old = self
            .plan
            .expected_btrfs_device
            .as_ref()
            .context("original persistence binding absent")?;
        anyhow::ensure!(
            rebuilt["devices"]["persist"]["source"] == old.mount_point
                && rebuilt["expanded_devices"]["persist"]["source"] == old.mount_point,
            "native persist source differs from original fstorage"
        );
        self.admit().await?;
        self.native
            .redirect_persist_source(
                self.session_id,
                &old.mount_point,
                &self.plan.target_persist_source,
                &etag,
            )
            .await?;
        let (persisted, _) = self.native.instance(self.session_id).await?;
        verify_persist_redirect(rebuilt, &persisted, self.plan)?;
        mount = verify_selected_persist(self.plan, mount, true).await?;
        self.publish_native().await?;
        self.enter("persist-ready").await?;
        Ok((mount, persisted))
    }

    async fn move_root(
        &mut self,
        mount: op_network::seeded_root::SeededRootMount,
        original: &JsonValue,
        persisted: &JsonValue,
    ) -> Result<(op_network::seeded_root::SeededRootMount, IncusInstance)> {
        self.admit().await?;
        self.enter("move-intent").await?;
        let mount = reverify_mounted_plan(self.plan, mount).await?;
        let mount = verify_selected_persist(self.plan, mount, true).await?;
        let _etag = self
            .pre_effect(persisted, &self.plan.source_pool, true)
            .await?;
        anyhow::ensure!(
            !native_root_present(self.native, self.plan, &self.plan.target_pool).await?,
            "target pool already has an identity root volume"
        );
        let pool = self
            .native
            .pool(&self.plan.target_pool)
            .await?
            .context("seeded target pool disappeared")?;
        anyhow::ensure!(
            pool["status"] == "Created" && pool["config"]["source"] == self.plan.target_pool_source,
            "seeded target pool is unavailable or changed before move"
        );
        self.admit().await?;
        // Incus 7.4 ignores If-Match on the migration POST. Owner/host locks,
        // fresh readback and post-effect verification provide the fence here.
        self.native
            .move_to_pool(self.session_id, &self.plan.target_pool)
            .await?;
        let (moved, _) = self.native.instance(self.session_id).await?;
        verify_after_move(
            original,
            persisted,
            &moved,
            &self.plan.target_pool,
            self.plan,
        )?;
        anyhow::ensure!(
            native_root_present(self.native, self.plan, &self.plan.target_pool).await?,
            "moved root subvolume is absent"
        );
        anyhow::ensure!(
            self.native
                .root_volume_present(&self.plan.target_pool, self.session_id)
                .await?,
            "moved root volume metadata is absent"
        );
        let snapshot = self.publish_native().await?;
        self.enter("moved").await?;
        let instance = snapshot
            .instances
            .into_iter()
            .find(|instance| instance.name == self.session_id)
            .context("moved identity absent from native snapshot")?;
        anyhow::ensure!(instance.status == "Stopped", "moved identity is not parked");
        let mount = verify_selected_persist(self.plan, mount, false).await?;
        Ok((mount, instance))
    }

    async fn commit_identity(
        &mut self,
        mount: op_network::seeded_root::SeededRootMount,
        mut sled: ContainerIdentitySled,
        instance: IncusInstance,
    ) -> Result<JsonValue> {
        let mount = reverify_mounted_plan(self.plan, mount).await?;
        let mount = verify_selected_persist(self.plan, mount, false).await?;
        let mount = install_boot_descriptor(self.plan, self.session_id, mount).await?;
        let old = self
            .plan
            .expected_btrfs_device
            .as_ref()
            .context("original persistence binding absent")?;
        let current = sled
            .btrfs_device
            .as_ref()
            .context("original persistence binding absent")?;
        anyhow::ensure!(
            old.matches(current),
            "original persistence binding changed during repair"
        );
        let cozo_id = current.cozo_id.clone();
        sled.btrfs_device = Some(SledBtrfsDevice {
            device_path: self.plan.selected_btrfs_device.device_path.clone(),
            mount_point: self.plan.selected_btrfs_device.mount_point.clone(),
            btrfs_uuid: self.plan.selected_btrfs_device.btrfs_uuid.clone(),
            cozo_id,
            attached: true,
        });
        sled.instance = Some(instance);
        sled.mutation_index = sled
            .mutation_index
            .checked_add(1)
            .context("mutation index overflow")?;
        sled.last_seen_at = now();
        self.enter("verified").await?;
        self.admit().await?;
        commit_sled_update(
            self.engine,
            self.store,
            format!("{}-identity", self.operation_id),
            self.expected.clone(),
            sled,
            (),
        )
        .await
        .map_err(|error| {
            pending(
                self.operation_id,
                "native-moved-identity-uncommitted",
                error,
            )
        })?;
        let committed = load_revision(self.store, self.session_id)
            .await
            .map_err(|error| {
                pending(
                    self.operation_id,
                    "native-moved-identity-uncommitted",
                    error,
                )
            })?;
        self.active_phase = "identity-committed";
        phase(
            self.store,
            self.session_id,
            self.digest,
            self.operation_id,
            "identity-committed",
            &committed,
        )
        .await
        .map_err(|error| pending(self.operation_id, "identity-committed", error))?;
        drop(mount);
        Ok(serde_json::json!({
            "status":"root_and_persist_moved",
            "phase":"identity-committed",
            "operation_id":self.operation_id,
            "session_id":self.session_id,
            "plan_sha256":self.digest,
            "pool_name":self.plan.target_pool.clone(),
            "btrfs_uuid":self.plan.leaf_fsid.clone(),
        }))
    }
}

async fn repair_own_root_with_backend(request: RepairRequest<'_>) -> Result<JsonValue> {
    let RepairRequest {
        engine,
        store,
        actor_id,
        session_id,
        session_genesis,
        input,
        original_seal,
        operation_id,
        native,
        plan_dir,
        plan_owner_uid,
    } = request;
    anyhow::ensure!(
        valid_digest(&input.plan_sha256),
        "invalid root repair plan digest"
    );
    let lifecycle_lock = store.lifecycle_lock(session_id).await;
    let _session_guard = lifecycle_lock.lock_owned().await;
    recheck_own_admission(actor_id, original_seal)?;
    let expected = load_revision(store, session_id).await?;
    let sled = revision_sled(&expected)?.context("own identity is absent")?;
    anyhow::ensure!(
        sled.is_anchored()
            && sled.session_id == session_id
            && sled.genesis.as_deref() == Some(session_genesis)
            && op_identity::session::derive_principal_id(&sled.wireguard_pubkey) == actor_id
            && op_identity::session::derive_session_id(&sled.wireguard_pubkey) == session_id
            && sled
                .sealed_id
                .as_deref()
                .is_some_and(|value| !value.is_empty())
            && !sled.active
            && sled.instance.as_ref().is_some_and(
                |instance| instance.name == session_id && instance.instance_type == "container"
            ),
        "root repair requires the verified parked owner of the identity container"
    );
    verify_current_repair_sid1(engine, &sled, actor_id, session_genesis).await?;
    let cozo = store.cozo.clone();
    let own_session = session_id.to_string();
    if let Some(existing) =
        tokio::task::spawn_blocking(move || cozo.read_root_repair_journal(&own_session)).await??
    {
        return Err(pending(
            &existing.operation_id,
            &existing.phase,
            anyhow::anyhow!("existing root repair requires reconciliation"),
        ));
    }
    let plan_dir_for_read = plan_dir.clone();
    let own_session = session_id.to_string();
    let digest = input.plan_sha256.clone();
    let plan = tokio::task::spawn_blocking(move || {
        trusted_plan_at(&plan_dir_for_read, &own_session, &digest, plan_owner_uid)
    })
    .await??;
    plan.validate(session_id, &expected, &sled)?;
    // A populated old persist source requires a separate migration plan.
    // Reject it before importing a pool or rebuilding the missing root.
    verify_old_persist_empty(&plan).await?;
    preflight_native_identity_effect().await?;
    let (original, etag) = native.instance(session_id).await?;
    verify_original_instance(&original, &etag, &plan)?;
    verify_source_pool(native.as_ref(), &plan).await?;
    anyhow::ensure!(
        !native_root_present(native.as_ref(), &plan, &plan.source_pool).await?,
        "identity root is already present; repair plan is stale"
    );
    anyhow::ensure!(
        native
            .root_volume_present(&plan.source_pool, session_id)
            .await?,
        "missing-root repair requires the original Incus volume record"
    );
    anyhow::ensure!(
        native.pool(&plan.target_pool).await?.is_none(),
        "dedicated target pool already exists; reconcile first"
    );

    // The first durable row is deliberately one-shot. Even a failure while
    // still in `prepared` requires explicit reconciliation; a retry cannot
    // invent a new operation ID and silently repeat later native intent.
    phase(
        store,
        session_id,
        &input.plan_sha256,
        &operation_id,
        "prepared",
        &expected,
    )
    .await
    .map_err(|error| pending(&operation_id, "prepared", error))?;
    recheck_own_admission(actor_id, original_seal)
        .map_err(|error| pending(&operation_id, "prepared", error))?;
    phase(
        store,
        session_id,
        &input.plan_sha256,
        &operation_id,
        "mount-intent",
        &expected,
    )
    .await
    .map_err(|error| pending(&operation_id, "mount-intent", error))?;
    let mut execution = RepairExecution {
        engine,
        store,
        actor_id,
        session_id,
        session_genesis,
        digest: &input.plan_sha256,
        original_seal,
        operation_id: &operation_id,
        native: native.as_ref(),
        plan: &plan,
        expected: &expected,
        active_phase: "mount-intent",
    };
    let continuation: Result<JsonValue> = async {
        let mut mounted = ensure_mounted_plan(&plan).await?;
        mounted = verify_selected_persist(&plan, mounted, true).await?;
        execution.enter("mounted").await?;

        let _observation = engine.lock_host_observation().await;
        let (current, current_etag) = native
            .instance(session_id)
            .await
            .map_err(|error| pending(&operation_id, "mounted", error))?;
        verify_original_instance(&current, &current_etag, &plan)
            .map_err(|error| pending(&operation_id, "mounted", error))?;
        verify_source_pool(native.as_ref(), &plan).await?;
        anyhow::ensure!(
            !native_root_present(native.as_ref(), &plan, &plan.source_pool).await?,
            "identity root appeared before pool recovery"
        );
        anyhow::ensure!(
            native.pool(&plan.target_pool).await?.is_none(),
            "target pool appeared before recovery"
        );
        mounted = execution.import_pool(mounted, &original).await?;
        mounted = execution.quarantine_stub(mounted, &original).await?;

        let (next_mount, rebuilt) = execution.rebuild_root(mounted, &original).await?;
        mounted = next_mount;

        let (next_mount, persisted) = execution.redirect_persist(mounted, &rebuilt).await?;
        mounted = next_mount;

        let (next_mount, native_instance) =
            execution.move_root(mounted, &original, &persisted).await?;
        mounted = next_mount;
        execution
            .commit_identity(mounted, sled, native_instance)
            .await
    }
    .await;
    continuation.map_err(|error| {
        if error
            .downcast_ref::<RepairReconciliationRequired>()
            .is_some()
        {
            error
        } else {
            pending(&operation_id, execution.active_phase, error)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_stub_receipt_pins_owner_and_original_backup() {
        use op_network::incus_root_subvolume::{MissingRootPreparation, QuarantinedRootStub};
        let plan = "a".repeat(64);
        let backup = "b".repeat(64);
        let native = MissingRootPreparation::Quarantined(QuarantinedRootStub {
            path: PathBuf::from("/private/pool/.opdbus-root-session-repair-op"),
            original_stub_inode: 256,
            backup_inode: 257,
            backup_sha256: backup.clone(),
            original_stub_mode: 0o100,
        });
        let receipt: JsonValue = serde_json::from_str(
            &native_stub_receipt_json(&native, "session", &plan, "repair-op").unwrap(),
        )
        .unwrap();
        assert_eq!(receipt["status"], "quarantined");
        assert_eq!(receipt["session_id"], "session");
        assert_eq!(receipt["plan_sha256"], plan);
        assert_eq!(receipt["operation_id"], "repair-op");
        assert_eq!(receipt["backup_sha256"], backup);
        assert_eq!(receipt["original_stub_mode"], 0o100);
        let absent: JsonValue = serde_json::from_str(
            &native_stub_receipt_json(
                &MissingRootPreparation::AlreadyAbsent,
                "session",
                &plan,
                "repair-op",
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(absent["status"], "already_absent");
        assert_eq!(absent.as_object().unwrap().len(), 4);
    }

    #[test]
    fn stale_active_flag_still_starts_a_stopped_seeded_container() {
        assert!(decide_seeded_native_start(true, "Stopped").unwrap());
        assert!(!decide_seeded_native_start(true, "Running").unwrap());
        assert!(decide_seeded_native_start(false, "Running").is_err());
    }

    #[test]
    fn only_observed_incus_runtime_config_deltas_are_admitted() {
        let before = Some(std::collections::HashMap::from([
            ("volatile.uuid".into(), "pinned".into()),
            ("volatile.idmap.next".into(), "mapped".into()),
            ("volatile.apply_template".into(), "create".into()),
        ]));
        let mut running = before.clone().unwrap();
        running.remove("volatile.apply_template");
        running.insert("volatile.idmap.current".into(), "mapped".into());
        running.insert("volatile.last_state.power".into(), "RUNNING".into());
        running.insert("volatile.last_state.ready".into(), "false".into());
        verify_native_runtime_config(&before, &Some(running.clone()), "Running").unwrap();
        let mut stopped = running.clone();
        stopped.insert("volatile.last_state.power".into(), "STOPPED".into());
        verify_native_runtime_config(&Some(running.clone()), &Some(stopped.clone()), "Stopped")
            .unwrap();
        let mut wrong = stopped.clone();
        wrong.insert("volatile.uuid".into(), "other".into());
        assert!(
            verify_native_runtime_config(&Some(stopped.clone()), &Some(wrong), "Stopped").is_err()
        );
        let mut wrong = stopped.clone();
        wrong.insert("volatile.idmap.current".into(), "different".into());
        assert!(
            verify_native_runtime_config(&Some(stopped.clone()), &Some(wrong), "Stopped").is_err()
        );
        let mut wrong = stopped;
        wrong.insert("volatile.last_state.power".into(), "RUNNING".into());
        assert!(verify_native_runtime_config(&Some(running), &Some(wrong), "Stopped").is_err());
    }

    #[test]
    fn repaired_first_start_requires_original_idmap_digest() {
        let before = Some(std::collections::HashMap::from([(
            "volatile.uuid".into(),
            "pinned".into(),
        )]));
        let mut running = before.clone().unwrap();
        running.insert("volatile.idmap.current".into(), "mapped".into());
        running.insert("volatile.last_state.power".into(), "RUNNING".into());
        let digest = hex::encode(Sha256::digest(b"mapped"));
        assert!(verify_native_runtime_config(&before, &Some(running.clone()), "Running").is_err());
        verify_native_runtime_config_with_repair_pin(
            &before,
            &Some(running.clone()),
            "Running",
            Some(&digest),
        )
        .unwrap();
        let wrong_digest = hex::encode(Sha256::digest(b"foreign"));
        assert!(verify_native_runtime_config_with_repair_pin(
            &before,
            &Some(running.clone()),
            "Running",
            Some(&wrong_digest),
        )
        .is_err());
        running.insert("volatile.uuid".into(), "other".into());
        assert!(verify_native_runtime_config_with_repair_pin(
            &before,
            &Some(running),
            "Running",
            Some(&digest),
        )
        .is_err());
    }

    #[test]
    fn native_repair_readback_preserves_all_identity_and_disk_options() {
        let session = "00000000-0000-4000-8000-000000000001";
        let plan = RootRepairPlan {
            version: 3,
            session_id: session.into(),
            expected_session_genesis: "c".repeat(64),
            expected_revision: 1,
            expected_revision_operation_id: "initial".into(),
            expected_btrfs_device: Some(PlanBtrfsBinding {
                device_path: "/old/fstorage.img".into(),
                mount_point: "/old".into(),
                btrfs_uuid: session.into(),
                cozo_id: "pinned".into(),
            }),
            expected_instance_etag: "etag-initial".into(),
            expected_instance_uuid: session.into(),
            expected_first_start_idmap_sha256: hex::encode(Sha256::digest(b"[]")),
            image_fingerprint: "a".repeat(64),
            source_pool: "source".into(),
            source_pool_source: "/mnt/source".into(),
            target_pool: "seeded".into(),
            target_pool_source: "/mnt/sprout/seeded".into(),
            target_persist_source: "/mnt/sprout/persist".into(),
            target_persist_shift: "true".into(),
            selected_btrfs_device: PlanBtrfsBinding {
                device_path: "/var/lib/leaf.img".into(),
                mount_point: "/mnt/sprout".into(),
                btrfs_uuid: session.into(),
                cozo_id: "pinned".into(),
            },
            seed_image_path: "/var/lib/seed.img".into(),
            seed_sha256: "b".repeat(64),
            seed_fsid: session.into(),
            leaf_image_path: "/var/lib/leaf.img".into(),
            leaf_fsid: session.into(),
            mount_point: "/mnt/sprout".into(),
        };
        let original = serde_json::json!({
            "name":session, "status":"Stopped", "type":"container",
            "config":{"volatile.uuid":session,"volatile.base_image":plan.image_fingerprint,"volatile.idmap.next":"mapped","volatile.last_state.idmap":"[]","volatile.cloud-init.instance-id":"00000000-0000-4000-8000-000000000005","boot.autostart":"false","user.marker":"kept"},
            "expanded_config":{"volatile.uuid":session,"volatile.idmap.next":"mapped","volatile.last_state.idmap":"[]","volatile.cloud-init.instance-id":"00000000-0000-4000-8000-000000000005","boot.autostart":"false"},
            "profiles":["identity"],
            "devices":{"root":{"type":"disk","path":"/","pool":"source","size":"1GiB"},"persist":{"type":"disk","path":"/var/lib/app","source":"/old"}},
            "expanded_devices":{"root":{"type":"disk","path":"/","pool":"source","size":"1GiB"},"persist":{"type":"disk","path":"/var/lib/app","source":"/old"}}
        });
        verify_original_instance(&original, "etag-initial", &plan).unwrap();
        let mut wrong_original = original.clone();
        wrong_original["devices"]["persist"]["source"] = "/unexpected".into();
        assert!(verify_original_instance(&wrong_original, "etag-initial", &plan).is_err());
        let mut before_rebuild = original.clone();
        for field in ["config", "expanded_config"] {
            before_rebuild[field]["volatile.idmap.next"] = "mapped".into();
            before_rebuild[field]["volatile.last_state.idmap"] = "[]".into();
        }
        let mut rebuilt = before_rebuild.clone();
        for field in ["config", "expanded_config"] {
            rebuilt[field]
                .as_object_mut()
                .unwrap()
                .remove("volatile.idmap.next");
            rebuilt[field]
                .as_object_mut()
                .unwrap()
                .remove("volatile.last_state.idmap");
            rebuilt[field]["volatile.apply_template"] = "create".into();
        }
        verify_after_rebuild(&before_rebuild, &rebuilt, "source", &plan).unwrap();
        assert!(verify_after_effect(&before_rebuild, &rebuilt, "source", &plan).is_err());
        // The next native effect must compare against this rebuilt readback.
        verify_after_effect(&rebuilt, &rebuilt, "source", &plan).unwrap();
        let mut changed_rebuild = rebuilt.clone();
        changed_rebuild["config"]["user.marker"] = "changed".into();
        assert!(verify_after_rebuild(&before_rebuild, &changed_rebuild, "source", &plan).is_err());
        let mut added_rebuild = rebuilt.clone();
        added_rebuild["expanded_config"]["volatile.unexpected"] = "value".into();
        assert!(verify_after_rebuild(&before_rebuild, &added_rebuild, "source", &plan).is_err());
        let mut wrong_template = rebuilt.clone();
        wrong_template["config"]["volatile.apply_template"] = "other".into();
        assert!(verify_after_rebuild(&before_rebuild, &wrong_template, "source", &plan).is_err());
        let mut changed_uuid = rebuilt.clone();
        changed_uuid["config"]["volatile.uuid"] = "00000000-0000-4000-8000-000000000002".into();
        assert!(verify_after_rebuild(&before_rebuild, &changed_uuid, "source", &plan).is_err());
        let mut changed_image = rebuilt.clone();
        changed_image["config"]["volatile.base_image"] = "other".into();
        assert!(verify_after_rebuild(&before_rebuild, &changed_image, "source", &plan).is_err());
        let mut persisted = original.clone();
        persisted["devices"]["persist"]["source"] = plan.target_persist_source.clone().into();
        persisted["expanded_devices"]["persist"]["source"] =
            plan.target_persist_source.clone().into();
        persisted["devices"]["persist"]["shift"] = "true".into();
        persisted["expanded_devices"]["persist"]["shift"] = "true".into();
        verify_persist_redirect(&original, &persisted, &plan).unwrap();
        let mut changed_option = persisted.clone();
        changed_option["devices"]["persist"]["path"] = "/unexpected".into();
        assert!(verify_persist_redirect(&original, &changed_option, &plan).is_err());
        let mut moved = persisted.clone();
        moved["devices"]["root"]["pool"] = "seeded".into();
        moved["expanded_devices"]["root"]["pool"] = "seeded".into();
        verify_after_effect(&persisted, &moved, "seeded", &plan).unwrap();
        moved["devices"]["root"]["size"] = "2GiB".into();
        assert!(verify_after_effect(&persisted, &moved, "seeded", &plan).is_err());
        moved = original.clone();
        moved["config"]["user.marker"] = "changed".into();
        assert!(verify_after_effect(&original, &moved, "source", &plan).is_err());
        let mut persisted_rebuilt = rebuilt.clone();
        for field in ["devices", "expanded_devices"] {
            persisted_rebuilt[field]["persist"]["source"] =
                plan.target_persist_source.clone().into();
            persisted_rebuilt[field]["persist"]["shift"] = "true".into();
        }
        verify_persist_redirect(&rebuilt, &persisted_rebuilt, &plan).unwrap();
        let mut moved_rebuilt = persisted_rebuilt.clone();
        for field in ["config", "expanded_config"] {
            moved_rebuilt[field]["volatile.idmap.next"] = "mapped".into();
            moved_rebuilt[field]["volatile.last_state.idmap"] = "[]".into();
            moved_rebuilt[field]["volatile.cloud-init.instance-id"] =
                "00000000-0000-4000-8000-000000000006".into();
        }
        for field in ["devices", "expanded_devices"] {
            moved_rebuilt[field]["root"]["pool"] = "seeded".into();
        }
        verify_after_move(
            &before_rebuild,
            &persisted_rebuilt,
            &moved_rebuilt,
            "seeded",
            &plan,
        )
        .unwrap();
        assert!(verify_after_effect(&persisted_rebuilt, &moved_rebuilt, "seeded", &plan).is_err());
        let mut wrong_move = moved_rebuilt.clone();
        wrong_move["config"]["volatile.idmap.next"] = "foreign".into();
        assert!(verify_after_move(
            &before_rebuild,
            &persisted_rebuilt,
            &wrong_move,
            "seeded",
            &plan
        )
        .is_err());
        wrong_move = moved_rebuilt.clone();
        wrong_move["expanded_config"]["volatile.cloud-init.instance-id"] = "malformed".into();
        assert!(verify_after_move(
            &before_rebuild,
            &persisted_rebuilt,
            &wrong_move,
            "seeded",
            &plan
        )
        .is_err());
        wrong_move = moved_rebuilt;
        wrong_move["config"]["user.marker"] = "changed".into();
        assert!(verify_after_move(
            &before_rebuild,
            &persisted_rebuilt,
            &wrong_move,
            "seeded",
            &plan
        )
        .is_err());
        moved = original.clone();
        moved["expanded_devices"]["eth0"] = serde_json::json!({"type":"nic"});
        assert!(verify_after_effect(&original, &moved, "source", &plan).is_err());
    }

    #[test]
    fn completed_journal_pins_genesis_and_original_binding_across_later_revisions() {
        let session = "00000000-0000-4000-8000-000000000001";
        let plan = RootRepairPlan {
            version: 3,
            session_id: session.into(),
            expected_session_genesis: "c".repeat(64),
            expected_revision: 1,
            expected_revision_operation_id: "initial".into(),
            expected_btrfs_device: Some(PlanBtrfsBinding {
                device_path: "/old/fstorage.img".into(),
                mount_point: "/old".into(),
                btrfs_uuid: session.into(),
                cozo_id: "pinned".into(),
            }),
            expected_instance_etag: "etag-initial".into(),
            expected_instance_uuid: session.into(),
            expected_first_start_idmap_sha256: hex::encode(Sha256::digest(b"[]")),
            image_fingerprint: "a".repeat(64),
            source_pool: "source".into(),
            source_pool_source: "/mnt/source".into(),
            target_pool: "seeded".into(),
            target_pool_source: "/mnt/sprout/seeded".into(),
            target_persist_source: "/mnt/sprout/persist".into(),
            target_persist_shift: "true".into(),
            selected_btrfs_device: PlanBtrfsBinding {
                device_path: "/var/lib/leaf.img".into(),
                mount_point: "/mnt/sprout".into(),
                btrfs_uuid: session.into(),
                cozo_id: "pinned".into(),
            },
            seed_image_path: "/var/lib/seed.img".into(),
            seed_sha256: "b".repeat(64),
            seed_fsid: session.into(),
            leaf_image_path: "/var/lib/leaf.img".into(),
            leaf_fsid: session.into(),
            mount_point: "/mnt/sprout".into(),
        };
        let mut sled: ContainerIdentitySled = serde_json::from_value(serde_json::json!({
            "session_id":session,
            "wireguard_pubkey":"fixture-key",
            "genesis":plan.expected_session_genesis,
            "arrival_timestamp":1,
            "chain_head_at_arrival":"head"
        }))
        .unwrap();
        sled.btrfs_device = Some(SledBtrfsDevice {
            device_path: plan.selected_btrfs_device.device_path.clone(),
            mount_point: plan.selected_btrfs_device.mount_point.clone(),
            btrfs_uuid: plan.selected_btrfs_device.btrfs_uuid.clone(),
            cozo_id: plan.selected_btrfs_device.cozo_id.clone(),
            attached: false,
        });
        let mut journal = op_cozo_store::RootRepairJournal {
            version: 1,
            plan_sha256: "d".repeat(64),
            operation_id: "repair".into(),
            phase: "identity-committed".into(),
            expected_revision: 1,
            expected_revision_operation_id: "initial".into(),
            committed_revision: 2,
            stub_receipt_json: serde_json::json!({
                "status": "already_absent",
                "session_id": session,
                "plan_sha256": "d".repeat(64),
                "operation_id": "repair",
            })
            .to_string(),
        };
        let mut revision = IdentityRevision {
            session_id: session.into(),
            revision: 3,
            operation_id: "later".into(),
            record: None,
            genesis: None,
        };
        plan.validate_completed_journal(&journal, &revision, &sled)
            .unwrap();
        journal.phase = "moved".into();
        assert!(plan
            .validate_completed_journal(&journal, &revision, &sled)
            .is_err());
        journal.phase = "identity-committed".into();
        sled.genesis = Some("e".repeat(64));
        assert!(plan
            .validate_completed_journal(&journal, &revision, &sled)
            .is_err());
        sled.genesis = Some("c".repeat(64));
        revision.revision = 1;
        assert!(plan
            .validate_completed_journal(&journal, &revision, &sled)
            .is_err());
        revision.revision = 3;
        sled.btrfs_device.as_mut().unwrap().cozo_id = "untrusted".into();
        assert!(plan
            .validate_completed_journal(&journal, &revision, &sled)
            .is_err());
    }
}
