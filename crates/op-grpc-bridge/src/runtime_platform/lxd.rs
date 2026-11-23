//! Admitted LXD operations for subsidiary containers on this host's daemon.
//!
//! Every catalog capability reaches `$ROOT`, so a grant alone must never turn
//! an LXD payload into host authority. Mutations are fixed to the subsidiary
//! shape and to a live human caller; exec, console, file, config, profile
//! and network methods have no route here. Power is limited to the recorded
//! parent starting or stopping its own admitted subsidiary.
use crate::runtime::{BackendContext, RuntimeError};
use anyhow::{Context, Result, ensure};
use op_plugins::state_plugins::lxd as native_lxd;
use serde_json::{Value, json};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

use crate::runtime_identity_owner::lxd_subsidiary::{self, SOCKET};
const IMAGE_DIR: &str = "/var/lib/op-dbus/lxd-images";
const POOL: &str = "lxd-pool";
/// Required on every subsidiary instance, exactly.
const ISOLATION: [(&str, &str); 13] = [
    ("boot.autostart", "false"),
    ("security.privileged", "false"),
    ("security.nesting", "false"),
    ("security.idmap.isolated", "true"),
    ("security.devlxd", "false"),
    ("security.syscalls.deny_default", "true"),
    ("security.syscalls.deny_compat", "true"),
    ("security.syscalls.intercept.mknod", "false"),
    ("security.syscalls.intercept.mount", "false"),
    ("security.syscalls.intercept.bpf", "false"),
    ("security.syscalls.intercept.setxattr", "false"),
    ("limits.memory.enforce", "hard"),
    ("user.opdbus.mode", "subsidiary"),
];

pub(crate) fn supports(method: &str) -> bool {
    matches!(
        method,
        "get_images" | "get_image" | "get_image_fingerprints" | "get_instance_names"
            | "get_instance" | "get_instance_state" | "get_operation" | "get_operation_wait"
            | "get_storage_pool" | "get_storage_pool_volumes" | "get_storage_pool_volume"
    ) || mutates(method)
}

fn mutates(method: &str) -> bool {
    matches!(method, "create_image" | "create_instance" | "create_storage_pool_volume" | "update_instance_state")
}

