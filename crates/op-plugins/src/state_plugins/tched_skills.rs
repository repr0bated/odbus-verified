//! Typed OP-DBUS skills surface of the running tched-router threetched binary.
//! The sealed CLI and bundle schema come from that binary.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::process::Stdio;
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{CapabilityDecl, PluginSchema, SideEffect};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use simd_json::OwnedValue;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;

pub const PLUGIN_NAME: &str = "tched_skills";
const VERSION: &str = "1.0.0";
const MAX_OUTPUT: u64 = 128 * 1024;
const MAX_ARG: usize = 4096;
const DEADLINE: Duration = Duration::from_secs(45);
const READ: &str = "cap.software.tched-skills.read@v1";
const WRITE: &str = "cap.software.tched-skills.write@v1";
const INSTALL: &str = "cap.software.tched-skills.install@v1";
const EXECUTE: &str = "cap.software.tched-skills.execute@v1";

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SkillsState {
    pub binary: String,
    pub config_dir: String,
    pub binary_sha256: String,
    pub command_group: String,
    /// Exact parsed CLI subtree supplied by the native binary at seal time.
    pub cli_surface: Value,
    /// Verbatim skill bundle configuration subtree from `threetched config schema`.
    pub bundle_config_schema: Value,
}

struct CapturedSkills {
    state: SkillsState,
}

fn captured() -> Result<&'static CapturedSkills> {
    static CAPTURED: OnceLock<std::result::Result<CapturedSkills, String>> = OnceLock::new();
    CAPTURED
        .get_or_init(|| capture().map_err(|error| error.to_string()))
        .as_ref()
        .map_err(|error| anyhow::anyhow!("{error}"))
}

pub(crate) fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

const CLI_METHODS: &[(&str, &[&str], &[&str])] = &[
    ("list_skills", &["skills", "list"], &["--agent", "--bundle"]),
    ("add_skill", &["skills", "add"], &["--author", "--bundle", "--category", "--description", "--edit", "--license", "--no-scaffold", "--version"]),
    ("edit_skill", &["skills", "edit"], &["--bundle", "--file"]),
    ("audit_skill", &["skills", "audit"], &[]),
    ("install_skill", &["skills", "install"], &["--agent", "--bundle", "--no-tier-banner", "--skill"]),
    ("remove_skill", &["skills", "remove"], &["--agent", "--bundle"]),
    ("test_skills", &["skills", "test"], &[]),
    ("list_bundles", &["skills", "bundle", "list"], &[]),
    ("add_bundle", &["skills", "bundle", "add"], &["--directory"]),
    ("remove_bundle", &["skills", "bundle", "remove"], &["--yes"]),
    ("rename_bundle", &["skills", "bundle", "rename"], &[]),
    ("show_bundle", &["skills", "bundle", "show"], &[]),
];

