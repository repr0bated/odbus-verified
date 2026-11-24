//! Explicit own-native-root cutover. Credentials, sled and the entire old root survive.
use super::*;
use op_cozo_store::NativeRootUpgradeJournal;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpgradePlan {
    version: u8,
    session_id: String,
    caller_principal_id: String,
    caller_session_id: String,
    caller_genesis: String,
    schema_hash: String,
    current: native_attachment::NativePlan,
    native_uuid: String,
    new_seed_name: String,
    new_seed_uuid: String,
    new_seed_id: u64,
    new_seed_metadata_sha256: String,
    isolated_host_uid: u32,
    idmap_current: String,
    idmap_next: String,
    idmap_last_state: String,
}

fn validate_idmap(text: &str, isolated_host_uid: u32) -> anyhow::Result<()> {
    let mappings: JsonValue = serde_json::from_str(text)?;
    let entries = mappings.as_array().context("native idmap is not an array")?;
    ensure!(isolated_host_uid >= 65536 && entries.len() == 2 && entries.iter().all(|entry|
        entry["Hostid"].as_u64() == Some(u64::from(isolated_host_uid))
            && entry["Nsid"] == 0 && entry["Maprange"] == 65536)
        && entries.iter().filter(|entry| entry["Isuid"] == true && entry["Isgid"] == false).count() == 1
        && entries.iter().filter(|entry| entry["Isgid"] == true && entry["Isuid"] == false).count() == 1,
        "native UID/GID maps do not describe the isolated guest root");
    Ok(())
}

impl UpgradePlan {
    fn validate(&self) -> anyhow::Result<()> {
        self.current.validate()?;
        ensure!(self.version == 1 && self.session_id == self.caller_session_id
            && self.session_id == self.current.session_id
            && self.caller_principal_id == self.current.caller_principal_id
            && self.caller_genesis == self.current.caller_genesis
            && self.current.creation_mode == op_plugins::state_plugins::identity_sled::ContainerCreationMode::AttachExistingHuman
            && self.native_uuid == self.current.instance["config"]["volatile.uuid"].as_str().context("native UUID absent")?
            && root_repair::valid_digest(&self.schema_hash)
            && root_repair::valid_digest(&self.new_seed_metadata_sha256)
            && self.new_seed_id >= 256 && self.isolated_host_uid != 0
            && self.idmap_last_state == "[]" && self.idmap_current == self.idmap_next,
            "invalid own-native-root upgrade authority, source or idmapped-root pins");
        validate_idmap(&self.idmap_current, self.isolated_host_uid)?;
        self.verify_mapping(&self.current.instance)?;
        Ok(())
    }

    fn verify_mapping(&self, raw: &JsonValue) -> anyhow::Result<()> {
        native_attachment::validate_instance(raw, &self.session_id, &self.current.wireguard_pubkey, None)?;
        ensure!(raw["config"]["volatile.uuid"] == self.native_uuid
            && raw["config"]["volatile.idmap.current"] == self.idmap_current
            && raw["config"]["volatile.idmap.next"] == self.idmap_next
            && raw["config"]["volatile.last_state.idmap"] == self.idmap_last_state
            && raw["config"]["volatile.idmap.base"].as_str().context("native idmap base absent")?.parse::<u32>()? == self.isolated_host_uid,
            "native UUID or allocated idmapped-root configuration changed");
        Ok(())
    }

    fn location(&self, require_current_policy: bool) -> anyhow::Result<op_container::SeedLocation> {
        if require_current_policy {
            for (key, value) in [
                ("OP_IDENTITY_GOLDEN_SEED_NAME", self.new_seed_name.as_str()),
                ("OP_IDENTITY_GOLDEN_SUBVOLUME_UUID", self.new_seed_uuid.as_str()),
                ("OP_IDENTITY_POOL_FSID", self.current.pool_fsid.as_str()),
                ("OP_IDENTITY_GOLDEN_METADATA_SHA256", self.new_seed_metadata_sha256.as_str()),
            ] { ensure!(std::env::var(key).ok().as_deref() == Some(value), "upgrade source differs from approved {key}"); }
            ensure!(std::env::var("OP_IDENTITY_GOLDEN_SUBVOLUME_ID").ok().and_then(|id| id.parse::<u64>().ok()) == Some(self.new_seed_id),
                "upgrade seed ID differs from approved policy");
        }
        Ok(op_container::SeedLocation {
            seed_name:self.new_seed_name.clone(), expected_seed_uuid:Some(uuid::Uuid::parse_str(&self.new_seed_uuid)?),
            expected_pool_fsid:Some(uuid::Uuid::parse_str(&self.current.pool_fsid)?),
            expected_container_uid:Some(self.isolated_host_uid), expected_seed_treeid:Some(self.new_seed_id),
            expected_metadata_sha256:Some(hex::decode(&self.new_seed_metadata_sha256)?.try_into()
                .map_err(|_| anyhow::anyhow!("invalid Golden metadata digest"))?), ..Default::default()
        })
    }
}