pub(crate) async fn dispatch(ctx: &BackendContext<'_>, method: &str, args: &Value)
    -> Result<Value, RuntimeError> {
    if mutates(method) {
        require_human(ctx).await?;
        let check = match method {
            "create_instance" => validate_instance(args, &ctx.caller.principal_id),
            "create_storage_pool_volume" => validate_volume(args),
            "update_instance_state" => validate_state(args).map(|_| ()),
            _ => validate_image(args).map(|_| ()),
        };
        check.map_err(|error| RuntimeError::denied(error.to_string()))?;
        ctx.caller.authority.revalidate().await
            .map_err(|error| RuntimeError::denied(error.to_string()))?;
        match method {
            "create_instance" => reserve(ctx, args).await?,
            "update_instance_state" => prepare_state(ctx, args).await?,
            _ => {}
        }
    }
    let scope = subsidiary_scope(ctx).await?;
    if let Some(scope) = &scope {
        ensure_in_scope(scope, method, args).map_err(|error| RuntimeError::denied(error.to_string()))?;
    }
    let client = native_lxd::Client::local(Path::new(SOCKET))
        .map_err(|error| RuntimeError::backend(error.into()))?;
    if method == "create_image" {
        return import_image(&client, args).await;
    }
    let mut result = native_lxd::dispatch_lxd_method(&client, method, args).await.map_err(|error| {
        if mutates(method) { RuntimeError::unknown(error.into()) }
        else { RuntimeError::backend(error.into()) }
    })?;
    if let (Some(scope), "get_instance_names") = (&scope, method) {
        result["result"].as_array_mut()
            .ok_or_else(|| RuntimeError::backend(anyhow::anyhow!("LXD instance names are not a list")))?
            .retain(|name| name.as_str().is_some_and(|name| scope.iter().any(|row| row.name == name)));
    }
    Ok(result)
}

/// The LXD containers a subsidiary may see: its own and those it parents.
/// `None` when the caller is not an LXD subsidiary. The rows are the durable
/// parent-authorized intents, never a label or a name the caller supplies.
async fn subsidiary_scope(ctx: &BackendContext<'_>)
    -> Result<Option<Vec<op_cozo_store::LxdSubsidiary>>, RuntimeError> {
    let store = ctx.runtime.identity_store().clone();
    let rows = tokio::task::spawn_blocking(move || store.lxd_subsidiaries()).await
        .map_err(|error| RuntimeError::backend(error.into()))?
        .map_err(|error| RuntimeError::backend(error.into()))?;
    let own = &ctx.caller.session_id;
    if !rows.iter().any(|row| &row.session_id == own) { return Ok(None) }
    Ok(Some(rows.into_iter().filter(|row| &row.session_id == own
        || row.actor_id == ctx.caller.principal_id).collect()))
}

/// A subsidiary reads only its own containers and sled volumes. Pool, volume
/// list and operation reads would name other identities, so they are refused.
fn ensure_in_scope(scope: &[op_cozo_store::LxdSubsidiary], method: &str, args: &Value) -> Result<()> {
    let name = args["name"].as_str().unwrap_or_default();
    ensure!(match method {
        "get_images" | "get_image" | "get_image_fingerprints" | "get_instance_names" => true,
        "get_instance" | "get_instance_state" => scope.iter().any(|row| row.name == name),
        "get_storage_pool_volume" => args["pool"] == POOL && args["vol_type"] == "custom"
            && scope.iter().any(|row| name == format!("identity-{}", row.session_id)),
        _ => false,
    }, "a subsidiary sees only its own containers");
    Ok(())
}

async fn require_human(ctx: &BackendContext<'_>) -> Result<(), RuntimeError> {
    let store = ctx.runtime.identity_store().clone();
    let session = ctx.caller.session_id.clone();
    let genesis = ctx.caller.session_genesis.clone();
    let revision = tokio::task::spawn_blocking(move || store.read_identity_revision(&session)).await
        .map_err(|error| RuntimeError::backend(error.into()))?
        .map_err(|error| RuntimeError::backend(error.into()))?;
    if revision.record.is_some_and(|record| record.active
        && record.principal_kind.as_deref() == Some("human") && record.session_genesis == genesis) {
        return Ok(());
    }
    Err(RuntimeError::denied("LXD creation requires a live human identity"))
}

fn object<'a>(value: &'a Value, allowed: &[&str]) -> Result<&'a serde_json::Map<String, Value>> {
    let map = value.as_object().context("LXD request must be an object")?;
    for key in map.keys() {
        ensure!(allowed.contains(&key.as_str()), "LXD request field {key} is not permitted");
    }
    Ok(map)
}

fn validate_instance(args: &Value, caller: &str) -> Result<()> {
    object(args, &["instance"])?;
    let instance = &args["instance"];
    object(instance, &["name", "type", "start", "profiles", "source", "devices", "config"])?;
    let name = instance["name"].as_str().unwrap_or_default();
    ensure!(!name.is_empty() && name.len() <= 63 && name.as_bytes()[0].is_ascii_lowercase()
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
        "invalid subsidiary instance name");
    ensure!(instance["type"] == "container" && instance["start"] == json!(false)
        && instance["profiles"] == json!([]), "a subsidiary is a stopped container without profiles");
    let source = object(&instance["source"], &["type", "fingerprint"])?;
    ensure!(source.get("type") == Some(&json!("image"))
        && source.get("fingerprint").and_then(Value::as_str).is_some_and(hex64),
        "a subsidiary root comes from one local image fingerprint");
    let config = instance["config"].as_object().context("isolation config is absent")?;
    for (key, value) in ISOLATION {
        ensure!(config.get(key).and_then(Value::as_str) == Some(value), "isolation requires {key}={value}");
    }
    for key in ["limits.cpu", "limits.memory", "limits.processes"] {
        ensure!(config.get(key).and_then(Value::as_str).is_some_and(|v| !v.is_empty()), "{key} is required");
    }
    for key in config.keys() {
        ensure!(ISOLATION.iter().any(|(known, _)| known == key)
            || ["limits.cpu", "limits.memory", "limits.processes", "user.opdbus.admission",
                "user.opdbus.session_id", "user.opdbus.wireguard_public_key", "user.opdbus.principal_kind",
                "user.opdbus.parent_principal_id"].contains(&key.as_str()),
            "instance config {key} is not permitted");
    }
    let text = |key: &str| config.get(key).and_then(Value::as_str).unwrap_or_default();
    let session = text("user.opdbus.session_id");
    ensure!(text("user.opdbus.admission") == "pending" && text("user.opdbus.principal_kind") == "service"
        && text("user.opdbus.parent_principal_id") == caller
        && op_identity::session::derive_session_id(text("user.opdbus.wireguard_public_key")) == session,
        "a subsidiary is a pending service identity of its calling parent");
    let devices = instance["devices"].as_array().context("devices are absent")?;
    ensure!(devices.len() == 3, "a subsidiary has exactly its root, identity sled and run directory");
    let root = json!({"name":"root","device":{"type":"disk","path":"/","pool":POOL}});
    let sled = json!({"name":"sled","device":{"type":"disk","path":"/agent/user","pool":POOL,
        "source":format!("identity-{session}"),"readonly":"true"}});
    let run = json!({"name":"run","device":{"type":"disk","path":"/run",
        "source":lxd_subsidiary::run_source(session),"recursive":"true"}});
    ensure!(devices.contains(&root) && devices.contains(&sled) && devices.contains(&run),
        "only the pool root, the read-only identity sled and its own run directory may be attached");
    Ok(())
}

