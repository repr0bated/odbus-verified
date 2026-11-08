//! Identity-owned, one-POST native-name cutover. A repeated request reconciles
//! the original UUID and names; an old-name observation never retries POST.
use super::*;
use op_cozo_store::{ContainerRenameJournal, ContainerRenameRequest};
use op_plugins::state_plugins::identity_sled::{RenameContainerInput, RenameContainerOutput};

fn native_uuid(instance: &IncusInstance) -> Option<&str> {
    instance.config.as_ref()?.get("volatile.uuid").map(String::as_str)
}

fn exact_instance<'a>(snapshot: &'a IncusState, request: &ContainerRenameRequest, require_stopped: bool) -> anyhow::Result<&'a IncusInstance> {
    let mut matching = snapshot.instances.iter().filter(|instance| native_uuid(instance) == Some(request.native_uuid.as_str()));
    let instance = matching.next().context("pinned native UUID is absent; rename is not recreated")?;
    ensure!(matching.next().is_none(), "native UUID is ambiguous");
    ensure!(snapshot.instances.iter().filter(|candidate| candidate.config.as_ref()
        .is_some_and(|config| config.get("user.opdbus.session_id") == Some(&request.session_id))).count() == 1,
        "native session has more than one container binding");
    let stopped = instance.status == "Stopped" && instance.status_code == Some(102);
    let running = instance.status == "Running" && instance.status_code == Some(103);
    ensure!(instance.instance_type == "container" && (stopped || (!require_stopped && running))
        && instance.location.as_deref() == Some("none")
        && instance.project.as_deref().is_none_or(|project| project == "default"),
        "native rename requires the local default-project container, stopped until identity commit");
    Ok(instance)
}

async fn journal(store: &IdentitySledStore, session: &str) -> anyhow::Result<Option<ContainerRenameJournal>> {
    let cozo = store.cozo.clone();
    let session = session.to_owned();
    Ok(tokio::task::spawn_blocking(move || cozo.read_container_rename(&session)).await??)
}

pub(super) async fn current_instance(store: &IdentitySledStore, session: &str, original: String) -> anyhow::Result<String> {
    let cozo = store.cozo.clone();
    let session = session.to_owned();
    Ok(tokio::task::spawn_blocking(move || cozo.identity_instance_with_current_name(&session, &original)).await??)
}

async fn retain_storage_proof(engine: &NativeOwner, store: &IdentitySledStore,
    expected: &IdentityRevision, sled: &ContainerIdentitySled) -> anyhow::Result<()> {
    // These existing owners verify their original receipts. No plan or device
    // source is rewritten and logical attachment is not committed by rename.
    let mut observed = sled.clone();
    if let Some(device) = observed.btrfs_device.as_mut() { device.attached = true; }
    if let Some(guard) = clean_provision::guard_start(store, expected, &observed).await? {
        engine.retain(guard);
        return Ok(());
    }
    if physical_root::guard_start(store, expected, &observed).await?.is_some()
        || root_repair::guard_own_root_start(store, expected, &observed).await?.is_some()
        || seed_provision::guard_start(store, expected, &observed).await?.is_some()
    {
        anyhow::bail!("legacy root plan derives the native name from the immutable session id; this label-only cutover cannot rewrite that retained storage binding");
    }
    Ok(())
}

fn result(journal: &ContainerRenameJournal, replayed: bool) -> anyhow::Result<JsonValue> {
    Ok(serde_json::to_value(RenameContainerOutput {
        operation_id: journal.request.operation_id.clone(), session_id: journal.request.session_id.clone(),
        native_uuid: journal.request.native_uuid.clone(), old_name: journal.request.old_name.clone(),
        name: journal.request.new_name.clone(), phase: "identity-committed".into(),
        committed_revision: journal.committed_revision, replayed,
    })?)
}

