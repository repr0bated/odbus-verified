//! Binary-discovered threetched methods owned by one sealed plugin contract.
//!
//! The binary supplies command words, flags and token schemas. This plugin owns
//! admission policy; help text cannot establish that a command is read-only.
//! opblob is the sole sealer. Filesystem and transport consumers project its
//! sealed declarations and dispatch through the existing MutationEngine gate.

use std::fs::File;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, OnceLock, atomic::{AtomicBool, Ordering}};

use anyhow::{Context, Result, bail, ensure};
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{CapabilityDecl, MethodDecl, PluginSchema, SideEffect};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use simd_json::OwnedValue;
use threetched_fs::threetched_surface::{SurfaceSnapshot, ThreetchedSurface};

pub const PLUGIN_NAME: &str = "threetched";
const VERSION: &str = "1.0.0";
const MANIFEST_FORMAT: &str = "opdbus.threetched.v1";

/// Immutable discovery evidence, never a copy of the running configuration.
/// Capture selectors are build/seal inputs; these paths select the installed
/// runtime and stay identical when the candidate is staged into a release.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ThreetchedState {
    pub binary: String,
    pub manifest: String,
    pub config_dir: String,
    pub binary_sha256: String,
    pub manifest_sha256: String,
    pub surface: Value,
}

struct Captured {
    state: ThreetchedState,
    surface: ThreetchedSurface,
}

fn captured() -> Result<&'static Captured> {
    static CAPTURED: OnceLock<std::result::Result<Captured, String>> = OnceLock::new();
    CAPTURED
        .get_or_init(|| capture().map_err(|error| format!("{error:#}")))
        .as_ref()
        .map_err(|error| anyhow::anyhow!("{error}"))
}

fn path_text(path: &Path) -> Result<String> {
    Ok(path.to_str().context("threetched path is not UTF-8")?.to_owned())
}

fn manifest_version(manifest: &Value, binary_hash: &str) -> Result<String> {
    ensure!(manifest["format"] == MANIFEST_FORMAT, "unsupported threetched manifest format");
    ensure!(manifest.pointer("/artifact/name").and_then(Value::as_str) == Some("threetched"), "manifest is not for the threetched executable");
    ensure!(manifest.pointer("/artifact/sha256").and_then(Value::as_str) == Some(binary_hash), "threetched executable differs from its manifest");
    let tag = manifest.pointer("/upstream/tag").and_then(Value::as_str)
        .context("threetched manifest has no upstream version")?;
    let version = tag.strip_prefix('v').filter(|value| !value.is_empty())
        .context("threetched manifest version must start with v")?;
    Ok(format!("threetched {version}"))
}

fn capture() -> Result<Captured> {
    let binary = super::tched_router_paths::binary()?;
    let manifest = super::tched_router_paths::manifest()?;
    let config_dir = super::tched_router_paths::config_dir()?;
    let (capture_binary, capture_manifest) = super::tched_router_paths::capture_paths()?;
    let manifest_sha256 = super::tched_skills::sha256_file(&capture_manifest)?;
    let descriptor: Value = serde_json::from_reader(File::open(&capture_manifest)?)?;
    let binary_sha256 = super::tched_skills::sha256_file(&capture_binary)?;
    let version = manifest_version(&descriptor, &binary_sha256)?;
    let surface = ThreetchedSurface::capture_with_config(&capture_binary, &config_dir)?;
    ensure!(surface.binary_sha256() == binary_sha256, "threetched executable changed during capture");
    ensure!(surface.version() == Some(version.as_str()), "threetched version differs from its manifest");
    ensure!(super::tched_skills::sha256_file(&capture_manifest)? == manifest_sha256, "threetched manifest changed during capture");
    let state = ThreetchedState {
        binary: path_text(&binary)?,
        manifest: path_text(&manifest)?,
        config_dir: path_text(&config_dir)?,
        binary_sha256: surface.binary_sha256().to_owned(),
        manifest_sha256,
        surface: serde_json::to_value(surface.snapshot())?,
    };
    Ok(Captured { state, surface })
}

