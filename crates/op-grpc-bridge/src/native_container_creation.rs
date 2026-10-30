//! MutationEngine-owned native name allocation and creation retry protection.
//! Names are human-facing; the creation journal is not a lifecycle authority.
//! No retry ever repeats POST.
use anyhow::{Context, Result, ensure};
use op_cozo_store::{NativeCreationRecord, NativeCreationRequest};
use op_plugins::state_plugins::incus::{self, IncusRootRepairBackend};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use crate::mutation_engine::MutationEngine;

pub(crate) async fn records(engine: &MutationEngine) -> Result<Vec<NativeCreationRecord>> {
    let cozo = engine.identity_sled_store()?.cozo().clone();
    Ok(tokio::task::spawn_blocking(move || cozo.native_creation_records()).await??)
}

pub(crate) fn request(operation: &str, actor: &str, session: &str, genesis: &str,
    kind: &str, target: &str, body: Value) -> Result<NativeCreationRequest> {
    ensure!(body.is_object() && body.get("name").is_none(), "native name is server assigned");
    let plugin = match kind { "identity" => "identity_sled", "project" => "project", "incus" => "incus",
        _ => anyhow::bail!("unknown native owner") };
    let pin = crate::sealed_schema_reader::read_manifest_pinned_schema_document(
        &crate::sealed_schema_reader::blob_catalog_dir_from_env(), plugin)?;
    let request_json = serde_json::to_string(&body)?;
    let request_digest = format!("{:x}", Sha256::digest(request_json.as_bytes()));
    Ok(NativeCreationRequest { operation_id: operation.into(), actor_id: actor.into(),
        caller_session: session.into(), caller_genesis: genesis.into(), owner_kind: kind.into(),
        logical_target: target.into(), schema_hash: pin.schema_hash, request_digest, request_json,
        reserved_name: None })
}

async fn admit(engine: &MutationEngine, request: &NativeCreationRequest) -> Result<()> {
    if let Some(name) = &request.reserved_name {
        let body: Value = serde_json::from_str(&request.request_json)?;
        let key = crate::identity_sled_dispatch::configured_chatbot_key()
            .context("reserved chatbot key is not configured")?;
        ensure!(name == "zeroclawCtlChatbot" && request.owner_kind == "identity"
            && request.logical_target == op_identity::session::derive_session_id(&key)
            && crate::identity_sled_dispatch::control_plane_chatbot_session_id().as_deref()
                == Some(request.logical_target.as_str())
            && body["config"]["user.opdbus.session_id"] == request.logical_target
            && body["config"]["user.opdbus.wireguard_public_key"] == key,
            "reserved native name requires the configured chatbot binding");
    }
    let (plugin, method, capability) = match request.owner_kind.as_str() {
        "identity" => ("identity_sled", "prepare_container", "identity_sled.write"),
        "project" => ("project", "create", "project.create"),
        "incus" => ("incus", "create_instance", "cap.service.incus.instance.create@v1"),
        _ => anyhow::bail!("unknown native owner"),
    };
    crate::method_contract_admission::admit_compiled_method(plugin, method, Some(capability))?;
    let pin = crate::sealed_schema_reader::read_manifest_pinned_schema_document(
        &crate::sealed_schema_reader::blob_catalog_dir_from_env(), plugin)?;
    ensure!(pin.schema_hash == request.schema_hash, "native creation seal changed; explicit reconciliation required");
    ensure!(crate::interceptor::load_capability_grants(&request.actor_id).contains(capability),
        "native creation requires current exact-principal grant");
    let current = engine.session_context_for_actor(&request.actor_id).await
        .context("native creation caller session missing")?;
    ensure!(current.session_id == request.caller_session && current.genesis_hex == request.caller_genesis,
        "native creation caller session changed");
    Ok(())
}

/// Caller holds lifecycle (if applicable), then host observation lock. The
/// durable intent is committed and flushed before the sole POST permission.
pub(crate) async fn create_locked(engine: &MutationEngine, request: NativeCreationRequest)
    -> Result<(NativeCreationRecord, bool)> {
    admit(engine, &request).await?;
    let native = IncusRootRepairBackend::local();
    let before = native.default_project_state().await?;
    if request.owner_kind == "project" {
        let prior = records(engine).await?;
        ensure!(!before.instances.iter().any(|i|
            (i.name == request.logical_target || i.config.as_ref().is_some_and(|c|
                c.get("user.opdbus.project_id") == Some(&request.logical_target)))
            && !prior.iter().any(|r| r.name == i.name && r.request.operation_id == request.operation_id)),
            "existing project native binding must not be recreated");
    }
    let names = before.instances.iter().map(|i| i.name.clone()).collect::<Vec<_>>();
    let cozo = engine.identity_sled_store()?.cozo().clone();
    let (record, fresh) = tokio::task::spawn_blocking(move || cozo.reserve_native_creation(request, &names)).await??;
    // Any failure from here retains the fence, including definite provider
    // errors. An absent object is not proof that an earlier POST was rejected.
    if fresh {
        admit(engine, &record.request).await?;
        let mut body: Value = serde_json::from_str(&record.request.request_json)?;
        body["name"] = record.name.clone().into();
        body["config"]["user.opdbus.creation_operation"] = record.request.operation_id.clone().into();
        body["config"]["user.opdbus.creation_digest"] = record.request.request_digest.clone().into();
        incus::create_reserved_native(&body).await
            .context("native POST outcome requires reconciliation; reservation retained")?;
    }
    let record = reconcile_locked(engine, record).await?;
    Ok((record, !fresh))
}

