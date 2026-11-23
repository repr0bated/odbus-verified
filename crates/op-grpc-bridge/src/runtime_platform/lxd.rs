//! Admitted LXD operations for subsidiary containers on this host's daemon.
//!
//! Every catalog capability reaches `$ROOT`, so a grant alone must never turn
//! an LXD payload into host authority. Mutations are fixed to the subsidiary
//! shape and to a live human caller; exec, console, file, config, profile,
//! network and lifecycle methods have no route here.
use crate::runtime::{BackendContext, RuntimeError};
use anyhow::{Context, Result, ensure};
use op_plugins::state_plugins::lxd as native_lxd;
use serde_json::{Value, json};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

const SOCKET: &str = "/var/lib/lxd/unix.socket";
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
    matches!(method, "create_image" | "create_instance" | "create_storage_pool_volume")
}

pub(crate) async fn dispatch(ctx: &BackendContext<'_>, method: &str, args: &Value)
    -> Result<Value, RuntimeError> {
    if mutates(method) {
        require_human(ctx).await?;
        let check = match method {
            "create_instance" => validate_instance(args, &ctx.caller.principal_id),
            "create_storage_pool_volume" => validate_volume(args),
            _ => validate_image(args).map(|_| ()),
        };
        check.map_err(|error| RuntimeError::denied(error.to_string()))?;
        ctx.caller.authority.revalidate().await
            .map_err(|error| RuntimeError::denied(error.to_string()))?;
    }
    let client = native_lxd::Client::local(Path::new(SOCKET))
        .map_err(|error| RuntimeError::backend(error.into()))?;
    if method == "create_image" {
        return import_image(&client, args).await;
    }
    native_lxd::dispatch_lxd_method(&client, method, args).await.map_err(|error| {
        if mutates(method) { RuntimeError::unknown(error.into()) }
        else { RuntimeError::backend(error.into()) }
    })
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
    ensure!(devices.len() == 2, "a subsidiary has exactly its root and identity sled");
    let root = json!({"name":"root","device":{"type":"disk","path":"/","pool":POOL}});
    let sled = json!({"name":"sled","device":{"type":"disk","path":"/agent/user","pool":POOL,
        "source":format!("identity-{session}"),"readonly":"true"}});
    ensure!(devices.contains(&root) && devices.contains(&sled),
        "only the pool root and the read-only identity sled may be attached");
    Ok(())
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
