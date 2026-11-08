//! Read-only lifecycle verification of an already committed legacy seed plan.
//! Creation is exclusively external; this module never creates a leaf or mounts storage.

use super::*;
use anyhow::{Context, Result};
use op_plugins::state_plugins::identity_sled::SledBtrfsDevice;
use op_plugins::state_plugins::incus::IncusRootRepairBackend;
use serde::Deserialize;
use std::path::{Path, PathBuf};

const PLAN_DIR: &str = "/var/lib/op-dbus/identity-provision-plans";
const PROVISION_CAPABILITY: &str = "identity_sled.write";

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProvisionInput {
    pub wireguard_pubkey: String,
    pub plan_sha256: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SeedProvisionPlan {
    version: u8,
    session_id: String,
    wireguard_pubkey: String,
    expected_revision: u64,
    expected_revision_operation_id: String,
    seed_image_path: String,
    seed_sha256: String,
    seed_fsid: String,
    new_leaf_image_path: String,
    leaf_size_bytes: u64,
    mount_point: String,
    pool_name: String,
    image_fingerprint: String,
    instance: IncusInstance,
}




impl SeedProvisionPlan {
    fn pool_source(&self) -> PathBuf {
        Path::new(&self.mount_point).join("pool")
    }
    fn persist_source(&self) -> PathBuf {
        Path::new(&self.mount_point).join("persist")
    }
}

pub(super) async fn target_key_is_registered(public_key: &str) -> Result<()> {
    let principal_id = op_identity::session::derive_principal_id(public_key);
    anyhow::ensure!(
        crate::human_principal_registry::resolve_key_for_assertion(public_key)
            .await
            .map_err(|_| anyhow::anyhow!("target principal registry is unavailable"))?
            .is_some_and(|record| record.revoked_at == 0 && record.principal_id == principal_id),
        "target public key is not an active registered principal"
    );
    Ok(())
}

pub(super) struct ProvisionSealPin {
    method: String,
    schema_hash: String,
    blob: op_blob::PluginObjectBlob,
}


pub(super) fn capture_method_seal(method: &str) -> Result<ProvisionSealPin> {
    crate::method_contract_admission::admit_compiled_method(
        "identity_sled",
        method,
        Some(PROVISION_CAPABILITY),
    )?;
    let pinned = strict_identity_contract()?;
    Ok(ProvisionSealPin {
        method: method.to_string(),
        schema_hash: pinned.schema_hash,
        blob: pinned.blob,
    })
}

pub(super) async fn admit(engine: &NativeOwner, store: &IdentitySledStore,
    actor: &str, actor_session_id: &str, actor_genesis: &str, original: &ProvisionSealPin) -> Result<()> {
    engine.require_actor(actor)?;
    anyhow::ensure!(engine.method == original.method
        && engine.caller.session_id == actor_session_id
        && engine.caller.session_genesis == actor_genesis,
        "provisioning caller or method changed while queued");
    engine.revalidate().await?;
    let current = strict_identity_contract()?;
    anyhow::ensure!(current.schema_hash == original.schema_hash
        && current.blob.manifest == original.blob.manifest
        && current.blob.schema_json == original.blob.schema_json
        && current.blob.descriptor_set == original.blob.descriptor_set
        && current.blob.meta_json == original.blob.meta_json,
        "identity provision seal changed");
    let sled = revision_sled(&load_revision(store, actor_session_id).await?)?
        .context("provisioning caller identity is absent")?;
    engine.verify_own(&sled).await
}

async fn completed_plan(
    store: &IdentitySledStore,
    revision: &IdentityRevision,
    sled: &ContainerIdentitySled,
) -> Result<Option<SeedProvisionPlan>> {
    let cozo = store.cozo.clone();
    let sid = sled.session_id.clone();
    let journal =
        tokio::task::spawn_blocking(move || cozo.read_seed_provision_journal(&sid)).await??;
    let Some(journal) = journal else {
        return Ok(None);
    };
    anyhow::ensure!(
        journal.version == 1
            && journal.phase == "identity-committed"
            && journal.committed_revision > journal.expected_revision
            && revision.revision >= journal.committed_revision
            && sled.is_anchored()
            && sled.genesis.as_deref() == Some(journal.committed_genesis.as_str()),
        "seed provision journal is incomplete or identity genesis changed"
    );
    let directory = PathBuf::from(PLAN_DIR);
    let sid = sled.session_id.clone();
    let digest = journal.plan_sha256;
    let plan: SeedProvisionPlan = tokio::task::spawn_blocking(move || {
        serde_json::from_slice(&super::root_repair::trusted_plan_bytes_at(
            &directory, &sid, &digest, 0,
        )?)
        .context("invalid completed seed provision plan")
    })
    .await??;
    let binding = sled
        .btrfs_device
        .as_ref()
        .context("selected seeded binding is absent")?;
    anyhow::ensure!(
        plan.version == 1
            && plan.session_id == sled.session_id
            && plan.wireguard_pubkey == sled.wireguard_pubkey
            && plan.expected_revision == journal.expected_revision
            && plan.expected_revision_operation_id == journal.expected_revision_operation_id
            && binding.device_path == plan.new_leaf_image_path
            && binding.mount_point == plan.mount_point
            && binding.cozo_id == sled.session_id
            && binding.attached
            && uuid::Uuid::parse_str(&binding.btrfs_uuid).is_ok(),
        "completed seeded binding differs from trusted plan"
    );
    Ok(Some(plan))
}

fn descriptor<'a>(
    plan: &'a SeedProvisionPlan,
    sled: &'a ContainerIdentitySled,
) -> Result<op_network::seeded_root::SeededRootDescriptor<'a>> {
    let binding = sled
        .btrfs_device
        .as_ref()
        .context("seeded binding absent")?;
    Ok(op_network::seeded_root::SeededRootDescriptor {
        seed_image: Path::new(&plan.seed_image_path),
        seed_sha256: &plan.seed_sha256,
        seed_fsid: uuid::Uuid::parse_str(&plan.seed_fsid)?,
        leaf_image: Path::new(&plan.new_leaf_image_path),
        sprout_fsid: uuid::Uuid::parse_str(&binding.btrfs_uuid)?,
        mount_point: Path::new(&plan.mount_point),
        pool_subvolume: "pool",
    })
}

