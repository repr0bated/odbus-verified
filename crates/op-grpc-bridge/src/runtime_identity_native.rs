//! Exact native-pool adoption; no mounts, root copies, host execution or local mint.
use super::*;
use op_plugins::state_plugins::identity_sled::ContainerCreationMode as Mode;
use op_plugins::state_plugins::incus::IncusRootRepairBackend;
use serde::{Deserialize, Serialize};
use std::path::Path;
use op_cozo_store::IdentityCleanProvision;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SubvolumePin {
    pub id: u64,
    pub uuid: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NativePlan {
    pub version: u8,
    pub creation_mode: Mode,
    pub parent_principal_id: Option<String>,
    pub caller_principal_id: String,
    pub caller_session_id: String,
    pub caller_genesis: String,
    pub session_id: String,
    pub wireguard_pubkey: String,
    pub expected_revision: u64,
    pub expected_revision_operation_id: String,
    pub creation_operation_id: Option<String>,
    pub instance: JsonValue,
    pub instance_etag: String,
    pub pool_fsid: String,
    pub root: SubvolumePin,
    pub sled: SubvolumePin,
    pub golden_subvolume_uuid: String,
    pub golden_seed_name: String,
    #[serde(default)]
    pub golden_subvolume_id: Option<u64>,
    #[serde(default)]
    pub golden_metadata_sha256: Option<String>,
}

fn valid_seed_name(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && value != "." && value != ".."
        && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'@'))
}

fn validate_target(mode: Mode, key: &str, principal: &str, session: &str,
    parent: Option<&str>, human: bool) -> anyhow::Result<()> {
    canonical_key(key)?;
    ensure!(human, "native provisioning requires a verified human owner");
    let target = op_identity::session::derive_session_id(key);
    match mode {
        Mode::AttachExistingHuman => ensure!(target == session
            && op_identity::session::derive_principal_id(key) == principal && parent.is_none(),
            "human attachment is restricted to the same principal and session"),
        Mode::CreateSubsidiary => ensure!(target != session
            && op_identity::session::derive_principal_id(key) != principal
            && parent == Some(principal), "subsidiary requires its exact distinct human parent"),
        Mode::ReplaceChatbot => anyhow::bail!("native attachment does not replace the chatbot"),
    }
    Ok(())
}

pub(super) async fn admit_target(engine: &NativeOwner, store: &IdentitySledStore,
    key: &str, mode: Mode, parent: Option<&str>) -> anyhow::Result<()> {
    let owner = revision_sled(&load_revision(store, &engine.caller.session_id).await?)?
        .context("native provisioning human is absent; import its Decoy-issued sled first")?;
    engine.verify_own(&owner).await?;
    validate_target(mode, key, &engine.caller.principal_id, &engine.caller.session_id,
        parent, owner.principal_kind.as_deref() == Some("human"))?;
    seed_provision::target_key_is_registered(key).await
}

pub(super) fn native_name(instance: &JsonValue) -> anyhow::Result<&str> {
    let name = instance["name"].as_str().context("native name absent")?;
    ensure!(!name.is_empty() && name.len() <= 63 && name.as_bytes()[0].is_ascii_alphanumeric()
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')),
        "unsafe native name");
    Ok(name)
}

