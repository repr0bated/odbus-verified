//! threetched plugin commands, captured from the pinned binary at seal time.
//! Configuration remains authored by threetched: selection uses its `plugins`
//! namespace through the existing tched_router config surface.

use std::{collections::BTreeSet, fs::File, path::Path, sync::OnceLock};
use anyhow::{Context, Result, bail, ensure};
use async_trait::async_trait;
use op_plugin_lint::{BinaryIntrospectOpts, BinarySurface, introspect_binary_subtree};
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{CapabilityDecl, PluginSchema, SideEffect};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use simd_json::OwnedValue;
use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;

pub const PLUGIN_NAME: &str = "tched_plugins";
const READ: &str = "cap.software.tched-plugins.read@v1";
const WRITE: &str = "cap.software.tched-plugins.write@v1";
const INSTALL: &str = "cap.software.tched-plugins.install@v1";
// Search writes the threetched registry cache, so it is a mutation, not a read.
const SEARCH: &str = "cap.software.tched-plugins.search@v1";
const COMMANDS: &[(&str, &str, &[&str])] = &[
    ("list_plugins", "list", &[]),
    ("search_plugins", "search", &["--registry"]),
    ("install_plugin", "install", &["--registry"]),
    ("remove_plugin", "remove", &[]),
    ("plugin_info", "info", &[]),
    ("migrate_plugins", "migrate", &[]),
];

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginsState {
    pub binary: String,
    pub config_dir: String,
    pub binary_sha256: String,
    pub command_group: String,
    pub cli_surface: Value,
    /// Verbatim threetched `plugins` configuration schema, including referenced types.
    pub config_schema: Value,
}

fn capture() -> Result<PluginsState> {
    let runtime_binary = super::tched_router_paths::binary()?;
    let config_dir = super::tched_router_paths::config_dir()?;
    let (capture_binary, capture_manifest) = super::tched_router_paths::capture_paths()?;
    let manifest: Value = serde_json::from_reader(File::open(capture_manifest)?)?;
    let binary_hash = super::tched_skills::sha256_file(&capture_binary)?;
    ensure!(manifest.pointer("/artifact/sha256").and_then(Value::as_str) == Some(binary_hash.as_str()), "threetched plugin binary/provenance mismatch");
    let mut surface = introspect_binary_subtree(&capture_binary, &["plugin".into()], &BinaryIntrospectOpts { max_depth: 1, ssh: None })?;
    verify_surface(&surface)?;
    let output = std::process::Command::new(&capture_binary).args(["config", "schema"]).output()?;
    ensure!(output.status.success(), "threetched config schema capture failed");
    let schema: Value = serde_json::from_slice(&output.stdout)?;
    // Keep build-time candidate paths out of the sealed runtime descriptor.
    surface.binary = runtime_binary.to_str().context("non-UTF8 binary path")?.into();
    Ok(PluginsState {
        binary: runtime_binary.to_str().context("non-UTF8 binary path")?.into(),
        config_dir: config_dir.to_str().context("non-UTF8 config path")?.into(),
        binary_sha256: binary_hash,
        command_group: "plugin".into(),
        cli_surface: serde_json::to_value(surface)?,
        config_schema: super::binary_namespace::filter_namespace(&schema, "plugins")?,
    })
}

fn verify_surface(surface: &BinarySurface) -> Result<()> {
    let root = surface.nodes.iter().find(|node| node.path == ["plugin"]).context("threetched has no plugin CLI; build with plugins-wasm backend")?;
    ensure!(root.commands.iter().map(String::as_str).collect::<BTreeSet<_>>() == COMMANDS.iter().map(|(_, command, _)| *command).collect(), "threetched plugin command set changed; update adapter before sealing");
    for (_, command, flags) in COMMANDS {
        let node = surface.nodes.iter().find(|node| node.path == ["plugin", *command]).context("threetched plugin subcommand missing")?;
        let observed: BTreeSet<_> = node.flags.iter().map(|flag| flag.name.as_str()).filter(|flag| !["--config-dir", "--log-level", "--verbose", "--help"].contains(flag)).collect();
        ensure!(observed == flags.iter().copied().collect(), "threetched plugin flags changed for {command}");
    }
    Ok(())
}

