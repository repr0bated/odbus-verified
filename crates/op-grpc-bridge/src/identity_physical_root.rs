//! Owner-authorized adoption of an already prepared physical seed/sprout root.
//! No native mutation, identity minting or historical journal removal occurs.
use super::*;
use crate::sealed_schema_reader::ManifestPinnedSchemaDocument as IdentityContract;
use anyhow::{Result, ensure};
use op_cozo_store::{
    IdentityPhysicalRootMigration, PhysicalRootMigrationReceipt, RootRepairJournal,
    SeedProvisionJournal,
};
use op_plugins::state_plugins::{identity_sled::SledBtrfsDevice, incus::IncusRootRepairBackend};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Component, Path};

#[path = "identity_physical_root/storage.rs"]
mod storage;

const METHOD: &str = "migrate_own_physical_root";
const CAPABILITY: &str = "cap.identity.sled.physical_root_migrate@v1";
const PLAN_DIR: &str = "/var/lib/op-dbus/identity-physical-migration-plans";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct MigrationInput {
    pub plan_sha256: String,
}

/// Operator intent, authenticated by private ownership and exact byte digest.
/// Credentials and genesis are never supplied; they come from the current row.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PhysicalRootPlan {
    version: u8,
    session_id: String,
    expected_revision: u64,
    expected_revision_operation_id: String,
    previous_binding: SledBtrfsDevice,
    previous_root_repair_sha256: Option<String>,
    previous_seed_provision_sha256: Option<String>,
    binding: SledBtrfsDevice,
    seed_device: String,
    seed_fsid: String,
    seed_generation: u64,
    leaf_inode: u64,
    leaf_size: u64,
    guest_subvolume_id: u64,
    native_root: String,
    pool_name: String,
    pool_source: String,
    /// Exact official Incus GET instance JSON, including UUID and hook policy.
    instance: JsonValue,
}

fn canonical_path(path: &str) -> bool {
    Path::new(path).is_absolute()
        && Path::new(path)
            .components()
            .all(|p| matches!(p, Component::RootDir | Component::Normal(_)))
}