pub(super) fn validate_instance(raw: &JsonValue, sid: &str, key: &str, parent: Option<&str>) -> anyhow::Result<()> {
    native_name(raw)?;
    ensure!(raw["type"] == "container" && matches!(raw["status"].as_str(), Some("Running" | "Stopped")),
        "native attachment requires a running or stopped container");
    ensure!(raw["profiles"] == serde_json::json!([]), "native identity must be profile-free");
    for field in ["config", "expanded_config"] {
        let config = raw[field].as_object().context("native configuration absent")?;
        let raw_lxc = config.get("raw.lxc").and_then(JsonValue::as_str).context("native init/hostname pin absent")?;
        let hostname = format!("lxc.uts.name = {sid}");
        ensure!(raw_lxc.lines().map(str::trim).all(|line|
            line == hostname || line == "lxc.init.cmd = /sbin/runit-init"
                || line == "lxc.mount.auto = cgroup2:force")
            && raw_lxc.lines().map(str::trim).any(|line| line == hostname)
            && config.keys().all(|key| !key.starts_with("raw.") || key == "raw.lxc")
            && !config.contains_key("security.syscalls.allow")
            && !config.contains_key("security.syscalls.whitelist")
            && config.get("security.syscalls.deny_default").and_then(JsonValue::as_str) != Some("false"),
            "native raw configuration or default syscall policy differs");
        ensure!(config.get("user.opdbus.session_id").and_then(JsonValue::as_str) == Some(sid)
            && config.get("user.opdbus.wireguard_public_key").and_then(JsonValue::as_str) == Some(key)
            && config.get("security.privileged").and_then(JsonValue::as_str) == Some("false")
            && config.get("security.nesting").and_then(JsonValue::as_str) == Some("false")
            && config.get("security.idmap.isolated").and_then(JsonValue::as_str) == Some("true")
            && !config.contains_key("raw.idmap") && !config.contains_key("raw.apparmor")
            && !config.contains_key("raw.seccomp")
            && config.get("user.opdbus.parent_principal_id").and_then(JsonValue::as_str) == parent,
            "native identity, parent or isolation policy differs");
    }
    let uuid = raw["config"]["volatile.uuid"].as_str().context("native UUID absent")?;
    ensure!(uuid::Uuid::parse_str(uuid)?.to_string() == uuid, "noncanonical native UUID");
    for field in ["devices", "expanded_devices"] {
        let devices = raw[field].as_object().context("native devices absent")?;
        let root = devices.get("root").context("native root absent")?;
        ensure!(root["type"] == "disk" && root["pool"] == "3tched-pool" && root["path"] == "/"
            && root.get("source").is_none(), "native root is not its own managed pool volume");
        let sled = devices.get("identity-sled").or_else(|| devices.get("sled")).context("native sled device absent")?;
        ensure!(sled["type"] == "disk" && sled["pool"] == "3tched-pool"
            && sled["source"] == format!("identity-{sid}") && sled["path"] == "/agent/user"
            && sled["readonly"] == "true", "native sled is not the session's own readonly volume");
        for (name, device) in devices {
            if matches!(name.as_str(), "root" | "identity-sled" | "sled") { continue; }
            let allowed = match device["type"].as_str() {
                Some("unix-char") => device["source"] == "/dev/fuse" && device["path"] == "/dev/fuse",
                Some("disk") => {
                    let pair = (device["source"].as_str(), device["path"].as_str());
                    (device["readonly"] == "true" && matches!(pair,
                        (Some("/run/opdbus/lungs"), Some("/run/opdbus/lungs" | "/run/mounts/opdbus/lungs" | "/opt/run-mounts/opdbus/lungs"))
                        | (Some("/var/lib/opdbus/plugin-blobs"), Some("/var/lib/opdbus/plugin-blobs"))
                        | (Some("/dev/shm/opdbus/plugin-blobs"), Some("/dev/shm/opdbus/plugin-blobs"))
                        | (Some("/etc/op-dbus/decoy-trust.json"), Some("/etc/op-dbus/decoy-trust.json"))))
                        || pair == (Some("tmpfs:"), Some("/run"))
                        || (device["shift"] == "true" && (device["path"] == "/run/mounts/opdbus" || device["path"] == "/opt/run-mounts/opdbus")
                            && device["source"] == format!("/run/opdbus/ct/{}", native_name(raw)?))
                }
                _ => false,
            };
            ensure!(allowed, "native device exposes an unapproved host resource");
        }
    }
    Ok(())
}

