//! Canonical seeded creation for a new identity session. All native inputs
//! come from one root-owned, digest-pinned plan, never from method arguments.
//! The absent-record Cozo journal fences uncertain storage/Incus effects.

use super::*;
use anyhow::{Context, Result};
use op_plugins::state_plugins::identity_sled::SledBtrfsDevice;
use op_plugins::state_plugins::incus::IncusRootRepairBackend;
use op_plugins::state_plugins::incus_device::Device;
use serde::Deserialize;
use std::path::{Component, Path, PathBuf};

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

fn normalized_absolute(path: &str) -> bool {
    Path::new(path).is_absolute()
        && Path::new(path)
            .components()
            .all(|part| matches!(part, Component::RootDir | Component::Normal(_)))
}

impl SeedProvisionPlan {
    fn pool_source(&self) -> PathBuf {
        Path::new(&self.mount_point).join("pool")
    }
    fn persist_source(&self) -> PathBuf {
        Path::new(&self.mount_point).join("persist")
    }

    fn validate(&self, input: &ProvisionInput, expected: &IdentityRevision) -> Result<()> {
        anyhow::ensure!(self.version == 1, "unsupported seed provision plan version");
        let key_bytes = base64::engine::general_purpose::STANDARD
            .decode(&input.wireguard_pubkey)
            .context("invalid provision public key encoding")?;
        anyhow::ensure!(
            key_bytes.len() == 32
                && base64::engine::general_purpose::STANDARD.encode(&key_bytes)
                    == input.wireguard_pubkey,
            "provision public key is not canonical 32-byte Base64"
        );
        anyhow::ensure!(
            self.wireguard_pubkey == input.wireguard_pubkey
                && self.session_id
                    == op_identity::session::derive_session_id(&input.wireguard_pubkey)
                && expected.session_id == self.session_id
                && expected.revision == self.expected_revision
                && expected.operation_id == self.expected_revision_operation_id
                && expected.record.is_none()
                && expected.genesis.is_none(),
            "trusted seed provision plan does not match the absent identity"
        );
        anyhow::ensure!(
            super::root_repair::valid_digest(&self.seed_sha256)
                && super::root_repair::valid_digest(&self.image_fingerprint)
                && uuid::Uuid::parse_str(&self.seed_fsid).is_ok()
                && self.leaf_size_bytes >= 1024 * 1024 * 1024,
            "invalid sealed seed or image provenance"
        );
        for path in [
            &self.seed_image_path,
            &self.new_leaf_image_path,
            &self.mount_point,
        ] {
            anyhow::ensure!(normalized_absolute(path), "noncanonical provision path");
        }
        op_plugins::state_plugins::incus::validate_root_repair_pool_name(&self.pool_name)?;
        anyhow::ensure!(
            self.instance.name == self.session_id
                && self.instance.instance_type == "container"
                && self.instance.status == "Stopped"
                && self.instance.image.as_deref() == Some(self.image_fingerprint.as_str())
                && self.instance.storage_pool.as_deref() == Some(self.pool_name.as_str())
                && self.instance.profiles == ["identity"],
            "seeded instance identity, image, pool or profile differs from plan"
        );
        let config = self
            .instance
            .config
            .as_ref()
            .context("seeded instance config absent")?;
        anyhow::ensure!(
            config.get("boot.autostart").map(String::as_str) == Some("false")
                && config.get("security.privileged").map(String::as_str) == Some("false")
                && config.get("security.nesting").map(String::as_str) == Some("false")
                && !config.keys().any(|key| key.starts_with("volatile.")
                    || key.starts_with("raw.")
                    || key.starts_with("image.")),
            "seeded instance is not parked and unprivileged"
        );
        anyhow::ensure!(
            self.instance
                .devices
                .iter()
                .all(|named| matches!(named.device, Device::Disk(_)))
                && self
                    .instance
                    .expanded_devices
                    .as_ref()
                    .is_some_and(|devices| devices.iter().all(|named| matches!(named.device, Device::Disk(_))))
                && self.instance.expanded_config.is_some(),
            "seeded instance includes untrusted local devices"
        );
        let devices = self
            .instance
            .devices
            .iter()
            .map(|named| (named.name.as_str(), named.device.to_incus_map()))
            .collect::<std::collections::BTreeMap<_, _>>();
        anyhow::ensure!(
            devices.len() == self.instance.devices.len(),
            "duplicate seeded device"
        );
        let root = devices.get("root").context("seeded root disk absent")?;
        let persist = devices
            .get("persist")
            .context("seeded persist disk absent")?;
        anyhow::ensure!(
            root.get("type").map(String::as_str) == Some("disk")
                && root.get("path").map(String::as_str) == Some("/")
                && root.get("pool").map(String::as_str) == Some(self.pool_name.as_str())
                && root.len() == 3
                && persist.get("type").map(String::as_str) == Some("disk")
                && persist.get("path").map(String::as_str) == Some("/opt/run-mounts/persist")
                && persist.get("source").map(String::as_str) == self.persist_source().to_str()
                && persist.get("shift").map(String::as_str) == Some("true")
                && persist.len() == 4
                && devices.keys().all(|name| matches!(
                    *name,
                    "root" | "persist" | "identity" | "ghostbridge-socket"
                )),
            "seeded root or shifted persistence differs from trusted plan"
        );
        let expanded = super::root_repair::projected_devices(&self.instance, true)?;
        let expanded_config = self
            .instance
            .expanded_config
            .as_ref()
            .context("trusted profile config absent")?;
        anyhow::ensure!(
            expanded
                .get("root")
                .is_some_and(|root| root["pool"] == self.pool_name)
                && expanded
                    .get("persist")
                    .is_some_and(|persist| persist["source"]
                        == self.persist_source().to_string_lossy().as_ref()
                        && persist["shift"] == "true")
                && !expanded.values().any(|device| device["type"] == "nic")
                && expanded.keys().all(|name| matches!(
                    name.as_str(),
                    "root" | "persist" | "identity" | "ghostbridge-socket"
                ))
                && expanded_config.get("boot.autostart").map(String::as_str) == Some("false")
                && expanded_config
                    .get("security.privileged")
                    .map(String::as_str)
                    == Some("false")
                && !expanded_config.keys().any(|key| key.starts_with("raw.")
                    || key.starts_with("volatile.")
                    || key.starts_with("image.")),
            "trusted profile expansion is not the pinned disk-only layout"
        );
        Ok(())
    }
}