fn captured() -> Result<&'static PluginsState> {
    static STATE: OnceLock<std::result::Result<PluginsState, String>> = OnceLock::new();
    STATE.get_or_init(|| capture().map_err(|error| error.to_string())).as_ref().map_err(|error| anyhow::anyhow!("{error}"))
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmptyInput {}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchPluginsInput { pub query: String, pub registry: Option<String> }
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InstallPluginInput { pub source: String, pub registry: Option<String> }
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginNameInput { pub name: String }
macro_rules! output {
    ($($name:ident),+ $(,)?) => { $(
        #[derive(Serialize, Deserialize, JsonSchema)]
        #[serde(deny_unknown_fields)]
        pub struct $name { pub text: String }
    )+ };
}
output!(ListPluginsOutput, SearchPluginsOutput, InstallPluginOutput, RemovePluginOutput, PluginInfoOutput, MigratePluginsOutput);

pub fn state_value() -> Result<OwnedValue> { Ok(simd_json::serde::to_owned_value(captured()?)?) }

pub fn tched_plugins_schema() -> PluginSchema {
    let state = captured().expect("threetched plugin contract must be captured before sealing");
    let root = serde_json::to_value(schemars::schema_for!(PluginsState)).expect("plugin state schema");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(PLUGIN_NAME, "1.0.0", "Typed threetched plugin commands and passthrough configuration schema", &root);
    schema.category = "software".into();
    schema.display_name = Some("Tched Plugins".into());
    schema.tags = vec!["threetched".into(), "plugins".into(), "binary".into()];
    schema.immutable_paths = ["binary", "config_dir", "binary_sha256", "command_group", "cli_surface", "config_schema"].map(|name| format!("/{name}")).into();
    // Publish closed metadata fields; opaque discovery evidence is sealed in example.
    schema.public_state.paths = ["binary", "config_dir", "binary_sha256", "command_group"]
        .map(|name| format!("/{name}")).into();
    let example = simd_json::serde::to_owned_value(state).expect("plugin state serialization");
    super::schemars_adapter::apply_state_defaults(&mut schema, &example);
    schema.example = Some(example);
    macro_rules! method {
        ($name:literal, $input:ty, $output:ty, $effect:expr, $idempotent:expr, $cap:expr, $subid:literal) => {
            schema.methods.insert($name.into(), method_decl_from_schemars_with_output::<$input, $output>($name, $effect, $idempotent, $cap, $subid));
        };
    }
    method!("list_plugins", EmptyInput, ListPluginsOutput, SideEffect::Read, true, READ, "obs.software.tched-plugins.list@v1");
    method!("plugin_info", PluginNameInput, PluginInfoOutput, SideEffect::Read, true, READ, "obs.software.tched-plugins.info@v1");
    method!("search_plugins", SearchPluginsInput, SearchPluginsOutput, SideEffect::Mutation, false, SEARCH, "mut.software.tched-plugins.search@v1");
    method!("install_plugin", InstallPluginInput, InstallPluginOutput, SideEffect::Mutation, false, INSTALL, "mut.software.tched-plugins.install@v1");
    method!("remove_plugin", PluginNameInput, RemovePluginOutput, SideEffect::Mutation, false, WRITE, "mut.software.tched-plugins.remove@v1");
    method!("migrate_plugins", EmptyInput, MigratePluginsOutput, SideEffect::Mutation, false, WRITE, "mut.software.tched-plugins.migrate@v1");
    for (id, description) in [(READ, "Read threetched installed plugin metadata."), (SEARCH, "Search threetched registry and update its cache."), (INSTALL, "Install threetched plugin packages."), (WRITE, "Remove or migrate threetched plugin packages.")] {
        schema.capabilities.insert(id.into(), CapabilityDecl { id: id.into(), description: description.into() });
    }
    schema
}

