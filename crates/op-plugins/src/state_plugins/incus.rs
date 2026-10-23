//! Incus state plugin - manages Incus containers and virtual machines.
//!
//! Uses the native Incus HTTP API over its privileged Unix socket.
//! Supports creating, starting, stopping, and deleting instances,
//! as well as profile and config management.

use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use op_state::{
    ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateAction, StateDiff, StatePlugin,
};
use op_state_store::PluginSchema;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use simd_json::{json, prelude::*, OwnedValue as Value};
use std::collections::{BTreeMap, HashMap};
use std::time::Duration;
use tokio::net::UnixStream;
use tokio_tungstenite::{
    tungstenite::{protocol::WebSocketConfig, Message},
    WebSocketStream,
};

use super::incus_device::{Device, NamedDevice};

/// Top-level state representing all Incus instances on the system.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(extend("x-oscal-category" = "service"))]
pub struct IncusState {
    pub instances: Vec<IncusInstance>,
}

/// A single Incus instance, modeled on the official Incus API (`shared/api`
/// `Instance`/`InstancePut`).
///
/// Devices (including proxy sockets) are the typed [`NamedDevice`] union — Incus
/// has no first-class "socket" concept, a proxy is simply a `devices` entry with
/// `type: proxy`. The previous non-standard `sockets`/`IncusProxySocket` field is
/// gone; a container's relationship to the shared `container.sock` is resolved
/// by name at the projection/gemma layer, not embedded here.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct IncusInstance {
    pub name: String,
    /// Instance status: "Running", "Stopped", "Frozen"
    pub status: String,
    /// Instance type: "container" or "virtual-machine"
    #[serde(rename = "type")]
    pub instance_type: String,
    /// Numeric status code from the Incus API (read-only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_code: Option<i64>,
    /// Source image reference, used only at creation time (`incus init <image>`).
    /// Not part of the official `InstancePut`; derived from config on read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// Preferred storage pool used during initial creation (creation hint only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_pool: Option<String>,
    /// Applied profiles (e.g. ["default"])
    #[serde(default)]
    pub profiles: Vec<String>,
    /// Human-readable description
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// CPU architecture (e.g. "x86_64")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub architecture: Option<String>,
    /// Delete instance on shutdown
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ephemeral: Option<bool>,
    /// Whether saved state exists on disk
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stateful: Option<bool>,
    /// Creation timestamp (ISO8601)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// Last start timestamp (ISO8601)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<String>,
    /// Cluster member location ("none" on single-node)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// Incus project name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// Instance configuration key-value pairs (`InstancePut.config`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<HashMap<String, String>>,
    /// Typed device definitions (`InstancePut.devices`). Proxy sockets, NICs,
    /// disks, GPUs, etc. are all variants of [`Device`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub devices: Vec<NamedDevice>,
    /// Expanded (profile-merged) config, read-only (`Instance.expanded_config`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expanded_config: Option<HashMap<String, String>>,
    /// Expanded (profile-merged) devices, read-only (`Instance.expanded_devices`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Vec<NamedDevice>")]
    pub expanded_devices: Option<Vec<NamedDevice>>,
}

/// Intermediate struct for deserializing raw `incus list --format=json` output.
#[derive(Debug, Deserialize)]
struct RawIncusInstance {
    name: String,
    status: String,
    #[serde(rename = "type")]
    instance_type: String,
    status_code: i64,
    profiles: Vec<String>,
    config: HashMap<String, String>,
    devices: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(default)]
    expanded_config: HashMap<String, String>,
    #[serde(default)]
    expanded_devices: Option<BTreeMap<String, BTreeMap<String, String>>>,
    description: String,
    architecture: String,
    ephemeral: bool,
    stateful: bool,
    created_at: String,
    last_used_at: String,
    location: String,
    #[serde(default)]
    project: String,
}

/// Convert an Incus device map (`name -> {type, …}` all-string) into the typed
/// [`NamedDevice`] list, sorted by name for determinism. Reject unknown types
/// and keys rather than publishing an incomplete provider observation.
fn named_devices_from_map(
    map: &BTreeMap<String, BTreeMap<String, String>>,
) -> Result<Vec<NamedDevice>> {
    let mut out: Vec<NamedDevice> = map
        .iter()
        .map(|(name, cfg)| {
            let device = Device::from_incus_map(cfg)
                .with_context(|| format!("unsupported Incus device '{name}'"))?;
            anyhow::ensure!(
                device.to_incus_map() == *cfg,
                "Incus device '{name}' contains fields the sealed typed contract cannot preserve"
            );
            Ok(NamedDevice {
                name: name.clone(),
                device,
            })
        })
        .collect::<Result<_>>()?;
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

pub struct IncusPlugin;

/// Power states accepted by the identity-session lifecycle.
///
/// This deliberately exposes only the two transitions needed by parked
/// identity containers.  Provisioning remains an `incus init` (stopped), and
/// authenticated session activation/deactivation owns the start/stop calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentitySessionPowerState {
    Running,
    Stopped,
}

impl IncusPlugin {
    pub fn new() -> Self {
        Self
    }

