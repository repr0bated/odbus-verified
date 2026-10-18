//! A human owns a persistence subvolume, never the shared runtime's container.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SharedRootPlan {
    pub version: u8,
    pub session_id: String,
    pub expected_revision: u64,
    pub expected_revision_operation_id: String,
    pub previous_binding: SledBtrfsDevice,
    pub previous_root_repair_sha256: Option<String>,
    pub previous_seed_provision_sha256: Option<String>,
    pub binding: SledBtrfsDevice,
    pub shared_runtime_session_id: String,
    pub shared_runtime_instance_uuid: String,
    pub shared_runtime_subvolume_id: u64,
    pub shared_runtime_subvolume_uuid: String,
    pub human_subvolume_id: u64,
    pub human_subvolume_uuid: String,
}

impl SharedRootPlan {
    pub fn validate_structure(&self) -> Result<()> {
        ensure!(self.version == 2, "invalid shared human plan version");
        for value in [
            &self.session_id,
            &self.shared_runtime_session_id,
            &self.shared_runtime_instance_uuid,
            &self.binding.btrfs_uuid,
            &self.shared_runtime_subvolume_uuid,
            &self.human_subvolume_uuid,
        ] {
            ensure!(
                uuid::Uuid::parse_str(value)?.to_string() == *value,
                "noncanonical shared human identity"
            );
        }
        ensure!(
            self.session_id != self.shared_runtime_session_id
                && self.human_subvolume_id > 256
                && self.shared_runtime_subvolume_id > 256
                && self.human_subvolume_id != self.shared_runtime_subvolume_id
                && self.human_subvolume_uuid != self.shared_runtime_subvolume_uuid,
            "human persistence must be distinct from the shared runtime root"
        );
        ensure!(
            canonical_path(&self.binding.device_path)
                && self.binding.device_path.starts_with("/dev/")
                && canonical_path(&self.binding.mount_point)
                && self
                    .binding
                    .mount_point
                    .ends_with(&format!("/@identity/{}", self.session_id))
                && self.binding.attached
                && !self.binding.cozo_id.is_empty()
                && self.binding.cozo_id == self.previous_binding.cozo_id
                && self.binding != self.previous_binding,
            "invalid shared human storage binding"
        );
        ensure!(
            self.expected_revision > 0 && !self.expected_revision_operation_id.is_empty(),
            "missing shared human revision"
        );
        ensure!(
            self.previous_root_repair_sha256.is_some()
                || self.previous_seed_provision_sha256.is_some(),
            "shared human migration requires completed history"
        );
        for digest in [
            &self.previous_root_repair_sha256,
            &self.previous_seed_provision_sha256,
        ]
        .into_iter()
        .flatten()
        {
            ensure!(
                root_repair::valid_digest(digest),
                "invalid historical journal digest"
            );
        }
        Ok(())
    }

    pub fn validate_expected(
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
                    .is_some_and(|value| !value.is_empty())
                && sled.session_id == self.session_id
                && revision.session_id == self.session_id
                && revision.revision == self.expected_revision
                && revision.operation_id == self.expected_revision_operation_id
                && sled.btrfs_device.as_ref() == Some(&self.previous_binding),
            "shared human migration does not match the anchored owner revision"
        );
        Ok(())
    }

    pub fn validate_runtime(&self, runtime: &ContainerIdentitySled) -> Result<()> {
        ensure!(
            runtime.session_id == self.shared_runtime_session_id
                && runtime.principal_kind.as_deref() == Some("service")
                && runtime.is_anchored()
                && runtime.active
                && runtime
                    .instance
                    .as_ref()
                    .is_some_and(|instance| instance.name == runtime.session_id
                        && instance.instance_type == "container"
                        && instance.status == "Running"),
            "shared human runtime is not an active canonical service container"
        );
        let root = runtime
            .btrfs_device
            .as_ref()
            .context("shared runtime storage absent")?;
        ensure!(
            root.attached
                && root.device_path == self.binding.device_path
                && root.btrfs_uuid == self.binding.btrfs_uuid
                && canonical_path(&root.mount_point)
                && self.binding.mount_point
                    == format!("{}/@identity/{}", root.mount_point, self.session_id),
            "human storage is not the shared runtime's exact own child"
        );
        Ok(())
    }
}

/// A narrow native proof dependency. The durable identity/receipt/history and
/// shared-runtime policy are always checked by the caller, including fixtures.
#[async_trait::async_trait]
pub(crate) trait SharedRootProof: Send + Sync {
    async fn verify(
        &self,
        plan: &SharedRootPlan,
        runtime: &ContainerIdentitySled,
    ) -> Result<storage::StorageGuard>;
}

pub(crate) struct NativeSharedRootProof;

#[async_trait::async_trait]
impl SharedRootProof for NativeSharedRootProof {
    async fn verify(
        &self,
        plan: &SharedRootPlan,
        runtime: &ContainerIdentitySled,
    ) -> Result<storage::StorageGuard> {
        verify_native(plan, runtime).await
    }
}

pub(super) async fn verify(
    plan: &SharedRootPlan,
    store: &IdentitySledStore,
) -> Result<storage::StorageGuard> {
    verify_with_proof(plan, store, &NativeSharedRootProof).await
}

pub(super) async fn verify_with_proof(
    plan: &SharedRootPlan,
    store: &IdentitySledStore,
    proof: &dyn SharedRootProof,
) -> Result<storage::StorageGuard> {
    plan.validate_structure()?;
    let revision = load_revision(store, &plan.shared_runtime_session_id).await?;
    let runtime = revision_sled(&revision)?.context("shared runtime identity absent")?;
    plan.validate_runtime(&runtime)?;
    let guard = proof.verify(plan, &runtime).await?;
    ensure!(
        load_revision(store, &runtime.session_id).await? == revision,
        "shared runtime revision raced"
    );
    Ok(guard)
}

async fn verify_native(
    plan: &SharedRootPlan,
    runtime: &ContainerIdentitySled,
) -> Result<storage::StorageGuard> {
    let root = runtime
        .btrfs_device
        .as_ref()
        .context("shared runtime storage absent")?
        .mount_point
        .clone();
    let pinned = plan.clone();
    let guard =
        tokio::task::spawn_blocking(move || storage::verify_shared(&pinned, &root)).await??;
    let (native, etag) = IncusRootRepairBackend::local()
        .instance(&runtime.session_id)
        .await?;
    verify_native_identity(plan, runtime, &native, &etag)?;
    Ok(guard)
}

/// The digest-pinned operator plan is the native UUID authority for a
/// historical minimal service record. An existing stored UUID must agree.
pub(super) fn verify_native_identity(
    plan: &SharedRootPlan,
    runtime: &ContainerIdentitySled,
    native: &JsonValue,
    etag: &str,
) -> Result<()> {
    let instance = runtime
        .instance
        .as_ref()
        .context("shared runtime instance absent")?;
    let stored_uuid = instance
        .config
        .as_ref()
        .and_then(|config| config.get("volatile.uuid"));
    ensure!(
        !etag.is_empty()
            && native["name"] == runtime.session_id
            && native["type"] == "container"
            && native["status"] == "Running"
            && native["config"]["volatile.uuid"].as_str()
                == Some(plan.shared_runtime_instance_uuid.as_str())
            && stored_uuid.is_none_or(|stored| stored == &plan.shared_runtime_instance_uuid),
        "shared runtime native identity/power changed"
    );
    Ok(())
}