impl PhysicalRootPlan {
    fn validate_structure(&self) -> Result<()> {
        ensure!(
            self.version == 1
                && uuid::Uuid::parse_str(&self.session_id)?.to_string() == self.session_id,
            "invalid physical-root plan identity"
        );
        ensure!(
            self.binding.cozo_id == self.previous_binding.cozo_id
                && !self.binding.cozo_id.is_empty()
                && self.binding.attached,
            "physical migration must preserve the Cozo key and verify attachment"
        );
        ensure!(
            self.binding != self.previous_binding,
            "physical migration requires a changed root"
        );
        for path in [
            &self.binding.device_path,
            &self.binding.mount_point,
            &self.seed_device,
            &self.native_root,
            &self.pool_source,
        ] {
            ensure!(canonical_path(path), "noncanonical physical-root path");
        }
        ensure!(
            uuid::Uuid::parse_str(&self.seed_fsid)?.to_string() == self.seed_fsid
                && uuid::Uuid::parse_str(&self.binding.btrfs_uuid)?.to_string()
                    == self.binding.btrfs_uuid
                && self.seed_fsid != self.binding.btrfs_uuid,
            "invalid seed/sprout identities"
        );
        ensure!(
            self.leaf_inode > 0
                && self.leaf_size >= 1024 * 1024 * 1024
                && self.seed_generation > 0
                && self.guest_subvolume_id > 256,
            "invalid physical storage provenance"
        );
        op_plugins::state_plugins::incus::validate_root_repair_pool_name(&self.pool_name)?;
        ensure!(
            self.native_root
                == format!(
                    "/var/lib/incus/storage-pools/{}/containers/{}/rootfs",
                    self.pool_name, self.session_id
                ),
            "physical native root is not canonical"
        );
        ensure!(
            self.instance["name"] == self.session_id
                && self.instance["type"] == "container"
                && self.instance["status"] == "Stopped",
            "physical plan must pin the stopped canonical container"
        );
        let config = self.instance["config"]
            .as_object()
            .context("physical native config missing")?;
        ensure!(
            config
                .get("security.privileged")
                .and_then(JsonValue::as_str)
                == Some("false")
                && config.get("security.nesting").and_then(JsonValue::as_str) == Some("false")
                && config.get("boot.autostart").and_then(JsonValue::as_str) == Some("false")
                && config
                    .get("user.opdbus.session_id")
                    .and_then(JsonValue::as_str)
                    == Some(self.session_id.as_str()),
            "physical native security or identity policy differs"
        );
        ensure!(
            config
                .get("security.syscalls.deny_default")
                .is_none_or(|value| value.as_str() == Some("true"))
                && config
                    .get("security.syscalls.blacklist_default")
                    .is_none_or(|value| value.as_str() == Some("true")),
            "physical root requires default seccomp protection"
        );
        ensure!(
            !config.contains_key("raw.seccomp")
                && !config.contains_key("security.syscalls.allow")
                && !config.contains_key("security.syscalls.whitelist"),
            "physical root has conflicting seccomp overrides"
        );
        let deny = config
            .get("security.syscalls.deny")
            .and_then(JsonValue::as_str)
            .context("physical root mutation protection missing")?;
        let actual: std::collections::BTreeSet<_> = deny
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect();
        let expected: std::collections::BTreeSet<_> = [
            "ioctl errno 1 [1,0x4008941a,SCMP_CMP_MASKED_EQ,0xffffffff]",
            "ioctl errno 1 [1,0x5000940f,SCMP_CMP_MASKED_EQ,0xffffffff]",
            "ioctl errno 1 [1,0x5000943f,SCMP_CMP_MASKED_EQ,0xffffffff]",
        ]
        .into_iter()
        .collect();
        ensure!(
            actual == expected,
            "physical root mutation protection differs"
        );
        let native_uuid = config
            .get("volatile.uuid")
            .and_then(JsonValue::as_str)
            .context("physical native UUID missing")?;
        ensure!(
            uuid::Uuid::parse_str(native_uuid)?.to_string() == native_uuid,
            "invalid physical native UUID"
        );
        ensure!(
            self.instance["profiles"] == serde_json::json!([]),
            "physical migration requires a profile-free native root"
        );
        for field in ["devices", "expanded_devices"] {
            let devices = self.instance[field]
                .as_object()
                .context("physical native devices missing")?;
            let root = devices.get("root").context("physical root disk missing")?;
            ensure!(
                devices.len() == 1
                    && root["type"] == "disk"
                    && root["path"] == "/"
                    && root["pool"] == self.pool_name,
                "physical root has unexpected native devices"
            );
        }
        Ok(())
    }

    fn validate_expected(
        &self,
        revision: &IdentityRevision,
        sled: &ContainerIdentitySled,
    ) -> Result<()> {
        self.validate_structure()?;
        ensure!(
            sled.is_anchored()
                && sled.principal_kind.as_deref() == Some("human")
                && sled
                    .sealed_id
                    .as_deref()
                    .is_some_and(|value| !value.is_empty()),
            "physical migration requires an anchored human with authored SID1"
        );
        ensure!(
            self.session_id == sled.session_id
                && revision.session_id == self.session_id
                && revision.revision == self.expected_revision
                && revision.operation_id == self.expected_revision_operation_id
                && sled.btrfs_device.as_ref() == Some(&self.previous_binding),
            "physical migration plan does not match the current revision/binding"
        );
        ensure!(
            self.previous_root_repair_sha256.is_some()
                || self.previous_seed_provision_sha256.is_some(),
            "physical migration requires completed historical provenance"
        );
        ensure!(
            self.previous_root_repair_sha256
                .as_deref()
                .is_none_or(root_repair::valid_digest)
                && self
                    .previous_seed_provision_sha256
                    .as_deref()
                    .is_none_or(root_repair::valid_digest),
            "invalid historical journal digest"
        );
        Ok(())
    }
}

fn admission(
    actor: &VerifiedIdentityActor<'_>,
    method: &str,
    contract: Option<&IdentityContract>,
) -> Result<IdentityContract> {
    crate::method_contract_admission::admit_compiled_method(
        "identity_sled",
        method,
        Some(CAPABILITY),
    )?;
    ensure!(
        crate::interceptor::load_capability_grants(actor.actor_id).contains(CAPABILITY),
        "physical migration requires the exact owner grant"
    );
    let current = strict_identity_contract()?;
    if let Some(original) = contract {
        same_identity_contract(original)?;
    }
    Ok(current)
}