async fn read_journal(store: &IdentitySledStore, sid: &str) -> anyhow::Result<Option<NativeRootUpgradeJournal>> {
    let cozo = store.cozo.clone(); let sid = sid.to_owned();
    Ok(tokio::task::spawn_blocking(move || cozo.read_native_root_upgrade(&sid)).await??)
}

async fn advance(store: &IdentitySledStore, journal: NativeRootUpgradeJournal, phase: &str,
    raw: Option<&JsonValue>, receipt: Option<&op_container::RootUpgradeReceipt>,
) -> anyhow::Result<NativeRootUpgradeJournal> {
    let cozo = store.cozo.clone(); let phase = phase.to_owned();
    let native_after = raw.map(|raw| IncusPlugin::instance_from_native_metadata(raw.clone())
        .and_then(|instance| serde_json::to_string(&instance).map_err(Into::into))).transpose()?
        .unwrap_or_else(|| journal.native_after_json.clone());
    let new_uuid = receipt.map(|receipt| receipt.volume_uuid.clone()).unwrap_or_else(|| journal.new_root_uuid.clone());
    let new_id = receipt.map_or(journal.new_root_id, |receipt| receipt.volume_treeid);
    let rollback = receipt.map(|receipt| receipt.rollback_name.clone()).unwrap_or_else(|| journal.rollback_name.clone());
    Ok(tokio::task::spawn_blocking(move || cozo.advance_native_root_upgrade(&journal, &phase,
        &native_after, &new_uuid, new_id, &rollback)).await??)
}

async fn admitted(engine: &NativeOwner, store: &IdentitySledStore, actor: &VerifiedIdentityActor<'_>,
    seal: &seed_provision::ProvisionSealPin, plan: &UpgradePlan,
) -> anyhow::Result<()> {
    seed_provision::admit(engine, store, actor.actor_id, actor.session_id, actor.genesis, seal).await?;
    native_attachment::admit_target(engine, store, &plan.current.wireguard_pubkey,
        op_plugins::state_plugins::identity_sled::ContainerCreationMode::AttachExistingHuman, None).await?;
    ensure!(plan.caller_principal_id == actor.actor_id && plan.caller_session_id == actor.session_id
        && plan.caller_genesis == actor.genesis && strict_identity_contract()?.schema_hash == plan.schema_hash,
        "root upgrade caller or sealed schema changed");
    Ok(())
}

fn upgrade_stage<T>(stage: &'static str, result: anyhow::Result<T>) -> anyhow::Result<T> {
    result.inspect_err(|_| tracing::warn!(stage = stage, "native root upgrade check failed"))
}

async fn inspected_upgrade(plan: &UpgradePlan, journal: &NativeRootUpgradeJournal) -> anyhow::Result<op_container::RootUpgradeReceipt> {
    let location = plan.location(false)?;
    let name = native_attachment::native_name(&plan.current.instance)?.to_owned();
    let digest = journal.plan_sha256.clone(); let old_uuid = uuid::Uuid::parse_str(&journal.old_root_uuid)?;
    Ok(tokio::task::spawn_blocking(move || op_container::inspect_upgrade(&location, &name, &digest, old_uuid)).await??)
}
fn verify_native(plan: &UpgradePlan, journal: &NativeRootUpgradeJournal, raw: &JsonValue) -> anyhow::Result<()> {
    plan.verify_mapping(raw)?;
    let hardened: Option<JsonValue> = if journal.hardened_native_raw_json.is_empty() { None }
        else { Some(serde_json::from_str(&journal.hardened_native_raw_json)?) };
    let pin = hardened.as_ref().unwrap_or(&plan.current.instance);
    ensure!(clean_provision::binding_matches(&serde_json::to_string(pin)?, &serde_json::to_string(raw)?),
        "native authority changed outside the exact upgrade transition");
    Ok(())
}