fn verify_creation_config(
    expected: &std::collections::HashMap<String, String>,
    observed: &std::collections::HashMap<String, String>,
) -> Result<()> {
    let mut observed = observed.clone();
    // Incus adds these while creating a stopped unprivileged container. Keep
    // the set closed and check their shape before accepting the native
    // readback; the full values are then retained in authoritative state.
    for key in [
        "volatile.uuid.generation",
        "volatile.cloud-init.instance-id",
    ] {
        if let Some(value) = observed.remove(key) {
            anyhow::ensure!(
                uuid::Uuid::parse_str(&value).is_ok_and(|uuid| uuid.to_string() == value),
                "native create added an invalid Incus UUID field"
            );
        }
    }
    if let Some(value) = observed.remove("volatile.idmap.base") {
        anyhow::ensure!(
            value.parse::<u64>().is_ok(),
            "native create added an invalid Incus idmap base"
        );
    }
    if let Some(value) = observed.remove("volatile.last_state.idmap") {
        anyhow::ensure!(
            serde_json::from_str::<serde_json::Value>(&value).is_ok_and(|json| json.is_array()),
            "native create added an invalid Incus prior idmap"
        );
    }
    for key in [
        "volatile.uuid",
        "volatile.base_image",
        "volatile.idmap.next",
        "volatile.apply_template",
        "volatile.last_state.power",
        "volatile.last_state.ready",
    ] {
        observed.remove(key);
    }
    // These descriptive fields come from the pinned image metadata, rather
    // than the create request. The exact base-image fingerprint is checked
    // separately before this comparison.
    observed.retain(|key, _| !key.starts_with("image."));
    anyhow::ensure!(
        !observed.keys().any(|key| key.starts_with("volatile.")) && &observed == expected,
        "native create added or changed unpinned configuration"
    );
    Ok(())
}

