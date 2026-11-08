//! Native name allocation with one durable POST permission and exact readback.
//! Retries reconcile the original operation; absence never authorizes a new POST.
use anyhow::{Context as _, ensure};
use op_cozo_store::{NativeCreationRecord, NativeCreationRequest};
use op_plugins::state_plugins::{incus::{self, IncusRootRepairBackend}, project::Project};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use crate::{core::{BackendStateUpdate, StateScope}, runtime::{BackendContext, BackendOutcome, Runtime, RuntimeError, VerifiedCaller}};

fn owner_method(kind: &str) -> anyhow::Result<(&'static str, &'static str)> {
    match kind {
        "project" => Ok(("project", "create")),
        "identity" => Ok(("identity_sled", "prepare_container")),
        "incus" => Ok(("incus", "create_instance")),
        _ => anyhow::bail!("native creation requires its dedicated identity owner"),
    }
}

async fn admit(runtime: &Runtime, caller: &VerifiedCaller, request: &NativeCreationRequest,
    renamed: Option<&op_cozo_store::ContainerRenameJournal>) -> anyhow::Result<()> {
    ensure!(request.actor_id == caller.principal_id && request.caller_session == caller.session_id
        && request.caller_genesis == caller.session_genesis, "native creation owner differs");
    if request.owner_kind == "identity" {
        let key = crate::identity_binding::configured_chatbot_key().context("configured chatbot key is absent")?;
        let session = crate::identity_binding::control_plane_chatbot_session_id().context("configured chatbot session is absent")?;
        let body: Value = serde_json::from_str(&request.request_json)?;
        ensure!(session == op_identity::session::derive_session_id(&key)
            && request.logical_target == session
            && (request.reserved_name.as_deref() == Some(crate::identity_binding::RESERVED_CHATBOT_CONTAINER)
                || renamed.is_some_and(|journal| request.reserved_name.as_deref() == Some(journal.request.old_name.as_str())))
            && body["config"]["user.opdbus.session_id"] == session
            && body["config"]["user.opdbus.wireguard_public_key"] == key,
            "reserved native creation requires the exact configured service identity");
    } else {
        ensure!(request.reserved_name.is_none(), "reserved identities require the dedicated identity provisioner");
    }
    let (plugin, method) = owner_method(&request.owner_kind)?;
    runtime.authorize_nested(caller, plugin, method).await?;
    let pin = crate::sealed_schema_reader::read_manifest_pinned_schema_document(runtime.catalog_dir(), plugin)?;
    ensure!(pin.schema_hash == request.schema_hash
        || renamed.is_some_and(|journal| journal.request.schema_hash == pin.schema_hash),
        "native creation seal changed; explicit reconciliation required");
    Ok(())
}

fn request(runtime: &Runtime, caller: &VerifiedCaller, operation: &str, kind: &str, target: &str, body: Value) -> anyhow::Result<NativeCreationRequest> {
    ensure!(body.is_object() && body.get("name").is_none(), "native name is owner allocated");
    let (plugin, _) = owner_method(kind)?;
    let pin = crate::sealed_schema_reader::read_manifest_pinned_schema_document(runtime.catalog_dir(), plugin)?;
    let request_json = serde_json::to_string(&body)?;
    let request_digest = format!("{:x}", Sha256::digest(request_json.as_bytes()));
    Ok(NativeCreationRequest { operation_id: operation.into(), actor_id: caller.principal_id.clone(),
        caller_session: caller.session_id.clone(), caller_genesis: caller.session_genesis.clone(),
        owner_kind: kind.into(), logical_target: target.into(), schema_hash: pin.schema_hash,
        request_digest, request_json, reserved_name: None })
}

async fn records(runtime: &Runtime) -> anyhow::Result<Vec<NativeCreationRecord>> {
    let store = runtime.identity_store().clone();
    Ok(tokio::task::spawn_blocking(move || store.native_creation_records()).await??)
}

async fn reconcile(runtime: &Runtime, caller: &VerifiedCaller, record: NativeCreationRecord) -> anyhow::Result<(NativeCreationRecord, incus::IncusState)> {
    let renamed = if record.request.owner_kind == "identity" && record.phase == "complete" {
        let store = runtime.identity_store().clone();
        let sid = record.request.logical_target.clone();
        tokio::task::spawn_blocking(move || store.read_container_rename(&sid)).await??
            .filter(|journal| journal.committed_revision != 0
                && journal.request.native_uuid == record.native_uuid && journal.request.new_name == record.name
                && journal.request.new_name == crate::identity_binding::RESERVED_CHATBOT_CONTAINER)
    } else { None };
    admit(runtime, caller, &record.request, renamed.as_ref()).await?;
    let native = IncusRootRepairBackend::local();
    let (raw, _) = native.instance(&record.name).await.context("reserved native instance unavailable; POST must not be repeated")?;
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
        for (key, value) in body["config"].as_object().context("creation configuration absent")? {
            ensure!(raw["config"].get(key) == Some(value), "native creation configuration differs");
        }
    }
    let native_uuid = raw["config"]["volatile.uuid"].as_str().context("native UUID absent")?.to_owned();
    ensure!(uuid::Uuid::parse_str(&native_uuid)?.to_string() == native_uuid, "native UUID is not canonical");
    let store = runtime.identity_store().clone();
    let binding = record.clone();
    let bound = tokio::task::spawn_blocking(move || store.bind_native_creation(&binding, &native_uuid)).await??;
    let snapshot = native.default_project_state().await?;
    let instance = snapshot.instances.iter().find(|instance| instance.name == bound.name)
        .context("native creation missing from complete readback")?;
    ensure!(serde_json::to_value(instance)? == serde_json::to_value(incus::IncusPlugin::instance_from_native_metadata(raw)?)?,
        "native creation changed during complete readback");
    caller.authority.revalidate().await?;
    let store = runtime.identity_store().clone();
    let complete = tokio::task::spawn_blocking(move || store.complete_native_creation(&bound)).await??;
    Ok((complete, snapshot))
}