fn arg(value: &str) -> Result<String> {
    ensure!(!value.is_empty() && value.len() <= 4096 && !value.starts_with('-') && !value.chars().any(char::is_control), "invalid threetched plugin argument");
    Ok(value.into())
}
fn name(value: &str) -> Result<String> {
    ensure!(value.len() <= 128 && value.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'), "invalid threetched plugin name");
    arg(value)
}
fn arguments(method: &str, value: &Value) -> Result<Vec<String>> {
    let mut argv = vec!["plugin".into()];
    match method {
        "list_plugins" | "migrate_plugins" => {
            let _: EmptyInput = serde_json::from_value(value.clone())?;
            argv.push(if method == "list_plugins" { "list" } else { "migrate" }.into());
        }
        "plugin_info" | "remove_plugin" => {
            let input: PluginNameInput = serde_json::from_value(value.clone())?;
            argv.extend([if method == "plugin_info" { "info" } else { "remove" }.into(), name(&input.name)?]);
        }
        "search_plugins" => {
            let input: SearchPluginsInput = serde_json::from_value(value.clone())?;
            argv.extend(["search".into(), arg(&input.query)?]);
            if let Some(registry) = input.registry { argv.extend(["--registry".into(), arg(&registry)?]); }
        }
        "install_plugin" => {
            let input: InstallPluginInput = serde_json::from_value(value.clone())?;
            argv.extend(["install".into(), arg(&input.source)?]);
            if let Some(registry) = input.registry { argv.extend(["--registry".into(), arg(&registry)?]); }
        }
        _ => bail!("unknown threetched plugin method: {method}"),
    }
    Ok(argv)
}

pub async fn dispatch(method: &str, value: &Value, source: &PluginsState) -> Result<Value> {
    let binary = super::tched_router_paths::binary()?;
    let config_dir = super::tched_router_paths::config_dir()?;
    let hash_path = binary.clone();
    let hash = tokio::task::spawn_blocking(move || super::tched_skills::sha256_file(&hash_path)).await??;
    ensure!(binary == Path::new(&source.binary) && config_dir == Path::new(&source.config_dir) && hash == source.binary_sha256, "threetched plugin binary or config paths changed since seal; reseal before dispatch");
    let argv = arguments(method, value)?;
    Ok(serde_json::to_value(super::tched_skills::run(&binary, &config_dir, &argv, None, true).await?)?)
}

#[derive(Default)]
pub struct TchedPluginsPlugin;
#[async_trait]
impl StatePlugin for TchedPluginsPlugin {
    fn name(&self) -> &str { PLUGIN_NAME }
    fn version(&self) -> &str { "1.0.0" }
    fn schema(&self) -> Option<PluginSchema> { Some(tched_plugins_schema()) }
    async fn calculate_diff(&self, current: &OwnedValue, desired: &OwnedValue) -> Result<StateDiff> {
        ensure!(current == desired && *desired == state_value()?, "threetched plugin descriptor is immutable");
        Ok(StateDiff { plugin: PLUGIN_NAME.into(), actions: vec![], metadata: DiffMetadata { timestamp: chrono::Utc::now().timestamp(), current_hash: String::new(), desired_hash: String::new() } })
    }
    async fn apply_state(&self, diff: &StateDiff) -> Result<ApplyResult> {
        ensure!(diff.plugin == PLUGIN_NAME && diff.actions.is_empty(), "threetched plugin descriptor is immutable");
        Ok(ApplyResult { success: true, changes_applied: vec![], errors: vec![], checkpoint: None })
    }
    async fn verify_state(&self, desired: &OwnedValue) -> Result<bool> { Ok(*desired == state_value()?) }
    async fn create_checkpoint(&self) -> Result<Checkpoint> { bail!("threetched plugin descriptor has no checkpoints") }
    async fn rollback(&self, _: &Checkpoint) -> Result<()> { bail!("threetched plugin descriptor has no rollback") }
    fn capabilities(&self) -> PluginCapabilities { PluginCapabilities { supports_rollback: false, supports_checkpoints: false, supports_verification: true, atomic_operations: false } }
}
inventory::submit! { crate::default_registry::PluginReg::new(PLUGIN_NAME, |_ctx| std::sync::Arc::new(TchedPluginsPlugin)) }