pub(super) async fn target_key_is_registered(public_key: &str) -> Result<()> {
    let principal_id = op_identity::session::derive_principal_id(public_key);
    anyhow::ensure!(
        crate::human_principal_dispatch::resolve_key_for_assertion(public_key)
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

pub(super) fn capture_provision_seal() -> Result<ProvisionSealPin> {
    capture_method_seal("provision_container")
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

pub(super) async fn admit(
    engine: &MutationEngine,
    store: &IdentitySledStore,
    actor: &str,
    actor_session_id: &str,
    actor_genesis: &str,
    original: &ProvisionSealPin,
) -> Result<()> {
    crate::method_contract_admission::admit_compiled_method(
        "identity_sled",
        &original.method,
        Some(PROVISION_CAPABILITY),
    )?;
    let current = strict_identity_contract()?;
    anyhow::ensure!(
        current.schema_hash == original.schema_hash
            && current.blob.manifest == original.blob.manifest
            && current.blob.schema_json == original.blob.schema_json
            && current.blob.descriptor_set == original.blob.descriptor_set
            && current.blob.meta_json == original.blob.meta_json,
        "seed provision seal changed while queued"
    );
    anyhow::ensure!(
        crate::interceptor::load_capability_grants(actor).contains(PROVISION_CAPABILITY),
        "seed provision requires a current exact-principal grant"
    );
    let current = engine
        .session_context_for_actor(actor)
        .await
        .context("provisioning caller has no current authenticated session context")?;
    anyhow::ensure!(
        current.session_id == actor_session_id && current.genesis_hex == actor_genesis,
        "provisioning caller session changed while queued"
    );
    let revision = load_revision(store, actor_session_id).await?;
    let sled = revision_sled(&revision)?.context("provisioning caller identity disappeared")?;
    // Humans have no per-account container (not scalable); their liveness is
    // already proven above by the live session/genesis match. The stored-SID1
    // self-instance check only makes sense for a principal that owns a
    // container, so it stays scoped to non-human actors.
    if sled.principal_kind.as_deref() != Some("human") {
        super::root_repair::verify_current_repair_sid1(engine, &sled, actor, actor_genesis).await?;
    }
    Ok(())
}

struct ProvisionPending {
    operation_id: String,
    phase: String,
    _cause: anyhow::Error,
}
impl std::fmt::Display for ProvisionPending {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "seed provision {} requires reconciliation after {}",
            self.operation_id, self.phase
        )
    }
}
impl std::fmt::Debug for ProvisionPending {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}
impl std::error::Error for ProvisionPending {
    #[cfg(test)]
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self._cause.as_ref())
    }
}
fn pending(operation_id: &str, phase: &str, cause: impl Into<anyhow::Error>) -> anyhow::Error {
    ProvisionPending {
        operation_id: operation_id.into(),
        phase: phase.into(),
        _cause: cause.into(),
    }
    .into()
}

async fn enter(
    store: &IdentitySledStore,
    sid: &str,
    digest: &str,
    operation: &str,
    phase: &str,
    expected: &IdentityRevision,
) -> Result<()> {
    let cozo = store.cozo.clone();
    let (sid, digest, operation, phase, expected) = (
        sid.to_owned(),
        digest.to_owned(),
        operation.to_owned(),
        phase.to_owned(),
        expected.clone(),
    );
    tokio::task::spawn_blocking(move || {
        cozo.append_seed_provision_phase(&sid, &digest, &operation, &phase, &expected, now())
    })
    .await??;
    Ok(())
}

async fn revalidate_provision(
    guard: op_network::seeded_root::SeededProvisionGuard,
) -> Result<op_network::seeded_root::SeededProvisionGuard> {
    tokio::task::spawn_blocking(move || {
        guard.revalidate()?;
        Ok::<_, anyhow::Error>(guard)
    })
    .await?
}

async fn install_provision_boot(
    guard: op_network::seeded_root::SeededProvisionGuard,
    plan: SeedProvisionPlan,
    session_id: String,
) -> Result<op_network::seeded_root::SeededProvisionGuard> {
    tokio::task::spawn_blocking(move || {
        guard.revalidate()?;
        let root_guard = guard.prepared_root_guard("pool")?;
        let descriptor = op_network::seeded_root::SeededRootDescriptor {
            seed_image: Path::new(&plan.seed_image_path),
            seed_sha256: &plan.seed_sha256,
            seed_fsid: uuid::Uuid::parse_str(&plan.seed_fsid)?,
            leaf_image: Path::new(&plan.new_leaf_image_path),
            sprout_fsid: guard.receipt.sprout_fsid,
            mount_point: Path::new(&plan.mount_point),
            pool_subvolume: "pool",
        };
        root_guard.verify_persist_subvolume(&descriptor, "persist", true)?;
        op_network::seeded_boot::install_verified(&session_id, &descriptor, &root_guard)?;
        Ok::<_, anyhow::Error>(guard)
    })
    .await?
}