async fn read_receipt(
    store: &IdentitySledStore,
    session_id: &str,
) -> Result<Option<PhysicalRootMigrationReceipt>> {
    let cozo = store.cozo.clone();
    let sid = session_id.to_string();
    tokio::task::spawn_blocking(move || cozo.read_physical_root_migration(&sid))
        .await?
        .map_err(Into::into)
}

async fn require_completed_history(store: &IdentitySledStore, session_id: &str) -> Result<()> {
    ensure!(
        !root_repair::repair_journal_incomplete(store, session_id).await?
            && !seed_provision::journal_incomplete(store, session_id).await?,
        "incomplete historical journal blocks physical root lifecycle"
    );
    Ok(())
}

fn journal_digest<T: Serialize>(journal: &Option<T>) -> Result<Option<String>> {
    journal
        .as_ref()
        .map(|journal| Ok(hex::encode(Sha256::digest(serde_json::to_vec(journal)?))))
        .transpose()
}

async fn journals(
    store: &IdentitySledStore,
    sid: &str,
) -> Result<(Option<RootRepairJournal>, Option<SeedProvisionJournal>)> {
    let cozo = store.cozo.clone();
    let sid = sid.to_string();
    tokio::task::spawn_blocking(move || {
        Ok((
            cozo.read_root_repair_journal(&sid)?,
            cozo.read_seed_provision_journal(&sid)?,
        ))
    })
    .await?
}

async fn verify_owner(
    engine: &MutationEngine,
    actor: &VerifiedIdentityActor<'_>,
    sled: &ContainerIdentitySled,
) -> Result<()> {
    ensure!(
        sled.session_id == actor.session_id
            && sled.is_anchored()
            && sled.genesis.as_deref() == Some(actor.genesis)
            && op_identity::session::derive_principal_id(&sled.wireguard_pubkey) == actor.actor_id
            && op_identity::session::derive_session_id(&sled.wireguard_pubkey) == actor.session_id
            && sled.principal_kind.as_deref() == Some("human"),
        "physical migration is restricted to the verified human owner"
    );
    root_repair::verify_current_repair_sid1(engine, sled, actor.actor_id, actor.genesis).await
}

pub(super) async fn preflight(
    engine: &MutationEngine,
    store: &IdentitySledStore,
    actor: VerifiedIdentityActor<'_>,
) -> Result<JsonValue> {
    let method = "inspect_own_physical_root_migration_preflight";
    let contract = admission(&actor, method, None)?;
    let _lifecycle = store
        .lifecycle_lock(actor.session_id)
        .await
        .lock_owned()
        .await;
    let revision = load_revision(store, actor.session_id).await?;
    let sled = revision_sled(&revision)?.context("physical migration identity absent")?;
    verify_owner(engine, &actor, &sled).await?;
    require_completed_history(store, actor.session_id).await?;
    let (root, seed) = journals(store, actor.session_id).await?;
    ensure!(
        root.is_some() || seed.is_some(),
        "physical migration requires completed historical provenance"
    );
    let binding = sled
        .btrfs_device
        .context("physical migration prior binding absent")?;
    ensure!(
        read_receipt(store, actor.session_id).await?.is_none(),
        "physical migration already committed; inspect inventory"
    );
    admission(&actor, method, Some(&contract))?;
    ensure!(
        load_revision(store, actor.session_id).await? == revision,
        "physical migration preflight raced"
    );
    Ok(
        serde_json::json!({"session_id":actor.session_id,"expected_revision":revision.revision,"expected_revision_operation_id":revision.operation_id,"previous_binding":binding,"previous_root_repair_sha256":journal_digest(&root)?,"previous_seed_provision_sha256":journal_digest(&seed)?}),
    )
}

fn verify_native(plan: &PhysicalRootPlan, current: &JsonValue, status: &str) -> Result<()> {
    ensure!(
        current["name"] == plan.session_id
            && current["type"] == "container"
            && current["status"] == status,
        "physical native identity/power differs"
    );
    for field in ["profiles", "devices", "expanded_devices"] {
        ensure!(
            current[field] == plan.instance[field],
            "physical native layout changed"
        );
    }
    for field in ["config", "expanded_config"] {
        let before = serde_json::from_value(plan.instance[field].clone())?;
        let after = serde_json::from_value(current[field].clone())?;
        root_repair::verify_native_runtime_config(&before, &after, status)?;
    }
    Ok(())
}