async fn create(runtime: &Runtime, caller: &VerifiedCaller, request: NativeCreationRequest) -> anyhow::Result<(NativeCreationRecord, incus::IncusState, bool)> {
    admit(runtime, caller, &request, None).await?;
    let before = IncusRootRepairBackend::local().default_project_state().await?;
    if request.owner_kind == "project" {
        let prior = records(runtime).await?;
        ensure!(!before.instances.iter().any(|instance|
            (instance.name == request.logical_target || instance.config.as_ref().is_some_and(|config|
                config.get("user.opdbus.project_id") == Some(&request.logical_target)))
            && !prior.iter().any(|record| record.name == instance.name && record.request.operation_id == request.operation_id)),
            "existing project native binding must not be recreated");
    }
    let names = before.instances.iter().map(|instance| instance.name.clone()).collect::<Vec<_>>();
    let store = runtime.identity_store().clone();
    let (record, fresh) = tokio::task::spawn_blocking(move || store.reserve_native_creation(request, &names)).await??;
    if fresh {
        admit(runtime, caller, &record.request, None).await?;
        let mut body: Value = serde_json::from_str(&record.request.request_json)?;
        body["name"] = record.name.clone().into();
        body["config"]["user.opdbus.creation_operation"] = record.request.operation_id.clone().into();
        body["config"]["user.opdbus.creation_digest"] = record.request.request_digest.clone().into();
        // No automatic retries, including a lost reply from this sole POST.
        incus::create_reserved_native(&body).await.context("native creation outcome requires reconciliation")?;
    }
    let (record, state) = reconcile(runtime, caller, record).await?;
    Ok((record, state, !fresh))
}

pub(crate) async fn create_project_container(runtime: &Runtime, caller: &VerifiedCaller, operation_id: &str,
    project: &Project, image_fingerprint: &str, pool: &str) -> Result<NativeCreationRecord, RuntimeError> {
    project.authorize(&caller.session_id, true, true)?;
    let body = incus::IncusPlugin::project_creation_body(&project.project_id, &project.owner_session_id,
        image_fingerprint, pool, &project.persistence_path)?;
    let request = request(runtime, caller, operation_id, "project", &project.project_id, body)?;
    let (record, state, _) = create(runtime, caller, request).await.map_err(RuntimeError::unknown)?;
    runtime.publish_backend_state(caller, "incus", BackendStateUpdate::Replace(serde_json::to_value(state)?), StateScope::Public).await?;
    Ok(record)
}

/// The identity owner retains its lifecycle/storage guards around this call.
/// This prepares the declared native definition; it never provisions a leaf.
pub(crate) async fn create_identity_container(runtime: &Runtime, caller: &VerifiedCaller,
    operation_id: &str, target_session: &str, body: Value, reserved_name: Option<String>)
    -> Result<(NativeCreationRecord, incus::IncusState, bool), RuntimeError> {
    if let Some(record) = records(runtime).await?.into_iter()
        .find(|record| record.request.operation_id == operation_id) {
        if record.request.owner_kind != "identity" || record.request.logical_target != target_session {
            return Err(RuntimeError::denied("native preparation has another identity owner"));
        }
        let (record, state) = reconcile(runtime, caller, record).await.map_err(RuntimeError::unknown)?;
        return Ok((record, state, true));
    }
    let mut request = request(runtime, caller, operation_id, "identity", target_session, body)?;
    request.reserved_name = reserved_name;
    create(runtime, caller, request).await.map_err(RuntimeError::unknown)
}

pub(crate) async fn dispatch(ctx: &BackendContext<'_>, method: &str, args: &Value) -> Result<BackendOutcome, RuntimeError> {
    let (record, state, replayed) = match method {
        "create_instance" => {
            let input: incus::CreateInstanceInput = serde_json::from_value(args.clone())?;
            let body = incus::native_creation_body(&input)?;
            let request = request(ctx.runtime, ctx.caller, &input.operation_id, "incus", &input.operation_id, body)?;
            create(ctx.runtime, ctx.caller, request).await.map_err(RuntimeError::unknown)?
        }
        "reconcile_creation" => {
            let input: incus::ReconcileNativeCreationInput = serde_json::from_value(args.clone())?;
            let record = records(ctx.runtime).await?.into_iter().find(|record| record.request.operation_id == input.operation_id)
                .ok_or_else(|| RuntimeError::denied("native creation operation is unavailable"))?;
            if record.request.owner_kind != "incus" { return Err(RuntimeError::denied("native creation has another owner")); }
            let (record, state) = reconcile(ctx.runtime, ctx.caller, record).await.map_err(RuntimeError::unknown)?;
            (record, state, true)
        }
        _ => return Err(RuntimeError::unsupported("method is not a native creation operation")),
    };
    Ok(BackendOutcome { result: json!({"name":record.name,"operation_id":record.request.operation_id,
        "native_uuid":record.native_uuid,"phase":record.phase,"replayed":replayed,"created":true,"changes":[]}),
        state: BackendStateUpdate::Replace(serde_json::to_value(state)?), scope: StateScope::Public,
        additional: Vec::new(), guard: None })
}