impl NativePlan {
    pub(super) fn validate(&self) -> anyhow::Result<()> {
        ensure!(self.version == 5 && self.session_id == op_identity::session::derive_session_id(&self.wireguard_pubkey)
            && !self.instance_etag.is_empty(), "invalid native plan identity/version");
        canonical_key(&self.wireguard_pubkey)?;
        for value in [&self.pool_fsid, &self.root.uuid, &self.sled.uuid, &self.golden_subvolume_uuid] {
            ensure!(uuid::Uuid::parse_str(value)?.to_string() == *value, "noncanonical subvolume pin");
        }
        ensure!(self.root.id >= 256 && self.sled.id >= 256 && self.root.uuid != self.sled.uuid,
            "native root/sled pins are invalid");
        validate_instance(&self.instance, &self.session_id, &self.wireguard_pubkey,
            self.parent_principal_id.as_deref())?;
        match self.creation_mode {
            Mode::AttachExistingHuman => ensure!(self.creation_operation_id.is_none()
                && self.parent_principal_id.is_none(), "human adoption cannot invent a native creation intent"),
            Mode::CreateSubsidiary => ensure!(self.creation_operation_id.is_some()
                && self.parent_principal_id.as_deref() == Some(self.caller_principal_id.as_str())
                && valid_seed_name(&self.golden_seed_name)
                && self.golden_subvolume_id.is_some_and(|id| id >= 256)
                && self.golden_metadata_sha256.as_deref().is_some_and(root_repair::valid_digest),
                "subsidiary requires durable creation and approved Golden seed pins"),
            Mode::ReplaceChatbot => anyhow::bail!("native plan cannot replace chatbot identity"),
        }
        Ok(())
    }
}

pub(super) async fn proof(plan: &NativePlan, exact: bool) -> anyhow::Result<(clean_provision::storage::StorageGuard, IncusInstance)> {
    plan.validate()?;
    let native = IncusRootRepairBackend::local();
    let (raw, etag) = native.instance(native_name(&plan.instance)?).await?;
    if exact {
        ensure!(raw == plan.instance && etag == plan.instance_etag, "native definition or ETag changed");
    } else {
        ensure!(clean_provision::binding_matches(
            &serde_json::to_string(&IncusPlugin::instance_from_native_metadata(plan.instance.clone())?)?,
            &serde_json::to_string(&IncusPlugin::instance_from_native_metadata(raw.clone())?)?),
            "native authority changed after attachment");
    }
    validate_instance(&raw, &plan.session_id, &plan.wireguard_pubkey, plan.parent_principal_id.as_deref())?;
    let pool = native.pool("3tched-pool").await?.context("native pool absent")?;
    ensure!(pool["driver"] == "btrfs" && pool["status"] == "Created", "native pool is not Btrfs");
    if plan.creation_mode == Mode::CreateSubsidiary {
        ensure!(raw["config"]["user.opdbus.golden_seed_name"] == plan.golden_seed_name
            && raw["config"]["user.opdbus.golden_seed_uuid"] == plan.golden_subvolume_uuid,
            "native Golden seed provenance differs");
    }
    let parsed = IncusPlugin::instance_from_native_metadata(raw)?;
    let owned = plan.clone();
    let guard = tokio::task::spawn_blocking(move || clean_provision::storage::verify_native(&owned)).await??;
    Ok((guard, parsed))
}

pub(super) async fn preflight(engine: &NativeOwner, store: &IdentitySledStore, key: &str,
    mode: Mode, parent: Option<&str>) -> anyhow::Result<JsonValue> {
    admit_target(engine, store, key, mode, parent).await?;
    let sid = op_identity::session::derive_session_id(key);
    let expected = load_revision(store, &sid).await?;
    let names = IncusRootRepairBackend::local().instances_with_session(&sid).await?;
    ensure!(names.len() <= 1, "multiple native containers carry this session");
    let (raw, etag) = if let Some(name) = names.first() {
        let (raw, etag) = IncusRootRepairBackend::local().instance(name).await?;
        validate_instance(&raw, &sid, key, parent)?;
        (raw, etag)
    } else { (JsonValue::Null, String::new()) };
    let previous_instance = expected.record.as_ref().filter(|row| !row.instance_json.is_empty())
        .map(|row| serde_json::from_str::<IncusInstance>(&row.instance_json)).transpose()?;
    engine.revalidate().await?;
    Ok(serde_json::json!({"session_id":sid,"wireguard_pubkey":key,
        "expected_revision":expected.revision,"expected_revision_operation_id":expected.operation_id,
        "previous_instance":previous_instance,
        "previous_instance_etag":etag,"previous_native_instance":raw}))
}