pub(super) async fn rename(engine: &NativeOwner, store: &IdentitySledStore, input: RenameContainerInput)
    -> anyhow::Result<JsonValue>
{
    ensure!(input.new_name == RESERVED_CHATBOT_CONTAINER && input.old_name != input.new_name
        && uuid::Uuid::parse_str(&input.operation_id)?.to_string() == input.operation_id
        && uuid::Uuid::parse_str(&input.native_uuid)?.to_string() == input.native_uuid,
        "rename requires the current configured service label and canonical operation/native UUIDs");
    ensure!(control_plane_chatbot_session_id().as_deref() == Some(input.session_id.as_str()),
        "rename target is not the configured service identity");
    engine.retain(store.lifecycle_lock(&input.session_id).await.lock_owned().await);
    engine.revalidate().await?;
    preflight_native_identity_effect().await?;
    let expected = load_revision(store, &input.session_id).await?;
    let sled = revision_sled(&expected)?.context("rename identity is absent")?;
    ensure!(sled.is_anchored() && sled.principal_kind.as_deref() == Some("service")
        && sled.sealed_id.as_ref().is_some_and(|value| !value.is_empty())
        && configured_chatbot_key().as_deref() == Some(sled.wireguard_pubkey.as_str())
        && op_identity::session::derive_session_id(&sled.wireguard_pubkey) == input.session_id,
        "rename must preserve the registered anchored configured service");
    seed_provision::target_key_is_registered(&sled.wireguard_pubkey).await?;
    let seal = crate::sealed_schema_reader::read_manifest_pinned_schema_document(engine.runtime.catalog_dir(), "identity_sled")?;
    let request = ContainerRenameRequest {
        operation_id: input.operation_id, session_id: input.session_id, native_uuid: input.native_uuid,
        old_name: input.old_name, new_name: input.new_name, actor_id: engine.caller.principal_id.clone(),
        caller_session: engine.caller.session_id.clone(), caller_genesis: engine.caller.session_genesis.clone(),
        schema_hash: seal.schema_hash,
    };
    let native = IncusRootRepairBackend::local();
    let existing = journal(store, &request.session_id).await?;
    let before = native.default_project_state().await?;
    let observed = exact_instance(&before, &request, existing.as_ref().is_none_or(|journal| journal.committed_revision == 0))?;
    ensure!(crate::identity_binding::configured_chatbot_instance_matches(&sled, observed),
        "native UUID no longer carries the configured key/session");
    let mut native_result = Ok(());
    let (intent, fresh) = if let Some(existing) = existing {
        ensure!(existing.request == request, "rename is owned by another caller, schema or operation");
        (existing, false)
    } else {
        ensure!(observed.name == request.old_name && identity_instance_matches(&sled, observed)
            && !before.instances.iter().any(|instance| instance.name == request.new_name),
            "rename source binding or destination differs");
        retain_storage_proof(engine, store, &expected, &sled).await?;
        let (raw, etag) = native.instance(&request.old_name).await?;
        let stopped = IncusPlugin::instance_from_native_metadata(raw)?;
        ensure!(serde_json::to_value(&stopped)? == serde_json::to_value(observed)?,
            "native rename source changed during preflight");
        engine.revalidate().await?;
        ensure!(load_revision(store, &request.session_id).await? == expected, "rename revision changed during preflight");
        let cozo = store.cozo.clone();
        let binding = request.clone();
        let names = before.instances.iter().map(|instance| instance.name.clone()).collect::<Vec<_>>();
        let native_before = serde_json::to_string(&stopped)?;
        store.mark_effect();
        let prepared = tokio::task::spawn_blocking(move || cozo.begin_container_rename(binding, &expected, native_before, &names)).await??;
        if prepared.1 {
            engine.revalidate().await?;
            preflight_native_identity_effect().await?;
            // The durable intent consumes POST permission before the call.
            // Even a transport error is reconciled below, never retried here.
            native_result = native.rename_stopped_identity(&request.old_name, &request.new_name,
                &request.native_uuid, &etag).await;
        }
        prepared
    };
    if intent.committed_revision == 0 { store.mark_effect(); }
    let after = native.default_project_state().await?;
    let renamed = exact_instance(&after, &request, intent.committed_revision == 0)?;
    if renamed.name != request.new_name || after.instances.iter().any(|instance| instance.name == request.old_name) {
        native_result.context("native rename unconfirmed; its durable POST permission is consumed")?;
        anyhow::bail!("rename intent unresolved: expected UUID must exist only at new name; POST will not be repeated");
    }
    ensure!(crate::identity_binding::configured_chatbot_instance_matches(&sled, renamed),
        "renamed native key/session binding changed");
    let after_json = serde_json::to_string(renamed)?;
    if intent.committed_revision != 0 {
        ensure!(identity_instance_matches(&sled, renamed)
            && op_cozo_store::clean_provision_instance_binding_matches(&intent.native_after_json, &after_json),
            "completed rename native binding changed");
        require_published_identity(store).await?;
        engine.revalidate().await?;
        engine.observe_incus(&after).await?;
        return result(&intent, true);
    }
    ensure!(op_cozo_store::container_rename_native_matches(&intent.native_before_json, &after_json, &request.new_name),
        "native rename changed a pinned field; explicit reconciliation required");
    engine.revalidate().await?;
    let cozo = store.cozo.clone();
    let sid = request.session_id.clone();
    let operation = format!("rename-{}", request.operation_id);
    let commit_id = operation.clone();
    let publication = engine.commit_identity_intent(operation, move |_| {
        let committed = cozo.apply_identity_intent(&sid, &commit_id,
            IdentityIntent::RenameContainer { journal: Box::new(intent), native_after_json: after_json })?;
        let snapshot = cozo.read_identity_recovery_state(MAX_EVENTS_IN_STATE)?;
        Ok((Some(state_value(&snapshot)?), DurableResult { value: (), replayed: committed.replayed }))
    }).await?;
    publication_value(publication)?;
    let completed = journal(store, &request.session_id).await?.context("completed rename receipt absent")?;
    engine.observe_incus(&after).await?;
    result(&completed, !fresh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_rename_reconciliation_requires_one_exact_local_uuid_and_session() {
        let request = ContainerRenameRequest {
            operation_id:"00000000-0000-4000-8000-000000000001".into(),
            session_id:"b0c777e3-0955-cd8b-3d01-ca12998cae28".into(),
            native_uuid:"d09a6200-6ff1-4dc0-922f-27ebdaa0861a".into(),
            old_name:"CockpitBot3tched".into(), new_name:RESERVED_CHATBOT_CONTAINER.into(),
            actor_id:"owner".into(), caller_session:"owner".into(), caller_genesis:"genesis".into(),
            schema_hash:"a".repeat(64),
        };
        let instance = IncusInstance {
            name:request.old_name.clone(), status:"Stopped".into(), status_code:Some(102),
            instance_type:"container".into(), location:Some("none".into()), project:Some("default".into()),
            config:Some(HashMap::from([
                ("volatile.uuid".into(), request.native_uuid.clone()),
                ("user.opdbus.session_id".into(), request.session_id.clone()),
            ])), ..Default::default()
        };
        let mut snapshot = IncusState { instances:vec![instance.clone()] };
        assert!(exact_instance(&snapshot, &request, true).is_ok());
        snapshot.instances.push(instance);
        assert!(exact_instance(&snapshot, &request, true).is_err());
        snapshot.instances[1].config.as_mut().unwrap().insert("volatile.uuid".into(), "another".into());
        assert!(exact_instance(&snapshot, &request, true).is_err());
        snapshot.instances.pop();
        snapshot.instances[0].status = "Running".into();
        snapshot.instances[0].status_code = Some(103);
        assert!(exact_instance(&snapshot, &request, true).is_err());
        assert!(exact_instance(&snapshot, &request, false).is_ok());
        snapshot.instances[0].project = Some("other".into());
        assert!(exact_instance(&snapshot, &request, false).is_err());
    }
}