fn native_instance_matches(plan: &SeedProvisionPlan, observed: &IncusInstance) -> Result<()> {
    anyhow::ensure!(
        observed.name == plan.session_id
            && observed.status == "Stopped"
            && observed.instance_type == "container"
            && observed.profiles == plan.instance.profiles
            && observed
                .config
                .as_ref()
                .and_then(|config| config.get("volatile.base_image"))
                == Some(&plan.image_fingerprint)
            && observed
                .config
                .as_ref()
                .and_then(|config| config.get("volatile.uuid"))
                .is_some_and(|value| uuid::Uuid::parse_str(value)
                    .is_ok_and(|uuid| uuid.to_string() == *value))
            && observed
                .config
                .as_ref()
                .and_then(|config| config.get("boot.autostart"))
                .map(String::as_str)
                == Some("false")
            && observed
                .config
                .as_ref()
                .and_then(|config| config.get("security.privileged"))
                .map(String::as_str)
                == Some("false"),
        "native seeded instance metadata differs from pinned plan"
    );
    verify_creation_config(
        plan.instance
            .config
            .as_ref()
            .context("plan config absent")?,
        observed.config.as_ref().context("native config absent")?,
    )?;
    verify_creation_config(
        plan.instance
            .expanded_config
            .as_ref()
            .context("plan expanded config absent")?,
        observed
            .expanded_config
            .as_ref()
            .context("native expanded config absent")?,
    )?;
    anyhow::ensure!(
        super::root_repair::projected_devices(&plan.instance, false)?
            == super::root_repair::projected_devices(observed, false)?
            && super::root_repair::projected_devices(&plan.instance, true)?
                == super::root_repair::projected_devices(observed, true)?,
        "native create device layout differs from trusted plan"
    );
    let local = super::root_repair::projected_devices(observed, false)?;
    let expanded = super::root_repair::projected_devices(observed, true)?;
    for devices in [&local, &expanded] {
        anyhow::ensure!(
            devices
                .get("root")
                .is_some_and(|root| root["pool"] == plan.pool_name)
                && devices
                    .get("persist")
                    .is_some_and(|persist| persist["source"]
                        == plan.persist_source().to_string_lossy().as_ref()
                        && persist["shift"] == "true")
                && !devices.values().any(|device| device["type"] == "nic"),
            "native seeded disks or network differ from pinned plan"
        );
    }
    Ok(())
}

struct ProvisionExecution<'a> {
    engine: &'a MutationEngine,
    store: &'a IdentitySledStore,
    actor: &'a str,
    actor_session_id: &'a str,
    actor_genesis: &'a str,
    sid: String,
    input: ProvisionInput,
    plan: SeedProvisionPlan,
    seal: ProvisionSealPin,
    operation_id: &'a str,
    expected: IdentityRevision,
    native: IncusRootRepairBackend,
    phase: &'static str,
}