/// Durable parent intent before the request can reach LXD.
async fn reserve(ctx: &BackendContext<'_>, args: &Value) -> Result<(), RuntimeError> {
    let instance = &args["instance"];
    let text = |key: &str| instance["config"][key].as_str().unwrap_or_default().to_owned();
    let row = op_cozo_store::LxdSubsidiary {
        session_id: text("user.opdbus.session_id"),
        name: instance["name"].as_str().unwrap_or_default().to_owned(),
        wireguard_pubkey: text("user.opdbus.wireguard_public_key"),
        actor_id: ctx.caller.principal_id.clone(),
        caller_session: ctx.caller.session_id.clone(),
        caller_genesis: ctx.caller.session_genesis.clone(),
        request_json: instance.to_string(),
        native_uuid: String::new(),
    };
    let store = ctx.runtime.identity_store().clone();
    tokio::task::spawn_blocking(move || store.reserve_lxd_subsidiary(row)).await
        .map_err(|error| RuntimeError::backend(error.into()))?
        .map_err(|error| RuntimeError::denied(error.to_string()))
}

/// Instance name and whether it starts; no other power action is accepted.
fn validate_state(args: &Value) -> Result<(&str, bool)> {
    object(args, &["name", "state"])?;
    object(&args["state"], &["action", "timeout", "force"])?;
    let start = match args["state"]["action"].as_str() {
        Some("start") => true,
        Some("stop") => false,
        _ => anyhow::bail!("only start and stop are permitted"),
    };
    Ok((args["name"].as_str().context("instance name is absent")?, start))
}