pub(crate) async fn reconcile_locked(engine: &MutationEngine, record: NativeCreationRecord)
    -> Result<NativeCreationRecord> {
    admit(engine, &record.request).await?;
    let native = IncusRootRepairBackend::local();
    let (raw, _) = native.instance(&record.name).await
        .context("reserved native instance absent or unreadable; POST will not be repeated")?;
    let body: Value = serde_json::from_str(&record.request.request_json)?;
    ensure!(raw["name"] == record.name && raw["type"] == body["type"]
        && raw["config"]["user.opdbus.creation_operation"] == record.request.operation_id
        && raw["config"]["user.opdbus.creation_digest"] == record.request.request_digest,
        "native creation ownership markers differ");
    if record.phase != "complete" {
        ensure!(raw["status"] == "Stopped" && raw["status_code"] == 102
            && raw["profiles"] == body["profiles"] && raw["devices"] == body["devices"],
            "native preparation is not the pinned stopped definition");
        if record.request.owner_kind == "identity" {
            ensure!(raw["expanded_config"]["security.privileged"] == "false"
                && raw["expanded_config"]["security.nesting"] == "false"
                && raw["expanded_config"]["boot.autostart"] == "false"
                && raw["expanded_config"]["raw.lxc"] == body["config"]["raw.lxc"],
                "expanded native identity security differs");
            let expanded = raw["expanded_devices"].as_object().context("expanded devices absent")?;
            ensure!(expanded.len() == 4 && expanded.get("root") == body["devices"].get("root")
                && expanded.get("runtime-run") == body["devices"].get("runtime-run")
                && expanded.get("fuse") == body["devices"].get("fuse")
                && expanded.get("lungs-socket-dir") == Some(&json!({"type":"disk", "source":"/run/opdbus/lungs",
                    "path":"/run/opdbus/lungs", "readonly":"true"})), "expanded identity devices differ");
        }
        for (key, value) in body["config"].as_object().context("creation config missing")? {
            ensure!(raw["config"].get(key) == Some(value), "native creation config differs");
        }
    }
    let native_uuid = raw["config"]["volatile.uuid"].as_str().context("native UUID absent")?.to_owned();
    ensure!(uuid::Uuid::parse_str(&native_uuid)?.to_string() == native_uuid, "noncanonical native UUID");
    let cozo = engine.identity_sled_store()?.cozo().clone();
    let pinned = record.clone();
    let bound = tokio::task::spawn_blocking(move || cozo.bind_native_creation(&pinned, &native_uuid)).await??;
    let snapshot = native.default_project_state().await?;
    let instance = snapshot.instances.iter().find(|i| i.name == bound.name)
        .context("created instance missing from full readback")?;
    ensure!(serde_json::to_value(instance)? == serde_json::to_value(incus::IncusPlugin::instance_from_native_metadata(raw)?)?,
        "native instance changed during full readback");
    crate::incus_observer::publish_repair_readback_locked(engine, &snapshot).await?;
    let cozo = engine.identity_sled_store()?.cozo().clone();
    Ok(tokio::task::spawn_blocking(move || cozo.complete_native_creation(&bound)).await??)
}

pub(crate) async fn dispatch(engine: &MutationEngine, method: &str, args: &Value,
    actor: &str, session: Option<&str>, genesis: Option<&str>) -> Result<Value> {
    let session = session.context("native creation requires verified session")?.to_owned();
    let genesis = genesis.context("native creation requires verified genesis")?.to_owned();
    let engine = engine.clone();
    let method = method.to_owned();
    let args = args.clone();
    let actor = actor.to_owned();
    tokio::spawn(async move {
        let _host = engine.lock_host_observation().await;
        let (record, replayed) = if method == "create_instance" {
            let input: incus::CreateInstanceInput = serde_json::from_value(args)?;
            let body = incus::native_creation_body(&input)?;
            let request = request(&input.operation_id, &actor, &session, &genesis,
                "incus", &input.operation_id, body)?;
            create_locked(&engine, request).await?
        } else {
            let input: incus::ReconcileNativeCreationInput = serde_json::from_value(args)?;
            let record = records(&engine).await?.into_iter()
                .find(|r| r.request.operation_id == input.operation_id)
                .context("native creation operation not found")?;
            ensure!(record.request.owner_kind == "incus" && record.request.actor_id == actor
                && record.request.caller_session == session && record.request.caller_genesis == genesis,
                "native creation operation belongs to another owner");
            (reconcile_locked(&engine, record).await?, true)
        };
        Ok(json!({"name":record.name,"operation_id":record.request.operation_id,
            "native_uuid":record.native_uuid,"phase":record.phase,"replayed":replayed,
            "created":true,"changes":[]}))
    }).await.context("native creation owner failed; reconcile the same operation")?
}