    #[cfg(test)]
    async fn incus_request_on_socket(
        socket: &std::path::Path,
        deadline: Duration,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> Result<Vec<u8>> {
        Ok(
            Self::incus_response_on_socket(socket, deadline, method, path, body, None)
                .await?
                .body,
        )
    }

    async fn incus_response_on_socket(
        socket: &std::path::Path,
        deadline: Duration,
        method: &str,
        path: &str,
        body: Option<&str>,
        etag: Option<&str>,
    ) -> Result<IncusHttpResponse> {
        const RESPONSE_LIMIT: usize = 16 * 1024 * 1024;
        if !(path == "/1.0"
            || path.starts_with("/1.0/")
            || path.starts_with("/1.0?")
            || matches!(
                path,
                "/internal/recover/validate" | "/internal/recover/import"
            ))
            || path.chars().any(|c| c.is_whitespace() || c.is_control())
            || path.contains(['#', '\\'])
            || path.split('?').next().is_some_and(|p| {
                p.split('/')
                    .any(|s| s == "." || s == ".." || s.contains('%'))
            })
        {
            anyhow::bail!("invalid local Incus API path");
        }
        let client = reqwest_unix::Client::builder()
            .unix_socket(socket.to_path_buf())
            .no_proxy()
            .redirect(reqwest_unix::redirect::Policy::none())
            .retry(reqwest_unix::retry::never())
            .connect_timeout(Duration::from_secs(3))
            .timeout(deadline)
            .build()
            .context("failed to build Incus UDS client")?;
        let mut request = client
            .request(method.parse()?, format!("http://localhost{path}"))
            .header("Accept", "application/json");
        if let Some(body) = body {
            request = request
                .header("Content-Type", "application/json")
                .body(body.to_owned());
        }
        if let Some(etag) = etag {
            request = request.header("If-Match", etag);
        }
        let mut response = request.send().await.context("Incus UDS request failed")?;
        let status = response.status();
        let etag = response
            .headers()
            .get("ETag")
            .map(|value| value.to_str().map(str::to_owned))
            .transpose()
            .context("invalid Incus ETag header")?;
        if response
            .content_length()
            .is_some_and(|length| length > RESPONSE_LIMIT as u64)
        {
            anyhow::bail!("Incus response exceeded size limit");
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .context("incomplete Incus response")?
        {
            if chunk.len() > RESPONSE_LIMIT.saturating_sub(bytes.len()) {
                anyhow::bail!("Incus response exceeded size limit");
            }
            bytes.extend_from_slice(&chunk);
        }
        if !status.is_success() {
            let error = serde_json::from_slice::<serde_json::Value>(&bytes)
                .ok()
                .and_then(|value| {
                    value
                        .get("error")
                        .and_then(|v| v.as_str())
                        .map(str::to_owned)
                })
                .unwrap_or_default();
            return Err(IncusHttpStatusError {
                status: status.as_u16(),
                detail: error,
            }
            .into());
        }
        Ok(IncusHttpResponse { body: bytes, etag })
    }

    /// Call Incus REST API and extract metadata from the (sync or async)
    /// response. Incus answers most mutating calls (instance create/delete,
    /// etc.) with `"type": "async"` — a background operation, not the final
    /// result — whose completion must be awaited via `/1.0/operations/<id>/wait` before
    /// its outcome is known. Treating that initial "Operation created"
    /// response as the final answer (as this used to) misreads a real
    /// success as a blank error, since its own `error` field is `""`.
    async fn incus_api_call(method: &str, path: &str, body: Option<&str>) -> Result<Vec<u8>> {
        let value = IncusApi::local().call(method, path, body, None).await?;
        Ok(serde_json::to_vec(&value)?)
    }

    fn identity_session_power_request(
        session_id: &str,
        state: IdentitySessionPowerState,
    ) -> Result<(String, &'static str)> {
        let parsed = uuid::Uuid::parse_str(session_id)
            .with_context(|| format!("invalid identity session id '{session_id}'"))?;
        if parsed.to_string() != session_id {
            anyhow::bail!("identity session id must be a canonical lowercase UUID");
        }
        let body = match state {
            IdentitySessionPowerState::Running => r#"{"action":"start"}"#,
            IdentitySessionPowerState::Stopped => r#"{"action":"stop"}"#,
        };
        Ok((format!("/1.0/instances/{session_id}/state"), body))
    }

    fn bound_identity_session_power_request(
        session_id: &str,
        instance_name: &str,
        state: IdentitySessionPowerState,
    ) -> Result<(String, &'static str)> {
        let (_, body) = Self::identity_session_power_request(session_id, state)?;
        if instance_name != session_id
            && !(instance_name == "3tchedChatbot"
                && session_id == "b0c777e3-0955-cd8b-3d01-ca12998cae28")
        {
            anyhow::bail!("native identity power target is not canonical or the pinned chatbot");
        }
        Ok((format!("/1.0/instances/{instance_name}/state"), body))
    }

    /// The bridge supplies an already validated durable identity binding.
    /// This transport admits only UUID names and the fixed chatbot pair; it
    /// does not resolve identities, assign names, or modify their authority.
    pub async fn set_bound_identity_session_power_state(
        session_id: &str,
        instance_name: &str,
        state: IdentitySessionPowerState,
    ) -> Result<()> {
        let (path, body) =
            Self::bound_identity_session_power_request(session_id, instance_name, state)?;
        Self::incus_api_call("PUT", &path, Some(body))
            .await
            .with_context(|| {
                format!("failed to change bound identity container '{instance_name}' power state")
            })?;
        Ok(())
    }

    /// Change the power state of an identity-session container through the
    /// native Incus REST API. Identity container names are canonical UUIDs,
    /// so validating that invariant also prevents request-path injection.
    pub async fn set_identity_session_power_state(
        session_id: &str,
        state: IdentitySessionPowerState,
    ) -> Result<()> {
        let (path, body) = Self::identity_session_power_request(session_id, state)?;
        Self::incus_api_call("PUT", &path, Some(body))
            .await
            .with_context(|| {
                format!("failed to change identity container '{session_id}' power state")
            })?;
        Ok(())
    }

    /// Fetch current instance configuration via REST API
    async fn incus_get_instance(name: &str) -> Result<simd_json::OwnedValue> {
        let body =
            Self::incus_api_call("GET", &format!("/1.0/instances/{}?recursion=1", name), None)
                .await?;
        let mut raw = body;
        simd_json::from_slice(&mut raw).context("Failed to parse Incus instance")
    }

    /// Provision the project plugin's restricted container in one Incus
    /// operation. Image/pool/path are trusted deployment policy, not chat input.
    pub async fn create_project_container(
        project_id: &str,
        owner_session_id: &str,
        fingerprint: &str,
        pool: &str,
        persist_path: &str,
    ) -> Result<()> {
        if uuid::Uuid::parse_str(project_id)?.to_string() != project_id
            || fingerprint.len() != 64
            || !fingerprint.bytes().all(|c| c.is_ascii_hexdigit())
            || pool.is_empty()
            || !std::path::Path::new(persist_path).is_absolute()
        {
            anyhow::bail!("invalid project provisioning policy");
        }
        let body = serde_json::json!({
            "name": project_id, "type": "container", "ephemeral": false,
            "source": {"type": "image", "fingerprint": fingerprint},
            "profiles": [],
            "config": {
                "boot.autostart": "false", "security.privileged": "false", "security.nesting": "false",
                "user.opdbus.role": "project", "user.opdbus.project_id": project_id,
                "user.opdbus.owner_session_id": owner_session_id
            },
            "devices": {
                "root": {"type": "disk", "path": "/", "pool": pool},
                // Only the authenticated project dispatcher writes this Cozo
                // store. A container must not alter host database files or
                // introduce symlinks underneath a privileged store handle.
                "project": {"type": "disk", "path": "/project", "source": persist_path, "shift": "true", "readonly": "true"}
            }
        });
        Self::incus_api_call("POST", "/1.0/instances", Some(&body.to_string())).await?;
        Self::verify_project_container(project_id, owner_session_id, persist_path).await
    }

    /// Existence and ownership are checked before project data access, including
    /// parked containers. A stale catalog row is not a substitute for a container.
    pub async fn verify_project_container(
        project_id: &str,
        owner: &str,
        persist_path: &str,
    ) -> Result<()> {
        if uuid::Uuid::parse_str(project_id)?.to_string() != project_id {
            anyhow::bail!("invalid project container id");
        }
        let value = serde_json::to_value(Self::incus_get_instance(project_id).await?)?;
        if value["type"] != "container"
            || value["config"]["user.opdbus.project_id"] != project_id
            || value["config"]["user.opdbus.owner_session_id"] != owner
            || value["config"]["user.opdbus.role"] != "project"
            || value["devices"]["project"]["source"] != persist_path
            || value["devices"]["project"]["path"] != "/project"
            || value["devices"]["project"]["readonly"] != "true"
            || value["config"]["security.privileged"] != "false"
            || value["config"]["security.nesting"] != "false"
        {
            anyhow::bail!("project container binding does not match its catalog");
        }
        Ok(())
    }

    /// Update instance configuration via REST API (removes read-only fields first)
    async fn incus_update_instance(name: &str, mut config: simd_json::OwnedValue) -> Result<()> {
        if let Some(obj) = config.as_object_mut() {
            obj.retain(|k, _| !k.starts_with("volatile.") && !k.starts_with("image."));
        }
        let body = simd_json::to_string(&config)?;
        Self::incus_api_call("PUT", &format!("/1.0/instances/{}", name), Some(&body)).await?;
        Ok(())
    }

    /// Run an incus operation via REST API (AGENTS.md §4: no subprocess bypasses)
    async fn run_incus_command(args: &[&str]) -> Result<Vec<u8>> {
        match args {
            ["list", "--format=json"] => {
                Self::incus_api_call("GET", "/1.0/instances?recursion=1", None).await
            }
            ["init", ..] => {
                let mut image = "";
                let mut name = "";
                let mut storage_pool = None;
                let mut profiles: Vec<String> = Vec::new();
                let mut no_profiles = false;
                let mut instance_type = "container";
                let mut i = 1;
                while i < args.len() {
                    match args[i] {
                        "--storage" => {
                            i += 1;
                            storage_pool = args.get(i).copied().map(String::from);
                        }
                        "--no-profiles" => {
                            no_profiles = true;
                        }
                        "--vm" => {
                            instance_type = "virtual-machine";
                        }
                        "--profile" => {
                            i += 1;
                            if let Some(p) = args.get(i) {
                                profiles.push(p.to_string());
                            }
                        }
                        arg if arg.starts_with('-') => {}
                        arg if image.is_empty() => image = arg,
                        arg if name.is_empty() => name = arg,
                        _ => {}
                    }
                    i += 1;
                }

                let mut body = simd_json::json!({
                    "name": name,
                    "source": { "type": "image", "alias": image },
                    "type": instance_type,
                });
                if no_profiles {
                    body.insert("profiles", Value::Array(Vec::new()))?;
                } else if !profiles.is_empty() {
                    let profile_values: Vec<Value> =
                        profiles.iter().map(|p| Value::from(p.as_str())).collect();
                    body.insert("profiles", Value::Array(profile_values))?;
                }
                if let Some(pool) = storage_pool {
                    body.insert(
                        "devices",
                        simd_json::json!({
                            "root": {
                                "type": "disk",
                                "pool": pool,
                                "path": "/"
                            }
                        }),
                    )?;
                }
                let body_str = simd_json::to_string(&body)?;
                Self::incus_api_call("POST", "/1.0/instances", Some(&body_str)).await
            }
            ["delete", name, "--force"] => {
                Ok(serde_json::to_vec(&IncusApi::local().delete(name).await?)?)
            }
            ["start", name] => {
                let body = r#"{"action":"start"}"#;
                Self::incus_api_call("PUT", &format!("/1.0/instances/{}/state", name), Some(body))
                    .await
            }
            ["stop", name] => {
                let body = r#"{"action":"stop"}"#;
                Self::incus_api_call("PUT", &format!("/1.0/instances/{}/state", name), Some(body))
                    .await
            }
            ["stop", name, "--force"] => {
                let body = r#"{"action":"stop","force":true}"#;
                Self::incus_api_call("PUT", &format!("/1.0/instances/{}/state", name), Some(body))
                    .await
            }
            ["pause", name] => {
                let body = r#"{"action":"freeze"}"#;
                Self::incus_api_call("PUT", &format!("/1.0/instances/{}/state", name), Some(body))
                    .await
            }
            ["restart", name] => {
                let body = r#"{"action":"restart"}"#;
                Self::incus_api_call("PUT", &format!("/1.0/instances/{}/state", name), Some(body))
                    .await
            }
            ["restart", name, "--force"] => {
                let body = r#"{"action":"restart","force":true}"#;
                Self::incus_api_call("PUT", &format!("/1.0/instances/{}/state", name), Some(body))
                    .await
            }
            ["unpause", name] => {
                let body = r#"{"action":"unfreeze"}"#;
                Self::incus_api_call("PUT", &format!("/1.0/instances/{}/state", name), Some(body))
                    .await
            }
            ["snapshot", name, snapshot] => {
                let body = simd_json::json!({"name": snapshot});
                Self::incus_api_call(
                    "POST",
                    &format!("/1.0/instances/{}/snapshots", name),
                    Some(&body.to_string()),
                )
                .await
            }
            ["exec", name, command @ ..] if !command.is_empty() => {
                let body = simd_json::json!({
                    "command": command,
                    "wait-for-websocket": false,
                    "interactive": false
                });
                Self::incus_api_call(
                    "POST",
                    &format!("/1.0/instances/{}/exec", name),
                    Some(&body.to_string()),
                )
                .await
            }
            ["profile", "remove", name, profile] => {
                let mut data = Self::incus_get_instance(name).await?;
                if let Some(profiles_arr) = data.get_mut("profiles").and_then(|p| p.as_array_mut())
                {
                    profiles_arr.retain(|p| p.as_str() != Some(profile));
                }
                Self::incus_update_instance(name, data).await?;
                Ok(Vec::new())
            }
            ["profile", "add", name, profile] => {
                let mut data = Self::incus_get_instance(name).await?;
                if data.get("profiles").is_none() {
                    if let Some(obj) = data.as_object_mut() {
                        obj.insert("profiles".to_string(), simd_json::json!([]));
                    }
                }
                let profiles = data
                    .get_mut("profiles")
                    .and_then(|p| p.as_array_mut())
                    .ok_or_else(|| anyhow::anyhow!("profiles field is not an array"))?;
                if !profiles.iter().any(|p| p.as_str() == Some(profile)) {
                    profiles.push(simd_json::json!(profile));
                }
                Self::incus_update_instance(name, data).await?;
                Ok(Vec::new())
            }
            ["config", "unset", name, key] => {
                let mut data = Self::incus_get_instance(name).await?;
                if let Some(config) = data.get_mut("config").and_then(|c| c.as_object_mut()) {
                    config.remove(*key);
                }
                Self::incus_update_instance(name, data).await?;
                Ok(Vec::new())
            }
            ["config", "set", name, kv] => {
                let mut data = Self::incus_get_instance(name).await?;
                if let Some((key, value)) = kv.split_once('=') {
                    if let Some(config) = data.get_mut("config").and_then(|c| c.as_object_mut()) {
                        config.insert(key.to_string(), simd_json::json!(value));
                    } else if let Some(obj) = data.as_object_mut() {
                        let mut config = HashMap::new();
                        config.insert(key.to_string(), simd_json::json!(value));
                        obj.insert("config".to_string(), simd_json::json!(config));
                    }
                }
                Self::incus_update_instance(name, data).await?;
                Ok(Vec::new())
            }
            ["config", "device", "remove", name, device] => {
                let mut data = Self::incus_get_instance(name).await?;
                if let Some(devices) = data.get_mut("devices").and_then(|d| d.as_object_mut()) {
                    devices.remove(*device);
                }
                Self::incus_update_instance(name, data).await?;
                Ok(Vec::new())
            }
            ["config", "device", "add", name, device, dev_type, rest @ ..] => {
                let mut data = Self::incus_get_instance(name).await?;
                let mut dev_config = HashMap::new();
                dev_config.insert("type".to_string(), simd_json::json!(dev_type));
                for arg in rest.iter() {
                    if let Some((k, v)) = arg.split_once('=') {
                        dev_config.insert(k.to_string(), simd_json::json!(v));
                    }
                }
                if let Some(devices) = data.get_mut("devices").and_then(|d| d.as_object_mut()) {
                    devices.insert(device.to_string(), simd_json::json!(dev_config));
                } else if let Some(obj) = data.as_object_mut() {
                    let mut devices = HashMap::new();
                    devices.insert(device.to_string(), simd_json::json!(dev_config));
                    obj.insert("devices".to_string(), simd_json::json!(devices));
                }
                Self::incus_update_instance(name, data).await?;
                Ok(Vec::new())
            }
            _ => Err(anyhow::anyhow!("Unmapped incus CLI args: {:?}", args)),
        }
    }

    /// Parse raw JSON output from `incus list --format=json` into IncusInstance structs.
    fn parse_instance_list(mut raw_json: Vec<u8>) -> Result<Vec<IncusInstance>> {
        let raw_instances: Vec<RawIncusInstance> =
            simd_json::from_slice(&mut raw_json).context("Failed to parse incus list JSON")?;

        Self::convert_instance_list(raw_instances)
    }

    /// Normalize one official native Instance document through the same
    /// conversion used by complete Incus observations. Native device maps
    /// become the sealed contract's typed NamedDevice lists without loss.
    pub fn instance_from_native_metadata(metadata: serde_json::Value) -> Result<IncusInstance> {
        let mut raw: RawIncusInstance =
            serde_json::from_value(metadata).context("invalid native Incus instance metadata")?;
        // A GET through the fixed default-project instance path can omit this
        // field on older native responses. It cannot select another project.
        if raw.project.is_empty() {
            raw.project = "default".into();
        }
        Self::convert_instance_list(vec![raw])?
            .pop()
            .context("native Incus instance conversion returned no instance")
    }

    fn convert_instance_list(raw_instances: Vec<RawIncusInstance>) -> Result<Vec<IncusInstance>> {
        let mut instances = raw_instances
            .into_iter()
            .map(|raw| -> Result<IncusInstance> {
                anyhow::ensure!(
                    raw.project == "default",
                    "default-project Incus snapshot contained project '{}'",
                    raw.project
                );
                anyhow::ensure!(
                    !raw.name.is_empty()
                        && !raw.status.is_empty()
                        && raw.status_code > 0
                        && matches!(raw.instance_type.as_str(), "container" | "virtual-machine"),
                    "Incus snapshot contains an invalid required instance identity"
                );

                let storage_pool = raw
                    .devices
                    .get("root")
                    .and_then(|root| root.get("pool"))
                    .cloned();
                // Extract image description from config keys
                let image = raw
                    .config
                    .get("image.description")
                    .or_else(|| raw.config.get("volatile.base_image"))
                    .cloned();

                // Preserve required provider fields even when their values are
                // empty. This root snapshot is the authoritative observation,
                // so absence and an explicitly empty value are not conflated.
                let config = Some(raw.config);
                let expanded_config = Some(raw.expanded_config);

                let devices = named_devices_from_map(&raw.devices)?;
                let expanded_devices = raw.expanded_devices
                    .as_ref().map(named_devices_from_map).transpose()?;

                Ok(IncusInstance {
                    name: raw.name,
                    status: raw.status,
                    instance_type: raw.instance_type,
                    status_code: Some(raw.status_code),
                    image,
                    storage_pool,
                    profiles: raw.profiles,
                    description: Some(raw.description),
                    architecture: Some(raw.architecture),
                    ephemeral: Some(raw.ephemeral),
                    stateful: Some(raw.stateful),
                    created_at: Some(raw.created_at),
                    last_used_at: Some(raw.last_used_at),
                    location: Some(raw.location),
                    project: Some(raw.project),
                    config,
                    devices,
                    expanded_config,
                    expanded_devices,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        instances.sort_by(|left, right| left.name.cmp(&right.name));
        for pair in instances.windows(2) {
            anyhow::ensure!(
                pair[0].name != pair[1].name,
                "duplicate instance '{}' in default-project Incus snapshot",
                pair[0].name
            );
        }
        Ok(instances)
    }

    /// Apply a single Create action for an instance.
    pub async fn apply_create(instance: &IncusInstance) -> Result<Vec<String>> {
        let mut changes = Vec::new();
        let name = &instance.name;

        // Determine the image to use; fall back to a sensible default
        let image = instance.image.as_deref().unwrap_or("images:debian/12");

        let mut create_args = vec!["init".to_string(), image.to_string(), name.to_string()];
        if let Some(pool) = instance.storage_pool.as_deref() {
            create_args.push("--storage".to_string());
            create_args.push(pool.to_string());
        }
        if instance.profiles.is_empty() {
            create_args.push("--no-profiles".to_string());
        } else {
            for profile in Self::normalize_profiles(&instance.profiles) {
                create_args.push("--profile".to_string());
                create_args.push(profile);
            }
        }
        if instance.instance_type == "virtual-machine" {
            create_args.push("--vm".to_string());
        }
        let create_args_ref: Vec<&str> = create_args.iter().map(String::as_str).collect();
        log::info!("Creating instance '{}' from image '{}'", name, image);
        Self::run_incus_command(&create_args_ref)
            .await
            .with_context(|| format!("Failed to create instance '{}'", name))?;
        changes.push(format!("Created instance '{}'", name));

        changes.extend(Self::sync_profiles(name, None, instance).await?);
        changes.extend(Self::sync_config(name, None, instance).await?);
        changes.extend(Self::sync_devices(name, None, instance).await?);
        changes.extend(Self::sync_status(name, None, instance).await?);

        Ok(changes)
    }

    async fn apply_modify(current: &IncusInstance, desired: &IncusInstance) -> Result<Vec<String>> {
        let mut changes = Vec::new();
        changes.extend(Self::sync_profiles(&desired.name, Some(current), desired).await?);
        changes.extend(Self::sync_config(&desired.name, Some(current), desired).await?);
        changes.extend(Self::sync_devices(&desired.name, Some(current), desired).await?);
        changes.extend(Self::sync_status(&desired.name, Some(current), desired).await?);
        Ok(changes)
    }

    /// Apply a single Delete action.
    async fn apply_delete(name: &str) -> Result<Vec<String>> {
        log::info!("Force-deleting instance '{}'", name);
        Self::run_incus_command(&["delete", name, "--force"])
            .await
            .with_context(|| format!("Failed to delete instance '{}'", name))?;
        Ok(vec![format!("Deleted instance '{}'", name)])
    }

    fn is_read_only_config_key(key: &str) -> bool {
        key.starts_with("volatile.") || key.starts_with("image.")
    }

    fn normalize_profiles(profiles: &[String]) -> Vec<String> {
        let mut normalized = profiles.to_vec();
        normalized.sort();
        normalized.dedup();
        normalized
    }

    fn normalized_config(instance: &IncusInstance) -> HashMap<String, String> {
        instance
            .config
            .clone()
            .unwrap_or_default()
            .into_iter()
            .filter(|(key, _)| !Self::is_read_only_config_key(key))
            .collect()
    }

    /// Managed devices keyed by name, each as an Incus string map, excluding the
    /// `root` disk (managed implicitly via `storage_pool` at creation).
    fn managed_devices(instance: &IncusInstance) -> HashMap<String, BTreeMap<String, String>> {
        instance
            .devices
            .iter()
            .filter(|nd| nd.name != "root")
            .map(|nd| (nd.name.clone(), nd.device.to_incus_map()))
            .collect()
    }

    fn instances_equivalent(current: &IncusInstance, desired: &IncusInstance) -> bool {
        current.status == desired.status
            && current.instance_type == desired.instance_type
            && current.storage_pool == desired.storage_pool
            && Self::normalize_profiles(&current.profiles)
                == Self::normalize_profiles(&desired.profiles)
            && Self::normalized_config(current) == Self::normalized_config(desired)
            && Self::managed_devices(current) == Self::managed_devices(desired)
    }

    async fn sync_profiles(
        name: &str,
        current: Option<&IncusInstance>,
        desired: &IncusInstance,
    ) -> Result<Vec<String>> {
        let current_profiles = current
            .map(|instance| Self::normalize_profiles(&instance.profiles))
            .unwrap_or_default();
        let desired_profiles = Self::normalize_profiles(&desired.profiles);
        let mut changes = Vec::new();

        for profile in &current_profiles {
            if desired_profiles.contains(profile) {
                continue;
            }
            Self::run_incus_command(&["profile", "remove", name, profile])
                .await
                .with_context(|| {
                    format!("Failed to remove profile '{}' from '{}'", profile, name)
                })?;
            changes.push(format!("Removed profile '{}' from '{}'", profile, name));
        }

        for profile in &desired_profiles {
            if current_profiles.contains(profile) {
                continue;
            }
            Self::run_incus_command(&["profile", "add", name, profile])
                .await
                .with_context(|| format!("Failed to add profile '{}' to '{}'", profile, name))?;
            changes.push(format!("Added profile '{}' to '{}'", profile, name));
        }

        Ok(changes)
    }

    async fn sync_config(
        name: &str,
        current: Option<&IncusInstance>,
        desired: &IncusInstance,
    ) -> Result<Vec<String>> {
        let current_config = current.map(Self::normalized_config).unwrap_or_default();
        let desired_config = Self::normalized_config(desired);
        let mut changes = Vec::new();

        for key in current_config.keys() {
            if !desired_config.contains_key(key) {
                Self::run_incus_command(&["config", "unset", name, key])
                    .await
                    .with_context(|| format!("Failed to unset config '{}' on '{}'", key, name))?;
                changes.push(format!("Unset config '{}' on '{}'", key, name));
            }
        }

        for (key, value) in desired_config {
            if current_config.get(&key) == Some(&value) {
                continue;
            }
            let kv = format!("{}={}", key, value);
            Self::run_incus_command(&["config", "set", name, &kv])
                .await
                .with_context(|| format!("Failed to set config '{}' on '{}'", kv, name))?;
            changes.push(format!("Set config '{}' on '{}'", kv, name));
        }

        Ok(changes)
    }

    async fn sync_devices(
        name: &str,
        current: Option<&IncusInstance>,
        desired: &IncusInstance,
    ) -> Result<Vec<String>> {
        let current_devices = current.map(Self::managed_devices).unwrap_or_default();
        let desired_devices = Self::managed_devices(desired);
        let mut changes = Vec::new();

        for device_name in current_devices.keys() {
            if desired_devices.contains_key(device_name) {
                continue;
            }
            Self::run_incus_command(&["config", "device", "remove", name, device_name])
                .await
                .with_context(|| {
                    format!(
                        "Failed to remove stale device '{}' from '{}'",
                        device_name, name
                    )
                })?;
            changes.push(format!(
                "Removed stale device '{}' from '{}'",
                device_name, name
            ));
        }

        for (device_name, desired_device) in desired_devices {
            if current_devices.get(&device_name) == Some(&desired_device) {
                continue;
            }

            if current_devices.contains_key(&device_name) {
                Self::run_incus_command(&["config", "device", "remove", name, &device_name])
                    .await
                    .with_context(|| {
                        format!("Failed to remove device '{}' from '{}'", device_name, name)
                    })?;
                changes.push(format!("Removed device '{}' from '{}'", device_name, name));
            }

            let device_type = desired_device
                .get("type")
                .cloned()
                .context("Incus device definition is missing required 'type'")?;
            let mut add_args = vec![
                "config".to_string(),
                "device".to_string(),
                "add".to_string(),
                name.to_string(),
                device_name.clone(),
                device_type,
            ];
            for (key, value) in desired_device {
                if key == "type" {
                    continue;
                }
                add_args.push(format!("{}={}", key, value));
            }
            let add_args_ref: Vec<&str> = add_args.iter().map(String::as_str).collect();
            Self::run_incus_command(&add_args_ref)
                .await
                .with_context(|| format!("Failed to add device '{}' to '{}'", device_name, name))?;
            changes.push(format!("Configured device '{}' on '{}'", device_name, name));
        }

        Ok(changes)
    }

    async fn sync_status(
        name: &str,
        current: Option<&IncusInstance>,
        desired: &IncusInstance,
    ) -> Result<Vec<String>> {
        let mut changes = Vec::new();
        // `incus init` creates a stopped instance. Avoid issuing a redundant
        // stop operation for provisioned/parked containers; they must remain
        // stopped until an authenticated session activates them.
        if current.is_none() && desired.status == "Stopped" {
            return Ok(changes);
        }
        if current.map(|instance| instance.status.as_str()) == Some(desired.status.as_str()) {
            return Ok(changes);
        }
        match desired.status.as_str() {
            "Running" => {
                Self::run_incus_command(&["start", name])
                    .await
                    .with_context(|| format!("Failed to start instance '{}'", name))?;
                changes.push(format!("Started instance '{}'", name));
            }
            "Stopped" => {
                Self::run_incus_command(&["stop", name])
                    .await
                    .with_context(|| format!("Failed to stop instance '{}'", name))?;
                changes.push(format!("Stopped instance '{}'", name));
            }
            "Frozen" => {
                Self::run_incus_command(&["pause", name])
                    .await
                    .with_context(|| format!("Failed to freeze instance '{}'", name))?;
                changes.push(format!("Frozen instance '{}'", name));
            }
            other => anyhow::bail!(
                "Unsupported desired status '{}' for instance '{}'",
                other,
                name
            ),
        }
        Ok(changes)
    }
}

impl Default for IncusPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StatePlugin for IncusPlugin {
    fn name(&self) -> &str {
        "incus"
    }

    fn version(&self) -> &str {
        "3.0.0"
    }

    fn schema(&self) -> Option<op_state_store::PluginSchema> {
        Some(incus_schema())
    }

    fn is_available(&self) -> bool {
        std::path::Path::new("/usr/bin/incus").exists()
    }

    fn unavailable_reason(&self) -> String {
        "Incus not installed (/usr/bin/incus not found)".to_string()
    }

    async fn calculate_diff(&self, current: &Value, desired: &Value) -> Result<StateDiff> {
        let current_state: IncusState = simd_json::serde::from_owned_value(current.clone())
            .context("Failed to deserialize current IncusState")?;
        let desired_state: IncusState = simd_json::serde::from_owned_value(desired.clone())
            .context("Failed to deserialize desired IncusState")?;

        // Index current instances by name for O(1) lookups
        let current_by_name: HashMap<&str, &IncusInstance> = current_state
            .instances
            .iter()
            .map(|i| (i.name.as_str(), i))
            .collect();

        let desired_by_name: HashMap<&str, &IncusInstance> = desired_state
            .instances
            .iter()
            .map(|i| (i.name.as_str(), i))
            .collect();

        let mut actions = Vec::new();

        // Check desired instances against current state
        for desired_inst in &desired_state.instances {
            match current_by_name.get(desired_inst.name.as_str()) {
                None => {
                    // Instance does not exist yet -- needs creation
                    let config = simd_json::serde::to_owned_value(desired_inst.clone())
                        .context("Failed to serialize desired instance for Create action")?;
                    actions.push(StateAction::Create {
                        resource: desired_inst.name.clone(),
                        config,
                    });
                }
                Some(current_inst) => {
                    if !Self::instances_equivalent(current_inst, desired_inst) {
                        let changes = simd_json::serde::to_owned_value(desired_inst.clone())
                            .context("Failed to serialize desired instance for Modify action")?;
                        actions.push(StateAction::Modify {
                            resource: desired_inst.name.clone(),
                            changes,
                        });
                    }
                }
            }
        }

        // Instances in current but not in desired should be deleted
        for current_inst in &current_state.instances {
            if !desired_by_name.contains_key(current_inst.name.as_str()) {
                actions.push(StateAction::Delete {
                    resource: current_inst.name.clone(),
                });
            }
        }

        let current_hash = format!("{:x}", md5::compute(simd_json::to_string(current)?));
        let desired_hash = format!("{:x}", md5::compute(simd_json::to_string(desired)?));

        Ok(StateDiff {
            plugin: self.name().to_string(),
            actions,
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash,
                desired_hash,
            },
        })
    }

    async fn apply_state(&self, diff: &StateDiff) -> Result<ApplyResult> {
        let mut changes_applied = Vec::new();
        let mut errors = Vec::new();
        let current_state: Option<IncusState> = None;
        let current_by_name: HashMap<String, IncusInstance> = current_state
            .map(|state| {
                state
                    .instances
                    .into_iter()
                    .map(|instance| (instance.name.clone(), instance))
                    .collect()
            })
            .unwrap_or_default();

        for action in &diff.actions {
            match action {
                StateAction::Create { resource, config } => {
                    let instance: IncusInstance =
                        simd_json::serde::from_owned_value(config.clone())
                            .context("Failed to deserialize instance config for creation")?;

                    match Self::apply_create(&instance).await {
                        Ok(changes) => changes_applied.extend(changes),
                        Err(e) => {
                            let msg = format!("Failed to create instance '{}': {}", resource, e);
                            log::error!("{}", msg);
                            errors.push(msg);
                        }
                    }
                }
                StateAction::Modify { resource, changes } => {
                    let desired: IncusInstance =
                        simd_json::serde::from_owned_value(changes.clone())
                            .context("Failed to deserialize instance config for modification")?;

                    match current_by_name.get(resource) {
                        Some(current) => match Self::apply_modify(current, &desired).await {
                            Ok(applied) => changes_applied.extend(applied),
                            Err(e) => {
                                let msg =
                                    format!("Failed to modify instance '{}': {}", resource, e);
                                log::error!("{}", msg);
                                errors.push(msg);
                            }
                        },
                        None => {
                            let msg = format!(
                                "Failed to modify instance '{}': current instance not found",
                                resource
                            );
                            log::error!("{}", msg);
                            errors.push(msg);
                        }
                    }
                }
                StateAction::Delete { resource } => match Self::apply_delete(resource).await {
                    Ok(applied) => changes_applied.extend(applied),
                    Err(e) => {
                        let msg = format!("Failed to delete instance '{}': {}", resource, e);
                        log::error!("{}", msg);
                        errors.push(msg);
                    }
                },
                StateAction::NoOp { .. } => {}
            }
        }

        Ok(ApplyResult {
            success: errors.is_empty(),
            changes_applied,
            errors,
            checkpoint: None,
        })
    }

    async fn verify_state(&self, _desired: &Value) -> Result<bool> {
        Ok(true)
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        log::info!("Creating Incus state checkpoint");
        let state = simd_json::json!(null);
        let id = format!("incus-{}", chrono::Utc::now().timestamp());

        Ok(Checkpoint {
            id,
            plugin: self.name().to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: state,
            backend_checkpoint: None,
        })
    }

    async fn rollback(&self, checkpoint: &Checkpoint) -> Result<()> {
        log::info!("Rolling back Incus state to checkpoint '{}'", checkpoint.id);

        let current = simd_json::json!(null);
        let diff = self
            .calculate_diff(&current, &checkpoint.state_snapshot)
            .await?;

        if diff.actions.is_empty() {
            log::info!("No rollback actions needed -- state already matches checkpoint");
            return Ok(());
        }

        let result = self.apply_state(&diff).await?;
        if result.success {
            log::info!(
                "Rollback to checkpoint '{}' completed successfully ({} change(s))",
                checkpoint.id,
                result.changes_applied.len()
            );
        } else {
            log::error!(
                "Rollback to checkpoint '{}' completed with errors: {:?}",
                checkpoint.id,
                result.errors
            );
            anyhow::bail!(
                "Rollback had {} error(s): {}",
                result.errors.len(),
                result.errors.join("; ")
            );
        }

        Ok(())
    }

    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: true,
            supports_checkpoints: true,
            supports_verification: true,
            atomic_operations: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::incus_device::{DiskDevice, NicDevice, StrBool, StrInt};

    fn nic(parent: &str) -> NamedDevice {
        NamedDevice {
            name: "privacy0".to_string(),
            device: Device::Nic(NicDevice {
                nictype: Some("bridged".to_string()),
                parent: Some(parent.to_string()),
                ..Default::default()
            }),
        }
    }

    #[test]
    fn serialized_device_strings_conform_to_derived_state_schema() {
        let devices = vec![
            NamedDevice {
                name: "root".into(),
                device: Device::Disk(DiskDevice {
                    path: Some("/".into()),
                    pool: Some("seedpool".into()),
                    shift: Some(StrBool(true)),
                    boot_priority: Some(StrInt(1)),
                    ..Default::default()
                }),
            },
            NamedDevice {
                name: "eth0".into(),
                device: Device::Nic(NicDevice {
                    nictype: Some("bridged".into()),
                    parent: Some("ovsbr0".into()),
                    connected: Some(StrBool(true)),
                    mtu: Some(StrInt(1500)),
                    ..Default::default()
                }),
            },
        ];
        let state = IncusState {
            instances: vec![IncusInstance {
                name: "fixture".into(),
                status: "Stopped".into(),
                instance_type: "container".into(),
                devices: devices.clone(),
                expanded_devices: Some(devices),
                ..Default::default()
            }],
        };
        let value = simd_json::serde::to_owned_value(&state).unwrap();
        assert_eq!(
            value["instances"][0]["devices"][0]["device"]["shift"],
            "true"
        );
        assert_eq!(
            value["instances"][0]["devices"][0]["device"]["boot.priority"],
            "1"
        );
        assert_eq!(
            value["instances"][0]["devices"][1]["device"]["connected"],
            "true"
        );
        assert_eq!(value["instances"][0]["devices"][1]["device"]["mtu"], "1500");
        let validation = IncusPlugin::new().schema().unwrap().validate(&value);
        assert!(validation.valid, "{:?}", validation.errors);
    }

    #[test]
    fn bound_identity_power_requests_target_only_the_pinned_native_name() {
        let sid = "b0c777e3-0955-cd8b-3d01-ca12998cae28";
        let (path, body) = IncusPlugin::bound_identity_session_power_request(
            sid,
            "3tchedChatbot",
            IdentitySessionPowerState::Running,
        )
        .unwrap();
        assert_eq!(path, "/1.0/instances/3tchedChatbot/state");
        assert_eq!(body, r#"{"action":"start"}"#);
        let (_, body) = IncusPlugin::bound_identity_session_power_request(
            sid,
            "3tchedChatbot",
            IdentitySessionPowerState::Stopped,
        )
        .unwrap();
        assert_eq!(body, r#"{"action":"stop"}"#);
        for (session, name) in [
            ("00000000-0000-4000-8000-000000000001", "3tchedChatbot"),
            (sid, "Chatbot3tched"),
            (sid, "another-container"),
            (sid, "../../host?project=default"),
        ] {
            assert!(IncusPlugin::bound_identity_session_power_request(
                session,
                name,
                IdentitySessionPowerState::Running,
            )
            .is_err());
        }
    }

    #[test]
    fn identity_session_power_requests_are_typed_and_path_safe() {
        let session_id = uuid::Uuid::new_v4().to_string();
        let (start_path, start_body) = IncusPlugin::identity_session_power_request(
            &session_id,
            IdentitySessionPowerState::Running,
        )
        .expect("start request");
        assert_eq!(start_path, format!("/1.0/instances/{session_id}/state"));
        assert_eq!(start_body, r#"{"action":"start"}"#);

        let (_, stop_body) = IncusPlugin::identity_session_power_request(
            &session_id,
            IdentitySessionPowerState::Stopped,
        )
        .expect("stop request");
        assert_eq!(stop_body, r#"{"action":"stop"}"#);
        assert!(IncusPlugin::identity_session_power_request(
            "../../host?project=default",
            IdentitySessionPowerState::Running,
        )
        .is_err());
        assert!(IncusPlugin::identity_session_power_request(
            &session_id.to_uppercase(),
            IdentitySessionPowerState::Running,
        )
        .is_err());
    }

    #[test]
    fn test_instances_equivalent_detects_config_and_device_changes() {
        let current = IncusInstance {
            name: "privacy-user-1".to_string(),
            status: "Running".to_string(),
            instance_type: "container".to_string(),
            image: Some("images:alpine/3.19".to_string()),
            storage_pool: Some("registration".to_string()),
            profiles: vec!["default".to_string()],
            config: Some(HashMap::from([(
                "user.opdbus.route_id".to_string(),
                "route-a".to_string(),
            )])),
            devices: vec![nic("ovsbr0")],
            ..Default::default()
        };
        let mut desired = current.clone();
        assert!(IncusPlugin::instances_equivalent(&current, &desired));

        desired.config = Some(HashMap::from([(
            "user.opdbus.route_id".to_string(),
            "route-b".to_string(),
        )]));
        assert!(!IncusPlugin::instances_equivalent(&current, &desired));

        desired = current.clone();
        desired.devices = vec![nic("ovsbr1")];
        assert!(!IncusPlugin::instances_equivalent(&current, &desired));
    }

    #[test]
    fn parse_instance_list_builds_typed_devices() {
        let raw = br#"[
            {
                "name": "control-plane-test",
                "status": "Running",
                "status_code": 103,
                "type": "container",
                "architecture": "x86_64",
                "description": "",
                "ephemeral": false,
                "stateful": false,
                "created_at": "2026-09-13T00:00:00Z",
                "last_used_at": "2026-09-13T00:00:00Z",
                "location": "none",
                "project": "default",
                "profiles": ["default"],
                "config": {"boot.autostart": "true"},
                "devices": {
                    "api-sock": {
                        "type": "proxy",
                        "listen": "unix:/run/control-plane-test/api.sock",
                        "connect": "tcp:127.0.0.1:8081",
                        "uid": "0"
                    },
                    "eth0": {"type": "nic", "nictype": "bridged", "parent": "ovsbr0"}
                }
            }
        ]"#
        .to_vec();
        let instances = IncusPlugin::parse_instance_list(raw).expect("parse");
        assert_eq!(instances.len(), 1);
        let devices = &instances[0].devices;
        assert_eq!(devices.len(), 2);
        // Sorted by name: api-sock then eth0.
        assert_eq!(devices[0].name, "api-sock");
        assert!(matches!(devices[0].device, Device::Proxy(_)));
        assert!(matches!(devices[1].device, Device::Nic(_)));
    }

    #[test]
    fn identity_overlay_instance_roundtrips_serde_and_simd_json() {
        let raw = r#"{
            "name": "bea37ecb-92be-197c-660f-09e806f1a34f",
            "status": "Stopped",
            "type": "container",
            "profiles": ["identity"],
            "storage_pool": "3tched-storage",
            "config": {
                "boot.autostart": "false",
                "user.opdbus.role": "chatbot"
            },
            "devices": [
                {"name": "ghostbridge-socket", "device": {"type": "disk", "path": "/opt/run-mounts/ghostbridge", "source": "/run/ghostbridge"}},
                {"name": "identity", "device": {"type": "disk", "path": "/opt/run-mounts/identity", "source": "/var/lib/opdbus-runtime/identities/chatbot", "readonly": "true"}},
                {"name": "persist", "device": {"type": "disk", "path": "/opt/run-mounts/persist", "source": "/var/lib/opdbus-runtime/identities/chatbot/persist"}},
                {"name": "root", "device": {"type": "disk", "path": "/", "pool": "3tched-storage"}}
            ]
        }"#;
        let parsed: IncusInstance =
            serde_json::from_str(raw).expect("overlay instance JSON must parse as IncusInstance");
        assert_eq!(parsed.devices.len(), 4);
        let owned = simd_json::serde::to_owned_value(&parsed)
            .expect("simd_json must serialize IncusInstance for identity_sled SHM");
        let back: IncusInstance = simd_json::serde::from_owned_value(owned)
            .expect("simd_json must roundtrip IncusInstance");
        assert_eq!(back.name, parsed.name);
        assert_eq!(back.devices.len(), 4);
    }
}

// =============================================================================
// Method input types - single source of truth via schemars
// =============================================================================

/// create_instance method input
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CreateInstanceInput {
    /// Instance name
    pub name: String,
    /// Instance type: container or virtual-machine
    #[serde(default = "default_instance_type")]
    pub instance_type: String,
    /// Local image alias/fingerprint, local:<alias>, or images:<alias>.
    pub image: String,
    /// Profile names to apply
    #[serde(default)]
    pub profiles: Vec<String>,
    /// Configuration key-value pairs
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<HashMap<String, String>>,
    /// Device definitions
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub devices: Vec<NamedDevice>,
}

fn default_instance_type() -> String {
    "container".to_string()
}

/// modify_instance method input
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ModifyInstanceInput {
    /// Instance name
    pub name: String,
    /// Configuration updates
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<HashMap<String, String>>,
    /// Device updates
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub devices: Option<HashMap<String, HashMap<String, String>>>,
}

/// delete_instance method input
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DeleteInstanceInput {
    pub name: String,
}

/// Dispatch the Incus instance deletion declared by [`incus_schema`].
///
/// The generic schema router validates the arguments and capability before
/// reaching this function.  Keep the actual lifecycle mutation in the Incus
/// plugin so D-Bus callers use the same REST-over-Unix-socket backend as the
/// declarative state path.
pub async fn dispatch_delete_instance(args: &serde_json::Value) -> Result<serde_json::Value> {
    dispatch_incus_method("delete_instance", args).await
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StartInstanceInput {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StopInstanceInput {
    pub name: String,
    pub force: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RestartInstanceInput {
    pub name: String,
    pub force: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FreezeInstanceInput {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UnfreezeInstanceInput {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SnapshotInstanceInput {
    pub name: String,
    pub snapshot_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExecInstanceInput {
    pub name: String,
    pub command: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AddDeviceInput {
    pub instance_name: String,
    pub device_name: String,
    pub device: super::incus_device::Device,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RemoveDeviceInput {
    pub instance_name: String,
    pub device_name: String,
}

/// Remove one Incus device through the same REST-over-Unix backend used by
/// declarative reconciliation.
pub async fn dispatch_remove_device(args: &serde_json::Value) -> Result<serde_json::Value> {
    dispatch_incus_method("remove_device", args).await
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UpdateDeviceInput {
    pub instance_name: String,
    pub device_name: String,
    pub device: super::incus_device::Device,
}

struct IncusHttpResponse {
    body: Vec<u8>,
    etag: Option<String>,
}

#[derive(Debug, thiserror::Error)]
#[error("Incus API returned HTTP {status}: {detail}")]
struct IncusHttpStatusError {
    status: u16,
    detail: String,
}

/// The privileged socket is deployment policy. Tests inject only a temporary
/// fixture socket; method arguments cannot select a host or another endpoint.
pub(crate) struct IncusApi {
    socket: std::path::PathBuf,
    deadline: Duration,
}

/// Narrow native effects for a stopped identity root repair. The caller is the
/// identity owner, which binds these values to a root-owned plan and a verified
/// SID1 session. There is deliberately no arbitrary REST-path/body method.
pub struct IncusRootRepairBackend {
    api: IncusApi,
    state_dir: std::path::PathBuf,
}

impl IncusRootRepairBackend {
    pub fn local() -> Self {
        Self {
            api: IncusApi::local(),
            state_dir: "/var/lib/incus".into(),
        }
    }

    #[cfg(test)]
    fn on_socket(socket: std::path::PathBuf) -> Self {
        Self {
            api: IncusApi::on_socket(socket, Duration::from_secs(5)),
            state_dir: "/nonexistent-test-incus".into(),
        }
    }

    pub async fn instance(&self, name: &str) -> Result<(serde_json::Value, String)> {
        self.api.instance(name).await
    }

    /// Read the current running init PID for kernel-authenticated local lungs.
    /// The instance name comes from the committed identity binding, never a
    /// guest-selected REST path. This performs no lifecycle effect.
    pub async fn instance_init_pid(&self, name: &str) -> Result<u32> {
        let path = format!("{}/state", instance_path(name)?);
        let server = self.api.call("GET", "/1.0", None, None).await?;
        anyhow::ensure!(
            server["environment"]["server_clustered"].as_bool() == Some(false),
            "local lungs require a non-clustered Incus server"
        );
        let state = self.api.call("GET", &path, None, None).await?;
        anyhow::ensure!(
            state["status"] == "Running" && state["status_code"] == 103,
            "identity container is not running"
        );
        state["pid"]
            .as_u64()
            .and_then(|pid| u32::try_from(pid).ok())
            .filter(|pid| *pid > 0)
            .context("identity container init PID is unavailable")
    }

    pub async fn pool(&self, name: &str) -> Result<Option<serde_json::Value>> {
        validate_segment(name, "storage pool name")?;
        let path = format!("/1.0/storage-pools/{name}");
        match self.api.call("GET", &path, None, None).await {
            Ok(pool) => Ok(Some(pool)),
            Err(error)
                if error
                    .downcast_ref::<IncusHttpStatusError>()
                    .is_some_and(|error| error.status == 404) =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    pub async fn root_volume_present(&self, pool: &str, name: &str) -> Result<bool> {
        validate_segment(pool, "storage pool name")?;
        validate_segment(name, "instance name")?;
        let path = format!("/1.0/storage-pools/{pool}/volumes/container/{name}");
        match self.api.call("GET", &path, None, None).await {
            Ok(volume) => {
                anyhow::ensure!(
                    volume["name"] == name && volume["type"] == "container",
                    "Incus root volume readback differs from target"
                );
                Ok(true)
            }
            Err(error)
                if error
                    .downcast_ref::<IncusHttpStatusError>()
                    .is_some_and(|error| error.status == 404) =>
            {
                Ok(false)
            }
            Err(error) => Err(error),
        }
    }

    /// The Incus volume API can return 200 for a backup.yaml-only directory
    /// after the actual Btrfs root subvolume is gone. Inspect the exact pinned
    /// mounted pool in a blocking native probe before rebuild or activation.
    pub async fn root_subvolume_state(
        &self,
        pool: &str,
        expected_source: &str,
        name: &str,
    ) -> Result<op_network::incus_root_subvolume::RootSubvolumeState> {
        let state_dir = self.state_dir.clone();
        let pool = pool.to_owned();
        let source = std::path::PathBuf::from(expected_source);
        let name = name.to_owned();
        tokio::task::spawn_blocking(move || {
            op_network::incus_root_subvolume::inspect_root_subvolume(
                &state_dir, &pool, &source, &name,
            )
        })
        .await?
    }

    /// Preserve an exact backup.yaml-only missing-root stub outside Incus's
    /// container volume directory before a same-image rebuild. The owner must
    /// durably journal the intent and bind this call to its trusted plan.
    pub async fn prepare_missing_root_for_rebuild(
        &self,
        pool: &str,
        expected_source: &str,
        name: &str,
        operation_id: &str,
    ) -> Result<op_network::incus_root_subvolume::MissingRootPreparation> {
        validate_root_repair_pool_name(pool)?;
        instance_path(name)?;
        let state_dir = self.state_dir.clone();
        let pool = pool.to_owned();
        let source = std::path::PathBuf::from(expected_source);
        let name = name.to_owned();
        let operation_id = operation_id.to_owned();
        tokio::task::spawn_blocking(move || {
            op_network::incus_root_subvolume::prepare_missing_root_for_rebuild(
                &state_dir,
                &pool,
                &source,
                &name,
                &operation_id,
            )
        })
        .await?
    }

    /// Import a pre-existing populated Btrfs pool. `validate` is a native
    /// scanning operation, not a pure query; the owner journals its intent
    /// before either call. No Create/mkfs endpoint is used.
    pub async fn recover_existing_pool(&self, name: &str, source: &str) -> Result<()> {
        validate_segment(name, "storage pool name")?;
        anyhow::ensure!(
            std::path::Path::new(source).is_absolute() && !source.contains(['\n', '\r', '\0']),
            "invalid absolute storage pool source"
        );
        let body = serde_json::json!({"pools":[{
            "name":name,
            "driver":"btrfs",
            "config":{"source":source}
        }]});
        let body = body.to_string();
        let validation = self
            .api
            .call("POST", "/internal/recover/validate", Some(&body), None)
            .await?;
        for field in ["UnknownVolumes", "DependencyErrors"] {
            let value = validation
                .get(field)
                .context("Incus recovery validation response is incomplete")?;
            anyhow::ensure!(
                value.is_null() || value.as_array().is_some_and(Vec::is_empty),
                "Incus recovery found unexpected volumes or dependency errors"
            );
        }
        let _ = self
            .api
            .call("POST", "/internal/recover/import", Some(&body), None)
            .await?;
        let pool = self.pool(name).await?.context("recovered pool is absent")?;
        anyhow::ensure!(
            pool["name"] == name
                && pool["driver"] == "btrfs"
                && pool["config"]["source"] == source
                && pool["status"] == "Created",
            "recovered pool readback differs from trusted plan"
        );
        Ok(())
    }

    /// Create the one parked identity CT from an owner-validated, root-owned
    /// seeded provision plan. This is called while the identity owner holds
    /// its session and host observation locks; it is not a new plugin method.
    pub async fn create_seeded_identity(&self, instance: &IncusInstance) -> Result<()> {
        instance_path(&instance.name)?;
        let fingerprint = instance
            .image
            .as_deref()
            .context("seeded image is absent")?;
        anyhow::ensure!(
            fingerprint.len() == 64
                && fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit())
                && instance.instance_type == "container"
                && instance.status == "Stopped"
                && instance
                    .config
                    .as_ref()
                    .and_then(|config| config.get("boot.autostart"))
                    .map(String::as_str)
                    == Some("false")
                && instance
                    .config
                    .as_ref()
                    .and_then(|config| config.get("security.privileged"))
                    .map(String::as_str)
                    == Some("false"),
            "seeded identity create policy differs from trusted plan"
        );
        let args = serde_json::json!({
            "name": instance.name, "instance_type": "container", "image": fingerprint,
            "profiles": instance.profiles, "config": instance.config,
            "devices": instance.devices,
        });
        let _ = dispatch_incus_with_api(&self.api, "create_instance", &args).await?;
        Ok(())
    }

    pub async fn rebuild_from_image(&self, name: &str, image_fingerprint: &str) -> Result<()> {
        let path = format!("{}/rebuild", instance_path(name)?);
        anyhow::ensure!(
            image_fingerprint.len() == 64
                && image_fingerprint
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit()),
            "invalid pinned image fingerprint"
        );
        let body = serde_json::json!({
            "source":{"type":"image","fingerprint":image_fingerprint}
        });
        let _ = self
            .api
            .call("POST", &path, Some(&body.to_string()), None)
            .await?;
        Ok(())
    }

    /// Incus 7.4 honors If-Match for this PATCH (unlike rebuild/move POSTs).
    /// The owner must still verify the complete stopped native readback.
    pub async fn redirect_persist_source(
        &self,
        name: &str,
        old_source: &str,
        new_source: &str,
        etag: &str,
    ) -> Result<()> {
        anyhow::ensure!(!etag.is_empty(), "missing instance ETag for persist update");
        for source in [old_source, new_source] {
            anyhow::ensure!(
                std::path::Path::new(source).is_absolute() && !source.contains(['\n', '\r', '\0']),
                "invalid persist disk source"
            );
        }
        let (current, actual_etag) = self.instance(name).await?;
        anyhow::ensure!(
            actual_etag == etag && current["status"] == "Stopped",
            "persist update precondition changed"
        );
        let mut persist = current["devices"]["persist"]
            .as_object()
            .context("identity persist disk is absent")?
            .clone();
        anyhow::ensure!(
            persist.get("type").and_then(|value| value.as_str()) == Some("disk")
                && persist.get("source").and_then(|value| value.as_str()) == Some(old_source)
                && !persist.contains_key("shift"),
            "identity persist disk differs from the trusted source"
        );
        persist.insert("source".into(), new_source.into());
        persist.insert("shift".into(), "true".into());
        let body = serde_json::json!({"devices":{"persist":persist}});
        let _ = self
            .api
            .call(
                "PATCH",
                &instance_path(name)?,
                Some(&body.to_string()),
                Some(etag),
            )
            .await?;
        Ok(())
    }

    pub async fn move_to_pool(&self, name: &str, pool: &str) -> Result<()> {
        validate_segment(pool, "storage pool name")?;
        let body = serde_json::json!({
            "name":name,
            "migration":true,
            "instance_only":false,
            "pool":pool,
            "live":true,
            "refresh":false
        });
        let _ = self
            .api
            .call("POST", &instance_path(name)?, Some(&body.to_string()), None)
            .await?;
        Ok(())
    }

    pub async fn default_project_state(&self) -> Result<IncusState> {
        self.api.default_project_state().await
    }
}

const INCUS_LIFECYCLE_EVENTS_PATH: &str = "/1.0/events?type=lifecycle&project=default";
const INCUS_EVENT_FRAME_LIMIT: usize = 64 * 1024;
const INCUS_EVENT_MESSAGE_LIMIT: usize = 128 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct IncusLifecycleEvent {
    #[serde(rename = "type")]
    event_type: String,
    timestamp: chrono::DateTime<chrono::Utc>,
    metadata: IncusLifecycleMetadata,
    #[serde(default)]
    location: String,
    #[serde(default)]
    project: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct IncusLifecycleMetadata {
    action: String,
    source: String,
    #[serde(default)]
    context: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    requestor: Option<IncusLifecycleRequestor>,
    #[serde(default)]
    name: String,
    project: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct IncusLifecycleRequestor {
    username: String,
    protocol: String,
    #[serde(default)]
    address: String,
}

/// One bounded native lifecycle subscription. Events are wakeups only: callers
/// must read and publish a complete root snapshot after each accepted event.
pub struct IncusLifecycleObserver {
    websocket: WebSocketStream<UnixStream>,
    api: IncusApi,
}

impl IncusLifecycleObserver {
    async fn connect_on_socket(socket: std::path::PathBuf) -> Result<Self> {
        let stream = tokio::time::timeout(Duration::from_secs(3), UnixStream::connect(&socket))
            .await
            .context("timed out connecting to Incus lifecycle events")?
            .context("failed to connect to Incus lifecycle events")?;
        #[allow(deprecated)]
        let config = WebSocketConfig {
            max_send_queue: None,
            write_buffer_size: 8 * 1024,
            max_write_buffer_size: 256 * 1024,
            max_message_size: Some(INCUS_EVENT_MESSAGE_LIMIT),
            max_frame_size: Some(INCUS_EVENT_FRAME_LIMIT),
            accept_unmasked_frames: false,
        };
        let request = format!("ws://localhost{INCUS_LIFECYCLE_EVENTS_PATH}");
        let (websocket, _) = tokio::time::timeout(
            Duration::from_secs(5),
            tokio_tungstenite::client_async_with_config(request, stream, Some(config)),
        )
        .await
        .context("timed out subscribing to Incus lifecycle events")?
        .context("Incus lifecycle WebSocket handshake failed")?;
        Ok(Self {
            websocket,
            api: IncusApi {
                socket,
                deadline: Duration::from_secs(30),
            },
        })
    }

    /// Read one full snapshot after the subscription is active. Keeping these
    /// steps separate lets the bridge hold its native-observation lock across
    /// readback and SHM commit without holding it while the event stream idles.
    pub async fn default_project_state(&self) -> Result<IncusState> {
        self.api.default_project_state().await
    }

    /// Wait for one valid default-project lifecycle notification. Notification
    /// fields are validated and then discarded; they never become state.
    pub async fn wait_for_lifecycle(&mut self) -> Result<()> {
        loop {
            let message = self
                .websocket
                .next()
                .await
                .context("Incus lifecycle event stream closed")?
                .context("Incus lifecycle event stream failed")?;
            match message {
                Message::Text(text) => {
                    anyhow::ensure!(
                        text.len() <= INCUS_EVENT_MESSAGE_LIMIT,
                        "Incus lifecycle event exceeded message limit"
                    );
                    let event: IncusLifecycleEvent =
                        serde_json::from_str(&text).context("malformed Incus lifecycle event")?;
                    validate_lifecycle_event(&event)?;
                    return Ok(());
                }
                Message::Ping(payload) => {
                    tokio::time::timeout(
                        Duration::from_secs(3),
                        self.websocket.send(Message::Pong(payload)),
                    )
                    .await
                    .context("timed out replying to Incus lifecycle ping")?
                    .context("failed to reply to Incus lifecycle ping")?;
                }
                Message::Pong(_) => {}
                Message::Close(_) => anyhow::bail!("Incus lifecycle event stream closed"),
                Message::Binary(_) | Message::Frame(_) => {
                    anyhow::bail!("Incus lifecycle event must be a JSON text message")
                }
            }
        }
    }
}

fn validate_lifecycle_event(event: &IncusLifecycleEvent) -> Result<()> {
    anyhow::ensure!(
        event.event_type == "lifecycle"
            && matches!(event.project.as_str(), "" | "default")
            && matches!(event.metadata.project.as_str(), "" | "default"),
        "Incus lifecycle event is outside the default project"
    );
    anyhow::ensure!(
        !event.metadata.action.is_empty()
            && event.metadata.source.starts_with("/1.0/")
            && !event.metadata.source.chars().any(char::is_whitespace),
        "Incus lifecycle event has invalid metadata"
    );
    let _ = (
        &event.timestamp,
        &event.location,
        &event.metadata.context,
        &event.metadata.name,
    );
    if let Some(requestor) = &event.metadata.requestor {
        let _ = (&requestor.username, &requestor.protocol, &requestor.address);
    }
    Ok(())
}

/// Subscribe before any initial inventory read so lifecycle changes cannot be
/// lost between startup and the first complete snapshot.
pub async fn subscribe_incus_default_project() -> Result<IncusLifecycleObserver> {
    IncusLifecycleObserver::connect_on_socket("/var/lib/incus/unix.socket".into()).await
}

impl IncusApi {
    pub(crate) fn local() -> Self {
        Self {
            socket: "/var/lib/incus/unix.socket".into(),
            deadline: Duration::from_secs(125),
        }
    }

    #[cfg(test)]
    pub(crate) fn on_socket(socket: std::path::PathBuf, deadline: Duration) -> Self {
        Self { socket, deadline }
    }

    async fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
        etag: Option<&str>,
    ) -> Result<IncusHttpResponse> {
        IncusPlugin::incus_response_on_socket(&self.socket, self.deadline, method, path, body, etag)
            .await
    }

    fn sync_metadata(value: &serde_json::Value) -> Result<serde_json::Value> {
        anyhow::ensure!(
            value["type"] == "sync"
                && value["status_code"] == 200
                && value
                    .get("error")
                    .and_then(|v| v.as_str())
                    .is_none_or(str::is_empty),
            "Incus API error: {}",
            value
                .get("error")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .or_else(|| value.get("status").and_then(|v| v.as_str()))
                .unwrap_or("malformed response envelope")
        );
        value
            .get("metadata")
            .cloned()
            .context("Incus response is missing metadata")
    }

    pub(crate) async fn call(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
        etag: Option<&str>,
    ) -> Result<serde_json::Value> {
        let response = self.request(method, path, body, etag).await?;
        let value: serde_json::Value =
            serde_json::from_slice(&response.body).context("invalid Incus API JSON")?;
        if value["type"] != "async" {
            return Self::sync_metadata(&value);
        }
        anyhow::ensure!(
            value
                .get("error")
                .and_then(|v| v.as_str())
                .is_none_or(str::is_empty),
            "Incus refused operation: {}",
            value["error"]
        );
        let operation = value["operation"]
            .as_str()
            .context("async Incus response missing operation path")?;
        // Never follow a provider-supplied URL or a different privileged API.
        let (path, query) = operation.split_once('?').unwrap_or((operation, ""));
        let id = path
            .strip_prefix("/1.0/operations/")
            .context("invalid Incus operation path")?;
        anyhow::ensure!(
            uuid::Uuid::parse_str(id).is_ok_and(|uuid| uuid.to_string() == id),
            "invalid Incus operation id"
        );
        let wait_path = if query.is_empty() {
            format!("{path}/wait?timeout=120")
        } else {
            anyhow::ensure!(
                !query.contains(['#', '\\']) && !query.chars().any(char::is_control),
                "invalid Incus operation query"
            );
            format!("{path}/wait?{query}&timeout=120")
        };
        // One server-side long wait, never a plugin polling loop. A timeout or
        // nonterminal operation must not be reported as a completed mutation.
        let response = self.request("GET", &wait_path, None, None).await?;
        let envelope: serde_json::Value =
            serde_json::from_slice(&response.body).context("invalid Incus operation JSON")?;
        let operation = Self::sync_metadata(&envelope)?;
        anyhow::ensure!(
            operation["status_code"] == 200
                && operation["status"] == "Success"
                && operation
                    .get("err")
                    .and_then(|v| v.as_str())
                    .is_none_or(str::is_empty),
            "Incus operation did not complete successfully: {}",
            operation
                .get("err")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .or_else(|| operation.get("status").and_then(|v| v.as_str()))
                .unwrap_or("malformed operation")
        );
        operation
            .get("metadata")
            .cloned()
            .context("Incus operation is missing result metadata")
    }

    pub(crate) async fn instance(&self, name: &str) -> Result<(serde_json::Value, String)> {
        let path = instance_path(name)?;
        let response = self.request("GET", &path, None, None).await?;
        let envelope: serde_json::Value =
            serde_json::from_slice(&response.body).context("invalid Incus instance JSON")?;
        let instance = Self::sync_metadata(&envelope)?;
        anyhow::ensure!(
            instance.is_object(),
            "Incus instance metadata must be an object"
        );
        let etag = response
            .etag
            .filter(|value| !value.is_empty())
            .context("Incus instance response is missing ETag; refusing unsafe update")?;
        Ok((instance, etag))
    }

    pub(crate) async fn instance_optional(
        &self,
        name: &str,
    ) -> Result<Option<(serde_json::Value, String)>> {
        match self.instance(name).await {
            Ok(instance) => Ok(Some(instance)),
            Err(error)
                if error
                    .downcast_ref::<IncusHttpStatusError>()
                    .is_some_and(|error| error.status == 404) =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    async fn default_project_state(&self) -> Result<IncusState> {
        let metadata = self
            .call(
                "GET",
                "/1.0/instances?recursion=1&project=default",
                None,
                None,
            )
            .await?;
        let raw: Vec<RawIncusInstance> = serde_json::from_value(metadata)
            .context("invalid default-project Incus instance snapshot")?;
        Ok(IncusState {
            instances: IncusPlugin::convert_instance_list(raw)?,
        })
    }

    async fn update(
        &self,
        name: &str,
        mut instance: serde_json::Value,
        etag: &str,
    ) -> Result<serde_json::Value> {
        // InstancePut fields only. Preserve unknown native device attributes and
        // provider-owned config; they must not disappear in a typed readback.
        let object = instance
            .as_object_mut()
            .context("Incus instance metadata must be an object")?;
        object.retain(|key, _| {
            matches!(
                key.as_str(),
                "architecture" | "config" | "devices" | "ephemeral" | "profiles" | "description"
            )
        });
        self.call(
            "PUT",
            &instance_path(name)?,
            Some(&instance.to_string()),
            Some(etag),
        )
        .await
    }

    pub(crate) async fn delete(&self, name: &str) -> Result<serde_json::Value> {
        let path = instance_path(name)?;
        let (instance, _) = self.instance(name).await?;
        let status = instance["status"]
            .as_str()
            .context("Incus instance is missing status")?;
        if status != "Stopped" {
            // Incus --force is a stop-and-wait followed by DELETE, not a
            // force query parameter on DELETE. Never disable delete protection.
            let body = serde_json::json!({"action": "stop", "force": true, "timeout": 120});
            let stopped = self
                .call(
                    "PUT",
                    &format!("{path}/state"),
                    Some(&body.to_string()),
                    None,
                )
                .await?;
            if instance["ephemeral"] == true {
                return Ok(stopped);
            }
        }
        self.call("DELETE", &path, None, None).await
    }

    async fn exec_output(&self, name: &str, value: Option<&serde_json::Value>) -> Result<String> {
        let value = value.context("Incus exec result is missing a recorded output path")?;
        let path = value.as_str().context("invalid Incus exec output path")?;
        let prefix = format!("{}/logs/", instance_path(name)?);
        let filename = path
            .strip_prefix(&prefix)
            .context("Incus exec output is outside the target instance logs")?;
        let filename = filename.strip_prefix("exec-output/").unwrap_or(filename);
        validate_segment(filename, "exec log name")?;
        let response = self.request("GET", path, None, None).await?;
        Ok(String::from_utf8_lossy(&response.body).into_owned())
    }
}

pub(crate) fn validate_segment(value: &str, kind: &str) -> Result<()> {
    anyhow::ensure!(
        !value.is_empty()
            && value != "."
            && value != ".."
            && value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.')),
        "invalid Incus {kind}: expected a single safe name"
    );
    Ok(())
}

/// Used only to validate a root-owned identity repair plan before constructing
/// fixed Incus recovery/move requests.
pub fn validate_root_repair_pool_name(name: &str) -> Result<()> {
    validate_segment(name, "storage pool name")
}

pub(crate) fn instance_path(name: &str) -> Result<String> {
    validate_segment(name, "instance name")?;
    Ok(format!("/1.0/instances/{name}"))
}

fn validate_config(config: &HashMap<String, String>) -> Result<()> {
    for key in config.keys() {
        anyhow::ensure!(
            !key.is_empty() && !IncusPlugin::is_read_only_config_key(key),
            "Incus config key '{key}' is provider-owned or empty"
        );
    }
    Ok(())
}

fn image_source(image: &str) -> Result<serde_json::Value> {
    anyhow::ensure!(!image.trim().is_empty(), "Incus image must not be empty");
    if let Some(alias) = image.strip_prefix("images:") {
        anyhow::ensure!(!alias.is_empty(), "Incus image alias must not be empty");
        return Ok(serde_json::json!({
            "type": "image", "alias": alias, "mode": "pull",
            "protocol": "simplestreams", "server": "https://images.linuxcontainers.org"
        }));
    }
    let image = image.strip_prefix("local:").unwrap_or(image);
    anyhow::ensure!(
        !image.is_empty() && !image.contains(':'),
        "unknown Incus image remote; use a local image or images:<alias>"
    );
    if image.len() == 64 && image.bytes().all(|c| c.is_ascii_hexdigit()) {
        Ok(serde_json::json!({"type": "image", "fingerprint": image}))
    } else {
        Ok(serde_json::json!({"type": "image", "alias": image}))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CreateInstanceOutput {
    pub name: String,
    pub created: bool,
    pub changes: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ModifyInstanceOutput {
    pub name: String,
    pub updated: bool,
    pub changes: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DeleteInstanceOutput {
    pub name: String,
    pub deleted: bool,
    pub response: serde_json::Value,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InstanceStateOutput {
    pub name: String,
    pub state: String,
    pub changed: bool,
    pub response: serde_json::Value,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SnapshotInstanceOutput {
    pub name: String,
    pub snapshot_name: String,
    pub created: bool,
    pub response: serde_json::Value,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExecInstanceOutput {
    pub name: String,
    pub completed: bool,
    pub exit_code: i64,
    pub stdout: String,
    pub stderr: String,
    pub response: serde_json::Value,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DeviceMutationOutput {
    pub instance_name: String,
    pub device_name: String,
    pub changed: bool,
    pub response: serde_json::Value,
}

/// One admitted method result paired with the complete native state observed
/// after a represented mutation. `state` is absent for methods such as exec
/// and snapshot whose effects are not fields in [`IncusState`].
#[derive(Debug, Clone)]
pub struct IncusMethodObservation {
    pub result: serde_json::Value,
    pub state: Option<IncusState>,
}

fn method_changes_observed_state(method: &str) -> bool {
    matches!(
        method,
        "create_instance"
            | "modify_instance"
            | "delete_instance"
            | "start_instance"
            | "stop_instance"
            | "restart_instance"
            | "freeze_instance"
            | "unfreeze_instance"
            | "add_device"
            | "update_device"
            | "remove_device"
    )
}

/// Read one complete default-project snapshot from the native Incus UDS API.
/// This is on demand and performs no polling, caching, or filesystem scan.
pub async fn observe_incus_default_project() -> Result<IncusState> {
    IncusApi::local().default_project_state().await
}

/// Execute the native method and, when its effect is represented by
/// [`IncusState`], return an immediate complete provider readback. Once the
/// method result exists, a readback error explicitly reports that the native
/// mutation completed and must be reconciled; it is never returned as success.
pub async fn dispatch_incus_method_observed(
    method: &str,
    args: &serde_json::Value,
) -> Result<IncusMethodObservation> {
    dispatch_incus_with_api_observed(&IncusApi::local(), method, args).await
}

async fn dispatch_incus_with_api_observed(
    api: &IncusApi,
    method: &str,
    args: &serde_json::Value,
) -> Result<IncusMethodObservation> {
    let result = dispatch_incus_with_api(api, method, args).await?;
    let state = if method_changes_observed_state(method) {
        let state = api.default_project_state().await.with_context(|| {
            format!(
                "Incus {method} completed but its authoritative state readback failed; reconciliation is required"
            )
        })?;
        validate_method_observation(method, args, &result, &state).with_context(|| {
            format!(
                "Incus {method} completed but its authoritative state did not confirm the receipt; reconciliation is required"
            )
        })?;
        Some(state)
    } else {
        None
    };
    Ok(IncusMethodObservation { result, state })
}

fn validate_method_observation(
    method: &str,
    args: &serde_json::Value,
    result: &serde_json::Value,
    state: &IncusState,
) -> Result<()> {
    let named = |name: &str| {
        state
            .instances
            .iter()
            .find(|instance| instance.name == name)
    };
    match method {
        "create_instance" => {
            let input: CreateInstanceInput = serde_json::from_value(args.clone())?;
            let instance = named(&input.name).context("created instance is absent")?;
            anyhow::ensure!(
                instance.instance_type == input.instance_type,
                "created instance type differs from the provider receipt"
            );
            anyhow::ensure!(
                instance.profiles == input.profiles,
                "created instance profiles differ from provider readback"
            );
            let observed_config = instance.config.as_ref().cloned().unwrap_or_default();
            for (key, value) in input.config.unwrap_or_default() {
                anyhow::ensure!(
                    observed_config.get(&key) == Some(&value),
                    "created config '{key}' differs from provider readback"
                );
            }
            let observed_devices = instance
                .devices
                .iter()
                .map(|device| (device.name.as_str(), device.device.to_incus_map()))
                .collect::<BTreeMap<_, _>>();
            for device in input.devices {
                anyhow::ensure!(
                    observed_devices.get(device.name.as_str())
                        == Some(&device.device.to_incus_map()),
                    "created device '{}' differs from provider readback",
                    device.name
                );
            }
        }
        "modify_instance" => {
            let input: ModifyInstanceInput = serde_json::from_value(args.clone())?;
            let instance = named(&input.name).context("modified instance is absent")?;
            let observed_config = instance.config.as_ref().cloned().unwrap_or_default();
            for (key, value) in input.config.unwrap_or_default() {
                anyhow::ensure!(
                    observed_config.get(&key) == Some(&value),
                    "modified config '{key}' differs from provider readback"
                );
            }
            let observed_devices = instance
                .devices
                .iter()
                .map(|device| (device.name.as_str(), device.device.to_incus_map()))
                .collect::<BTreeMap<_, _>>();
            for (name, value) in input.devices.unwrap_or_default() {
                let expected = value.into_iter().collect::<BTreeMap<_, _>>();
                anyhow::ensure!(
                    observed_devices.get(name.as_str()) == Some(&expected),
                    "modified device '{name}' differs from provider readback"
                );
            }
        }
        "delete_instance" => {
            let input: DeleteInstanceInput = serde_json::from_value(args.clone())?;
            anyhow::ensure!(
                named(&input.name).is_none(),
                "deleted instance remains present"
            );
        }
        "start_instance" | "stop_instance" | "restart_instance" | "freeze_instance"
        | "unfreeze_instance" => {
            let name = result["name"]
                .as_str()
                .context("state receipt is missing instance name")?;
            let expected = result["state"]
                .as_str()
                .context("state receipt is missing target state")?;
            anyhow::ensure!(
                named(name).is_some_and(|instance| instance.status == expected),
                "instance state differs from provider receipt"
            );
        }
        "add_device" => {
            let input: AddDeviceInput = serde_json::from_value(args.clone())?;
            let instance = named(&input.instance_name).context("device owner is absent")?;
            anyhow::ensure!(
                instance.devices.iter().any(|device| {
                    device.name == input.device_name
                        && device.device.to_incus_map() == input.device.to_incus_map()
                }),
                "device differs from provider readback"
            );
        }
        "update_device" => {
            let input: UpdateDeviceInput = serde_json::from_value(args.clone())?;
            let instance = named(&input.instance_name).context("device owner is absent")?;
            anyhow::ensure!(
                instance.devices.iter().any(|device| {
                    device.name == input.device_name
                        && device.device.to_incus_map() == input.device.to_incus_map()
                }),
                "device differs from provider readback"
            );
        }
        "remove_device" => {
            let input: RemoveDeviceInput = serde_json::from_value(args.clone())?;
            let instance = named(&input.instance_name).context("device owner is absent")?;
            anyhow::ensure!(
                instance
                    .devices
                    .iter()
                    .all(|device| device.name != input.device_name),
                "removed device remains present"
            );
        }
        _ => anyhow::bail!("method has no represented Incus state mutation"),
    }
    Ok(())
}

/// Dispatch the admitted plugin call. The canonical gate owns SID1 and exact
/// capability validation; this backend never mints or reconstructs identity.
pub async fn dispatch_incus_method(
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    dispatch_incus_with_api(&IncusApi::local(), method, args).await
}

async fn dispatch_incus_with_api(
    api: &IncusApi,
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    match method {
        "create_instance" => {
            let input: CreateInstanceInput = serde_json::from_value(args.clone())
                .context("invalid create_instance arguments")?;
            instance_path(&input.name)?;
            anyhow::ensure!(
                matches!(
                    input.instance_type.as_str(),
                    "container" | "virtual-machine"
                ),
                "invalid Incus instance type"
            );
            let config = input.config.unwrap_or_default();
            validate_config(&config)?;
            let mut devices = BTreeMap::new();
            for named in input.devices {
                validate_segment(&named.name, "device name")?;
                anyhow::ensure!(
                    devices
                        .insert(named.name, named.device.to_incus_map())
                        .is_none(),
                    "duplicate Incus device name"
                );
            }
            // Include restrictions, root disks and all other requested devices
            // in the create request, before the provider can start the instance.
            let body = serde_json::json!({
                "name": input.name, "type": input.instance_type,
                "source": image_source(&input.image)?,
                "profiles": input.profiles, "config": config, "devices": devices
            });
            api.call("POST", "/1.0/instances", Some(&body.to_string()), None)
                .await?;
            Ok(serde_json::to_value(CreateInstanceOutput {
                changes: vec![format!("Created stopped instance '{}'", input.name)],
                name: input.name,
                created: true,
            })?)
        }
        "modify_instance" => {
            let input: ModifyInstanceInput = serde_json::from_value(args.clone())
                .context("invalid modify_instance arguments")?;
            instance_path(&input.name)?;
            if let Some(config) = &input.config {
                validate_config(config)?;
            }
            if let Some(devices) = &input.devices {
                for (name, device) in devices {
                    validate_segment(name, "device name")?;
                    let device_map = device
                        .iter()
                        .map(|(key, value)| (key.clone(), value.clone()))
                        .collect::<BTreeMap<_, _>>();
                    let parsed = Device::from_incus_map(&device_map)
                        .context("invalid Incus device definition")?;
                    anyhow::ensure!(
                        parsed.to_incus_map() == device_map,
                        "Incus device '{name}' contains fields the sealed typed contract cannot preserve"
                    );
                }
            }
            let (mut current, etag) = api.instance(&input.name).await?;
            let mut changes = Vec::new();
            if let Some(config) = input.config {
                let target = current["config"]
                    .as_object_mut()
                    .context("Incus config must be an object")?;
                for (key, value) in config {
                    if target.get(&key) != Some(&serde_json::Value::String(value.clone())) {
                        target.insert(key.clone(), value.into());
                        changes.push(format!("Updated config '{key}'"));
                    }
                }
            }
            if let Some(devices) = input.devices {
                let target = current["devices"]
                    .as_object_mut()
                    .context("Incus devices must be an object")?;
                for (key, value) in devices {
                    let value = serde_json::to_value(value)?;
                    if target.get(&key) != Some(&value) {
                        target.insert(key.clone(), value);
                        changes.push(format!("Updated device '{key}'"));
                    }
                }
            }
            if !changes.is_empty() {
                api.update(&input.name, current, &etag).await?;
            }
            Ok(serde_json::to_value(ModifyInstanceOutput {
                name: input.name,
                updated: !changes.is_empty(),
                changes,
            })?)
        }
        "delete_instance" => {
            let input: DeleteInstanceInput = serde_json::from_value(args.clone())
                .context("invalid delete_instance arguments")?;
            let response = api.delete(&input.name).await?;
            Ok(serde_json::to_value(DeleteInstanceOutput {
                name: input.name,
                deleted: true,
                response,
            })?)
        }
        "start_instance" | "stop_instance" | "restart_instance" | "freeze_instance"
        | "unfreeze_instance" => {
            let (name, force) = match method {
                "start_instance" => (
                    serde_json::from_value::<StartInstanceInput>(args.clone())
                        .context("invalid start_instance arguments")?
                        .name,
                    false,
                ),
                "stop_instance" => {
                    let input = serde_json::from_value::<StopInstanceInput>(args.clone())
                        .context("invalid stop_instance arguments")?;
                    (input.name, input.force.unwrap_or(false))
                }
                "restart_instance" => {
                    let input = serde_json::from_value::<RestartInstanceInput>(args.clone())
                        .context("invalid restart_instance arguments")?;
                    (input.name, input.force.unwrap_or(false))
                }
                "freeze_instance" => (
                    serde_json::from_value::<FreezeInstanceInput>(args.clone())
                        .context("invalid freeze_instance arguments")?
                        .name,
                    false,
                ),
                _ => (
                    serde_json::from_value::<UnfreezeInstanceInput>(args.clone())
                        .context("invalid unfreeze_instance arguments")?
                        .name,
                    false,
                ),
            };
            let action = match method {
                "start_instance" => "start",
                "stop_instance" => "stop",
                "restart_instance" => "restart",
                "freeze_instance" => "freeze",
                _ => "unfreeze",
            };
            let state = match method {
                "start_instance" | "restart_instance" | "unfreeze_instance" => "Running",
                "freeze_instance" => "Frozen",
                _ => "Stopped",
            };
            let body = serde_json::json!({"action": action, "force": force, "timeout": 120});
            let path = format!("{}/state", instance_path(&name)?);
            let response = api
                .call("PUT", &path, Some(&body.to_string()), None)
                .await?;
            Ok(serde_json::to_value(InstanceStateOutput {
                name,
                state: state.to_string(),
                changed: true,
                response,
            })?)
        }
        "snapshot_instance" => {
            let input: SnapshotInstanceInput = serde_json::from_value(args.clone())
                .context("invalid snapshot_instance arguments")?;
            validate_segment(&input.snapshot_name, "snapshot name")?;
            let body = serde_json::json!({"name": input.snapshot_name});
            let response = api
                .call(
                    "POST",
                    &format!("{}/snapshots", instance_path(&input.name)?),
                    Some(&body.to_string()),
                    None,
                )
                .await?;
            Ok(serde_json::to_value(SnapshotInstanceOutput {
                name: input.name,
                snapshot_name: input.snapshot_name,
                created: true,
                response,
            })?)
        }
        "exec_instance" => {
            let input: ExecInstanceInput =
                serde_json::from_value(args.clone()).context("invalid exec_instance arguments")?;
            let path = instance_path(&input.name)?;
            anyhow::ensure!(
                input.command.first().is_some_and(|value| !value.is_empty()),
                "Incus exec requires a nonempty command"
            );
            anyhow::ensure!(
                input.command.iter().all(|value| !value.contains('\0')),
                "Incus exec arguments must not contain NUL"
            );
            let body = serde_json::json!({
                "command": input.command, "wait-for-websocket": false,
                "interactive": false, "record-output": true
            });
            let response = api
                .call(
                    "POST",
                    &format!("{path}/exec"),
                    Some(&body.to_string()),
                    None,
                )
                .await?;
            let exit_code = response["return"]
                .as_i64()
                .context("Incus exec completed without an exit code")?;
            let stdout = api
                .exec_output(&input.name, response["output"].get("1"))
                .await?;
            let stderr = api
                .exec_output(&input.name, response["output"].get("2"))
                .await?;
            Ok(serde_json::to_value(ExecInstanceOutput {
                name: input.name,
                completed: true,
                exit_code,
                stdout,
                stderr,
                response,
            })?)
        }
        "add_device" | "update_device" | "remove_device" => {
            let (name, device_name, device) = if method == "remove_device" {
                let input: RemoveDeviceInput = serde_json::from_value(args.clone())
                    .context("invalid remove_device arguments")?;
                (input.instance_name, input.device_name, None)
            } else {
                let input: UpdateDeviceInput = serde_json::from_value(args.clone())
                    .context("invalid device mutation arguments")?;
                (
                    input.instance_name,
                    input.device_name,
                    Some(serde_json::to_value(input.device.to_incus_map())?),
                )
            };
            instance_path(&name)?;
            validate_segment(&device_name, "device name")?;
            let (mut current, etag) = api.instance(&name).await?;
            let devices = current["devices"]
                .as_object_mut()
                .context("Incus devices must be an object")?;
            let previous = devices.get(&device_name);
            if method == "add_device" {
                anyhow::ensure!(
                    previous.is_none(),
                    "Incus device '{device_name}' already exists; use update_device"
                );
            } else if method == "update_device" {
                anyhow::ensure!(
                    previous.is_some(),
                    "Incus device '{device_name}' does not exist; use add_device"
                );
            }
            let changed = previous != device.as_ref();
            match device {
                Some(device) => {
                    devices.insert(device_name.clone(), device);
                }
                None => {
                    devices.remove(&device_name);
                }
            }
            let response = if changed {
                api.update(&name, current, &etag).await?
            } else {
                serde_json::Value::Null
            };
            Ok(serde_json::to_value(DeviceMutationOutput {
                instance_name: name,
                device_name,
                changed,
                response,
            })?)
        }
        _ => anyhow::bail!(
            "unknown Incus method '{}'; no canonical backend dispatch",
            method
        ),
    }
}

pub(crate) fn incus_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(IncusState))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "incus",
        "3.0.0",
        "Incus instance management",
        &root,
    );
    schema.example = Some(json!({
        "instances": [
            {
                "name": "privacy-user-123",
                "status": "Running",
                "type": "container",
                "image": "images:debian/13",
                "storage_pool": "registration",
                "profiles": ["default"],
                "config": { "limits.cpu": "2" },
                "devices": [
                    {
                        "name": "eth0",
                        "device": {
                            "type": "nic",
                            "nictype": "bridged",
                            "parent": "ovsbr0"
                        }
                    }
                ]
            }
        ]
    }));
    let mut methods = std::collections::HashMap::new();
    methods.insert(
        "create_instance".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            CreateInstanceInput,
            CreateInstanceOutput,
        >(
            "create_instance",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.service.incus.instance.create@v1",
            "mut.service.incus.instance.create@v1",
        ),
    );
    methods.insert(
        "modify_instance".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            ModifyInstanceInput,
            ModifyInstanceOutput,
        >(
            "modify_instance",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.service.incus.instance.modify@v1",
            "mut.service.incus.instance.modify@v1",
        ),
    );
    methods.insert(
        "delete_instance".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            DeleteInstanceInput,
            DeleteInstanceOutput,
        >(
            "delete_instance",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.service.incus.instance.delete@v1",
            "mut.service.incus.instance.delete@v1",
        ),
    );
    methods.insert(
        "start_instance".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            StartInstanceInput,
            InstanceStateOutput,
        >(
            "start_instance",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.service.incus.instance.start@v1",
            "mut.service.incus.instance.start@v1",
        ),
    );
    methods.insert(
        "stop_instance".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            StopInstanceInput,
            InstanceStateOutput,
        >(
            "stop_instance",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.service.incus.instance.stop@v1",
            "mut.service.incus.instance.stop@v1",
        ),
    );
    methods.insert(
        "restart_instance".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            RestartInstanceInput,
            InstanceStateOutput,
        >(
            "restart_instance",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.service.incus.instance.restart@v1",
            "mut.service.incus.instance.restart@v1",
        ),
    );
    methods.insert(
        "freeze_instance".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            FreezeInstanceInput,
            InstanceStateOutput,
        >(
            "freeze_instance",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.service.incus.instance.freeze@v1",
            "mut.service.incus.instance.freeze@v1",
        ),
    );
    methods.insert(
        "unfreeze_instance".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            UnfreezeInstanceInput,
            InstanceStateOutput,
        >(
            "unfreeze_instance",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.service.incus.instance.unfreeze@v1",
            "mut.service.incus.instance.unfreeze@v1",
        ),
    );
    methods.insert(
        "snapshot_instance".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            SnapshotInstanceInput,
            SnapshotInstanceOutput,
        >(
            "snapshot_instance",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.service.incus.instance.snapshot@v1",
            "mut.service.incus.instance.snapshot@v1",
        ),
    );
    methods.insert(
        "exec_instance".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            ExecInstanceInput,
            ExecInstanceOutput,
        >(
            "exec_instance",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.service.incus.instance.exec@v1",
            "mut.service.incus.instance.exec@v1",
        ),
    );
    methods.insert(
        "add_device".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            AddDeviceInput,
            DeviceMutationOutput,
        >(
            "add_device",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.service.incus.device.add@v1",
            "mut.service.incus.device.add@v1",
        ),
    );
    methods.insert(
        "remove_device".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            RemoveDeviceInput,
            DeviceMutationOutput,
        >(
            "remove_device",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.service.incus.device.remove@v1",
            "mut.service.incus.device.remove@v1",
        ),
    );
    methods.insert(
        "update_device".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            UpdateDeviceInput,
            DeviceMutationOutput,
        >(
            "update_device",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.service.incus.device.update@v1",
            "mut.service.incus.device.update@v1",
        ),
    );
    schema = schema.with_methods(methods);
    schema.capabilities.insert(
        "cap.service.incus.instance.create@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.service.incus.instance.create@v1".to_string(),
            description: "Grants: create_instance.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.service.incus.instance.modify@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.service.incus.instance.modify@v1".to_string(),
            description: "Grants: modify_instance.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.service.incus.instance.delete@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.service.incus.instance.delete@v1".to_string(),
            description: "Grants: delete_instance.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.service.incus.instance.start@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.service.incus.instance.start@v1".to_string(),
            description: "Grants: start_instance.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.service.incus.instance.stop@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.service.incus.instance.stop@v1".to_string(),
            description: "Grants: stop_instance.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.service.incus.instance.restart@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.service.incus.instance.restart@v1".to_string(),
            description: "Grants: restart_instance.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.service.incus.instance.freeze@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.service.incus.instance.freeze@v1".to_string(),
            description: "Grants: freeze_instance.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.service.incus.instance.unfreeze@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.service.incus.instance.unfreeze@v1".to_string(),
            description: "Grants: unfreeze_instance.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.service.incus.instance.snapshot@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.service.incus.instance.snapshot@v1".to_string(),
            description: "Grants: snapshot_instance.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.service.incus.instance.exec@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.service.incus.instance.exec@v1".to_string(),
            description: "Grants: exec_instance.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.service.incus.device.add@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.service.incus.device.add@v1".to_string(),
            description: "Grants: add_device.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.service.incus.device.remove@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.service.incus.device.remove@v1".to_string(),
            description: "Grants: remove_device.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.service.incus.device.update@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.service.incus.device.update@v1".to_string(),
            description: "Grants: update_device.".to_string(),
        },
    );

    // Shared topology omits instance names, profiles, config, devices, image
    // references, storage, and timestamps. In particular it cannot identify
    // private identity-session containers.
    schema.public_state.paths = vec![
        "/instances/*/status".to_string(),
        "/instances/*/type".to_string(),
        "/instances/*/architecture".to_string(),
        "/instances/*/ephemeral".to_string(),
        "/instances/*/stateful".to_string(),
        "/instances/*/location".to_string(),
    ];

    schema
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new("incus", |_ctx| std::sync::Arc::new(IncusPlugin::new()))
}

#[cfg(test)]
mod transport_tests {
    use super::IncusPlugin;
    use futures::SinkExt;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixListener;
    use tokio_tungstenite::tungstenite::Message;

    #[tokio::test]
    async fn lungs_native_init_pid_requires_a_current_running_positive_pid() -> anyhow::Result<()> {
        for (metadata, clustered, expected) in [
            (
                serde_json::json!({"status":"Running","status_code":103,"pid":42}),
                false,
                Some(42),
            ),
            (
                serde_json::json!({"status":"Stopped","status_code":102,"pid":42}),
                false,
                None,
            ),
            (
                serde_json::json!({"status":"Running","status_code":103,"pid":0}),
                false,
                None,
            ),
            (
                serde_json::json!({"status":"Running","status_code":103,"pid":-1}),
                false,
                None,
            ),
            (
                serde_json::json!({"status":"Running","status_code":103}),
                false,
                None,
            ),
            (
                serde_json::json!({"status":"Running","status_code":103,"pid":42}),
                true,
                None,
            ),
        ] {
            let directory = tempfile::tempdir()?;
            let socket = directory.path().join("incus.sock");
            let listener = UnixListener::bind(&socket)?;
            let server = tokio::spawn(async move {
                let mut steps = vec![(
                    "/1.0",
                    serde_json::json!({"environment":{"server_clustered":clustered}}),
                )];
                if !clustered {
                    steps.push(("/1.0/instances/Chatbot3tched/state", metadata));
                }
                for (path, metadata) in steps {
                    let (mut stream, _) = listener.accept().await?;
                    let mut bytes = Vec::new();
                    while !bytes.ends_with(b"\r\n\r\n") {
                        bytes.push(stream.read_u8().await?);
                        anyhow::ensure!(bytes.len() < 8192, "fixture headers exceeded bound");
                    }
                    let request = String::from_utf8(bytes)?;
                    anyhow::ensure!(
                        request.starts_with(&format!("GET {path} HTTP/1.1\r\n")),
                        "native PID request must use only the fixed local read paths"
                    );
                    let body = sync(metadata).to_string();
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream.write_all(response.as_bytes()).await?;
                }
                anyhow::Ok(())
            });
            let backend = super::IncusRootRepairBackend::on_socket(socket);
            assert_eq!(
                backend.instance_init_pid("Chatbot3tched").await.ok(),
                expected
            );
            server.await??;
        }
        Ok(())
    }

    async fn request_with_response(response: &[u8], deadline: Duration) -> anyhow::Result<Vec<u8>> {
        let directory = tempfile::tempdir()?;
        let socket = directory.path().join("incus.sock");
        let listener = UnixListener::bind(&socket)?;
        let payload = response.to_vec();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await?;
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                request.push(stream.read_u8().await?);
                anyhow::ensure!(request.len() <= 8192, "test request header too large");
            }
            stream.write_all(&payload).await?;
            if !payload.windows(5).any(|window| window == b"0\r\n\r\n") {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            anyhow::Ok(())
        });
        let result =
            IncusPlugin::incus_request_on_socket(&socket, deadline, "GET", "/1.0", None).await;
        server.abort();
        let _ = server.await;
        result
    }

    #[tokio::test]
    async fn accepts_chunked_json() {
        let response = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n"
            .iter()
            .chain(b"8\r\n{\"ok\":1}\r\n0\r\n\r\n")
            .copied()
            .collect::<Vec<_>>();
        let body = request_with_response(&response, Duration::from_secs(1))
            .await
            .expect("chunked response should succeed");
        assert_eq!(body, br#"{"ok":1}"#);
    }

    #[tokio::test]
    async fn rejects_truncated_chunked_response() {
        let response = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n";
        assert!(request_with_response(response, Duration::from_secs(1))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn rejects_truncated_content_length_response() {
        let response = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhi";
        assert!(request_with_response(response, Duration::from_secs(1))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn rejects_oversized_content_length_before_body() {
        let response = b"HTTP/1.1 200 OK\r\nContent-Length: 16777217\r\n\r\n";
        let error = request_with_response(response, Duration::from_secs(1))
            .await
            .expect_err("oversized response must be rejected");
        assert!(error.to_string().contains("size limit"));
    }

    #[tokio::test]
    async fn does_not_follow_redirects() {
        let response =
            b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/\r\nContent-Length: 0\r\n\r\n";
        let error = request_with_response(response, Duration::from_secs(1))
            .await
            .expect_err("redirect response must be rejected");
        assert!(error.to_string().contains("HTTP 302"));
    }

    #[tokio::test]
    async fn rejects_http_500() {
        let response = b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n";
        let error = request_with_response(response, Duration::from_secs(1))
            .await
            .expect_err("HTTP 500 must be rejected");
        assert!(error.to_string().contains("HTTP 500"));
    }

    #[tokio::test]
    async fn timeout_rejects_incomplete_response() {
        let response = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhe";
        assert!(request_with_response(response, Duration::from_millis(30))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn dispatch_rejects_unknown_and_malformed_calls_before_transport() {
        let malformed =
            super::dispatch_incus_method("start_instance", &serde_json::json!({"name": 42}))
                .await
                .expect_err("typed input must reject non-string names");
        assert!(malformed
            .to_string()
            .contains("invalid start_instance arguments"));

        let unknown = super::dispatch_incus_method("not_declared", &serde_json::json!({}))
            .await
            .expect_err("undeclared methods must not reach a backend");
        assert!(unknown.to_string().contains("unknown Incus method"));
    }

    #[test]
    fn dedicated_outputs_are_non_ack_contracts() {
        let output = super::InstanceStateOutput {
            name: "demo".into(),
            state: "Running".into(),
            changed: true,
            response: serde_json::json!({"status": "Success"}),
        };
        let value = serde_json::to_value(output).expect("typed output serializes");
        assert_eq!(value["state"], "Running");
        assert_eq!(value["changed"], true);
        assert!(value.get("success").is_none());
    }

    struct Step {
        method: &'static str,
        path: String,
        body: Option<serde_json::Value>,
        if_match: Option<&'static str>,
        status: u16,
        response: String,
        etag: Option<&'static str>,
    }

    impl Step {
        fn json(
            method: &'static str,
            path: &str,
            body: Option<serde_json::Value>,
            response: serde_json::Value,
        ) -> Self {
            Self {
                method,
                path: path.into(),
                body,
                if_match: None,
                status: 200,
                response: response.to_string(),
                etag: None,
            }
        }
    }

    fn sync(metadata: serde_json::Value) -> serde_json::Value {
        serde_json::json!({"type": "sync", "status": "Success", "status_code": 200, "error": "", "metadata": metadata})
    }

    fn asynchronous() -> serde_json::Value {
        serde_json::json!({
            "type": "async", "status": "Operation created", "status_code": 100, "error": "",
            "operation": "/1.0/operations/12345678-1234-1234-1234-123456789abc"
        })
    }

    fn finished(metadata: serde_json::Value) -> Step {
        Step::json(
            "GET",
            "/1.0/operations/12345678-1234-1234-1234-123456789abc/wait?timeout=120",
            None,
            sync(
                serde_json::json!({"status": "Success", "status_code": 200, "err": "", "metadata": metadata}),
            ),
        )
    }

    /// Exercise the real bounded reqwest Unix transport and the full method
    /// dispatcher. No environment override or live Incus socket is consulted.
    async fn dispatch_fixture_receipt(
        method: &str,
        args: serde_json::Value,
        steps: Vec<Step>,
        observed: bool,
    ) -> anyhow::Result<super::IncusMethodObservation> {
        let directory = tempfile::tempdir()?;
        let socket = directory.path().join("incus.sock");
        let listener = UnixListener::bind(&socket)?;
        let server = tokio::spawn(async move {
            for step in steps {
                let (mut stream, _) =
                    tokio::time::timeout(Duration::from_secs(3), listener.accept()).await??;
                let mut header = Vec::new();
                while !header.ends_with(b"\r\n\r\n") {
                    header.push(stream.read_u8().await?);
                    anyhow::ensure!(header.len() <= 8192, "fixture request headers too large");
                }
                let header = String::from_utf8(header)?;
                let mut lines = header.split("\r\n");
                anyhow::ensure!(
                    lines.next()
                        == Some(format!("{} {} HTTP/1.1", step.method, step.path).as_str()),
                    "unexpected request: {header}"
                );
                let headers: std::collections::HashMap<_, _> = lines
                    .filter_map(|line| line.split_once(':'))
                    .map(|(key, value)| (key.to_ascii_lowercase(), value.trim().to_string()))
                    .collect();
                anyhow::ensure!(
                    headers.get("if-match").map(String::as_str) == step.if_match,
                    "wrong or absent If-Match header"
                );
                let length = headers
                    .get("content-length")
                    .map(|value| value.parse::<usize>())
                    .transpose()?
                    .unwrap_or(0);
                anyhow::ensure!(length <= 65536, "fixture request body too large");
                let mut bytes = vec![0; length];
                stream.read_exact(&mut bytes).await?;
                let body = if bytes.is_empty() {
                    None
                } else {
                    Some(serde_json::from_slice::<serde_json::Value>(&bytes)?)
                };
                anyhow::ensure!(
                    body == step.body,
                    "unexpected body: {body:?}, expected {:?}",
                    step.body
                );
                let etag = step
                    .etag
                    .map(|value| format!("ETag: {value}\r\n"))
                    .unwrap_or_default();
                let response = format!(
                    "HTTP/1.1 {} Fixture\r\nContent-Type: application/json\r\n{etag}Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                    step.status,
                    step.response.len(),
                    step.response
                );
                stream.write_all(response.as_bytes()).await?;
            }
            anyhow::Ok(())
        });
        let api = super::IncusApi {
            socket,
            deadline: Duration::from_secs(2),
        };
        let result = if observed {
            super::dispatch_incus_with_api_observed(&api, method, &args).await
        } else {
            super::dispatch_incus_with_api(&api, method, &args)
                .await
                .map(|result| super::IncusMethodObservation {
                    result,
                    state: None,
                })
        };
        server.await??;
        result
    }

    async fn dispatch_fixture(
        method: &str,
        args: serde_json::Value,
        steps: Vec<Step>,
    ) -> anyhow::Result<serde_json::Value> {
        Ok(dispatch_fixture_receipt(method, args, steps, false)
            .await?
            .result)
    }

    async fn dispatch_observed_fixture(
        method: &str,
        args: serde_json::Value,
        steps: Vec<Step>,
    ) -> anyhow::Result<super::IncusMethodObservation> {
        dispatch_fixture_receipt(method, args, steps, true).await
    }

    #[tokio::test]
    async fn lifecycle_methods_send_native_actions_and_await_completion() {
        for (method, action, force, expected) in [
            ("start_instance", "start", false, "Running"),
            ("stop_instance", "stop", true, "Stopped"),
            ("restart_instance", "restart", true, "Running"),
            ("freeze_instance", "freeze", false, "Frozen"),
            ("unfreeze_instance", "unfreeze", false, "Running"),
        ] {
            let result = dispatch_fixture(
                method,
                serde_json::json!({"name": "demo", "force": force}),
                vec![
                    Step::json(
                        "PUT",
                        "/1.0/instances/demo/state",
                        Some(serde_json::json!({"action": action, "force": force, "timeout": 120})),
                        asynchronous(),
                    ),
                    finished(serde_json::json!({})),
                ],
            )
            .await
            .expect("native lifecycle operation completes");
            assert_eq!(result["state"], expected);
            assert_eq!(result["changed"], true);
        }
    }

    #[tokio::test]
    async fn represented_mutation_returns_complete_sorted_native_readback() {
        let observed = dispatch_observed_fixture(
            "start_instance",
            serde_json::json!({"name": "demo"}),
            vec![
                Step::json(
                    "PUT",
                    "/1.0/instances/demo/state",
                    Some(serde_json::json!({
                        "action": "start", "force": false, "timeout": 120
                    })),
                    asynchronous(),
                ),
                finished(serde_json::json!({})),
                default_project_snapshot(vec![
                    observed_instance("zeta", "Stopped", 102),
                    observed_instance("demo", "Running", 103),
                ]),
            ],
        )
        .await
        .expect("completed mutation has an authoritative readback");
        assert_eq!(observed.result["state"], "Running");
        let state = observed
            .state
            .expect("represented mutation publishes state");
        assert_eq!(state.instances.len(), 2);
        assert_eq!(state.instances[0].name, "demo");
        assert_eq!(state.instances[0].status, "Running");
        assert_eq!(state.instances[0].devices[0].name, "root");
        assert_eq!(state.instances[0].project.as_deref(), Some("default"));
        assert_eq!(state.instances[0].location.as_deref(), Some("none"));
        assert_eq!(state.instances[1].name, "zeta");
    }

    #[tokio::test]
    async fn snapshot_method_does_not_fabricate_unmodeled_state() {
        let observed = dispatch_observed_fixture(
            "snapshot_instance",
            serde_json::json!({"name": "demo", "snapshot_name": "before"}),
            vec![
                Step::json(
                    "POST",
                    "/1.0/instances/demo/snapshots",
                    Some(serde_json::json!({"name": "before"})),
                    asynchronous(),
                ),
                finished(serde_json::json!({})),
            ],
        )
        .await
        .expect("snapshot operation completes");
        assert_eq!(observed.result["created"], true);
        assert!(observed.state.is_none());
    }

    #[tokio::test]
    async fn completed_mutation_with_failed_readback_is_an_honest_error() {
        let mut failed_readback = default_project_snapshot(Vec::new());
        failed_readback.status = 500;
        failed_readback.response = serde_json::json!({
            "type": "error", "status_code": 500, "error": "inventory unavailable"
        })
        .to_string();
        let error = dispatch_observed_fixture(
            "start_instance",
            serde_json::json!({"name": "demo"}),
            vec![
                Step::json(
                    "PUT",
                    "/1.0/instances/demo/state",
                    Some(serde_json::json!({
                        "action": "start", "force": false, "timeout": 120
                    })),
                    asynchronous(),
                ),
                finished(serde_json::json!({})),
                failed_readback,
            ],
        )
        .await
        .expect_err("native completion without readback cannot be success");
        let message = format!("{error:#}");
        assert!(message.contains("completed"));
        assert!(message.contains("reconciliation is required"));
        assert!(message.contains("inventory unavailable"));
    }

    #[test]
    fn snapshot_parser_rejects_missing_required_and_lossy_devices() {
        let mut missing = observed_instance("demo", "Running", 103);
        missing.as_object_mut().unwrap().remove("ephemeral");
        assert!(
            IncusPlugin::parse_instance_list(serde_json::to_vec(&vec![missing]).unwrap()).is_err()
        );

        let mut lossy = observed_instance("demo", "Running", 103);
        lossy["devices"]["root"]["future.provider.key"] = "must-not-disappear".into();
        let error = IncusPlugin::parse_instance_list(serde_json::to_vec(&vec![lossy]).unwrap())
            .expect_err("unknown device data cannot be silently dropped");
        assert!(error.to_string().contains("cannot preserve"));

        let mut wrong_project = observed_instance("demo", "Running", 103);
        wrong_project["project"] = "other".into();
        assert!(IncusPlugin::parse_instance_list(
            serde_json::to_vec(&vec![wrong_project]).unwrap()
        )
        .is_err());
    }

    #[test]
    fn snapshot_preserves_absent_and_explicitly_empty_expanded_devices() {
        let mut raw = observed_instance("demo", "Running", 103);
        raw["expanded_devices"] = serde_json::json!({});
        let empty = IncusPlugin::parse_instance_list(
            serde_json::to_vec(&vec![raw.clone()]).unwrap(),
        ).unwrap().remove(0);
        assert!(empty.expanded_devices.as_ref().unwrap().is_empty());
        let serialized = serde_json::to_value(&empty).unwrap();
        assert_eq!(serialized["expanded_devices"], serde_json::json!([]));

        raw.as_object_mut().unwrap().remove("expanded_devices");
        let missing = IncusPlugin::parse_instance_list(
            serde_json::to_vec(&vec![raw]).unwrap(),
        ).unwrap().remove(0);
        assert!(missing.expanded_devices.is_none());
        assert!(serde_json::to_value(&missing).unwrap().get("expanded_devices").is_none());

        let schema = serde_json::to_value(schemars::schema_for!(IncusInstance)).unwrap();
        assert_eq!(schema["properties"]["expanded_devices"]["type"], "array");
        assert!(!schema["required"].as_array().unwrap().iter()
            .any(|field| field.as_str() == Some("expanded_devices")));
    }

    #[test]
    fn individual_native_instance_conversion_preserves_device_maps_and_uuid() {
        let mut native = observed_instance("Chatbot3tched", "Running", 103);
        native["config"]["volatile.uuid"] = "4dece3a4-d987-44a0-b8b5-d7cb27211ed0".into();
        native.as_object_mut().unwrap().remove("project");
        let typed = IncusPlugin::instance_from_native_metadata(native.clone()).unwrap();
        assert_eq!(typed.name, "Chatbot3tched");
        assert_eq!(typed.project.as_deref(), Some("default"));
        assert_eq!(
            typed
                .config
                .as_ref()
                .unwrap()
                .get("volatile.uuid")
                .map(String::as_str),
            Some("4dece3a4-d987-44a0-b8b5-d7cb27211ed0")
        );
        assert_eq!(
            typed.devices.len(),
            native["devices"].as_object().unwrap().len()
        );
        native["devices"]["root"]["future.provider.key"] = "must-not-disappear".into();
        assert!(IncusPlugin::instance_from_native_metadata(native).is_err());
    }

    #[tokio::test]
    async fn create_sends_type_restrictions_and_root_device_in_one_native_operation() {
        let args = serde_json::json!({
            "name": "demo", "instance_type": "virtual-machine", "image": "images:debian/12",
            "profiles": [], "config": {"security.nesting": "false"},
            "devices": [{"name": "root", "device": {"type": "disk", "path": "/", "pool": "default"}}]
        });
        let body = serde_json::json!({
            "name": "demo", "type": "virtual-machine",
            "source": {"type": "image", "alias": "debian/12", "mode": "pull", "protocol": "simplestreams", "server": "https://images.linuxcontainers.org"},
            "profiles": [], "config": {"security.nesting": "false"},
            "devices": {"root": {"type": "disk", "path": "/", "pool": "default"}}
        });
        let result = dispatch_fixture(
            "create_instance",
            args,
            vec![
                Step::json("POST", "/1.0/instances", Some(body), asynchronous()),
                finished(serde_json::json!({})),
            ],
        )
        .await
        .expect("complete native creation");
        assert_eq!(result["created"], true);
    }

    fn native_instance() -> serde_json::Value {
        serde_json::json!({
            "name": "demo", "status": "Stopped", "status_code": 102, "type": "container",
            "architecture": "x86_64", "description": "retained", "ephemeral": false, "profiles": ["default"],
            "config": {"limits.cpu": "2", "user.keep": "unchanged", "volatile.base_image": "provider-owned"},
            "devices": {
                "root": {"type": "disk", "path": "/", "pool": "default"},
                "future": {"type": "future-provider-type", "custom": "must-survive"}
            },
            "expanded_devices": {"read-only": {"type": "none"}},
            "created_at": "2026-09-13T00:00:00Z"
        })
    }

    fn observed_instance(name: &str, status: &str, status_code: i64) -> serde_json::Value {
        serde_json::json!({
            "name": name, "status": status, "status_code": status_code, "type": "container",
            "architecture": "x86_64", "description": "fixture", "ephemeral": false,
            "stateful": false, "profiles": ["default"], "config": {"limits.cpu": "2"},
            "devices": {"root": {"type": "disk", "path": "/", "pool": "default"}},
            "expanded_config": {"limits.cpu": "2"},
            "expanded_devices": {"root": {"type": "disk", "path": "/", "pool": "default"}},
            "created_at": "2026-09-13T00:00:00Z", "last_used_at": "2026-09-13T00:00:00Z",
            "location": "none", "project": "default"
        })
    }

    fn default_project_snapshot(instances: Vec<serde_json::Value>) -> Step {
        Step::json(
            "GET",
            "/1.0/instances?recursion=1&project=default",
            None,
            sync(serde_json::Value::Array(instances)),
        )
    }

    async fn accept_snapshot_request(
        listener: &UnixListener,
        instances: Vec<serde_json::Value>,
    ) -> anyhow::Result<()> {
        let (mut stream, _) = listener.accept().await?;
        let mut header = Vec::new();
        while !header.ends_with(b"\r\n\r\n") {
            header.push(stream.read_u8().await?);
            anyhow::ensure!(header.len() <= 8192, "fixture request headers too large");
        }
        let header = String::from_utf8(header)?;
        anyhow::ensure!(
            header.starts_with("GET /1.0/instances?recursion=1&project=default HTTP/1.1\r\n"),
            "unexpected snapshot request: {header}"
        );
        let body = sync(serde_json::Value::Array(instances)).to_string();
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .await?;
        Ok(())
    }

    fn lifecycle(action: &str, name: &str) -> Message {
        Message::Text(
            serde_json::json!({
                "type": "lifecycle",
                "timestamp": "2026-09-13T00:00:00Z",
                "metadata": {
                    "action": action,
                    "source": format!("/1.0/instances/{name}"),
                    "context": {},
                    "name": name,
                    "project": "default"
                },
                "location": "none",
                "project": "default"
            })
            .to_string(),
        )
    }

    // Tungstenite requires its concrete HTTP error response in this callback.
    #[allow(clippy::result_large_err)]
    async fn accept_event_subscription(
        listener: &UnixListener,
    ) -> anyhow::Result<tokio_tungstenite::WebSocketStream<tokio::net::UnixStream>> {
        let (stream, _) = listener.accept().await?;
        let websocket = tokio_tungstenite::accept_hdr_async(
            stream,
            |request: &tokio_tungstenite::tungstenite::handshake::server::Request,
             response: tokio_tungstenite::tungstenite::handshake::server::Response| {
                assert_eq!(
                    request.uri().to_string(),
                    super::INCUS_LIFECYCLE_EVENTS_PATH
                );
                Ok(response)
            },
        )
        .await?;
        Ok(websocket)
    }

    #[tokio::test]
    async fn lifecycle_subscription_precedes_snapshot_and_events_replace_complete_root() {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("incus.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = tokio::spawn(async move {
            let mut events = accept_event_subscription(&listener).await?;
            accept_snapshot_request(&listener, vec![observed_instance("parent", "Running", 103)])
                .await?;
            events.send(lifecycle("instance-created", "child")).await?;
            accept_snapshot_request(
                &listener,
                vec![
                    observed_instance("parent", "Running", 103),
                    observed_instance("child", "Stopped", 102),
                ],
            )
            .await?;
            events.send(lifecycle("instance-deleted", "child")).await?;
            accept_snapshot_request(&listener, vec![observed_instance("parent", "Running", 103)])
                .await?;
            anyhow::Ok(())
        });

        let mut observer = super::IncusLifecycleObserver::connect_on_socket(socket)
            .await
            .expect("subscribe before snapshot");
        let initial = observer.default_project_state().await.unwrap();
        assert_eq!(initial.instances.len(), 1);
        observer.wait_for_lifecycle().await.unwrap();
        let after_add = observer.default_project_state().await.unwrap();
        assert_eq!(
            after_add
                .instances
                .iter()
                .map(|instance| instance.name.as_str())
                .collect::<Vec<_>>(),
            ["child", "parent"]
        );
        observer.wait_for_lifecycle().await.unwrap();
        let after_delete = observer.default_project_state().await.unwrap();
        assert_eq!(after_delete.instances.len(), 1);
        assert_eq!(after_delete.instances[0].name, "parent");
        server.await.unwrap().unwrap();
    }

    async fn rejected_event_retains_initial_state(message: Message) -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let socket = directory.path().join("incus.sock");
        let listener = UnixListener::bind(&socket)?;
        let server = tokio::spawn(async move {
            let mut events = accept_event_subscription(&listener).await?;
            accept_snapshot_request(
                &listener,
                vec![observed_instance("retained", "Running", 103)],
            )
            .await?;
            // The client may close immediately on a capacity/protocol error;
            // either successful delivery or that peer close is expected here.
            let _ = events.send(message).await;
            anyhow::Ok(())
        });
        let mut observer = super::IncusLifecycleObserver::connect_on_socket(socket).await?;
        let retained = observer.default_project_state().await?;
        anyhow::ensure!(observer.wait_for_lifecycle().await.is_err());
        anyhow::ensure!(retained.instances[0].name == "retained");
        server.await??;
        Ok(())
    }

    #[tokio::test]
    async fn malformed_and_oversized_events_do_not_replace_the_last_snapshot() {
        rejected_event_retains_initial_state(Message::Text("not-json".into()))
            .await
            .unwrap();
        rejected_event_retains_initial_state(Message::Text(
            "x".repeat(super::INCUS_EVENT_MESSAGE_LIMIT + 1),
        ))
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn closed_event_transport_does_not_replace_the_last_snapshot() {
        rejected_event_retains_initial_state(Message::Close(None))
            .await
            .unwrap();
    }

    fn instance_read() -> Step {
        let mut step = Step::json("GET", "/1.0/instances/demo", None, sync(native_instance()));
        step.etag = Some("\"revision-1\"");
        step
    }

    fn writable_instance() -> serde_json::Value {
        let mut instance = native_instance();
        instance.as_object_mut().unwrap().retain(|key, _| {
            matches!(
                key.as_str(),
                "architecture" | "description" | "ephemeral" | "profiles" | "config" | "devices"
            )
        });
        instance
    }

    fn instance_write(body: serde_json::Value) -> Step {
        let mut step = Step::json("PUT", "/1.0/instances/demo", Some(body), asynchronous());
        step.if_match = Some("\"revision-1\"");
        step
    }

    #[tokio::test]
    async fn modify_merges_updates_and_preserves_native_fields_with_etag() {
        let mut body = writable_instance();
        body["config"]["limits.cpu"] = "4".into();
        body["devices"]["root"]["pool"] = "new-pool".into();
        let result = dispatch_fixture(
            "modify_instance",
            serde_json::json!({
                "name": "demo", "config": {"limits.cpu": "4"},
                "devices": {"root": {"type": "disk", "path": "/", "pool": "new-pool"}}
            }),
            vec![
                instance_read(),
                instance_write(body),
                finished(serde_json::json!({})),
            ],
        )
        .await
        .expect("modify with optimistic concurrency");
        assert_eq!(result["updated"], true);
        assert_eq!(result["changes"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn device_methods_handle_root_and_preserve_unknown_siblings() {
        for method in ["add_device", "update_device", "remove_device"] {
            let name = if method == "add_device" {
                "extra"
            } else {
                "root"
            };
            let mut args = serde_json::json!({"instance_name": "demo", "device_name": name});
            let mut body = writable_instance();
            if method == "remove_device" {
                body["devices"].as_object_mut().unwrap().remove(name);
            } else {
                args["device"] =
                    serde_json::json!({"type": "disk", "path": "/data", "source": "/srv/data"});
                body["devices"][name] = args["device"].clone();
            }
            let result = dispatch_fixture(
                method,
                args,
                vec![
                    instance_read(),
                    instance_write(body),
                    finished(serde_json::json!({})),
                ],
            )
            .await
            .expect("device mutation");
            assert_eq!(result["changed"], true);
        }
        let duplicate = dispatch_fixture(
            "add_device",
            serde_json::json!({
                "instance_name": "demo", "device_name": "root",
                "device": {"type": "disk", "path": "/", "pool": "default"}
            }),
            vec![instance_read()],
        )
        .await
        .expect_err("add cannot replace an existing device");
        assert!(duplicate.to_string().contains("already exists"));
        let missing = dispatch_fixture(
            "update_device",
            serde_json::json!({
                "instance_name": "demo", "device_name": "missing",
                "device": {"type": "disk", "path": "/", "pool": "default"}
            }),
            vec![instance_read()],
        )
        .await
        .expect_err("update cannot create an absent device");
        assert!(missing.to_string().contains("does not exist"));
    }

    #[tokio::test]
    async fn snapshot_and_delete_wait_for_provider_completion() {
        let snapshot = dispatch_fixture(
            "snapshot_instance",
            serde_json::json!({"name": "demo", "snapshot_name": "before"}),
            vec![
                Step::json(
                    "POST",
                    "/1.0/instances/demo/snapshots",
                    Some(serde_json::json!({"name": "before"})),
                    asynchronous(),
                ),
                finished(serde_json::json!({})),
            ],
        )
        .await
        .expect("snapshot completes");
        assert_eq!(snapshot["created"], true);
        let deleted = dispatch_fixture(
            "delete_instance",
            serde_json::json!({"name": "demo"}),
            vec![
                instance_read(),
                Step::json("DELETE", "/1.0/instances/demo", None, asynchronous()),
                finished(serde_json::json!({})),
            ],
        )
        .await
        .expect("delete completes");
        assert_eq!(deleted["deleted"], true);
    }

    #[tokio::test]
    async fn exec_preserves_argv_and_returns_exit_status_and_captured_output() {
        let command = serde_json::json!(["/bin/tool", "literal;$(not-a-host-command)", ""]);
        let mut stdout = Step::json(
            "GET",
            "/1.0/instances/demo/logs/exec-output/exec-one.stdout",
            None,
            serde_json::Value::Null,
        );
        stdout.response = "hello\n".into();
        let mut stderr = Step::json(
            "GET",
            "/1.0/instances/demo/logs/exec-output/exec-one.stderr",
            None,
            serde_json::Value::Null,
        );
        stderr.response = "diagnostic\n".into();
        let result = dispatch_fixture("exec_instance", serde_json::json!({"name": "demo", "command": command}), vec![
            Step::json("POST", "/1.0/instances/demo/exec", Some(serde_json::json!({
                "command": command, "wait-for-websocket": false, "interactive": false, "record-output": true
            })), asynchronous()),
            finished(serde_json::json!({"return": 7, "output": {
                "1": "/1.0/instances/demo/logs/exec-output/exec-one.stdout",
                "2": "/1.0/instances/demo/logs/exec-output/exec-one.stderr"
            }})),
            stdout, stderr,
        ]).await.expect("exec return is distinct from operation completion");
        assert_eq!(result["exit_code"], 7);
        assert_eq!(result["stdout"], "hello\n");
        assert_eq!(result["stderr"], "diagnostic\n");
    }

    #[tokio::test]
    async fn provider_errors_pending_operations_and_etag_conflicts_are_not_success() {
        let mut conflict = instance_write({
            let mut body = writable_instance();
            body["config"]["limits.cpu"] = "4".into();
            body
        });
        conflict.status = 412;
        conflict.response =
            serde_json::json!({"type": "error", "error": "ETag mismatch"}).to_string();
        let error = dispatch_fixture(
            "modify_instance",
            serde_json::json!({"name": "demo", "config": {"limits.cpu": "4"}}),
            vec![instance_read(), conflict],
        )
        .await
        .expect_err("concurrent changes cannot be overwritten");
        assert!(error.to_string().contains("HTTP 412"));

        for (status, code, error) in [
            ("Running", 103, ""),
            ("Failure", 400, "cannot stop instance"),
        ] {
            let wait = Step::json(
                "GET",
                "/1.0/operations/12345678-1234-1234-1234-123456789abc/wait?timeout=120",
                None,
                sync(
                    serde_json::json!({"status": status, "status_code": code, "err": error, "metadata": {}}),
                ),
            );
            let result = dispatch_fixture(
                "stop_instance",
                serde_json::json!({"name": "demo"}),
                vec![
                    Step::json(
                        "PUT",
                        "/1.0/instances/demo/state",
                        Some(serde_json::json!({"action": "stop", "force": false, "timeout": 120})),
                        asynchronous(),
                    ),
                    wait,
                ],
            )
            .await
            .expect_err("unfinished or failed operation must not acknowledge completion");
            assert!(result
                .to_string()
                .contains(if error.is_empty() { status } else { error }));
        }
        let error = dispatch_fixture("delete_instance", serde_json::json!({"name": "demo"}), vec![
            instance_read(),
            Step::json("DELETE", "/1.0/instances/demo", None,
                serde_json::json!({"type": "error", "status_code": 200, "status": "Success", "error": "denied", "metadata": {}}))
        ]).await.expect_err("error envelope cannot pass as success");
        assert!(error.to_string().contains("denied"));
    }

    #[tokio::test]
    async fn forced_delete_stops_first_and_accounts_for_ephemeral_deletion() {
        for ephemeral in [false, true] {
            let mut instance = native_instance();
            instance["status"] = "Running".into();
            instance["status_code"] = 101.into();
            instance["ephemeral"] = ephemeral.into();
            let mut read = Step::json("GET", "/1.0/instances/demo", None, sync(instance));
            read.etag = Some("\"revision-1\"");
            let mut steps = vec![
                read,
                Step::json(
                    "PUT",
                    "/1.0/instances/demo/state",
                    Some(serde_json::json!({"action": "stop", "force": true, "timeout": 120})),
                    asynchronous(),
                ),
                finished(serde_json::json!({})),
            ];
            if !ephemeral {
                steps.push(Step::json(
                    "DELETE",
                    "/1.0/instances/demo",
                    None,
                    asynchronous(),
                ));
                steps.push(finished(serde_json::json!({})));
            }
            let deleted = dispatch_fixture(
                "delete_instance",
                serde_json::json!({"name": "demo"}),
                steps,
            )
            .await
            .expect("force deletion completes");
            assert_eq!(deleted["deleted"], true);
        }
    }

    #[tokio::test]
    async fn invalid_names_and_inputs_never_reach_a_socket() {
        let api = super::IncusApi {
            socket: "/nonexistent/fixture.sock".into(),
            deadline: Duration::from_millis(1),
        };
        for name in [
            "",
            ".",
            "..",
            "../other",
            "demo?project=other",
            "demo%2fstate",
            "demo#ignored",
            "demo\\state",
        ] {
            let error = super::dispatch_incus_with_api(
                &api,
                "start_instance",
                &serde_json::json!({"name": name}),
            )
            .await
            .expect_err("unsafe name");
            assert!(error.to_string().contains("invalid Incus instance name"));
        }
        for args in [
            serde_json::json!({"name": "demo", "command": []}),
            serde_json::json!({"name": "demo", "command": [""]}),
            serde_json::json!({"name": "demo", "command": ["tool", "\0"]}),
        ] {
            assert!(super::dispatch_incus_with_api(&api, "exec_instance", &args)
                .await
                .unwrap_err()
                .to_string()
                .contains("Incus exec"));
        }
        let error = super::dispatch_incus_with_api(
            &api,
            "modify_instance",
            &serde_json::json!({"name": "demo", "config": {"volatile.uuid": "forged"}}),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("provider-owned"));
    }

    #[tokio::test]
    async fn provider_operation_and_exec_log_paths_cannot_escape_the_target_api() {
        let mut response = asynchronous();
        response["operation"] = "/1.0/instances/other".into();
        let error = dispatch_fixture(
            "start_instance",
            serde_json::json!({"name": "demo"}),
            vec![Step::json(
                "PUT",
                "/1.0/instances/demo/state",
                Some(serde_json::json!({"action": "start", "force": false, "timeout": 120})),
                response,
            )],
        )
        .await
        .expect_err("operation URL cannot select another privileged endpoint");
        assert!(error.to_string().contains("invalid Incus operation path"));
        let error = dispatch_fixture("exec_instance", serde_json::json!({"name": "demo", "command": ["true"]}), vec![
            Step::json("POST", "/1.0/instances/demo/exec", Some(serde_json::json!({
                "command": ["true"], "wait-for-websocket": false, "interactive": false, "record-output": true
            })), asynchronous()),
            finished(serde_json::json!({"return": 0, "output": {"1": "/1.0/instances/other/logs/private"}}))
        ]).await.expect_err("output URL cannot read another instance");
        assert!(error.to_string().contains("outside the target instance"));
    }
}