fn command_policy(method: &str) -> (String, String) {
    // Exact method bytes keep dotted/hyphenated CLI names distinct. This is a
    // policy identifier, not an inferred CLI type or a hardcoded method table.
    let key = format!("c{:x}", Sha256::digest(method.as_bytes()));
    (
        format!("cap.software.threetched.call.{key}@v1"),
        format!("mut.software.threetched.call.{key}@v1"),
    )
}

fn schema_from_capture(captured: &Captured) -> Result<PluginSchema> {
    let root = serde_json::to_value(schemars::schema_for!(ThreetchedState))?;
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        PLUGIN_NAME, VERSION, "Commands discovered from the pinned threetched binary", &root,
    );
    schema.category = "software".into();
    schema.display_name = Some("threetched".into());
    schema.tags = vec!["threetched".into(), "binary".into(), "immutable".into()];
    schema.immutable_paths = root["properties"].as_object().context("missing threetched descriptor fields")?
        .keys().map(|name| format!("/{name}")).collect();
    // Publish closed metadata fields; opaque discovery evidence is sealed in example.
    schema.public_state.paths = ["binary", "manifest", "config_dir", "binary_sha256", "manifest_sha256"]
        .map(|name| format!("/{name}")).into();
    let state = simd_json::serde::to_owned_value(&captured.state)?;
    super::schemars_adapter::apply_state_defaults(&mut schema, &state);
    schema.example = Some(state);
    for method in captured.surface.commands() {
        let input = captured.surface.input_schema(method)?;
        let output = captured.surface.output_schema(method)?;
        for contract in [&input, &output] {
            op_blob::descriptor::validate_json_schema_field_aliases(contract)
                .map_err(anyhow::Error::msg)
                .with_context(|| format!("threetched method {method} has ambiguous transport fields"))?;
        }
        let (capability, subid) = command_policy(method);
        schema.capabilities.insert(capability.clone(), CapabilityDecl {
            id: capability.clone(),
            description: format!("Execute threetched CLI command {method} in the sealed runtime context."),
        });
        schema.methods.insert(method.to_owned(), MethodDecl {
            name: method.to_owned(),
            args: simd_json::serde::to_owned_value(input)?,
            returns: Some(simd_json::serde::to_owned_value(output)?),
            side_effect: SideEffect::Mutation,
            idempotent: false,
            required_capability: Some(capability),
            subid,
        });
    }
    ensure!(!schema.methods.is_empty(), "threetched binary supplied no callable commands");
    Ok(schema)
}

pub fn threetched_schema() -> PluginSchema {
    schema_from_capture(captured().expect("threetched discovery must succeed before sealing"))
        .expect("threetched discovered contract must be representable")
}

pub fn state_value() -> Result<OwnedValue> {
    Ok(simd_json::serde::to_owned_value(&captured()?.state)?)
}

#[derive(Default)]
pub struct ThreetchedPlugin;

#[async_trait]
impl StatePlugin for ThreetchedPlugin {
    fn name(&self) -> &str { PLUGIN_NAME }
    fn version(&self) -> &str { VERSION }
    fn schema(&self) -> Option<PluginSchema> { Some(threetched_schema()) }
    async fn calculate_diff(&self, current: &OwnedValue, desired: &OwnedValue) -> Result<StateDiff> {
        ensure!(current == desired && *desired == state_value()?, "threetched discovery descriptor is immutable");
        Ok(StateDiff {
            plugin: PLUGIN_NAME.into(), actions: vec![],
            metadata: DiffMetadata { timestamp: chrono::Utc::now().timestamp(), current_hash: String::new(), desired_hash: String::new() },
        })
    }
    async fn apply_state(&self, diff: &StateDiff) -> Result<ApplyResult> {
        ensure!(diff.plugin == PLUGIN_NAME && diff.actions.is_empty(), "threetched discovery descriptor is immutable");
        Ok(ApplyResult { success: true, changes_applied: vec![], errors: vec![], checkpoint: None })
    }
    async fn verify_state(&self, desired: &OwnedValue) -> Result<bool> { Ok(*desired == state_value()?) }
    async fn create_checkpoint(&self) -> Result<Checkpoint> { bail!("threetched discovery descriptor has no checkpoints") }
    async fn rollback(&self, _: &Checkpoint) -> Result<()> { bail!("threetched discovery descriptor has no rollback") }
    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities { supports_rollback: false, supports_checkpoints: false, supports_verification: true, atomic_operations: false }
    }
}