async fn mount_completed(
    plan: SeedProvisionPlan,
    sled: ContainerIdentitySled,
) -> Result<Arc<op_network::seeded_root::SeededRootMount>> {
    tokio::task::spawn_blocking(move || {
        let descriptor = descriptor(&plan, &sled)?;
        let mount = op_network::seeded_root::verify_seeded_root_mounted(&descriptor)
            .context("seeded identity storage must already be mounted by the external host provisioner or Incus; the bridge does not mount storage")?;
        mount.verify_persist_subvolume(&descriptor, "persist", false)?;
        Ok::<_, anyhow::Error>(Arc::new(mount))
    })
    .await?
}

async fn reverify_completed(
    mount: Arc<op_network::seeded_root::SeededRootMount>,
    plan: SeedProvisionPlan,
    sled: ContainerIdentitySled,
) -> Result<Arc<op_network::seeded_root::SeededRootMount>> {
    tokio::task::spawn_blocking(move || {
        let descriptor = descriptor(&plan, &sled)?;
        mount.revalidate(&descriptor)?;
        mount.verify_persist_subvolume(&descriptor, "persist", false)?;
        Ok::<_, anyhow::Error>(mount)
    })
    .await?
}

fn verify_committed_native(
    plan: &SeedProvisionPlan,
    sled: &ContainerIdentitySled,
    current: &IncusInstance,
    native_status: &str,
) -> Result<()> {
    let pinned = sled
        .instance
        .as_ref()
        .context("committed Incus readback absent")?;
    anyhow::ensure!(
        current.name == plan.session_id
            && current.status == native_status
            && current.instance_type == "container"
            && current.profiles == pinned.profiles
            && current
                .config
                .as_ref()
                .and_then(|config| config.get("volatile.base_image"))
                == Some(&plan.image_fingerprint)
            && current
                .config
                .as_ref()
                .and_then(|config| config.get("volatile.uuid"))
                == pinned
                    .config
                    .as_ref()
                    .and_then(|config| config.get("volatile.uuid")),
        "native seeded identity UUID or image changed"
    );
    super::root_repair::verify_native_runtime_config(
        &pinned.config,
        &current.config,
        native_status,
    )?;
    super::root_repair::verify_native_runtime_config(
        &pinned.expanded_config,
        &current.expanded_config,
        native_status,
    )?;
    for expanded in [false, true] {
        let before = super::root_repair::projected_devices(pinned, expanded)?;
        let after = super::root_repair::projected_devices(current, expanded)?;
        anyhow::ensure!(before == after, "native seeded devices changed");
        anyhow::ensure!(
            after
                .get("root")
                .is_some_and(|root| root["pool"] == plan.pool_name)
                && after.get("persist").is_some_and(|persist| persist["source"]
                    == plan.persist_source().to_string_lossy().as_ref()
                    && persist["shift"] == "true")
                && !after.values().any(|device| device["type"] == "nic"),
            "native seeded root, persist or network changed"
        );
    }
    Ok(())
}