/// The recorded parent may start its subsidiary only once its sled identity is
/// admitted and the host has mounted its 3tchedFS; stopping needs neither.
async fn prepare_state(ctx: &BackendContext<'_>, args: &Value) -> Result<(), RuntimeError> {
    let denied = |error: anyhow::Error| RuntimeError::denied(error.to_string());
    let (name, start) = validate_state(args).map_err(denied)?;
    let store = ctx.runtime.identity_store().clone();
    let reader = store.clone();
    let rows = tokio::task::spawn_blocking(move || reader.lxd_subsidiaries()).await
        .map_err(|error| RuntimeError::backend(error.into()))?
        .map_err(|error| RuntimeError::backend(error.into()))?;
    let row = rows.into_iter().find(|row| row.name == name && row.actor_id == ctx.caller.principal_id
        && row.caller_session == ctx.caller.session_id && row.caller_genesis == ctx.caller.session_genesis)
        .ok_or_else(|| RuntimeError::denied("LXD target is not this human's subsidiary"))?;
    if !start { return Ok(()) }
    if lxd_subsidiary::adopt(store.clone(), &row.session_id).await.map_err(denied)? {
        crate::runtime_identity::recover_startup(store.clone(), ctx.runtime.catalog_dir().to_path_buf())
            .await.map_err(RuntimeError::backend)?;
    }
    let session = row.session_id.clone();
    let record = tokio::task::spawn_blocking(move || store.read_identity_revision(&session)).await
        .map_err(|error| RuntimeError::backend(error.into()))?
        .map_err(|error| RuntimeError::backend(error.into()))?.record;
    if !record.is_some_and(|record| !record.sealed_id.is_empty()) {
        return Err(RuntimeError::denied("subsidiary identity is not admitted"));
    }
    let mounts = tokio::fs::read_to_string("/proc/self/mountinfo").await
        .map_err(|error| RuntimeError::backend(error.into()))?;
    let mountpoint = format!("{}/mounts/3tchedFS", lxd_subsidiary::run_source(&row.session_id));
    if !mounts.lines().any(|line| line.split(' ').nth(4) == Some(mountpoint.as_str())
        && line.split_once(" - ").is_some_and(|(_, tail)| tail.starts_with("fuse"))) {
        return Err(RuntimeError::denied("subsidiary 3tchedFS is not mounted on the host"));
    }
    ctx.caller.authority.revalidate().await.map_err(denied)
}

fn validate_volume(args: &Value) -> Result<()> {
    object(args, &["pool", "volume"])?;
    let volume = object(&args["volume"], &["name", "type", "content_type"])?;
    let session = volume.get("name").and_then(Value::as_str)
        .and_then(|name| name.strip_prefix("identity-")).context("only identity sled volumes are created")?;
    ensure!(args["pool"] == POOL && args["volume"]["type"] == "custom"
        && args["volume"]["content_type"] == "filesystem"
        && uuid::Uuid::parse_str(session).is_ok_and(|id| id.to_string() == session),
        "an identity sled is a custom filesystem volume in the subsidiary pool");
    Ok(())
}

/// The staged file name and its encoded image properties.
fn validate_image(args: &Value) -> Result<(String, String)> {
    object(args, &["image"])?;
    object(&args["image"], &["filename", "properties", "public"])?;
    ensure!(args["image"].get("public").is_none_or(|public| public == &json!(false)), "images stay private");
    let filename = args["image"]["filename"].as_str().unwrap_or_default();
    let plain = |text: &str| !text.is_empty() && text.len() <= 128 && !text.starts_with('.')
        && text.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    ensure!(plain(filename), "image filename must be one staged file name");
    let mut properties = Vec::new();
    if let Some(value) = args["image"].get("properties") {
        for (key, value) in value.as_object().context("image properties must be an object")? {
            let value = value.as_str().unwrap_or_default();
            ensure!(plain(key) && plain(value), "image properties must be plain text");
            properties.push(format!("{key}={value}"));
        }
    }
    Ok((filename.to_owned(), properties.join("&")))
}

/// Image bytes come only from the host's root-owned staging directory.
async fn import_image(client: &native_lxd::Client, args: &Value) -> Result<Value, RuntimeError> {
    let (filename, properties) = validate_image(args).map_err(|e| RuntimeError::denied(e.to_string()))?;
    let path = Path::new(IMAGE_DIR).join(&filename);
    let opened = tokio::task::spawn_blocking(move || -> Result<(std::fs::File, u64)> {
        let dir = std::fs::symlink_metadata(IMAGE_DIR)?;
        ensure!(dir.is_dir() && dir.uid() == 0 && dir.mode() & 0o022 == 0, "image staging directory is untrusted");
        let file = std::fs::OpenOptions::new().read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK).open(&path)?;
        let metadata = file.metadata()?;
        ensure!(metadata.is_file() && metadata.uid() == 0 && metadata.nlink() == 1
            && metadata.mode() & 0o022 == 0, "staged image is untrusted");
        Ok((file, metadata.len()))
    }).await.map_err(|error| RuntimeError::backend(error.into()))?.map_err(RuntimeError::backend)?;
    let operation = client.import_local_image(opened.0, opened.1, &filename, &properties).await
        .map_err(|error| RuntimeError::unknown(error.into()))?;
    Ok(json!({"operation": operation}))
}

fn hex64(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