struct CancelOnDrop(Arc<AtomicBool>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) { self.0.store(true, Ordering::Release); }
}

/// Preserve the sealed service context. The CLI's own config selector remains
/// discoverable, but it cannot redirect an admitted operation to another root.
fn validate_config_selector(value: &Value, config_dir: &Path, spellings: &[&str]) -> Result<()> {
    let expected = path_text(config_dir)?;
    let one = serde_json::json!([expected]);
    let mut selected = false;
    for spelling in spellings {
        if let Some(selector) = value.get(*spelling) {
            ensure!(!selected, "threetched config selector was supplied through multiple aliases");
            ensure!(selector == &one || selector == &serde_json::json!([one.clone()]), "threetched config selector must select the sealed runtime context");
            selected = true;
        }
    }
    Ok(())
}

fn prepare_command(method: &str, value: &Value, source: &ThreetchedState) -> Result<Command> {
    let binary = super::tched_router_paths::binary()?;
    let manifest = super::tched_router_paths::manifest()?;
    let config_dir = super::tched_router_paths::config_dir()?;
    ensure!(binary == Path::new(&source.binary) && manifest == Path::new(&source.manifest)
        && config_dir == Path::new(&source.config_dir), "threetched runtime paths changed since seal");
    ensure!(super::tched_skills::sha256_file(&manifest)? == source.manifest_sha256,
        "threetched manifest changed since seal");
    let snapshot: SurfaceSnapshot = serde_json::from_value(source.surface.clone())?;
    let surface = ThreetchedSurface::from_snapshot(&binary, &snapshot)?;
    ensure!(surface.binary_sha256() == source.binary_sha256, "threetched sealed source digests disagree");
    let descriptor: Value = serde_json::from_reader(File::open(&manifest)?)?;
    manifest_version(&descriptor, surface.binary_sha256())?;
    let config_spellings = surface.option_spellings(method, "--config-dir")?;
    validate_config_selector(value, &config_dir, &config_spellings)?;
    let argv = surface.arguments(method, value).context("invalid threetched CLI arguments")?;
    // Match the existing tched-router service owner. This backend is a shared
    // service selected by exact capabilities, never a caller-supplied identity
    // or a guessed mapping from SID1 to an operating-system account.
    let mut command = if nix::unistd::geteuid().is_root() {
        let mut command = Command::new("/usr/bin/chpst");
        command.args(["-u", "jeremy:jeremy"]).arg(surface.binary_path());
        command
    } else {
        let owner = nix::unistd::User::from_name("jeremy")?
            .context("threetched service owner is absent")?;
        ensure!(nix::unistd::geteuid() == owner.uid && nix::unistd::getegid() == owner.gid,
            "threetched dispatch must run as its configured service owner");
        Command::new(surface.binary_path())
    };
    command.args(argv)
        .env_clear()
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("HOME", config_dir.join("home"))
        .env("USER", "jeremy")
        .env("THREETCHED_CONFIG_DIR", config_dir)
        .env("LANGUAGE", "en")
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .env("NO_COLOR", "1");
    Ok(command)
}

/// Called only after canonical method admission and its final queued-call
/// recheck. The bounded runner kills/reaps the whole child group on timeout,
/// cancellation or output exhaustion; it does not retry an uncertain result.
pub async fn dispatch(
    method: &str,
    value: &Value,
    source: &ThreetchedState,
    operation: Arc<tokio::sync::MutexGuard<'static, ()>>,
) -> Result<Value> {
    let method = method.to_owned();
    let value = value.clone();
    let source = source.clone();
    let cancelled = Arc::new(AtomicBool::new(false));
    let _cancel_on_drop = CancelOnDrop(cancelled.clone());
    tokio::task::spawn_blocking(move || -> Result<Value> {
        // A cancelled RPC must not release service serialization before the
        // worker has killed and reaped its child group.
        let _operation = operation;
        let mut command = prepare_command(&method, &value, &source)?;
        let output = threetched_fs::fuse_process::run_command(&mut command, &cancelled)?;
        ensure!(output.exit == 0, "threetched command exited with {}; inspect the identity-bound method receipt before retrying", output.exit);
        Ok(serde_json::to_value(&output)?)
    }).await.context("threetched execution worker failed")?
}