pub(super) async fn prepare(engine: &NativeOwner, store: &IdentitySledStore,
    actor: VerifiedIdentityActor<'_>, args: &JsonValue, mode: Mode, parent: Option<&str>) -> anyhow::Result<JsonValue> {
    let key = arg_str(args, "wireguard_pubkey");
    let sid = op_identity::session::derive_session_id(key);
    admit_target(engine, store, key, mode, parent).await?;
    engine.retain(store.lifecycle_lock(&sid).await.lock_owned().await);
    let seal = seed_provision::capture_method_seal("prepare_container")?;
    seed_provision::admit(engine, store, actor.actor_id, actor.session_id, actor.genesis, &seal).await?;
    let operation = arg_str(args, "operation_id");
    ensure!(uuid::Uuid::parse_str(operation)?.to_string() == operation, "prepare operation UUID is noncanonical");
    if mode == Mode::AttachExistingHuman {
        let result = preflight(engine, store, key, mode, parent).await?;
        let raw = &result["previous_native_instance"];
        ensure!(!raw.is_null(), "human attachment requires its existing native container");
        let revision = load_revision(store, &sid).await?;
        if let Some(old) = revision_sled(&revision)?.and_then(|sled| sled.instance) {
            ensure!(old.name == native_name(raw)?
                && old.config.as_ref().and_then(|config| config.get("volatile.uuid")).map(String::as_str)
                    == raw["config"]["volatile.uuid"].as_str(),
                "existing human native UUID cannot be replaced");
        }
        return Ok(serde_json::json!({"session_id":sid,"name":raw["name"],"native_uuid":raw["config"]["volatile.uuid"],
            "operation_id":operation,"phase":"existing-native","replayed":true}));
    }
    let cozo = store.cozo.clone();
    let records = tokio::task::spawn_blocking(move || cozo.native_creation_records()).await??;
    let body = if let Some(record) = records.iter().find(|record| record.request.operation_id == operation) {
        ensure!(record.request.owner_kind == "identity" && record.request.logical_target == sid
            && record.request.actor_id == actor.actor_id && record.request.caller_session == actor.session_id
            && record.request.caller_genesis == actor.genesis, "native creation belongs to a different owner");
        let body: JsonValue = serde_json::from_str(&record.request.request_json)?;
        ensure!(body["config"]["user.opdbus.creation_mode"] == "create_subsidiary"
            && body["config"]["user.opdbus.parent_principal_id"] == actor.actor_id,
            "native creation mode or parent differs");
        body
    } else {
        let revision = load_revision(store, &sid).await?;
        ensure!(revision.revision == 0 && revision.record.is_none() && revision.genesis.is_none()
            && revision.operation_id.is_empty(), "existing or revoked subsidiary cannot be recreated");
        ensure!(IncusRootRepairBackend::local().instances_with_session(&sid).await?.is_empty(),
            "existing native subsidiary requires reconciliation, not a new POST");
        op_plugins::state_plugins::incus::native_identity_creation_body(&sid, key,
            parent.context("subsidiary parent absent")?).await?
    };
    preflight_native_identity_effect().await?;
    let (record, snapshot, replayed) = crate::runtime_creation::create_identity_container(
        &engine.runtime, &engine.caller, operation, &sid, body, None).await?;
    store.mark_effect();
    engine.observe_incus(&snapshot).await?;
    Ok(serde_json::json!({"session_id":sid,"name":record.name,"native_uuid":record.native_uuid,
        "operation_id":record.request.operation_id,"phase":record.phase,"replayed":replayed}))
}