impl ProvisionExecution<'_> {
    async fn admit(&self) -> Result<()> {
        admit(
            self.engine,
            self.store,
            self.actor,
            self.actor_session_id,
            self.actor_genesis,
            &self.seal,
        )
        .await
    }

    async fn enter(&mut self, next: &'static str) -> Result<()> {
        enter(
            self.store,
            &self.sid,
            &self.input.plan_sha256,
            self.operation_id,
            next,
            &self.expected,
        )
        .await?;
        self.phase = next;
        Ok(())
    }

    async fn compose_leaf(&mut self) -> Result<op_network::seeded_root::SeededProvisionGuard> {
        // Defense in depth: even an internal caller cannot allocate/map a leaf,
        // change membership or mount storage through this retired continuation.
        Err(anyhow::anyhow!(EXTERNAL_PROVISIONING_REQUIRED))
    }

    async fn import_pool(
        &mut self,
        guard: op_network::seeded_root::SeededProvisionGuard,
    ) -> Result<op_network::seeded_root::SeededProvisionGuard> {
        self.admit().await?;
        self.enter("pool-import-intent").await?;
        let guard = revalidate_provision(guard).await?;
        self.native
            .recover_existing_pool(
                &self.plan.pool_name,
                self.plan
                    .pool_source()
                    .to_str()
                    .context("invalid pool source")?,
            )
            .await?;
        let pool = self
            .native
            .pool(&self.plan.pool_name)
            .await?
            .context("seed pool import absent")?;
        anyhow::ensure!(
            pool["status"] == "Created"
                && pool["config"]["source"] == self.plan.pool_source().to_string_lossy().as_ref(),
            "seed pool unavailable or source changed"
        );
        let snapshot = self.native.default_project_state().await?;
        crate::incus_observer::publish_repair_readback_locked(self.engine, &snapshot).await?;
        self.enter("pool-ready").await?;
        Ok(guard)
    }

    async fn create_instance(
        &mut self,
        guard: op_network::seeded_root::SeededProvisionGuard,
    ) -> Result<(op_network::seeded_root::SeededProvisionGuard, IncusInstance)> {
        self.admit().await?;
        self.enter("instance-create-intent").await?;
        let guard = revalidate_provision(guard).await?;
        self.native
            .create_seeded_identity(&self.plan.instance)
            .await?;
        let (raw, etag) = self.native.instance(&self.sid).await?;
        anyhow::ensure!(
            !etag.is_empty()
                && raw["status"] == "Stopped"
                && raw["config"]["volatile.base_image"] == self.plan.image_fingerprint,
            "native create readback differs from plan"
        );
        anyhow::ensure!(
            matches!(
                self.native
                    .root_subvolume_state(
                        &self.plan.pool_name,
                        self.plan
                            .pool_source()
                            .to_str()
                            .context("invalid pool source")?,
                        &self.sid,
                    )
                    .await?,
                op_network::incus_root_subvolume::RootSubvolumeState::Present
            ),
            "created identity root subvolume is absent"
        );
        anyhow::ensure!(
            self.native
                .root_volume_present(&self.plan.pool_name, &self.sid)
                .await?,
            "created identity root volume metadata is absent"
        );
        let snapshot = self.native.default_project_state().await?;
        let observed = snapshot
            .instances
            .iter()
            .find(|instance| instance.name == self.sid)
            .context("seeded instance absent from native snapshot")?
            .clone();
        native_instance_matches(&self.plan, &observed)?;
        crate::incus_observer::publish_repair_readback_locked(self.engine, &snapshot).await?;
        self.enter("instance-ready").await?;
        Ok((guard, observed))
    }

    async fn commit_identity(
        &mut self,
        guard: op_network::seeded_root::SeededProvisionGuard,
        observed: IncusInstance,
    ) -> Result<JsonValue> {
        let guard = install_provision_boot(guard, self.plan.clone(), self.sid.clone()).await?;
        self.admit().await?;
        target_key_is_registered(&self.input.wireguard_pubkey).await?;
        anyhow::ensure!(
            load_revision(self.store, &self.sid).await? == self.expected,
            "identity revision changed before mint"
        );
        let stamp = self
            .engine
            .mint_identity_genesis_stamp(&self.sid, &self.input.wireguard_pubkey, 0)
            .await?;
        let timestamp = now();
        let mut sled = ContainerIdentitySled {
            session_id: self.sid.clone(),
            wireguard_pubkey: self.input.wireguard_pubkey.clone(),
            interface: String::new(),
            peer_ip: None,
            mutation_index: 0,
            genesis: None,
            trace_id: String::new(),
            schema_version: RECORD_FORMAT,
            vector_id: String::new(),
            sealed_id: None,
            principal_kind: None,
            btrfs_device: Some(SledBtrfsDevice {
                device_path: self.plan.new_leaf_image_path.clone(),
                mount_point: self.plan.mount_point.clone(),
                btrfs_uuid: guard.receipt.sprout_fsid.to_string(),
                // identity_sessions is keyed by this derived session ID; the
                // same row durably owns btrfs_device_json in existing Cozo.
                cozo_id: self.sid.clone(),
                attached: true,
            }),
            instance: Some(observed),
            session_started_at: timestamp,
            last_seen_at: timestamp,
            active: false,
            expires_at: None,
            arrival_timestamp: 0,
            chain_head_at_arrival: String::new(),
            catalog_hash_at_arrival: String::new(),
            head_timestamp_at_arrival: 0,
        };
        apply_genesis_stamp(&mut sled, &stamp)?;
        self.enter("verified").await?;
        self.admit().await?;
        let sled = commit_sled_update(
            self.engine,
            self.store,
            format!("{}-identity", self.operation_id),
            self.expected.clone(),
            sled.clone(),
            sled,
        )
        .await?;
        let committed = load_revision(self.store, &self.sid).await?;
        enter(
            self.store,
            &self.sid,
            &self.input.plan_sha256,
            self.operation_id,
            "identity-committed",
            &committed,
        )
        .await?;
        self.phase = "identity-committed";
        drop(guard);
        Ok(serde_json::json!({"identity":sled}))
    }

    async fn run(&mut self) -> Result<JsonValue> {
        let guard = self.compose_leaf().await?;
        let guard = self.import_pool(guard).await?;
        let (guard, observed) = self.create_instance(guard).await?;
        self.commit_identity(guard, observed).await
    }
}