inventory::submit! {
    crate::default_registry::PluginReg::new(PLUGIN_NAME, |_ctx| Arc::new(ThreetchedPlugin))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovered_cli_flags_survive_sealing_and_descriptor_transport_with_admission_policy() {
        use std::os::unix::fs::PermissionsExt;
        use prost::Message;
        use prost_reflect::{DescriptorPool, DynamicMessage};

        let directory = tempfile::tempdir().unwrap();
        let binary = directory.path().join("threetched-fixture");
        // Only advertised introspection paths succeed; this fixture cannot
        // execute a feature operation while generating or transporting input.
        std::fs::write(&binary, r#"#!/bin/sh
case "$*" in
  '--version') printf 'threetched 0.0.1\n' ;;
  'config schema') printf '{"type":"object","properties":{}}\n' ;;
  '--help')
    printf 'Usage: threetched <COMMAND>\n\nCommands:\n  config  Configuration\n  widgets  Widget operations\n' ;;
  'config --help')
    printf 'Usage: threetched config <COMMAND>\n\nCommands:\n  schema  Export schema\n' ;;
  'config schema --help') printf 'Usage: threetched config schema\n' ;;
  'widgets --help')
    printf 'Usage: threetched widgets <COMMAND>\n\nCommands:\n  publish  Publish a widget\n' ;;
  'widgets publish --help')
    cat <<'HELP'
Usage: threetched widgets publish [OPTIONS] <NAME>

Arguments:
  <NAME>  Widget name

Options:
  -f, --format <FORMAT>  Output format
          [possible values: json, text]
  -n, --count <COUNT>  Literal CLI value
  -v, --verbose  Verbose output
  -c, --config-dir <CONFIG_DIR>  Runtime configuration
HELP
    ;;
  *) exit 91 ;;