pub(super) async fn guard_start(
    store: &IdentitySledStore,
    revision: &IdentityRevision,
    sled: &ContainerIdentitySled,
) -> Result<Option<super::root_repair::RootStartGuard>> {
    let Some(plan) = completed_plan(store, revision, sled).await? else {
        return Ok(None);
    };
    preflight_native_identity_effect().await?;
    let native = IncusRootRepairBackend::local();
    let mount = mount_completed(plan.clone(), sled.clone()).await?;
    let pool = native
        .pool(&plan.pool_name)
        .await?
        .context("seeded pool absent")?;
    anyhow::ensure!(
        pool["status"] == "Created"
            && pool["config"]["source"] == plan.pool_source().to_string_lossy().as_ref(),
        "seeded pool is unavailable or source changed"
    );
    let (raw, etag) = native.instance(&plan.session_id).await?;
    anyhow::ensure!(!etag.is_empty(), "native seeded instance ETag absent");
    let status = raw["status"]
        .as_str()
        .context("native seeded status absent")?;
    let should_start = super::root_repair::decide_seeded_native_start(sled.active, status)?;
    anyhow::ensure!(
        matches!(
            native
                .root_subvolume_state(
                    &plan.pool_name,
                    plan.pool_source().to_str().context("invalid pool source")?,
                    &plan.session_id,
                )
                .await?,
            op_network::incus_root_subvolume::RootSubvolumeState::Present
        ),
        "seeded identity root subvolume is absent"
    );
    anyhow::ensure!(
        native
            .root_volume_present(&plan.pool_name, &plan.session_id)
            .await?,
        "seeded identity root volume metadata is absent"
    );
    let snapshot = native.default_project_state().await?;
    let current = snapshot
        .instances
        .iter()
        .find(|instance| instance.name == plan.session_id)
        .context("seeded instance absent from native project")?;
    verify_committed_native(&plan, sled, current, status)?;
    let mount = reverify_completed(mount, plan.clone(), sled.clone()).await?;
    Ok(Some(super::root_repair::RootStartGuard {
        should_start,
        _mount: mount,
    }))
}

pub(super) async fn observe_power(
    engine: &NativeOwner,
    store: &IdentitySledStore,
    revision: &IdentityRevision,
    sled: &ContainerIdentitySled,
    expected_status: &str,
) -> Result<Option<IncusInstance>> {
    let Some(plan) = completed_plan(store, revision, sled).await? else {
        return Ok(None);
    };
    let native = IncusRootRepairBackend::local();
    let snapshot = native.default_project_state().await?;
    let current = snapshot
        .instances
        .iter()
        .find(|instance| instance.name == plan.session_id)
        .context("seeded power readback absent")?;
    verify_committed_native(&plan, sled, current, expected_status)?;
    let observed = current.clone();
    engine.observe_incus(&snapshot).await?;
    Ok(Some(observed))
}