pub(super) struct PhysicalRootGuard {
    pub should_start: bool,
    _storage: storage::StorageGuard,
}

async fn verify_plan_native(
    plan: &PhysicalRootPlan,
    require_stopped: bool,
) -> Result<(PhysicalRootGuard, IncusInstance)> {
    let pinned = plan.clone();
    let storage = tokio::task::spawn_blocking(move || storage::verify(&pinned)).await??;
    let native = IncusRootRepairBackend::local();
    let pool = native
        .pool(&plan.pool_name)
        .await?
        .context("physical dir pool absent")?;
    ensure!(
        pool["name"] == plan.pool_name
            && pool["driver"] == "dir"
            && pool["status"] == "Created"
            && pool["config"]["source"] == plan.pool_source,
        "physical native pool provenance changed"
    );
    let (current, etag) = native.instance(&plan.session_id).await?;
    ensure!(!etag.is_empty(), "physical instance ETag absent");
    let status = current["status"]
        .as_str()
        .context("physical native status absent")?;
    ensure!(
        matches!(status, "Running" | "Stopped") && (!require_stopped || status == "Stopped"),
        "physical migration requires a stopped canonical target"
    );
    verify_native(plan, &current, status)?;
    let snapshot = native.default_project_state().await?;
    let instance = snapshot
        .instances
        .iter()
        .find(|instance| instance.name == plan.session_id)
        .context("physical native readback missing")?
        .clone();
    ensure!(
        instance.status == status
            && instance
                .config
                .as_ref()
                .and_then(|config| config.get("volatile.uuid"))
                .map(String::as_str)
                == plan.instance["config"]["volatile.uuid"].as_str(),
        "physical native readback raced"
    );
    Ok((
        PhysicalRootGuard {
            should_start: status == "Stopped",
            _storage: storage,
        },
        instance,
    ))
}

fn verify_current_receipt(
    receipt: &PhysicalRootMigrationReceipt,
    revision: &IdentityRevision,
    sled: &ContainerIdentitySled,
    plan: &PhysicalRootPlan,
) -> Result<()> {
    ensure!(
        receipt.committed_revision <= revision.revision
            && plan.session_id == sled.session_id
            && sled.btrfs_device.as_ref() == Some(&plan.binding),
        "current physical migration provenance differs from identity"
    );
    let current = sled_to_record(sled)?;
    ensure!(
        serde_json::from_str::<JsonValue>(&receipt.btrfs_device_json)?
            == serde_json::to_value(&plan.binding)?
            && op_cozo_store::physical_root_instance_binding_matches(
                &receipt.instance_json,
                &current.instance_json
            ),
        "current physical native binding differs from migration receipt"
    );
    Ok(())
}