/// The caller owns this task beyond transport cancellation. Any error after
/// the first durable native intent is returned as reconciliation required.
pub(super) async fn provision_container(
    engine: &MutationEngine,
    store: &IdentitySledStore,
    actor: VerifiedIdentityActor<'_>,
    input: ProvisionInput,
    seal: ProvisionSealPin,
    operation_id: &str,
) -> Result<JsonValue> {
    if super::root_repair::valid_digest(&input.plan_sha256) {
        return super::clean_provision::provision(
            engine,
            store,
            actor,
            input,
            seal,
            operation_id,
            false,
        )
        .await;
    }
    let VerifiedIdentityActor {
        actor_id: actor,
        session_id: actor_session_id,
        genesis: actor_genesis,
    } = actor;
    anyhow::ensure!(
        !actor.is_empty() && super::root_repair::valid_digest(&input.plan_sha256),
        "invalid provision admission or digest"
    );
    let sid = op_identity::session::derive_session_id(&input.wireguard_pubkey);
    let lifecycle = store.lifecycle_lock(&sid).await;
    let _session_guard = lifecycle.lock_owned().await;
    let _host_guard = engine.lock_host_observation().await;
    admit(engine, store, actor, actor_session_id, actor_genesis, &seal).await?;
    // Preserve the sealed method and exact-principal admission, but the old
    // image-based implementation cannot own shared physical storage or roots.
    // This unconditional gate precedes all journals and native effects.
    require_external_shared_seed_provisioning()?;
    preflight_native_identity_effect().await?;
    let expected = load_revision(store, &sid).await?;
    anyhow::ensure!(
        expected.record.is_none() && expected.genesis.is_none(),
        "identity is already provisioned"
    );
    let cozo = store.cozo.clone();
    let check_sid = sid.clone();
    anyhow::ensure!(
        tokio::task::spawn_blocking(move || cozo.read_seed_provision_journal(&check_sid))
            .await??
            .is_none(),
        "prior seed provision requires reconciliation"
    );
    let directory = PathBuf::from(PLAN_DIR);
    let sid_for_read = sid.clone();
    let digest = input.plan_sha256.clone();
    let plan: SeedProvisionPlan = tokio::task::spawn_blocking(move || {
        serde_json::from_slice(&super::root_repair::trusted_plan_bytes_at(
            &directory,
            &sid_for_read,
            &digest,
            0,
        )?)
        .context("invalid trusted seed provision plan")
    })
    .await??;
    plan.validate(&input, &expected)?;
    target_key_is_registered(&input.wireguard_pubkey).await?;
    let native = IncusRootRepairBackend::local();
    anyhow::ensure!(
        native.pool(&plan.pool_name).await?.is_none(),
        "seed pool already exists"
    );
    anyhow::ensure!(
        native
            .default_project_state()
            .await?
            .instances
            .iter()
            .all(|instance| instance.name != sid),
        "native target already exists"
    );
    enter(
        store,
        &sid,
        &input.plan_sha256,
        operation_id,
        "prepared",
        &expected,
    )
    .await?;
    let mut execution = ProvisionExecution {
        engine,
        store,
        actor,
        actor_session_id,
        actor_genesis,
        sid,
        input,
        plan,
        seal,
        operation_id,
        expected,
        native,
        phase: "prepared",
    };
    let outcome = execution.run().await;
    outcome.map_err(|error| pending(operation_id, execution.phase, error))
}