esac
"#).unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        let surface = ThreetchedSurface::capture_with_config(&binary, directory.path()).unwrap();
        let captured = Captured {
            state: ThreetchedState {
                binary: super::super::tched_router_paths::DEFAULT_BINARY.into(),
                manifest: super::super::tched_router_paths::DEFAULT_MANIFEST.into(),
                config_dir: super::super::tched_router_paths::DEFAULT_CONFIG_DIR.into(),
                binary_sha256: surface.binary_sha256().into(),
                manifest_sha256: "a".repeat(64),
                surface: serde_json::to_value(surface.snapshot()).unwrap(),
            },
            surface,
        };
        let schema = schema_from_capture(&captured).unwrap();
        let method_name = "widgets.publish";
        let source_input = captured.surface.input_schema(method_name).unwrap();
        let source_output = captured.surface.output_schema(method_name).unwrap();
        let blob = op_blob::blobify_plugin_schema(PLUGIN_NAME, schema);
        let bytes = blob.seal();
        assert!(bytes.len() < 1024 * 1024);
        let pinned = op_blob::BlobRef::from_manifest_pinned(&bytes, &blob.manifest.schema_hash).unwrap();
        let sealed = pinned.state_store_schema().unwrap();
        let declaration = &sealed.methods[method_name];
        assert_eq!(serde_json::to_value(&declaration.args).unwrap(), source_input);
        assert_eq!(serde_json::to_value(declaration.returns.as_ref().unwrap()).unwrap(), source_output);
        assert_eq!(declaration.side_effect, SideEffect::Mutation);
        assert!(!declaration.idempotent);
        let (capability, subid) = command_policy(method_name);
        assert_eq!(declaration.required_capability.as_deref(), Some(capability.as_str()));
        assert_eq!(declaration.subid, subid);
        assert!(sealed.capabilities.contains_key(&capability));
        assert!(sealed.validate_capability_closure().missing.is_empty());
        assert!(sealed.capability_grants.is_empty());

        let manifest = pinned.manifest().unwrap();
        let method_manifest = manifest.methods.iter().find(|method| method.schema_name == method_name).unwrap();
        let pool = DescriptorPool::decode(pinned.descriptor_set()).unwrap();
        let input = pool.get_message_by_name(&method_manifest.input_message).unwrap();
        for spelling in ["--format", "-f", "--count", "-n", "--verbose", "-v", "--config-dir", "-c"] {
            let field = input.get_field_by_name(&op_blob::descriptor::proto_field_name(spelling)).unwrap();
            assert_eq!(field.json_name(), spelling);
        }
        let validator = jsonschema::validator_for(&source_input).unwrap();
        for (spelling, value) in [
            ("--format", serde_json::json!(["json"])),
            ("-f", serde_json::json!(["text"])),
            ("--count", serde_json::json!(["007"])),
            ("-n", serde_json::json!(["007"])),
            ("--verbose", serde_json::json!(true)),
            ("-v", serde_json::json!(true)),
        ] {
            let arguments = serde_json::json!({(spelling):value,"arguments":["sample"]});
            assert!(validator.is_valid(&arguments));
            let message = DynamicMessage::deserialize(input.clone(), &arguments).unwrap();
            let encoded = message.encode_to_vec();
            let decoded = DynamicMessage::decode(input.clone(), encoded.as_slice()).unwrap();
            let transported = serde_json::to_value(decoded).unwrap();
            assert_eq!(transported, arguments);
            assert!(validator.is_valid(&transported));
            let mut expected = vec!["widgets".to_owned(), "publish".to_owned(), spelling.to_owned()];
            if let Some(tokens) = value.as_array() {
                expected.extend(tokens.iter().map(|token| token.as_str().unwrap().to_owned()));
            }
            expected.extend(["--".to_owned(), "sample".to_owned()]);
            assert_eq!(captured.surface.arguments(method_name, &transported).unwrap(), expected);
        }
        assert!(!validator.is_valid(&serde_json::json!({"--count":[7],"arguments":["sample"]})));
        assert!(!validator.is_valid(&serde_json::json!({"--unknown":true,"arguments":["sample"]})));
        assert!(!validator.is_valid(&serde_json::json!({"-f":["json"],"--format":["json"],"arguments":["sample"]})));

        // Context restrictions use aliases from the same sealed snapshot.
        let snapshot: SurfaceSnapshot = serde_json::from_value(captured.state.surface.clone()).unwrap();
        let restored = ThreetchedSurface::from_snapshot(&binary, &snapshot).unwrap();
        let spellings = restored.option_spellings(method_name, "--config-dir").unwrap();
        assert_eq!(spellings, ["-c", "--config-dir"]);
        assert!(validate_config_selector(&serde_json::json!({"-c":["/other/identity"]}),
            Path::new(&captured.state.config_dir), &spellings).is_err());
    }

    #[test]
    fn command_policy_preserves_distinct_method_identity() {
        let (cap, subid) = command_policy("plugins.install");
        assert_ne!(cap, command_policy("plugins-install").0);
        super::super::common::oscal::validate_subid(&subid).unwrap();
        assert!(cap.starts_with("cap.software.threetched.call."));
    }

    #[test]
    fn config_selector_cannot_escape_sealed_context() {
        let root = Path::new("/var/lib/tched-router");
        // Fixture spellings; production obtains this list from captured help.
        let spellings = ["-c", "--config-dir"];
        validate_config_selector(&serde_json::json!({}), root, &spellings).unwrap();
        for spelling in spellings {
            validate_config_selector(&serde_json::json!({(spelling):["/var/lib/tched-router"]}), root, &spellings).unwrap();
            validate_config_selector(&serde_json::json!({(spelling):[["/var/lib/tched-router"]]}), root, &spellings).unwrap();
            assert!(validate_config_selector(&serde_json::json!({(spelling):["/other/identity"]}), root, &spellings).is_err());
            assert!(validate_config_selector(&serde_json::json!({(spelling):[["/var/lib/tched-router"],["/other/identity"]]}), root, &spellings).is_err());
        }
        assert!(validate_config_selector(&serde_json::json!({"-c":["/var/lib/tched-router"], "--config-dir":["/var/lib/tched-router"]}), root, &spellings).is_err());
    }

    #[test]
    fn cancelled_future_signals_the_owned_process_worker() {
        let flag = Arc::new(AtomicBool::new(false));
        { let _guard = CancelOnDrop(flag.clone()); }
        assert!(flag.load(Ordering::Acquire));
    }
}
