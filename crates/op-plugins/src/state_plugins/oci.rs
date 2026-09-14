//! OCI application-container lifecycle backed by the native Incus REST API.
//!
//! Incus imports OCI registry images, initializes its `oci.*` configuration
//! from the image configuration, and owns instance lifecycle. This plugin does
//! not interpret runc bundles or invoke Docker-compatible tooling.

use anyhow::{Context, Result};
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{PluginSchema, SideEffect};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;
use std::collections::BTreeMap;

use super::incus::{instance_path, validate_segment, IncusApi};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-oscal-subid" = "sch.container.oci.schema@v2"))]
pub struct OciState {
    pub provider: String,
    pub instance_type: String,
}

impl Default for OciState {
    fn default() -> Self {
        Self {
            provider: "incus".into(),
            instance_type: "application_container".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PullImageInput {
    /// HTTPS OCI registry endpoint, for example `https://docker.io`.
    pub registry: String,
    /// OCI repository reference accepted by the registry, for example `library/alpine:3.22`.
    pub image: String,
    #[serde(default)]
    pub auto_update: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateContainerInput {
    pub name: String,
    pub registry: String,
    pub image: String,
    #[serde(default)]
    pub privileged: bool,
    #[serde(default)]
    pub ephemeral: bool,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

pub type RunContainerInput = CreateContainerInput;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StartContainerInput {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum KillSignal {
    Sigkill,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KillContainerInput {
    pub name: String,
    #[serde(default)]
    pub signal: Option<KillSignal>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteContainerInput {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetContainerStateInput {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImagePullOutput {
    pub registry: String,
    pub image: String,
    pub fingerprint: String,
    pub cached: bool,
    pub auto_update: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContainerStateOutput {
    pub name: String,
    pub status: String,
    pub status_code: i64,
    pub instance_type: String,
    pub ephemeral: bool,
    pub image_fingerprint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContainerMutationOutput {
    pub operation: String,
    pub changed: bool,
    pub name: String,
    pub deleted: bool,
    pub container: Option<ContainerStateOutput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteContainerOutput {
    pub name: String,
    pub deleted: bool,
}

fn validate_image_source(registry: &str, image: &str) -> Result<()> {
    let url = reqwest::Url::parse(registry).context("OCI registry must be a valid HTTPS URL")?;
    anyhow::ensure!(url.scheme() == "https", "OCI registry must use HTTPS");
    anyhow::ensure!(
        url.host_str().is_some(),
        "OCI registry URL must include a host"
    );
    anyhow::ensure!(
        url.username().is_empty() && url.password().is_none(),
        "OCI registry URL must not contain credentials"
    );
    anyhow::ensure!(
        url.query().is_none() && url.fragment().is_none(),
        "OCI registry URL must not contain a query or fragment"
    );
    anyhow::ensure!(
        !image.is_empty()
            && image.len() <= 512
            && !image.chars().any(|c| c.is_whitespace() || c.is_control()),
        "OCI image reference must be nonempty and contain no whitespace or control characters"
    );
    Ok(())
}

fn validate_environment(env: &BTreeMap<String, String>) -> Result<()> {
    for (key, value) in env {
        let mut bytes = key.bytes();
        anyhow::ensure!(
            bytes
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
                && bytes.all(|c| c.is_ascii_alphanumeric() || c == b'_'),
            "invalid environment variable name '{key}'"
        );
        anyhow::ensure!(!value.contains('\0'), "environment value contains NUL");
    }
    Ok(())
}

fn reject_retired_loopback(args: &serde_json::Value) -> Result<()> {
    if args.get("loopback_required").is_some() {
        anyhow::bail!(
            "loopback_required is unsupported: Incus owns OCI network-namespace initialization and no separate loopback mutation backend is registered"
        );
    }
    Ok(())
}

fn image_source(registry: &str, image: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "image", "mode": "pull", "protocol": "oci",
        "server": registry, "alias": image
    })
}

fn instance_body(input: &CreateContainerInput) -> Result<serde_json::Value> {
    validate_segment(&input.name, "OCI instance name")?;
    validate_image_source(&input.registry, &input.image)?;
    validate_environment(&input.env)?;
    let mut config = serde_json::Map::new();
    config.insert(
        "security.privileged".into(),
        input.privileged.to_string().into(),
    );
    for (key, value) in &input.env {
        config.insert(format!("environment.{key}"), value.clone().into());
    }
    Ok(serde_json::json!({
        "name": input.name,
        "type": "container",
        "ephemeral": input.ephemeral,
        "source": image_source(&input.registry, &input.image),
        "config": config
    }))
}

fn parse_instance(value: &serde_json::Value) -> Result<ContainerStateOutput> {
    let config = value
        .get("config")
        .and_then(serde_json::Value::as_object)
        .context("Incus instance is missing config")?;
    let output = ContainerStateOutput {
        name: value["name"]
            .as_str()
            .context("Incus instance is missing name")?
            .into(),
        status: value["status"]
            .as_str()
            .context("Incus instance is missing status")?
            .into(),
        status_code: value["status_code"]
            .as_i64()
            .context("Incus instance is missing status_code")?,
        instance_type: value["type"]
            .as_str()
            .context("Incus instance is missing type")?
            .into(),
        ephemeral: value["ephemeral"]
            .as_bool()
            .context("Incus instance is missing ephemeral")?,
        image_fingerprint: config
            .get("volatile.base_image")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
    };
    anyhow::ensure!(
        output.instance_type == "container",
        "Incus provider returned a non-container instance"
    );
    let expanded = value
        .get("expanded_config")
        .and_then(serde_json::Value::as_object);
    let is_oci = config
        .get("volatile.container.oci")
        .and_then(serde_json::Value::as_str)
        == Some("true")
        || config.get("image.type").and_then(serde_json::Value::as_str) == Some("oci")
        || expanded
            .and_then(|config| config.get("volatile.container.oci"))
            .and_then(serde_json::Value::as_str)
            == Some("true")
        || expanded
            .and_then(|config| config.get("image.type"))
            .and_then(serde_json::Value::as_str)
            == Some("oci");
    anyhow::ensure!(is_oci, "Incus instance is not an OCI application container");
    Ok(output)
}

async fn read_instance(api: &IncusApi, name: &str) -> Result<ContainerStateOutput> {
    let (value, _) = api.instance(name).await?;
    let observed = parse_instance(&value)?;
    anyhow::ensure!(
        observed.name == name,
        "Incus readback returned foreign instance '{}' for requested '{name}'",
        observed.name
    );
    Ok(observed)
}

async fn create_stopped(
    api: &IncusApi,
    input: &CreateContainerInput,
) -> Result<ContainerStateOutput> {
    let body = instance_body(input)?;
    api.call("POST", "/1.0/instances", Some(&body.to_string()), None)
        .await?;
    let observed = read_instance(api, &input.name).await.context(
        "Incus create completed but instance readback failed; creation effect is indeterminate",
    )?;
    anyhow::ensure!(
        observed.status == "Stopped",
        "Incus create completed but instance readback status was '{}' rather than 'Stopped'",
        observed.status
    );
    Ok(observed)
}

async fn set_state(
    api: &IncusApi,
    name: &str,
    action: &str,
    force: bool,
    expected: &str,
) -> Result<ContainerStateOutput> {
    let body = serde_json::json!({"action": action, "force": force, "timeout": 120});
    api.call(
        "PUT",
        &format!("{}/state", instance_path(name)?),
        Some(&body.to_string()),
        None,
    )
    .await?;
    let observed = read_instance(api, name).await.with_context(|| {
        format!("Incus {action} completed but instance readback failed; lifecycle state is indeterminate")
    })?;
    anyhow::ensure!(
        observed.status == expected,
        "Incus {action} completed but observed status was '{}' rather than '{expected}'",
        observed.status
    );
    Ok(observed)
}

async fn force_stop(
    api: &IncusApi,
    before: ContainerStateOutput,
) -> Result<(bool, Option<ContainerStateOutput>)> {
    let body = serde_json::json!({"action":"stop","force":true,"timeout":120});
    api.call(
        "PUT",
        &format!("{}/state", instance_path(&before.name)?),
        Some(&body.to_string()),
        None,
    )
    .await?;
    match api.instance_optional(&before.name).await? {
        Some((value, _)) => {
            let observed = parse_instance(&value)?;
            anyhow::ensure!(
                observed.name == before.name,
                "Incus forced-stop readback returned foreign instance '{}' for requested '{}'",
                observed.name,
                before.name
            );
            anyhow::ensure!(
                observed.status == "Stopped",
                "Incus forced stop completed but observed status was '{}'",
                observed.status
            );
            Ok((false, Some(observed)))
        }
        None if before.ephemeral => Ok((true, None)),
        None => {
            anyhow::bail!("Incus forced stop completed but non-ephemeral OCI instance disappeared")
        }
    }
}

async fn pull_image(api: &IncusApi, input: PullImageInput) -> Result<serde_json::Value> {
    validate_image_source(&input.registry, &input.image)?;
    let body = serde_json::json!({
        "auto_update": input.auto_update,
        "source": image_source(&input.registry, &input.image)
    });
    api.call("POST", "/1.0/images", Some(&body.to_string()), None)
        .await?;
    let images = api
        .call("GET", "/1.0/images?recursion=1", None, None)
        .await
        .context(
            "Incus image import completed but image readback failed; cache state is indeterminate",
        )?;
    let image = images
        .as_array()
        .context("Incus image list metadata must be an array")?
        .iter()
        .find(|candidate| {
            let source = &candidate["update_source"];
            source["server"] == input.registry
                && source["protocol"] == "oci"
                && source["alias"] == input.image
        })
        .context("Incus image import completed but matching cached OCI image was not observed")?;
    Ok(serde_json::to_value(ImagePullOutput {
        registry: input.registry,
        image: input.image,
        fingerprint: image["fingerprint"]
            .as_str()
            .context("Incus image readback is missing fingerprint")?
            .into(),
        cached: image["cached"]
            .as_bool()
            .context("Incus image readback is missing cached")?,
        auto_update: image["auto_update"]
            .as_bool()
            .context("Incus image readback is missing auto_update")?,
    })?)
}

pub async fn dispatch_oci_method(
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    dispatch_oci_with_api(&IncusApi::local(), method, args).await
}

async fn dispatch_oci_with_api(
    api: &IncusApi,
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    reject_retired_loopback(args)?;
    match method {
        "pull_image" => {
            let input = serde_json::from_value::<PullImageInput>(args.clone())
                .context("invalid pull_image arguments")?;
            pull_image(api, input).await
        }
        "create" => {
            let input = serde_json::from_value::<CreateContainerInput>(args.clone())
                .context("invalid create arguments")?;
            let container = create_stopped(api, &input).await?;
            Ok(serde_json::to_value(ContainerMutationOutput {
                operation: "created".into(),
                changed: true,
                name: input.name,
                deleted: false,
                container: Some(container),
            })?)
        }
        "run_container" => {
            let input = serde_json::from_value::<RunContainerInput>(args.clone())
                .context("invalid run_container arguments")?;
            create_stopped(api, &input).await?;
            let container = set_state(api, &input.name, "start", false, "Running")
                .await
                .context(
                    "OCI instance was created but start failed; stopped instance may remain",
                )?;
            Ok(serde_json::to_value(ContainerMutationOutput {
                operation: "created_and_started".into(),
                changed: true,
                name: input.name,
                deleted: false,
                container: Some(container),
            })?)
        }
        "start" => {
            let input = serde_json::from_value::<StartContainerInput>(args.clone())
                .context("invalid start arguments")?;
            validate_segment(&input.name, "OCI instance name")?;
            let before = read_instance(api, &input.name).await?;
            let (changed, container) = if before.status == "Running" {
                (false, before)
            } else {
                (
                    true,
                    set_state(api, &input.name, "start", false, "Running").await?,
                )
            };
            Ok(serde_json::to_value(ContainerMutationOutput {
                operation: "started".into(),
                changed,
                name: input.name,
                deleted: false,
                container: Some(container),
            })?)
        }
        "kill" => {
            let input = serde_json::from_value::<KillContainerInput>(args.clone())
                .context("invalid kill arguments; only signal 'sigkill' is supported")?;
            validate_segment(&input.name, "OCI instance name")?;
            let before = read_instance(api, &input.name).await?;
            let (changed, deleted, container) = if before.status == "Stopped" {
                (false, false, Some(before))
            } else {
                let (deleted, container) = force_stop(api, before).await?;
                (true, deleted, container)
            };
            Ok(serde_json::to_value(ContainerMutationOutput {
                operation: "force_stopped".into(),
                changed,
                name: input.name,
                deleted,
                container,
            })?)
        }
        "delete" => {
            let input = serde_json::from_value::<DeleteContainerInput>(args.clone())
                .context("invalid delete arguments")?;
            validate_segment(&input.name, "OCI instance name")?;
            read_instance(api, &input.name).await?;
            api.delete(&input.name)
                .await
                .context("Incus delete failed; forced-stop or deletion effect may be partial")?;
            Ok(serde_json::to_value(DeleteContainerOutput {
                name: input.name,
                deleted: true,
            })?)
        }
        "state" => {
            let input = serde_json::from_value::<GetContainerStateInput>(args.clone())
                .context("invalid state arguments")?;
            validate_segment(&input.name, "OCI instance name")?;
            Ok(serde_json::to_value(
                read_instance(api, &input.name).await?,
            )?)
        }
        other => anyhow::bail!("unknown OCI method '{other}'"),
    }
}

pub(crate) fn oci_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(OciState)).expect("schema serializes");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "oci",
        "2.0.0",
        "Incus-managed OCI image import and application-container lifecycle",
        &root,
    );
    schema.category = "container".into();
    macro_rules! method {
        ($name:literal, $input:ty, $output:ty, $effect:expr, $idempotent:expr, $cap:literal, $subid:literal) => {
            schema.methods.insert(
                $name.into(),
                super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
                    $input,
                    $output,
                >($name, $effect, $idempotent, $cap, $subid),
            );
            schema.capabilities.insert(
                $cap.into(),
                op_state_store::CapabilityDecl {
                    id: $cap.into(),
                    description: concat!("Grants: ", $name, ".").into(),
                },
            );
        };
    }
    method!(
        "pull_image",
        PullImageInput,
        ImagePullOutput,
        SideEffect::Mutation,
        false,
        "cap.container.oci.image.pull@v1",
        "mut.container.oci.image.pull@v2"
    );
    method!(
        "run_container",
        RunContainerInput,
        ContainerMutationOutput,
        SideEffect::Mutation,
        false,
        "cap.container.oci.run@v1",
        "mut.container.oci.run@v2"
    );
    method!(
        "create",
        CreateContainerInput,
        ContainerMutationOutput,
        SideEffect::Mutation,
        false,
        "cap.container.oci.create@v1",
        "mut.container.oci.create@v2"
    );
    method!(
        "start",
        StartContainerInput,
        ContainerMutationOutput,
        SideEffect::Mutation,
        false,
        "cap.container.oci.start@v1",
        "mut.container.oci.start@v2"
    );
    method!(
        "kill",
        KillContainerInput,
        ContainerMutationOutput,
        SideEffect::Mutation,
        false,
        "cap.container.oci.kill@v1",
        "mut.container.oci.kill@v2"
    );
    method!(
        "delete",
        DeleteContainerInput,
        DeleteContainerOutput,
        SideEffect::Mutation,
        false,
        "cap.container.oci.delete@v1",
        "mut.container.oci.delete@v2"
    );
    method!(
        "state",
        GetContainerStateInput,
        ContainerStateOutput,
        SideEffect::Read,
        true,
        "cap.container.oci.state.get@v1",
        "obs.container.oci.state.get@v2"
    );
    schema.example = Some(simd_json::json!({
        "provider": "incus", "instance_type": "application_container"
    }));
    schema
}

pub struct OciPlugin;
impl Default for OciPlugin {
    fn default() -> Self {
        Self
    }
}
impl OciPlugin {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl StatePlugin for OciPlugin {
    fn name(&self) -> &str {
        "oci"
    }
    fn version(&self) -> &str {
        "2.0.0"
    }
    fn schema(&self) -> Option<PluginSchema> {
        Some(oci_schema())
    }
    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        Ok(StateDiff {
            plugin: self.name().into(),
            actions: vec![],
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: String::new(),
                desired_hash: String::new(),
            },
        })
    }
    async fn apply_state(&self, _diff: &StateDiff) -> Result<ApplyResult> {
        Ok(ApplyResult {
            success: true,
            changes_applied: vec![],
            errors: vec![],
            checkpoint: None,
        })
    }
    async fn verify_state(&self, _desired: &Value) -> Result<bool> {
        Ok(true)
    }
    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        Ok(Checkpoint {
            id: uuid::Uuid::new_v4().to_string(),
            plugin: self.name().into(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: simd_json::serde::to_owned_value(OciState::default())?,
            backend_checkpoint: None,
        })
    }
    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        Ok(())
    }
    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: false,
            supports_checkpoints: true,
            supports_verification: true,
            atomic_operations: false,
        }
    }
}

inventory::submit! {
    crate::default_registry::PluginReg::new("oci", |_ctx| std::sync::Arc::new(OciPlugin::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixListener;

    struct Step {
        method: &'static str,
        path: &'static str,
        body: Option<serde_json::Value>,
        response: serde_json::Value,
        etag: Option<&'static str>,
    }

    fn sync(metadata: serde_json::Value) -> serde_json::Value {
        serde_json::json!({"type":"sync","status":"Success","status_code":200,"error":"","metadata":metadata})
    }
    fn asynchronous() -> serde_json::Value {
        serde_json::json!({"type":"async","status":"Operation created","status_code":100,"error":"","operation":"/1.0/operations/12345678-1234-1234-1234-123456789abc"})
    }
    fn mutation(method: &'static str, path: &'static str, body: serde_json::Value) -> Vec<Step> {
        vec![
            Step {
                method,
                path,
                body: Some(body),
                response: asynchronous(),
                etag: None,
            },
            Step {
                method: "GET",
                path: "/1.0/operations/12345678-1234-1234-1234-123456789abc/wait?timeout=120",
                body: None,
                response: sync(
                    serde_json::json!({"status":"Success","status_code":200,"err":"","metadata":{}}),
                ),
                etag: None,
            },
        ]
    }
    fn instance(status: &str) -> serde_json::Value {
        serde_json::json!({"name":"demo","status":status,"status_code":if status == "Running" {103} else {102},"type":"container","ephemeral":false,"config":{"volatile.base_image":"abc123","volatile.container.oci":"true","image.type":"oci"},"expanded_config":{"volatile.container.oci":"true"}})
    }
    fn read(status: &'static str) -> Step {
        Step {
            method: "GET",
            path: "/1.0/instances/demo",
            body: None,
            response: sync(instance(status)),
            etag: Some("\"rev1\""),
        }
    }

    async fn fixture(
        method: &str,
        args: serde_json::Value,
        steps: Vec<Step>,
    ) -> Result<serde_json::Value> {
        let dir = tempfile::tempdir()?;
        let socket = dir.path().join("incus.sock");
        let listener = UnixListener::bind(&socket)?;
        let server = tokio::spawn(async move {
            let mut steps = VecDeque::from(steps);
            while let Some(step) = steps.pop_front() {
                let (mut stream, _) = listener.accept().await?;
                let mut header = Vec::new();
                while !header.ends_with(b"\r\n\r\n") {
                    header.push(stream.read_u8().await?);
                }
                let header = String::from_utf8(header)?;
                let mut lines = header.split("\r\n");
                anyhow::ensure!(
                    lines.next()
                        == Some(format!("{} {} HTTP/1.1", step.method, step.path).as_str()),
                    "unexpected request: {header}"
                );
                let headers: BTreeMap<_, _> = lines
                    .filter_map(|line| line.split_once(':'))
                    .map(|(k, v)| (k.to_ascii_lowercase(), v.trim().to_string()))
                    .collect();
                let length = headers
                    .get("content-length")
                    .map(|v| v.parse::<usize>())
                    .transpose()?
                    .unwrap_or(0);
                let mut body = vec![0; length];
                stream.read_exact(&mut body).await?;
                let body = if body.is_empty() {
                    None
                } else {
                    Some(serde_json::from_slice(&body)?)
                };
                anyhow::ensure!(body == step.body, "unexpected body: {body:?}");
                let status = step
                    .response
                    .get("fixture_http_status")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(200);
                let payload = if status == 404 {
                    serde_json::json!({"error":"not found"})
                } else {
                    step.response
                };
                let encoded = payload.to_string();
                let etag = step
                    .etag
                    .map(|v| format!("ETag: {v}\r\n"))
                    .unwrap_or_default();
                stream.write_all(format!("HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\n{etag}Content-Length: {}\r\nConnection: close\r\n\r\n{encoded}", encoded.len()).as_bytes()).await?;
            }
            anyhow::Ok(())
        });
        let api = IncusApi::on_socket(socket, Duration::from_secs(2));
        let result = dispatch_oci_with_api(&api, method, &args).await;
        server.await??;
        result
    }

    fn create_args() -> serde_json::Value {
        serde_json::json!({"name":"demo","registry":"https://docker.io","image":"library/alpine:3.22","env":{"PORT":"8080"}})
    }
    fn create_body() -> serde_json::Value {
        serde_json::json!({"name":"demo","type":"container","ephemeral":false,"source":{"type":"image","mode":"pull","protocol":"oci","server":"https://docker.io","alias":"library/alpine:3.22"},"config":{"security.privileged":"false","environment.PORT":"8080"}})
    }

    #[tokio::test]
    async fn all_seven_methods_use_real_incus_uds_contracts() {
        let pull_body = serde_json::json!({"auto_update":false,"source":{"type":"image","mode":"pull","protocol":"oci","server":"https://docker.io","alias":"library/alpine:3.22"}});
        let mut pull_steps = mutation("POST", "/1.0/images", pull_body);
        pull_steps.push(Step { method:"GET", path:"/1.0/images?recursion=1", body:None, response:sync(serde_json::json!([{"fingerprint":"abc123","cached":true,"auto_update":false,"update_source":{"server":"https://docker.io","protocol":"oci","alias":"library/alpine:3.22"}}])), etag:None });
        let pulled = fixture(
            "pull_image",
            serde_json::json!({"registry":"https://docker.io","image":"library/alpine:3.22"}),
            pull_steps,
        )
        .await
        .unwrap();
        assert_eq!(pulled["fingerprint"], "abc123");

        let mut create_steps = mutation("POST", "/1.0/instances", create_body());
        create_steps.push(read("Stopped"));
        assert_eq!(
            fixture("create", create_args(), create_steps)
                .await
                .unwrap()["container"]["status"],
            "Stopped"
        );

        let mut run_steps = mutation("POST", "/1.0/instances", create_body());
        run_steps.push(read("Stopped"));
        run_steps.extend(mutation(
            "PUT",
            "/1.0/instances/demo/state",
            serde_json::json!({"action":"start","force":false,"timeout":120}),
        ));
        run_steps.push(read("Running"));
        assert_eq!(
            fixture("run_container", create_args(), run_steps)
                .await
                .unwrap()["container"]["status"],
            "Running"
        );

        let mut start_steps = vec![read("Stopped")];
        start_steps.extend(mutation(
            "PUT",
            "/1.0/instances/demo/state",
            serde_json::json!({"action":"start","force":false,"timeout":120}),
        ));
        start_steps.push(read("Running"));
        assert_eq!(
            fixture("start", serde_json::json!({"name":"demo"}), start_steps)
                .await
                .unwrap()["changed"],
            true
        );

        let mut kill_steps = vec![read("Running")];
        kill_steps.extend(mutation(
            "PUT",
            "/1.0/instances/demo/state",
            serde_json::json!({"action":"stop","force":true,"timeout":120}),
        ));
        kill_steps.push(read("Stopped"));
        assert_eq!(
            fixture(
                "kill",
                serde_json::json!({"name":"demo","signal":"sigkill"}),
                kill_steps
            )
            .await
            .unwrap()["container"]["status"],
            "Stopped"
        );

        let mut delete_steps = vec![read("Stopped"), read("Stopped")];
        delete_steps.extend(
            mutation("DELETE", "/1.0/instances/demo", serde_json::Value::Null)
                .into_iter()
                .map(|mut step| {
                    if step.method == "DELETE" {
                        step.body = None;
                    }
                    step
                })
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            fixture("delete", serde_json::json!({"name":"demo"}), delete_steps)
                .await
                .unwrap()["deleted"],
            true
        );

        assert_eq!(
            fixture(
                "state",
                serde_json::json!({"name":"demo"}),
                vec![read("Running")]
            )
            .await
            .unwrap()["status"],
            "Running"
        );
    }

    #[tokio::test]
    async fn rejects_unsupported_and_invalid_inputs_before_socket_access() {
        for (method, args, expected) in [
            (
                "create",
                serde_json::json!({"name":"demo","registry":"http://docker.io","image":"x"}),
                "HTTPS",
            ),
            (
                "create",
                serde_json::json!({"name":"demo","registry":"https://docker.io","image":"x","loopback_required":true}),
                "loopback_required is unsupported",
            ),
            (
                "kill",
                serde_json::json!({"name":"demo","signal":"sigterm"}),
                "only signal 'sigkill'",
            ),
        ] {
            let error = dispatch_oci_with_api(
                &IncusApi::on_socket("/nonexistent".into(), Duration::from_millis(1)),
                method,
                &args,
            )
            .await
            .unwrap_err();
            assert!(error.to_string().contains(expected), "{error:#}");
        }
    }

    #[tokio::test]
    async fn run_reports_created_instance_when_start_operation_fails() {
        let mut steps = mutation("POST", "/1.0/instances", create_body());
        steps.push(read("Stopped"));
        steps.push(Step {
            method: "PUT",
            path: "/1.0/instances/demo/state",
            body: Some(serde_json::json!({"action":"start","force":false,"timeout":120})),
            response: asynchronous(),
            etag: None,
        });
        steps.push(Step {
            method: "GET",
            path: "/1.0/operations/12345678-1234-1234-1234-123456789abc/wait?timeout=120",
            body: None,
            response: sync(serde_json::json!({
                "status":"Failure", "status_code":400, "err":"start rejected", "metadata":{}
            })),
            etag: None,
        });
        let error = fixture("run_container", create_args(), steps)
            .await
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("instance was created but start failed"));
    }

    #[tokio::test]
    async fn lifecycle_rejects_non_oci_before_mutation() {
        let non_oci = serde_json::json!({
            "name":"demo", "status":"Running", "status_code":103,
            "type":"container", "ephemeral":false,
            "config":{"volatile.base_image":"system-image"}
        });
        let error = fixture(
            "kill",
            serde_json::json!({"name":"demo","signal":"sigkill"}),
            vec![Step {
                method: "GET",
                path: "/1.0/instances/demo",
                body: None,
                response: sync(non_oci),
                etag: Some("\"rev1\""),
            }],
        )
        .await
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("not an OCI application container"));
    }

    #[tokio::test]
    async fn force_stop_reports_ephemeral_instance_deletion() {
        let ephemeral = serde_json::json!({
            "name":"demo", "status":"Running", "status_code":103,
            "type":"container", "ephemeral":true,
            "config":{"volatile.base_image":"abc123","volatile.container.oci":"true"},
            "expanded_config":{"image.type":"oci"}
        });
        let mut steps = vec![Step {
            method: "GET",
            path: "/1.0/instances/demo",
            body: None,
            response: sync(ephemeral),
            etag: Some("\"rev1\""),
        }];
        steps.extend(mutation(
            "PUT",
            "/1.0/instances/demo/state",
            serde_json::json!({"action":"stop","force":true,"timeout":120}),
        ));
        steps.push(Step {
            method: "GET",
            path: "/1.0/instances/demo",
            body: None,
            response: serde_json::json!({"fixture_http_status":404}),
            etag: None,
        });
        let output = fixture(
            "kill",
            serde_json::json!({"name":"demo","signal":"sigkill"}),
            steps,
        )
        .await
        .unwrap();
        assert_eq!(output["deleted"], true);
        assert!(output["container"].is_null());
    }

    #[test]
    fn schema_declares_exact_seven_typed_methods_without_bundle_or_loopback() {
        let schema = oci_schema();
        assert_eq!(schema.methods.len(), 7);
        let encoded = serde_json::to_string(&schema).unwrap();
        assert!(!encoded.contains("bundle_path"));
        assert!(!encoded.contains("loopback_required"));
        assert!(!encoded.contains("AckOutput"));
    }
}