const EXTERNAL_PROVISIONING_REQUIRED: &str = "prepare the shared physical seed, private writable leaf and container through the external host provisioning script or Incus; the bridge cannot compose, mount or copy identity roots";

fn require_external_shared_seed_provisioning() -> Result<()> {
    anyhow::bail!(EXTERNAL_PROVISIONING_REQUIRED)
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
) -> Result<op_network::seeded_root::SeededRootMount> {
    tokio::task::spawn_blocking(move || {
        let descriptor = descriptor(&plan, &sled)?;
        let mount = op_network::seeded_root::verify_seeded_root_mounted(&descriptor)
            .context("seeded identity storage must already be mounted by the external host provisioner or Incus; the bridge does not mount storage")?;
        mount.verify_persist_subvolume(&descriptor, "persist", false)?;
        Ok::<_, anyhow::Error>(mount)
    })
    .await?
}

async fn reverify_completed(
    mount: op_network::seeded_root::SeededRootMount,
    plan: SeedProvisionPlan,
    sled: ContainerIdentitySled,
) -> Result<op_network::seeded_root::SeededRootMount> {
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
    engine: &MutationEngine,
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
    crate::incus_observer::publish_repair_readback_locked(engine, &snapshot).await?;
    Ok(Some(observed))
}

pub(super) async fn journal_incomplete(
    store: &IdentitySledStore,
    session_id: &str,
) -> Result<bool> {
    let cozo = store.cozo.clone();
    let sid = session_id.to_owned();
    let row = tokio::task::spawn_blocking(move || cozo.read_seed_provision_journal(&sid)).await??;
    Ok(row.is_some_and(|row| row.phase != "identity-committed"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_provisioning_requires_external_shared_seed_owner() {
        let error = require_external_shared_seed_provisioning().unwrap_err();
        assert!(error.to_string().contains("shared physical seed"));
        assert!(error.to_string().contains("cannot compose, mount or copy"));
    }

    fn fixture() -> (ProvisionInput, IdentityRevision, SeedProvisionPlan) {
        let key = "QkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkI=".to_string();
        let sid = op_identity::session::derive_session_id(&key);
        let image = "a".repeat(64);
        let config = serde_json::json!({
            "boot.autostart":"false", "security.privileged":"false", "security.nesting":"false"
        });
        let devices = serde_json::json!([
            {"name":"root","device":{"type":"disk","path":"/","pool":"seedpool"}},
            {"name":"persist","device":{"type":"disk","path":"/opt/run-mounts/persist",
                "source":"/mnt/sprout/persist","shift":"true"}}
        ]);
        let instance: IncusInstance = serde_json::from_value(serde_json::json!({
            "name":sid,"status":"Stopped","type":"container","image":image,
            "storage_pool":"seedpool","profiles":["identity"],
            "config":config,"expanded_config":config,
            "devices":devices,"expanded_devices":devices
        }))
        .unwrap();
        let plan = SeedProvisionPlan {
            version: 1,
            session_id: sid.clone(),
            wireguard_pubkey: key.clone(),
            expected_revision: 0,
            expected_revision_operation_id: String::new(),
            seed_image_path: "/var/lib/op-dbus/core.seed".into(),
            seed_sha256: "b".repeat(64),
            seed_fsid: "00000000-0000-4000-8000-000000000001".into(),
            new_leaf_image_path: "/var/lib/op-dbus/identity-leaves/new.img".into(),
            leaf_size_bytes: 1024 * 1024 * 1024,
            mount_point: "/mnt/sprout".into(),
            pool_name: "seedpool".into(),
            image_fingerprint: image,
            instance,
        };
        let input = ProvisionInput {
            wireguard_pubkey: key,
            plan_sha256: "c".repeat(64),
        };
        let revision = IdentityRevision {
            session_id: sid,
            revision: 0,
            operation_id: String::new(),
            record: None,
            genesis: None,
        };
        (input, revision, plan)
    }

    #[test]
    fn trusted_provision_plan_pins_session_revision_and_sealed_layout() {
        let (input, revision, mut plan) = fixture();
        plan.validate(&input, &revision).unwrap();
        let mut foreign = input.clone();
        foreign.wireguard_pubkey = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into();
        assert!(plan.validate(&foreign, &revision).is_err());
        let mut stale = revision.clone();
        stale.revision += 1;
        assert!(plan.validate(&input, &stale).is_err());
        plan.instance
            .config
            .as_mut()
            .unwrap()
            .insert("raw.idmap".into(), "unsafe".into());
        assert!(plan.validate(&input, &revision).is_err());
    }

    #[test]
    fn trusted_plan_rejects_privileged_nic_duplicate_and_wrong_disks() {
        let (input, revision, plan) = fixture();
        let mut privileged = plan.clone();
        privileged
            .instance
            .config
            .as_mut()
            .unwrap()
            .insert("security.privileged".into(), "true".into());
        assert!(privileged.validate(&input, &revision).is_err());
        let mut nic = plan.clone();
        nic.instance.expanded_devices.as_mut().unwrap().push(
            serde_json::from_value(serde_json::json!({
                "name":"eth0","device":{"type":"nic","network":"default"}
            }))
            .unwrap(),
        );
        assert!(nic.validate(&input, &revision).is_err());
        let mut duplicate = plan.clone();
        duplicate
            .instance
            .devices
            .push(duplicate.instance.devices[0].clone());
        assert!(duplicate.validate(&input, &revision).is_err());
        let mut wrong_persist = plan.clone();
        wrong_persist.instance.devices[1] = serde_json::from_value(serde_json::json!({
            "name":"persist","device":{"type":"disk","path":"/opt/run-mounts/persist",
                "source":"/old/fstorage","shift":"true"}
        }))
        .unwrap();
        assert!(wrong_persist.validate(&input, &revision).is_err());
        let mut wrong_root = plan;
        wrong_root.instance.devices[0] = serde_json::from_value(serde_json::json!({
            "name":"root","device":{"type":"disk","path":"/","pool":"shared"}
        }))
        .unwrap();
        assert!(wrong_root.validate(&input, &revision).is_err());
    }

    #[test]
    fn native_create_readback_rejects_profile_nic_and_configuration_drift() {
        let (input, revision, plan) = fixture();
        plan.validate(&input, &revision).unwrap();
        let mut observed = plan.instance.clone();
        let uuid = "00000000-0000-4000-8000-000000000002";
        for config in [
            observed.config.as_mut().unwrap(),
            observed.expanded_config.as_mut().unwrap(),
        ] {
            config.insert("volatile.uuid".into(), uuid.into());
            config.insert("volatile.base_image".into(), plan.image_fingerprint.clone());
            config.insert(
                "volatile.uuid.generation".into(),
                "00000000-0000-4000-8000-000000000003".into(),
            );
            config.insert(
                "volatile.cloud-init.instance-id".into(),
                "00000000-0000-4000-8000-000000000004".into(),
            );
            config.insert("volatile.idmap.base".into(), "0".into());
            config.insert("volatile.last_state.idmap".into(), "[]".into());
        }
        native_instance_matches(&plan, &observed).unwrap();
        let mut foreign_volatile = observed.clone();
        foreign_volatile
            .config
            .as_mut()
            .unwrap()
            .insert("volatile.unexpected".into(), "value".into());
        assert!(native_instance_matches(&plan, &foreign_volatile).is_err());
        let mut drift = observed.clone();
        drift
            .config
            .as_mut()
            .unwrap()
            .insert("security.nesting".into(), "true".into());
        assert!(native_instance_matches(&plan, &drift).is_err());
        let mut nic = observed.clone();
        nic.expanded_devices.as_mut().unwrap().push(
            serde_json::from_value(serde_json::json!({
                "name":"eth0","device":{"type":"nic","network":"default"}
            }))
            .unwrap(),
        );
        assert!(native_instance_matches(&plan, &nic).is_err());
        let mut root = observed;
        root.expanded_devices.as_mut().unwrap()[0] = serde_json::from_value(serde_json::json!({
            "name":"root","device":{"type":"disk","path":"/","pool":"shared"}
        }))
        .unwrap();
        assert!(native_instance_matches(&plan, &root).is_err());
    }
}