pub(super) async fn migrate(
    engine: &MutationEngine,
    store: &IdentitySledStore,
    actor: VerifiedIdentityActor<'_>,
    input: MigrationInput,
) -> Result<JsonValue> {
    ensure!(
        !actor.session_id.is_empty()
            && !actor.genesis.is_empty()
            && root_repair::valid_digest(&input.plan_sha256),
        "physical migration requires verified own-session metadata"
    );
    let contract = admission(&actor, METHOD, None)?;
    let _lifecycle = store
        .lifecycle_lock(actor.session_id)
        .await
        .lock_owned()
        .await;
    let _observation = engine.lock_host_observation().await;
    let expected = load_revision(store, actor.session_id).await?;
    let mut sled = revision_sled(&expected)?.context("physical migration identity absent")?;
    verify_owner(engine, &actor, &sled).await?;
    if let Some(receipt) = read_receipt(store, actor.session_id).await? {
        ensure!(
            receipt.plan_sha256 == input.plan_sha256,
            "physical migration already committed a different plan"
        );
        require_completed_history(store, actor.session_id).await?;
        let plan: PhysicalRootPlan = serde_json::from_str(&receipt.provenance_json)?;
        plan.validate_structure()?;
        verify_current_receipt(&receipt, &expected, &sled, &plan)?;
        let (_guard, _) = verify_plan_native(&plan, false).await?;
        verify_owner(engine, &actor, &sled).await?;
        admission(&actor, METHOD, Some(&contract))?;
        // Recover the existing paired publication after an ambiguous commit;
        // this never replays native effects or invents a second migration.
        recover_identity_startup_with_store(engine, store).await?;
        return Ok(result(actor.session_id, &receipt, true));
    }
    let sid = actor.session_id.to_string();
    let digest = input.plan_sha256.clone();
    let bytes = tokio::task::spawn_blocking(move || {
        root_repair::trusted_plan_bytes_at(Path::new(PLAN_DIR), &sid, &digest, 0)
    })
    .await??;
    ensure!(
        bytes.len() <= 64 * 1024,
        "physical migration plan exceeds64KiB"
    );
    let plan: PhysicalRootPlan = serde_json::from_slice(&bytes)?;
    plan.validate_expected(&expected, &sled)?;
    require_completed_history(store, actor.session_id).await?;
    let (expected_root_repair, expected_seed_provision) = journals(store, actor.session_id).await?;
    ensure!(
        journal_digest(&expected_root_repair)? == plan.previous_root_repair_sha256
            && journal_digest(&expected_seed_provision)? == plan.previous_seed_provision_sha256,
        "physical migration historical journal changed"
    );
    let (_guard, instance) = verify_plan_native(&plan, true).await?;
    ensure!(
        load_revision(store, actor.session_id).await? == expected,
        "physical migration identity raced during validation"
    );
    verify_owner(engine, &actor, &sled).await?;
    admission(&actor, METHOD, Some(&contract))?;
    sled.btrfs_device = Some(plan.binding.clone());
    sled.instance = Some(instance);
    sled.mutation_index = sled
        .mutation_index
        .checked_add(1)
        .context("physical migration index overflow")?;
    let record = sled_to_record(&sled)?;
    let intent = IdentityPhysicalRootMigration {
        expected,
        record,
        expected_root_repair,
        expected_seed_provision,
        plan_sha256: input.plan_sha256.clone(),
        provenance_json: serde_json::to_string(&plan)?,
    };
    let cozo = store.cozo.clone();
    let sid = actor.session_id.to_string();
    let operation_id = format!("physical-migrate-{}", input.plan_sha256);
    let commit_id = operation_id.clone();
    let publication = engine
        .commit_identity_intent(operation_id, move |_| {
            let committed = cozo.apply_identity_intent(
                &sid,
                &commit_id,
                IdentityIntent::MigratePhysicalRoot(Box::new(intent)),
            )?;
            let snapshot = cozo.read_identity_recovery_state(MAX_EVENTS_IN_STATE)?;
            Ok((
                Some(state_value(&snapshot)?),
                DurableResult {
                    value: (),
                    replayed: committed.replayed,
                },
            ))
        })
        .await?;
    publication_value(publication)?;
    let receipt = read_receipt(store, actor.session_id)
        .await?
        .context("physical migration receipt absent after commit")?;
    Ok(result(actor.session_id, &receipt, false))
}

fn result(session_id: &str, receipt: &PhysicalRootMigrationReceipt, replayed: bool) -> JsonValue {
    serde_json::json!({"session_id":session_id,"plan_sha256":receipt.plan_sha256,"operation_id":receipt.operation_id,"committed_revision":receipt.committed_revision,"replayed":replayed,"phase":"identity-committed"})
}