fn capture() -> Result<CapturedSkills> {
    let shared = super::threetched::capture_state()?;
    let expected: Vec<_> = CLI_METHODS.iter().map(|(_, path, flags)| (*path, *flags)).collect();
    let cli_surface = super::threetched::captured_cli_group("skills", &expected)?;
    let bundle_config_schema =
        super::binary_namespace::filter_namespace(super::threetched::config_schema()?, "skill_bundles")?;
    // The candidate path is a build-time selector only. Seal the runtime path
    // so this descriptor remains valid after the binary is staged in a release.
    let state = SkillsState {
        binary: shared.binary.clone(),
        config_dir: shared.config_dir.clone(),
        binary_sha256: shared.binary_sha256.clone(),
        command_group: "skills".into(),
        cli_surface,
        bundle_config_schema,
    };
    Ok(CapturedSkills { state })
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SkillsOutput {
    /// The installed binary's command output, not a synthesized status flag.
    pub text: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListSkillsInput {
    pub agent: Option<String>,
    pub bundle: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NameInput {
    pub name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AuditSkillInput {
    pub source: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddSkillInput {
    pub name: String,
    pub description: String,
    pub bundle: Option<String>,
    pub license: Option<String>,
    pub author: Option<String>,
    pub version: Option<String>,
    pub category: Option<String>,
    #[serde(default)]
    pub no_scaffold: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InstallSkillInput {
    pub source: String,
    pub skill: Option<String>,
    pub agent: Option<String>,
    pub bundle: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveSkillInput {
    pub name: String,
    pub agent: Option<String>,
    pub bundle: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditSkillInput {
    pub name: String,
    pub bundle: Option<String>,
    /// A path inside the installed skill; omitted means SKILL.md.
    pub file: Option<String>,
    pub contents: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TestSkillsInput {
    pub name: Option<String>,
    #[serde(default)]
    pub verbose: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmptyInput {}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddBundleInput {
    pub alias: String,
    pub directory: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenameBundleInput {
    pub from: String,
    pub to: String,
}

#[derive(Default)]
pub struct TchedSkillsPlugin;

#[async_trait]
impl StatePlugin for TchedSkillsPlugin {
    fn name(&self) -> &str {
        PLUGIN_NAME
    }
    fn version(&self) -> &str {
        VERSION
    }
    fn schema(&self) -> Option<PluginSchema> {
        Some(tched_skills_schema())
    }

    async fn calculate_diff(
        &self,
        current: &OwnedValue,
        desired: &OwnedValue,
    ) -> Result<StateDiff> {
        ensure!(
            current == desired && *desired == state_value()?,
            "threetched skills descriptor is immutable"
        );
        Ok(StateDiff {
            plugin: PLUGIN_NAME.into(),
            actions: vec![],
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: String::new(),
                desired_hash: String::new(),
            },
        })
    }
    async fn apply_state(&self, diff: &StateDiff) -> Result<ApplyResult> {
        ensure!(
            diff.plugin == PLUGIN_NAME && diff.actions.is_empty(),
            "threetched skills descriptor is immutable"
        );
        Ok(ApplyResult {
            success: true,
            changes_applied: vec![],
            errors: vec![],
            checkpoint: None,
        })
    }
    async fn verify_state(&self, desired: &OwnedValue) -> Result<bool> {
        Ok(*desired == state_value()?)
    }
    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        bail!("threetched skills has no mutable plugin-state checkpoints")
    }
    async fn rollback(&self, _: &Checkpoint) -> Result<()> {
        bail!("threetched skills has no mutable plugin-state checkpoints")
    }
    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: false,
            supports_checkpoints: false,
            supports_verification: true,
            atomic_operations: false,
        }
    }
}

pub fn state_value() -> Result<OwnedValue> {
    if !super::threetched::is_schema_capture() {
        return super::threetched::sealed_state(PLUGIN_NAME);
    }
    Ok(simd_json::serde::to_owned_value(&captured()?.state)?)
}

pub fn tched_skills_schema() -> PluginSchema {
    if !super::threetched::is_schema_capture() {
        return super::threetched::sealed_schema(PLUGIN_NAME)
            .expect("tched_skills runtime requires its sealed schema");
    }
    let captured = captured().expect("installed threetched skills contract must be captured before sealing");
    let root =
        serde_json::to_value(schemars::schema_for!(SkillsState)).expect("skills state schema");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        PLUGIN_NAME,
        VERSION,
        "Typed commands backed by the installed threetched skills binary",
        &root,
    );
    schema.category = "software".into();
    schema.display_name = Some("Tched Skills".into());
    schema.tags = vec!["threetched".into(), "skills".into(), "binary".into()];
    schema.immutable_paths = vec![
        "/binary".into(),
        "/config_dir".into(),
        "/binary_sha256".into(),
        "/command_group".into(),
        "/cli_surface".into(),
        "/bundle_config_schema".into(),
    ];
    // Publish closed metadata fields; opaque discovery evidence is sealed in example.
    schema.public_state.paths = ["binary", "config_dir", "binary_sha256", "command_group"]
        .map(|name| format!("/{name}")).into();
    let state = simd_json::serde::to_owned_value(&captured.state).expect("skills state serializes");
    super::schemars_adapter::apply_state_defaults(&mut schema, &state);
    schema.example = Some(state);

    macro_rules! method {
        ($name:literal, $input:ty, $effect:expr, $idempotent:expr, $cap:expr, $subid:literal) => {
            schema.methods.insert(
                $name.into(),
                method_decl_from_schemars_with_output::<$input, SkillsOutput>(
                    $name,
                    $effect,
                    $idempotent,
                    $cap,
                    $subid,
                ),
            );
        };
    }
    method!(
        "list_skills",
        ListSkillsInput,
        SideEffect::Read,
        true,
        READ,
        "obs.software.tched-skills.list@v1"
    );
    method!(
        "audit_skill",
        AuditSkillInput,
        SideEffect::Read,
        true,
        READ,
        "obs.software.tched-skills.audit@v1"
    );
    method!(
        "list_bundles",
        EmptyInput,
        SideEffect::Read,
        true,
        READ,
        "obs.software.tched-skills.bundles-list@v1"
    );
    method!(
        "show_bundle",
        NameInput,
        SideEffect::Read,
        true,
        READ,
        "obs.software.tched-skills.bundle-show@v1"
    );
    method!(
        "add_skill",
        AddSkillInput,
        SideEffect::Mutation,
        false,
        WRITE,
        "mut.software.tched-skills.add@v1"
    );
    method!(
        "edit_skill",
        EditSkillInput,
        SideEffect::Mutation,
        false,
        WRITE,
        "mut.software.tched-skills.edit@v1"
    );
    method!(
        "remove_skill",
        RemoveSkillInput,
        SideEffect::Mutation,
        false,
        WRITE,
        "mut.software.tched-skills.remove@v1"
    );
    method!(
        "add_bundle",
        AddBundleInput,
        SideEffect::Mutation,
        false,
        WRITE,
        "mut.software.tched-skills.bundle-add@v1"
    );
    method!(
        "rename_bundle",
        RenameBundleInput,
        SideEffect::Mutation,
        false,
        WRITE,
        "mut.software.tched-skills.bundle-rename@v1"
    );
    method!(
        "remove_bundle",
        NameInput,
        SideEffect::Mutation,
        false,
        WRITE,
        "mut.software.tched-skills.bundle-remove@v1"
    );
    method!(
        "install_skill",
        InstallSkillInput,
        SideEffect::Mutation,
        false,
        INSTALL,
        "mut.software.tched-skills.install@v1"
    );
    method!(
        "test_skills",
        TestSkillsInput,
        SideEffect::Mutation,
        false,
        EXECUTE,
        "mut.software.tched-skills.test@v1"
    );
    assert_eq!(
        schema.methods.keys().map(String::as_str).collect::<BTreeSet<_>>(),
        CLI_METHODS.iter().map(|(name, _, _)| *name).collect(),
        "every binary-backed skills command needs exactly one sealed method"
    );
    for (id, description) in [
        (READ, "Read installed threetched skills and audit metadata."),
        (
            WRITE,
            "Create, remove, or rename installed threetched skills and bundles.",
        ),
        (
            INSTALL,
            "Install skill code from a URL or local source through threetched.",
        ),
        (EXECUTE, "Run skill TEST.sh scripts through threetched."),
    ] {
        schema.capabilities.insert(
            id.into(),
            CapabilityDecl {
                id: id.into(),
                description: description.into(),
            },
        );
    }
    schema
}

fn token(value: &str) -> Result<String> {
    ensure!(
        !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-'),
        "skill or bundle name must be lowercase letters, digits, and hyphens"
    );
    Ok(value.into())
}

fn text_arg(value: &str) -> Result<String> {
    ensure!(
        !value.is_empty() && value.len() <= MAX_ARG && !value.chars().any(char::is_control),
        "invalid skill argument"
    );
    Ok(value.into())
}

fn source_arg(value: &str) -> Result<String> {
    let value = text_arg(value)?;
    ensure!(!value.starts_with('-'), "source cannot begin with a flag");
    Ok(value)
}

fn relative_file(value: &str) -> Result<String> {
    let value = text_arg(value)?;
    let path = Path::new(&value);
    ensure!(
        !path.is_absolute()
            && path.components().all(|component| matches!(component, std::path::Component::Normal(_))),
        "edited file must be a relative path within the skill"
    );
    Ok(value)
}

fn option(argv: &mut Vec<String>, flag: &str, value: Option<&str>, is_token: bool) -> Result<()> {
    if let Some(value) = value {
        argv.push(flag.into());
        argv.push(if is_token {
            token(value)?
        } else {
            text_arg(value)?
        });
    }
    Ok(())
}

/// Build argv from typed arguments only. There is no caller-provided command word.
fn arguments(method: &str, value: &Value) -> Result<Vec<String>> {
    let mut argv = vec!["skills".into()];
    match method {
        "list_skills" => {
            let input: ListSkillsInput = serde_json::from_value(value.clone())?;
            argv.push("list".into());
            option(&mut argv, "--agent", input.agent.as_deref(), true)?;
            option(&mut argv, "--bundle", input.bundle.as_deref(), true)?;
        }
        "audit_skill" => {
            let input: AuditSkillInput = serde_json::from_value(value.clone())?;
            argv.extend(["audit".into(), source_arg(&input.source)?]);
        }
        "list_bundles" => {
            let _: EmptyInput = serde_json::from_value(value.clone())?;
            argv.extend(["bundle".into(), "list".into()]);
        }
        "show_bundle" | "remove_bundle" => {
            let input: NameInput = serde_json::from_value(value.clone())?;
            argv.extend([
                "bundle".into(),
                if method == "show_bundle" {
                    "show"
                } else {
                    "remove"
                }
                .into(),
                token(&input.name)?,
            ]);
            if method == "remove_bundle" {
                argv.push("--yes".into());
            }
        }
        "add_skill" => {
            let input: AddSkillInput = serde_json::from_value(value.clone())?;
            argv.extend([
                "add".into(),
                token(&input.name)?,
                "--description".into(),
                text_arg(&input.description)?,
            ]);
            option(&mut argv, "--bundle", input.bundle.as_deref(), true)?;
            option(&mut argv, "--license", input.license.as_deref(), false)?;
            option(&mut argv, "--author", input.author.as_deref(), false)?;
            option(&mut argv, "--version", input.version.as_deref(), false)?;
            option(&mut argv, "--category", input.category.as_deref(), false)?;
            if input.no_scaffold {
                argv.push("--no-scaffold".into());
            }
        }
        "edit_skill" => {
            let input: EditSkillInput = serde_json::from_value(value.clone())?;
            ensure!(input.contents.len() <= 64 * 1024 && !input.contents.contains('\0'), "edited skill content exceeds 64 KiB or contains NUL");
            argv.extend(["edit".into(), token(&input.name)?]);
            option(&mut argv, "--bundle", input.bundle.as_deref(), true)?;
            if let Some(file) = input.file {
                argv.extend(["--file".into(), relative_file(&file)?]);
            }
        }
        "remove_skill" => {
            let input: RemoveSkillInput = serde_json::from_value(value.clone())?;
            argv.extend(["remove".into(), token(&input.name)?]);
            option(&mut argv, "--agent", input.agent.as_deref(), true)?;
            option(&mut argv, "--bundle", input.bundle.as_deref(), true)?;
        }
        "install_skill" => {
            let input: InstallSkillInput = serde_json::from_value(value.clone())?;
            argv.extend(["install".into(), source_arg(&input.source)?]);
            option(&mut argv, "--skill", input.skill.as_deref(), true)?;
            option(&mut argv, "--agent", input.agent.as_deref(), true)?;
            option(&mut argv, "--bundle", input.bundle.as_deref(), true)?;
            argv.push("--no-tier-banner".into());
        }
        "test_skills" => {
            let input: TestSkillsInput = serde_json::from_value(value.clone())?;
            argv.push("test".into());
            if let Some(name) = input.name {
                argv.push(token(&name)?);
            }
            if input.verbose {
                argv.push("--verbose".into());
            }
        }
        "add_bundle" => {
            let input: AddBundleInput = serde_json::from_value(value.clone())?;
            argv.extend(["bundle".into(), "add".into(), token(&input.alias)?]);
            option(&mut argv, "--directory", input.directory.as_deref(), false)?;
        }
        "rename_bundle" => {
            let input: RenameBundleInput = serde_json::from_value(value.clone())?;
            argv.extend([
                "bundle".into(),
                "rename".into(),
                token(&input.from)?,
                token(&input.to)?,
            ]);
        }
        _ => bail!("unknown threetched skills method: {method}"),
    }
    Ok(argv)
}

async fn read_limited<R: tokio::io::AsyncRead + Unpin>(reader: R) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(MAX_OUTPUT + 1).read_to_end(&mut bytes).await?;
    ensure!(
        bytes.len() as u64 <= MAX_OUTPUT,
        "threetched command output exceeded 128 KiB"
    );
    Ok(bytes)
}

pub(crate) async fn run(
    binary: &Path,
    config_dir: &Path,
    argv: &[String],
    contents: Option<&str>,
    drop_privileges: bool,
) -> Result<SkillsOutput> {
    let mut command = if drop_privileges && nix::unistd::geteuid().is_root() {
        // chpst clears supplementary groups as well as uid/gid. Setting only
        // CommandExt::uid/gid would retain the bridge's supplementary groups.
        let mut command = Command::new("/usr/bin/chpst");
        command.args(["-u", "jeremy:jeremy"]).arg(binary);
        command
    } else {
        if drop_privileges {
            let owner = nix::unistd::User::from_name("jeremy")?
                .context("threetched service owner is absent")?;
            ensure!(nix::unistd::geteuid() == owner.uid && nix::unistd::getegid() == owner.gid,
                "threetched skills dispatch must run as its configured service owner");
        }
        Command::new(binary)
    };
    command.args(argv).args([
        "--config-dir",
        config_dir.to_str().context("non-UTF8 config path")?,
    ]);
    command
        .env_clear()
        .env("HOME", config_dir.join("home"))
        .env("USER", "jeremy")
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("THREETCHED_CONFIG_DIR", config_dir)
        .stdin(if contents.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if contents.is_some() {
        command.env("EDITOR", "/usr/bin/tee").env("VISUAL", "/usr/bin/tee");
    }
    let mut child = command.spawn().context("start installed threetched binary")?;
    let stdout = child.stdout.take().context("missing threetched stdout")?;
    let stderr = child.stderr.take().context("missing threetched stderr")?;
    let stdin = child.stdin.take();
    let result = tokio::time::timeout(DEADLINE, async {
        let (stdout, stderr, status, ()) =
            tokio::try_join!(read_limited(stdout), read_limited(stderr), async {
                Ok::<_, anyhow::Error>(child.wait().await?)
            }, async {
                if let (Some(mut stdin), Some(contents)) = (stdin, contents) {
                    stdin.write_all(contents.as_bytes()).await?;
                }
                Ok::<_, anyhow::Error>(())
            })?;
        Ok::<_, anyhow::Error>((stdout, stderr, status))
    })
    .await
    .context("threetched command timed out")??;
    let (stdout, stderr, status) = result;
    let output = String::from_utf8(stdout).context("threetched stdout is not UTF-8")?;
    let diagnostic = String::from_utf8_lossy(&stderr);
    ensure!(
        status.success(),
        "threetched command failed: {}",
        diagnostic.trim()
    );
    Ok(SkillsOutput { text: output })
}

pub async fn dispatch(method: &str, value: &Value, sealed_source: &SkillsState) -> Result<Value> {
    let binary = super::tched_router_paths::binary()?;
    let config_dir = super::tched_router_paths::config_dir()?;
    let binary_for_hash = binary.clone();
    let current_hash = tokio::task::spawn_blocking(move || sha256_file(&binary_for_hash)).await??;
    ensure!(
        binary == Path::new(&sealed_source.binary)
            && config_dir == Path::new(&sealed_source.config_dir)
            && current_hash == sealed_source.binary_sha256,
        "threetched skills binary or router paths changed since seal; reseal before calling methods"
    );
    let argv = arguments(method, value)?;
    let contents = if method == "edit_skill" {
        Some(serde_json::from_value::<EditSkillInput>(value.clone())?.contents)
    } else {
        None
    };
    Ok(serde_json::to_value(
        run(&binary, &config_dir, &argv, contents.as_deref(), true).await?,
    )?)
}

inventory::submit! {
    crate::default_registry::PluginReg::new(PLUGIN_NAME, |_ctx| std::sync::Arc::new(TchedSkillsPlugin))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_is_sealable_and_every_method_has_a_capability() {
        super::super::threetched::prepare_schema_capture().expect("explicit schema capture");
        let schema = tched_skills_schema();
        assert!(schema.validate_capability_closure().missing.is_empty());
        schema.validate_public_state_policy().unwrap();
        for method in schema.methods.values() {
            super::super::common::oscal::validate_subid(&method.subid).unwrap();
            assert!(method.returns.is_some());
        }
        let blob = op_blob::blobify_plugin_schema(PLUGIN_NAME, schema);
        let bytes = blob.seal();
        let pinned =
            op_blob::BlobRef::from_manifest_pinned(&bytes, &blob.manifest.schema_hash).unwrap();
        let sealed = pinned.state_store_schema().unwrap();
        assert!(sealed.validate(&state_value().unwrap()).valid);
    }

    #[test]
    fn typed_arguments_reject_flags_and_extra_fields() {
        assert!(
            arguments(
                "remove_skill",
                &serde_json::json!({"name":"--help","agent":null,"bundle":null})
            )
            .is_err()
        );
        assert!(arguments("list_bundles", &serde_json::json!({"command":"remove"})).is_err());
        assert!(
            arguments(
                "edit_skill",
                &serde_json::json!({
                    "name": "sample",
                    "bundle": null,
                    "file": "../outside",
                    "contents": "data"
                })
            )
            .is_err()
        );
        assert_eq!(
            arguments("install_skill", &serde_json::json!({"source":"https://example.test/skill","skill":null,"agent":null,"bundle":null})).unwrap(),
            ["skills", "install", "https://example.test/skill", "--no-tier-banner"],
        );
    }
}