pub(super) async fn provision(engine: &NativeOwner, store: &IdentitySledStore,
    actor: VerifiedIdentityActor<'_>, input: seed_provision::ProvisionInput,
    seal: seed_provision::ProvisionSealPin, inspect: bool) -> anyhow::Result<JsonValue> {
    admit_target(engine, store, &input.wireguard_pubkey, input.creation_mode, input.parent_principal_id.as_deref()).await?;
    let sid = op_identity::session::derive_session_id(&input.wireguard_pubkey);
    engine.retain(store.lifecycle_lock(&sid).await.lock_owned().await);
    seed_provision::admit(engine, store, actor.actor_id, actor.session_id, actor.genesis, &seal).await?;
    if inspect {
        return preflight(engine, store, &input.wireguard_pubkey, input.creation_mode, input.parent_principal_id.as_deref()).await;
    }
    ensure!(root_repair::valid_digest(&input.plan_sha256), "invalid native plan digest");
    let read_sid = sid.clone();
    let digest = input.plan_sha256.clone();
    let bytes = tokio::task::spawn_blocking(move || root_repair::trusted_plan_bytes_at(
        Path::new("/var/lib/op-dbus/identity-provision-plans"), &read_sid, &digest, 0)).await??;
    let plan: NativePlan = serde_json::from_slice(&bytes)?;
    plan.validate()?;
    ensure!(plan.creation_mode == input.creation_mode && plan.parent_principal_id == input.parent_principal_id
        && plan.session_id == sid && plan.wireguard_pubkey == input.wireguard_pubkey
        && plan.caller_principal_id == actor.actor_id && plan.caller_session_id == actor.session_id
        && plan.caller_genesis == actor.genesis, "native plan belongs to another caller or target");
    let cozo = store.cozo.clone();
    let receipt_sid = sid.clone();
    let receipt = tokio::task::spawn_blocking(move || cozo.read_clean_provision(&receipt_sid)).await??;
    let expected = load_revision(store, &sid).await?;
    if let Some(receipt) = receipt.filter(|receipt| receipt.plan_sha256 == input.plan_sha256) {
        ensure!(receipt.provenance_json == serde_json::to_string(&plan)?, "native replay plan changed");
        let old = revision_sled(&expected)?.context("native replay identity absent")?;
        engine.retain(guard_start(store, &expected, &old).await?.context("native receipt absent")?);
        seed_provision::admit(engine, store, actor.actor_id, actor.session_id, actor.genesis, &seal).await?;
        require_published_identity(store).await?;
        return Ok(serde_json::json!({"identity":old,"plan_sha256":input.plan_sha256,
            "operation_id":receipt.operation_id,"committed_revision":receipt.committed_revision,"replayed":true}));
    }
    ensure!(expected.revision == plan.expected_revision && expected.operation_id == plan.expected_revision_operation_id,
        "native attachment revision changed");
    if input.creation_mode == Mode::CreateSubsidiary {
        ensure!(std::env::var("OP_IDENTITY_GOLDEN_SUBVOLUME_UUID").ok().as_deref()
            == Some(plan.golden_subvolume_uuid.as_str())
            && std::env::var("OP_IDENTITY_GOLDEN_SEED_NAME").ok().as_deref()
                == Some(plan.golden_seed_name.as_str()), "native plan is not the approved Golden source");
    }
    let (guard, instance) = proof(&plan, true).await?;
    ensure!(guard.native_sled_text("session_id")?.trim() == sid, "sled session differs");
    let trust = std::fs::read(std::env::var("OP_DECOY_TRUST_STORE")
        .unwrap_or_else(|_| crate::oracle_assertion::DEFAULT_DECOY_TRUST_STORE.into()))?;
    let claims = verify_issued_claims(&guard.native_sled_text("issued_identity.iid1")?, &trust, &sid, &input.wireguard_pubkey, 0)?;
    let inline = claims.to_inline_ref().map_err(|error| anyhow::anyhow!("{error}"))?;
    ensure!(guard.native_sled_text("sealed_id")?.trim() == inline, "sled SID1 differs from Decoy-issued identity");
    let sled = match input.creation_mode {
        Mode::AttachExistingHuman => {
            let mut old = revision_sled(&expected)?.context("import the existing Decoy-issued human sled before attachment")?;
            ensure!(old.principal_kind.as_deref() == Some("human") && old.sealed_id.as_deref() == Some(inline.as_str()),
                "human attachment cannot remint or change classification");
            if let Some(bound) = &old.instance {
                ensure!(bound.name == instance.name && bound.config.as_ref().and_then(|c| c.get("volatile.uuid"))
                    == instance.config.as_ref().and_then(|c| c.get("volatile.uuid")),
                    "human is already bound to a different native UUID");
            }
            old.active = instance.status == "Running";
            old.instance = Some(instance);
            old
        }
        Mode::CreateSubsidiary => {
            ensure!(expected.revision == 0 && expected.record.is_none() && expected.genesis.is_none()
                && expected.operation_id.is_empty(), "subsidiary identity already exists or was revoked");
            let mut fresh: ContainerIdentitySled = serde_json::from_value(serde_json::json!({
                "session_id":sid,"wireguard_pubkey":input.wireguard_pubkey,"principal_kind":"service",
                "session_started_at":claims.arrival_timestamp,"last_seen_at":claims.arrival_timestamp}))?;
            fresh.schema_version = RECORD_FORMAT;
            fresh.org_relation = Some(op_plugins::state_plugins::identity_sled::OrgRelation {
                kind: op_plugins::state_plugins::identity_sled::OrgRelationKind::Subsidiary,
                name: instance.name.clone(),
                parent: input.parent_principal_id.clone(),
            });
            fresh.active = instance.status == "Running";
            fresh.instance = Some(instance);
            apply_genesis_stamp(&mut fresh, &GenesisStamp { sealed_id: inline, claims })?;
            fresh
        }
        Mode::ReplaceChatbot => unreachable!("mode admitted above"),
    };
    validate_sled(&sled)?;
    engine.retain(guard);
    let (final_guard, _) = proof(&plan, true).await?;
    engine.retain(final_guard);
    admit_target(engine, store, &input.wireguard_pubkey, input.creation_mode, input.parent_principal_id.as_deref()).await?;
    seed_provision::admit(engine, store, actor.actor_id, actor.session_id, actor.genesis, &seal).await?;
    let operation = format!("native-attachment-{}", input.plan_sha256);
    let commit_operation = operation.clone();
    let record = sled_to_record(&sled)?;
    let genesis = if input.creation_mode == Mode::AttachExistingHuman {
        expected.genesis.clone().context("human genesis absent")?
    } else { sled_to_genesis(&sled) };
    let provenance_json = serde_json::to_string(&plan)?;
    let cozo = store.cozo.clone();
    let plan_sha256 = input.plan_sha256.clone();
    let value = sled;
    let publication = engine.commit_identity_intent(operation.clone(), move |_| {
        let result = cozo.apply_identity_intent(&sid, &commit_operation,
            IdentityIntent::AttachNativeContainer(Box::new(IdentityCleanProvision {
                expected, record, genesis, plan_sha256, provenance_json,
            })))?;
        let state = cozo.read_identity_recovery_state(MAX_EVENTS_IN_STATE)?;
        Ok((Some(state_value(&state)?), DurableResult { value: (value, result.current.revision), replayed: result.replayed }))
    }).await?;
    let (identity, committed_revision) = publication_value(publication)?;
    Ok(serde_json::json!({"identity":identity,"plan_sha256":input.plan_sha256,"operation_id":operation,
        "committed_revision":committed_revision,"replayed":false}))
}