pub(super) async fn guard_start(
    store: &IdentitySledStore,
    revision: &IdentityRevision,
    sled: &ContainerIdentitySled,
) -> Result<Option<PhysicalRootGuard>> {
    let Some(receipt) = read_receipt(store, &sled.session_id).await? else {
        return Ok(None);
    };
    require_completed_history(store, &sled.session_id).await?;
    let plan: PhysicalRootPlan = serde_json::from_str(&receipt.provenance_json)?;
    plan.validate_structure()?;
    verify_current_receipt(&receipt, revision, sled, &plan)?;
    let (guard, instance) = verify_plan_native(&plan, false).await?;
    ensure!(
        instance.status != "Running" || sled.active,
        "physical native power conflicts with parked policy"
    );
    Ok(Some(guard))
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
        .find(|instance| instance.name == sled.session_id)
        .context("physical identity readback missing")?
        .clone();
    ensure!(
        instance.status == status,
        "physical native power readback differs"
    );
    crate::incus_observer::publish_repair_readback_locked(engine, &snapshot).await?;
    Ok(Some(instance))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (PhysicalRootPlan, IdentityRevision, ContainerIdentitySled) {
        let sid = "414fbc56-03fb-20ab-758a-bcc7a7cbdc0e";
        let old = SledBtrfsDevice {
            device_path: "/var/lib/old.img".into(),
            mount_point: "/var/lib/old-volume".into(),
            btrfs_uuid: "00000000-0000-4000-8000-000000000001".into(),
            cozo_id: "retained-owner-key".into(),
            attached: true,
        };
        let config = serde_json::json!({"security.privileged":"false","security.nesting":"false","boot.autostart":"false","user.opdbus.session_id":sid,"volatile.uuid":"00000000-0000-4000-8000-000000000002","volatile.last_state.power":"STOPPED","security.syscalls.deny_default":"true","security.syscalls.deny":"ioctl errno 1 [1,0x4008941a,SCMP_CMP_MASKED_EQ,0xffffffff]\nioctl errno 1 [1,0x5000940f,SCMP_CMP_MASKED_EQ,0xffffffff]\nioctl errno 1 [1,0x5000943f,SCMP_CMP_MASKED_EQ,0xffffffff]"});
        let devices =
            serde_json::json!({"root":{"type":"disk","path":"/","pool":"identity-roots"}});
        let plan = PhysicalRootPlan {
            version: 1,
            session_id: sid.into(),
            expected_revision: 1598,
            expected_revision_operation_id: "old-operation".into(),
            previous_binding: old.clone(),
            previous_root_repair_sha256: None,
            previous_seed_provision_sha256: Some("a".repeat(64)),
            binding: SledBtrfsDevice {
                device_path: "/var/lib/private/leaf.img".into(),
                mount_point: "/var/lib/private/volume".into(),
                btrfs_uuid: "00000000-0000-4000-8000-000000000003".into(),
                cozo_id: old.cozo_id.clone(),
                attached: true,
            },
            seed_device: "/dev/physical-seed".into(),
            seed_fsid: "00000000-0000-4000-8000-000000000004".into(),
            seed_generation: 9,
            leaf_inode: 123,
            leaf_size: 1024 * 1024 * 1024,
            guest_subvolume_id: 300,
            native_root: format!(
                "/var/lib/incus/storage-pools/identity-roots/containers/{sid}/rootfs"
            ),
            pool_name: "identity-roots".into(),
            pool_source: "/var/lib/incus/storage-pools/identity-roots".into(),
            instance: serde_json::json!({"name":sid,"type":"container","status":"Stopped","profiles":[],"config":config,"expanded_config":config,"devices":devices,"expanded_devices":devices}),
        };
        let sled:ContainerIdentitySled = serde_json::from_value(serde_json::json!({"session_id":sid,"wireguard_pubkey":"existing-key","interface":"unchanged","mutation_index":20,"genesis":"existing-genesis","arrival_timestamp":5,"chain_head_at_arrival":"existing-chain","sealed_id":"existing-credential","principal_kind":"human","btrfs_device":old,"session_started_at":1,"last_seen_at":2,"active":true})).unwrap();
        let revision = IdentityRevision {
            session_id: sid.into(),
            revision: 1598,
            operation_id: "old-operation".into(),
            record: Some(sled_to_record(&sled).unwrap()),
            genesis: Some(sled_to_genesis(&sled)),
        };
        (plan, revision, sled)
    }

    #[test]
    fn physical_migration_plan_requires_exact_revision_owner_storage_and_protection() {
        let (plan, revision, sled) = fixture();
        plan.validate_expected(&revision, &sled).unwrap();
        let mut stale = revision.clone();
        stale.revision += 1;
        assert!(plan.validate_expected(&stale, &sled).is_err());
        stale = revision.clone();
        stale.operation_id = "different-operation".into();
        assert!(plan.validate_expected(&stale, &sled).is_err());
        let mut wrong = plan.clone();
        wrong.binding.cozo_id = "another-owner".into();
        assert!(wrong.validate_expected(&revision, &sled).is_err());
        wrong = plan.clone();
        wrong.previous_seed_provision_sha256 = None;
        assert!(wrong.validate_expected(&revision, &sled).is_err());
        wrong = plan.clone();
        wrong.instance["config"]["security.syscalls.deny"] =
            serde_json::json!("ioctl errno 1 [1,0x4008941a,SCMP_CMP_EQ]");
        assert!(wrong.validate_structure().is_err());
        wrong = plan.clone();
        wrong.instance["config"]["raw.seccomp"] = serde_json::json!("override");
        assert!(wrong.validate_structure().is_err());
        wrong = plan.clone();
        wrong.instance["config"]
            .as_object_mut()
            .unwrap()
            .remove("security.syscalls.deny_default");
        wrong.validate_structure().unwrap();
        wrong.instance["config"]["security.syscalls.deny_default"] = serde_json::json!("false");
        assert!(wrong.validate_structure().is_err());
        wrong = plan.clone();
        wrong.instance["devices"]["eth0"] = serde_json::json!({"type":"nic"});
        assert!(wrong.validate_structure().is_err());
        wrong = plan.clone();
        wrong.native_root = "/var/lib/another-root".into();
        assert!(wrong.validate_structure().is_err());
        let mut unanchored = sled.clone();
        unanchored.arrival_timestamp = 0;
        assert!(plan.validate_expected(&revision, &unanchored).is_err());
        unanchored = sled.clone();
        unanchored.principal_kind = Some("service".into());
        assert!(plan.validate_expected(&revision, &unanchored).is_err());
    }

    #[test]
    fn physical_native_provenance_allows_power_readback_but_rejects_uuid_and_layout_drift() {
        let (plan, _, _) = fixture();
        verify_native(&plan, &plan.instance, "Stopped").unwrap();
        let mut running = plan.instance.clone();
        running["status"] = serde_json::json!("Running");
        for field in ["config", "expanded_config"] {
            running[field]["volatile.last_state.power"] = serde_json::json!("RUNNING");
        }
        verify_native(&plan, &running, "Running").unwrap();
        running["config"]["volatile.uuid"] =
            serde_json::json!("00000000-0000-4000-8000-000000000009");
        assert!(verify_native(&plan, &running, "Running").is_err());
        let mut drift = plan.instance.clone();
        drift["expanded_devices"]["root"]["pool"] = serde_json::json!("other");
        assert!(verify_native(&plan, &drift, "Stopped").is_err());
    }

    #[test]
    fn physical_migration_history_digest_binds_all_completed_journal_metadata() {
        let mut seed = SeedProvisionJournal {
            version: 1,
            plan_sha256: "a".repeat(64),
            operation_id: "original-owner-operation".into(),
            phase: "identity-committed".into(),
            expected_revision: 1,
            expected_revision_operation_id: "prior".into(),
            committed_revision: 2,
            committed_genesis: "private-never-returned".into(),
        };
        let first = journal_digest(&Some(seed.clone())).unwrap();
        seed.committed_revision += 1;
        assert_ne!(first, journal_digest(&Some(seed)).unwrap());
        assert_eq!(journal_digest::<SeedProvisionJournal>(&None).unwrap(), None);
        let encoded = serde_json::to_string(&first).unwrap();
        assert!(!encoded.contains("private-never-returned"));
    }

    #[tokio::test]
    async fn physical_migration_dispatch_refuses_unbound_or_caller_selected_targets() {
        let (engine, _shm) = super::super::tests::sled_engine();
        for method in [METHOD, "inspect_own_physical_root_migration_preflight"] {
            assert!(
                dispatch_identity_sled_method(&engine, method, &serde_json::json!({}))
                    .await
                    .is_err()
            );
            assert!(
                dispatch_identity_sled_method_for_verified_session(
                    &engine,
                    method,
                    &serde_json::json!({}),
                    "actor",
                    None,
                    None
                )
                .await
                .is_err()
            );
        }
        assert!(
            serde_json::from_value::<MigrationInput>(
                serde_json::json!({"plan_sha256":"a".repeat(64),"session_id":"foreign-target"})
            )
            .is_err()
        );
    }
}