pub(super) async fn upgrade(engine: &NativeOwner, store: &IdentitySledStore,
    actor: VerifiedIdentityActor<'_>, digest: &str,
) -> anyhow::Result<JsonValue> {
    ensure!(root_repair::valid_digest(digest), "invalid own-root upgrade plan digest");
    let sid = actor.session_id.to_owned();
    engine.retain(store.lifecycle_lock(&sid).await.lock_owned().await);
    let seal = seed_provision::capture_method_seal("upgrade_own_native_root")?;
    let read_sid = sid.clone(); let read_digest = digest.to_owned();
    let bytes = tokio::task::spawn_blocking(move || root_repair::trusted_plan_bytes_at(
        Path::new("/var/lib/op-dbus/identity-provision-plans"), &read_sid, &read_digest, 0)).await??;
    let plan: UpgradePlan = upgrade_stage("decode plan", serde_json::from_slice(&bytes).map_err(Into::into))?;
    upgrade_stage("validate plan", plan.validate())?;
    upgrade_stage("admit caller", admitted(engine, store, &actor, &seal, &plan).await)?;
    upgrade_stage("check identity publication", preflight_native_identity_effect().await)?;
    let operation = format!("native-upgrade-{digest}");
    let previous = read_journal(store, &sid).await?;
    let replayed = previous.as_ref().is_some_and(|journal| journal.operation_id == operation);
    let mut journal = if let Some(journal) = previous.filter(|journal| journal.operation_id == operation) {
        ensure!(journal.plan_sha256 == digest && journal.plan_json == serde_json::to_string(&plan)?, "upgrade replay plan changed");
        journal
    } else {
        let expected = load_revision(store, &sid).await?;
        ensure!(expected.revision == plan.current.expected_revision
            && expected.operation_id == plan.current.expected_revision_operation_id, "upgrade identity revision changed");
        let old = revision_sled(&expected)?.context("upgrade identity absent")?;
        engine.verify_own(&old).await?;
        let (guard, native) = upgrade_stage("verify current root", native_attachment::proof(&plan.current, true).await)?;
        engine.retain(guard);
        ensure!(old.instance.as_ref().is_some_and(|bound| identity_instance_matches(&old, &native)
            && bound.name == native.name), "upgrade target is not its committed native UUID");
        let location = upgrade_stage("check approved seed", plan.location(true))?;
        upgrade_stage("inspect approved seed", tokio::task::spawn_blocking(move || op_container::inspect_seed(&location)).await?)?;
        let cozo = store.cozo.clone();
        let new_journal = NativeRootUpgradeJournal { operation_id:operation.clone(), plan_sha256:digest.into(),
            plan_json:serde_json::to_string(&plan)?, expected, phase:"prepared".into(),
            native_before_json:serde_json::to_string(&native)?, native_after_json:String::new(),
            stopped_native_raw_json:String::new(), stopped_native_etag:String::new(), hardened_native_raw_json:String::new(),
            old_root_uuid:plan.current.root.uuid.clone(), new_root_uuid:String::new(), new_root_id:0,
            rollback_name:String::new(), committed_revision:0 };
        upgrade_stage("open upgrade journal", tokio::task::spawn_blocking(move || cozo.begin_native_root_upgrade(new_journal)).await?.map_err(Into::into))?
    };
    let native = IncusRootRepairBackend::local();
    let name = native_attachment::native_name(&plan.current.instance)?.to_owned();
    if journal.phase == "identity-committed" {
        inspected_upgrade(&plan, &journal).await?;
        require_published_identity(store).await?;
        return output(&plan, &journal, true);
    }
    ensure!(load_revision(store, &sid).await? == journal.expected, "upgrade identity was revoked or changed");
    let fresh_stop = journal.phase == "prepared";
    if fresh_stop { journal = advance(store, journal, "stop-intent", None, None).await?; }
    if journal.phase == "stop-intent" {
        admitted(engine, store, &actor, &seal, &plan).await?;
        let (before, _) = native.instance(&name).await?; verify_native(&plan, &journal, &before)?;
        if before["status"] == "Running" {
            ensure!(fresh_stop, "unknown stop outcome: reconcile original operation without repeating native stop");
            store.mark_effect();
            let result = IncusPlugin::set_bound_identity_session_power_state(&sid, &name, IdentitySessionPowerState::Stopped).await;
            let (after, _) = native.instance(&name).await?; verify_native(&plan, &journal, &after)?;
            ensure!(after["status"] == "Stopped", "native stop did not produce stopped readback");
            result?;
        }
        let (stopped, _) = native.instance(&name).await?; verify_native(&plan, &journal, &stopped)?;
        ensure!(stopped["status"] == "Stopped", "native guest is not stopped");
        journal = advance(store, journal, "stopped", Some(&stopped), None).await?;
    }
    let fresh_hardening = journal.phase == "stopped";
    if fresh_hardening {
        admitted(engine, store, &actor, &seal, &plan).await?;
        let (stopped, etag) = native.instance(&name).await?;
        verify_native(&plan, &journal, &stopped)?;
        ensure!(stopped["status"] == "Stopped", "hardening requires the same stopped native guest");
        let cozo = store.cozo.clone(); let expected = journal.clone();
        let raw = serde_json::to_string(&stopped)?;
        journal = tokio::task::spawn_blocking(move || cozo.record_native_root_hardening(
            &expected, "harden-intent", &raw, &etag)).await??;
    }
    if journal.phase == "harden-intent" {
        admitted(engine, store, &actor, &seal, &plan).await?;
        let (before, etag) = native.instance(&name).await?;
        plan.verify_mapping(&before)?;
        ensure!(before["status"] == "Stopped", "native guest started during hardening");
        let old: JsonValue = serde_json::from_str(&journal.stopped_native_raw_json)?;
        let already_hardened = op_cozo_store::native_root_policy_transition_matches(
            &journal.stopped_native_raw_json, &serde_json::to_string(&before)?);
        if !already_hardened {
            ensure!(fresh_hardening && before == old && etag == journal.stopped_native_etag,
                "unknown hardening outcome: reconcile exact old/new policy without repeating PATCH");
            store.mark_effect();
            let effect = native.harden_stopped_identity(&name, &plan.native_uuid, &old, &etag).await;
            let (after, _) = native.instance(&name).await?;
            plan.verify_mapping(&after)?;
            ensure!(op_cozo_store::native_root_policy_transition_matches(
                &journal.stopped_native_raw_json, &serde_json::to_string(&after)?),
                "native hardening readback changed unrelated authority");
            effect?;
        }
        let (hardened, _) = native.instance(&name).await?;
        plan.verify_mapping(&hardened)?;
        let cozo = store.cozo.clone(); let expected = journal.clone();
        let raw = serde_json::to_string(&hardened)?;
        journal = tokio::task::spawn_blocking(move || cozo.record_native_root_hardening(
            &expected, "hardened", &raw, "")).await??;
    }
    if journal.phase == "hardened" { journal = advance(store, journal, "exchange-intent", None, None).await?; }
    if journal.phase == "exchange-intent" {
        admitted(engine, store, &actor, &seal, &plan).await?;
        let (stopped, _) = native.instance(&name).await?; verify_native(&plan, &journal, &stopped)?;
        ensure!(stopped["status"] == "Stopped", "root exchange requires the same stopped native UUID");
        let location = plan.location(true)?;
        let target = name.clone(); let operation_digest = digest.to_owned();
        let old_uuid = uuid::Uuid::parse_str(&journal.old_root_uuid)?;
        store.mark_effect();
        let receipt = tokio::task::spawn_blocking(move || op_container::upgrade_from_seed(
            &location, &target, &operation_digest, old_uuid)).await??;
        let (after, _) = native.instance(&name).await?; verify_native(&plan, &journal, &after)?;
        ensure!(after["status"] == "Stopped", "native guest started during root exchange");
        journal = advance(store, journal, "exchanged", Some(&after), Some(&receipt)).await?;
    }
    let fresh_start = journal.phase == "exchanged";
    if fresh_start {
        inspected_upgrade(&plan, &journal).await?;
        journal = advance(store, journal, "start-intent", None, None).await?;
    }
    if journal.phase == "start-intent" {
        admitted(engine, store, &actor, &seal, &plan).await?;
        inspected_upgrade(&plan, &journal).await?;
        let (before, _) = native.instance(&name).await?; verify_native(&plan, &journal, &before)?;
        if before["status"] == "Stopped" {
            ensure!(fresh_start, "unknown start outcome: reconcile original operation without repeating native start");
            // Incus retains its existing current/next map and last_state=[];
            // rootfs remains uid0 under its already-established idmapped mount.
            // Neither the shared seed nor the private snapshot is chowned here.
            store.mark_effect();
            let result = IncusPlugin::set_bound_identity_session_power_state(&sid, &name, IdentitySessionPowerState::Running).await;
            let (after, _) = native.instance(&name).await?; verify_native(&plan, &journal, &after)?;
            ensure!(after["status"] == "Running", "upgraded native guest did not start");
            result?;
        }
        let (running, _) = native.instance(&name).await?; verify_native(&plan, &journal, &running)?;
        ensure!(running["status"] == "Running" && native.instance_init_pid(&name).await? > 0,
            "native running/init-PID proof absent");
        inspected_upgrade(&plan, &journal).await?;
        journal = advance(store, journal, "running", Some(&running), None).await?;
    }
    ensure!(journal.phase == "running", "root upgrade phase is unresolved");
    admitted(engine, store, &actor, &seal, &plan).await?;
    let (running, _) = native.instance(&name).await?; verify_native(&plan, &journal, &running)?;
    ensure!(running["status"] == "Running", "native power changed before upgrade commit");
    inspected_upgrade(&plan, &journal).await?;
    let mut record = journal.expected.record.clone().context("upgrade original identity absent")?;
    // The live binding was verified above. Commit the journaled text itself:
    // config is an unordered map, so a second serialization never matches it.
    record.instance_json = journal.native_after_json.clone();
    record.active = true;
    let cozo = store.cozo.clone(); let committing = journal.clone(); let commit_sid = sid.clone();
    let commit_operation = operation.clone();
    let publication = engine.commit_identity_intent(operation, move |_| {
        let result = cozo.apply_identity_intent(&commit_sid, &commit_operation,
            IdentityIntent::UpgradeNativeRoot {journal:Box::new(committing), record})?;
        let state = cozo.read_identity_recovery_state(MAX_EVENTS_IN_STATE)?;
        Ok((Some(state_value(&state)?), DurableResult {value:result.current.revision, replayed:result.replayed}))
    }).await?;
    journal.committed_revision = publication_value(publication)?;
    output(&plan, &journal, replayed)
}