pub(super) async fn guard_start(store: &IdentitySledStore, revision: &IdentityRevision,
    sled: &ContainerIdentitySled) -> anyhow::Result<Option<clean_provision::CleanRootGuard>> {
    let cozo = store.cozo.clone();
    let sid = sled.session_id.clone();
    let receipt = tokio::task::spawn_blocking(move || cozo.read_clean_provision(&sid)).await??;
    let Some(receipt) = receipt else { return Ok(None) };
    let value: JsonValue = serde_json::from_str(&receipt.provenance_json)?;
    if value["version"] != 5 { return Ok(None); }
    let plan: NativePlan = serde_json::from_value(value)?;
    ensure!(revision.revision >= receipt.committed_revision && sled.session_id == plan.session_id
        && sled.wireguard_pubkey == plan.wireguard_pubkey
        && sled.instance.as_ref().is_some_and(|instance| clean_provision::binding_matches(
            &receipt.instance_json, &serde_json::to_string(instance).unwrap_or_default())),
        "native attachment receipt differs from identity");
    seed_provision::target_key_is_registered(&sled.wireguard_pubkey).await?;
    let (guard, instance) = proof(&plan, false).await?;
    let should_start = root_repair::decide_seeded_native_start(sled.active, &instance.status)?;
    Ok(Some(clean_provision::CleanRootGuard { should_start, _storage: guard }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_attachment_requires_exact_human_caller_and_no_parent() {
        let key = base64::engine::general_purpose::STANDARD.encode([23u8; 32]);
        let session = op_identity::session::derive_session_id(&key);
        let principal = op_identity::session::derive_principal_id(&key);
        assert!(validate_target(Mode::AttachExistingHuman, &key, &principal, &session, None, true).is_ok());
        assert!(validate_target(Mode::AttachExistingHuman, &key, "foreign", &session, None, true).is_err());
        assert!(validate_target(Mode::AttachExistingHuman, &key, &principal, &session, Some(&principal), true).is_err());
        assert!(validate_target(Mode::AttachExistingHuman, &key, &principal, &session, None, false).is_err());
    }

    #[test]
    fn subsidiary_is_distinct_and_bound_to_exact_human_parent() {
        let key = base64::engine::general_purpose::STANDARD.encode([24u8; 32]);
        assert!(validate_target(Mode::CreateSubsidiary, &key, "parent", "parent-session", Some("parent"), true).is_ok());
        assert!(validate_target(Mode::CreateSubsidiary, &key, "parent", "parent-session", Some("other"), true).is_err());
        assert!(validate_target(Mode::CreateSubsidiary, &key, "parent", "parent-session", Some("parent"), false).is_err());
    }
}

#[cfg(test)]
mod native_policy_tests {
    use super::*;

    fn trusted_human() -> JsonValue {
        let sid = "fb86888e-d6ab-d4f7-6278-9fe116c46b1c";
        let config = serde_json::json!({
            "user.opdbus.session_id":sid,
            "user.opdbus.wireguard_public_key":"lnF9hBrcX5rpHoX87VGwlIbkOspN8cWER+vtohkW0lg=",
            "volatile.uuid":"cc84a2d6-acc8-4327-b80b-9c243d6eda6e",
            "security.privileged":"false","security.nesting":"false","security.idmap.isolated":"true",
            "raw.lxc":format!("lxc.uts.name = {sid}\nlxc.mount.auto = cgroup2:force\nlxc.init.cmd = /sbin/runit-init")
        });
        let devices = serde_json::json!({"root":{"type":"disk","pool":"3tched-pool","path":"/"},
            "sled":{"type":"disk","pool":"3tched-pool","source":format!("identity-{sid}"),"path":"/agent/user","readonly":"true"}});
        serde_json::json!({"name":"Jeremy","type":"container","status":"Running","profiles":[],
            "config":config,"expanded_config":config,"devices":devices,"expanded_devices":devices})
    }

    #[test]
    fn existing_idmapped_human_is_accepted_without_fabricating_syscall_policy() {
        let raw = trusted_human();
        assert!(validate_instance(&raw, raw["config"]["user.opdbus.session_id"].as_str().unwrap(),
            raw["config"]["user.opdbus.wireguard_public_key"].as_str().unwrap(), None).is_ok());
    }

    #[test]
    fn identity_labels_do_not_authorize_profiles_hooks_or_other_host_mounts() {
        let raw = trusted_human();
        let sid = raw["config"]["user.opdbus.session_id"].as_str().unwrap();
        let key = raw["config"]["user.opdbus.wireguard_public_key"].as_str().unwrap();
        let mut injected = raw.clone();
        injected["profiles"] = serde_json::json!(["default"]);
        assert!(validate_instance(&injected, sid, key, None).is_err());
        injected = raw.clone();
        injected["config"]["raw.lxc"] = format!("lxc.uts.name = {sid}\nlxc.hook.pre-start = /host/script").into();
        assert!(validate_instance(&injected, sid, key, None).is_err());
        injected = raw.clone();
        injected["expanded_devices"]["host"] = serde_json::json!({"type":"disk","source":"/","path":"/host","readonly":"true"});
        assert!(validate_instance(&injected, sid, key, None).is_err());
    }
}