fn output(plan: &UpgradePlan, journal: &NativeRootUpgradeJournal, replayed: bool) -> anyhow::Result<JsonValue> {
    Ok(serde_json::json!({"session_id":plan.session_id,"native_uuid":plan.native_uuid,
        "operation_id":journal.operation_id,"old_root_uuid":journal.old_root_uuid,"new_root_uuid":journal.new_root_uuid,
        "rollback_name":journal.rollback_name,"committed_revision":journal.committed_revision,"replayed":replayed}))
}

pub(super) async fn guard_start(store: &IdentitySledStore, revision: &IdentityRevision,
    sled: &ContainerIdentitySled,
) -> anyhow::Result<Option<clean_provision::CleanRootGuard>> {
    let Some(journal) = read_journal(store, &sled.session_id).await? else { return Ok(None) };
    ensure!(journal.phase == "identity-committed" && journal.committed_revision != 0
        && revision.revision >= journal.committed_revision, "native root upgrade remains unresolved");
    let plan: UpgradePlan = serde_json::from_str(&journal.plan_json)?; plan.validate()?;
    inspected_upgrade(&plan, &journal).await?;
    let mut current = plan.current.clone();
    current.instance = serde_json::from_str(&journal.hardened_native_raw_json)?;
    current.root.uuid = journal.new_root_uuid;
    current.root.id = journal.new_root_id;
    current.golden_subvolume_uuid = plan.new_seed_uuid;
    current.golden_seed_name = plan.new_seed_name;
    let (guard, native) = native_attachment::proof(&current, false).await?;
    ensure!(identity_instance_matches(sled, &native), "upgraded native binding no longer matches identity");
    Ok(Some(clean_provision::CleanRootGuard {should_start:root_repair::decide_seeded_native_start(sled.active, &native.status)?, _storage:guard}))
}

#[cfg(test)]
mod tests {
    #[test]
    fn native_idmap_requires_exact_isolated_uid_and_gid_ranges() {
        let mapping = r#"[{"Isuid":true,"Isgid":false,"Hostid":1196608,"Nsid":0,"Maprange":65536},{"Isuid":false,"Isgid":true,"Hostid":1196608,"Nsid":0,"Maprange":65536}]"#;
        assert!(super::validate_idmap(mapping, 1196608).is_ok());
        assert!(super::validate_idmap(mapping, 0).is_err());
        assert!(super::validate_idmap(mapping, 1196609).is_err());
        assert!(super::validate_idmap(&mapping.replace("65536", "1"), 1196608).is_err());
        assert!(super::validate_idmap(&mapping.replace("\"Isgid\":true", "\"Isgid\":false"), 1196608).is_err());
    }
}
