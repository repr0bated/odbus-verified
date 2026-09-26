//! Deterministic audience and toolset policy for the single MCP ingress.
//!
//! These documents contain projection policy only. They never contain grants,
//! identity assertions, session anchors, or footprint/hash material.

use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use anyhow::{bail, Context, Result};
use op_plugins::state_plugins::mcp::{
    ConfigureToolGroupsInput, ConfigureToolGroupsOutput, ToolsetConfig, ToolsetTemperature,
};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

pub const DEFAULT_AUDIENCE_POLICY_PATH: &str = "/etc/opdbus/mcp-audience-policy.json";
pub const DEFAULT_TOOLSETS_PATH: &str = "/etc/opdbus/mcp-toolsets.json";

pub const COMPACT_TOOL_NAMES: [&str; 4] = [
    "list_tools",
    "search_tools",
    "get_tool_schema",
    "execute_tool",
];

pub const HOT_TOOL_NAMES: [&str; 19] = [
    "memory_recall",
    "memory_store",
    "memory_query",
    "memory_retrieve",
    "memory_delete",
    "memory_list_namespaces",
    "memory_semantic_query",
    "oscal_subids",
    "rust_pro",
    "python_pro",
    "typescript_pro",
    "sequential_thinking",
    "context_manager",
    "code_review_orchestrator",
    "specialist",
    "schema_read",
    "workflow_query",
    "workflow_run",
    "toolsets",
];

const SINGLETON_CHATBOT_DIRECT_TOOL_PREFIXES: [&str; 2] = ["shell_", "file_"];

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AudiencePolicy {
    pub version: u64,
    pub rotation_epoch: u64,
    pub singleton_chatbot_principal_id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolTemperature {
    Hot,
    Warm,
    Cold,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ToolsetDefinition {
    pub id: String,
    pub temperature: ToolTemperature,
    pub provider: String,
    pub requires_provider_health: bool,
    pub tools: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ToolsetManifest {
    pub generation: u64,
    pub hot: Vec<String>,
    pub sets: Vec<ToolsetDefinition>,
}

#[derive(Debug, Clone)]
pub struct McpProjectionPolicy {
    pub audience: AudiencePolicy,
    pub toolsets: ToolsetManifest,
}

/// One live owner shared by the public MCP gateway and MutationEngine's
/// authenticated configuration path.
pub(crate) struct McpPolicyOwner {
    live: RwLock<Arc<McpProjectionPolicy>>,
    mutation: Mutex<()>,
    toolsets_path: Option<PathBuf>,
    catalog_dir: PathBuf,
    required_uid: u32,
    #[cfg(test)]
    mutation_attempts: std::sync::atomic::AtomicUsize,
}

pub(crate) struct McpPolicyMutation<'a> {
    owner: &'a McpPolicyOwner,
    _guard: tokio::sync::MutexGuard<'a, ()>,
}

impl McpPolicyOwner {
    pub(crate) fn from_env() -> Result<Arc<Self>> {
        let audience_path = std::env::var_os("OP_MCP_AUDIENCE_POLICY_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_AUDIENCE_POLICY_PATH));
        let toolsets_path = std::env::var_os("OP_MCP_TOOLSETS_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_TOOLSETS_PATH));
        let policy = McpProjectionPolicy::load_protected(&audience_path, &toolsets_path)?;
        Ok(Arc::new(Self {
            live: RwLock::new(Arc::new(policy)),
            mutation: Mutex::new(()),
            toolsets_path: Some(toolsets_path),
            catalog_dir: crate::sealed_schema_reader::blob_catalog_dir_from_env(),
            required_uid: 0,
            #[cfg(test)]
            mutation_attempts: std::sync::atomic::AtomicUsize::new(0),
        }))
    }

    pub(crate) fn for_fixture(policy: McpProjectionPolicy) -> Arc<Self> {
        Arc::new(Self {
            live: RwLock::new(Arc::new(policy)),
            mutation: Mutex::new(()),
            toolsets_path: None,
            catalog_dir: crate::sealed_schema_reader::blob_catalog_dir_from_env(),
            required_uid: 0,
            #[cfg(test)]
            mutation_attempts: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    #[cfg(test)]
    pub(crate) fn for_durable_fixture(
        policy: McpProjectionPolicy,
        toolsets_path: PathBuf,
        catalog_dir: PathBuf,
    ) -> Result<Arc<Self>> {
        use std::os::unix::fs::OpenOptionsExt;
        let bytes = serde_json::to_vec_pretty(&policy.toolsets)?;
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&toolsets_path)?;
        std::io::Write::write_all(&mut file, &bytes)?;
        file.sync_all()?;
        let required_uid = nix::unistd::geteuid().as_raw();
        anyhow::ensure!(
            parse_toolset_manifest(&read_protected_for_uid(&toolsets_path, required_uid)?)?
                == policy.toolsets,
            "fixture MCP toolset policy differs from durable readback"
        );
        Ok(Arc::new(Self {
            live: RwLock::new(Arc::new(policy)),
            mutation: Mutex::new(()),
            toolsets_path: Some(toolsets_path),
            catalog_dir,
            required_uid,
            mutation_attempts: std::sync::atomic::AtomicUsize::new(0),
        }))
    }

    pub(crate) fn snapshot(&self) -> Arc<McpProjectionPolicy> {
        self.live.read().expect("MCP policy lock poisoned").clone()
    }

    pub(crate) async fn begin_mutation(&self) -> McpPolicyMutation<'_> {
        #[cfg(test)]
        self.mutation_attempts
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        McpPolicyMutation {
            owner: self,
            _guard: self.mutation.lock().await,
        }
    }

    #[cfg(test)]
    pub(crate) fn mutation_attempts(&self) -> usize {
        self.mutation_attempts
            .load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl McpPolicyMutation<'_> {
    pub(crate) fn configure(
        &self,
        input: ConfigureToolGroupsInput,
    ) -> Result<ConfigureToolGroupsOutput> {
        let path = self
            .owner
            .toolsets_path
            .as_deref()
            .context("MCP policy owner is not durable")?;
        let (output, toolsets) = if self.owner.required_uid == 0 {
            configure_tool_groups_at(path, &self.owner.catalog_dir, input)?
        } else {
            configure_tool_groups_at_for_uid(
                path,
                &self.owner.catalog_dir,
                input,
                self.owner.required_uid,
            )?
        };
        let current = self.owner.snapshot();
        let replacement = McpProjectionPolicy {
            audience: current.audience.clone(),
            toolsets,
        };
        *self.owner.live.write().expect("MCP policy lock poisoned") = Arc::new(replacement);
        Ok(output)
    }
}

impl McpProjectionPolicy {
    pub fn load_from_env() -> Result<Self> {
        let audience_path = std::env::var_os("OP_MCP_AUDIENCE_POLICY_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_AUDIENCE_POLICY_PATH));
        let toolsets_path = std::env::var_os("OP_MCP_TOOLSETS_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_TOOLSETS_PATH));
        Self::load_protected(&audience_path, &toolsets_path)
    }

    pub fn load_protected(audience_path: &Path, toolsets_path: &Path) -> Result<Self> {
        let audience_bytes = read_protected(audience_path)?;
        let toolsets_bytes = read_protected(toolsets_path)?;
        let audience = parse_audience_policy(&audience_bytes)?;
        let toolsets = parse_toolset_manifest(&toolsets_bytes)?;
        Ok(Self { audience, toolsets })
    }

    pub fn is_singleton_chatbot(&self, principal_id: &str) -> bool {
        principal_id == self.audience.singleton_chatbot_principal_id
    }

    /// Return whether the singleton chatbot audience must be denied a direct
    /// execution tool before any catalog, provider, or grant lookup occurs.
    ///
    /// MCP names may be raw during compatibility handling or canonical typed
    /// names such as `plugin.example.shell_execute`; the policy applies to the
    /// final name component in either form. Matching is intentionally
    /// case-sensitive and exact for the `dbus_call` leaf and the two prefixes.
    pub fn denies_singleton_chatbot_direct_tool(&self, principal_id: &str, name: &str) -> bool {
        if !self.is_singleton_chatbot(principal_id) {
            return false;
        }
        let leaf = name.rsplit('.').next().unwrap_or(name);
        leaf == "dbus_call"
            || SINGLETON_CHATBOT_DIRECT_TOOL_PREFIXES
                .iter()
                .any(|prefix| leaf.starts_with(prefix))
    }

    pub fn toolset(&self, id: &str) -> Option<&ToolsetDefinition> {
        self.toolsets.sets.iter().find(|set| set.id == id)
    }

    pub fn is_hot_tool(&self, name: &str) -> bool {
        HOT_TOOL_NAMES.contains(&name)
            || self.toolsets.sets.iter().any(|set| {
                set.temperature == ToolTemperature::Hot && set.tools.iter().any(|tool| tool == name)
            })
    }
}

pub fn parse_audience_policy(bytes: &[u8]) -> Result<AudiencePolicy> {
    let policy: AudiencePolicy =
        serde_json::from_slice(bytes).context("MCP audience policy is not valid JSON")?;
    if policy.version == 0 {
        bail!("MCP audience policy version must be positive");
    }
    if policy.rotation_epoch == 0 {
        bail!("MCP audience rotation_epoch must be positive");
    }
    let parsed = uuid::Uuid::parse_str(&policy.singleton_chatbot_principal_id)
        .context("singleton_chatbot_principal_id must be a UUID")?;
    if parsed.to_string() != policy.singleton_chatbot_principal_id {
        bail!("singleton_chatbot_principal_id must use canonical lowercase UUID form");
    }
    Ok(policy)
}

pub fn parse_toolset_manifest(bytes: &[u8]) -> Result<ToolsetManifest> {
    let manifest: ToolsetManifest =
        serde_json::from_slice(bytes).context("MCP toolset manifest is not valid JSON")?;
    if manifest.generation == 0 {
        bail!("MCP toolset generation must be positive");
    }
    if manifest.hot.as_slice() != HOT_TOOL_NAMES {
        bail!(
            "MCP HOT surface must be exactly: {}",
            HOT_TOOL_NAMES.join(", ")
        );
    }

    let mut set_ids = HashSet::new();
    for set in &manifest.sets {
        if !valid_identifier(&set.id) {
            bail!("invalid MCP toolset id '{}'", set.id);
        }
        if !set_ids.insert(set.id.as_str()) {
            bail!("duplicate MCP toolset id '{}'", set.id);
        }
        if !valid_provider_id(&set.provider) {
            bail!("invalid MCP toolset provider '{}'", set.provider);
        }
        if set.tools.is_empty() {
            bail!("MCP toolset '{}' contains no typed tools", set.id);
        }
        let mut tools = HashSet::new();
        for tool in &set.tools {
            if !valid_typed_tool_name(tool) {
                bail!("MCP toolset '{}' has non-canonical tool '{}'", set.id, tool);
            }
            if prohibited_external_tool(tool) {
                bail!(
                    "MCP toolset '{}' contains generic/compact tool '{}'",
                    set.id,
                    tool
                );
            }
            if !tools.insert(tool.as_str()) {
                bail!("MCP toolset '{}' repeats tool '{}'", set.id, tool);
            }
        }
    }
    Ok(manifest)
}

fn api_toolset(set: &ToolsetDefinition) -> ToolsetConfig {
    ToolsetConfig {
        id: set.id.clone(),
        temperature: match set.temperature {
            ToolTemperature::Hot => ToolsetTemperature::Hot,
            ToolTemperature::Warm => ToolsetTemperature::Warm,
            ToolTemperature::Cold => ToolsetTemperature::Cold,
        },
        provider: set.provider.clone(),
        requires_provider_health: set.requires_provider_health,
        tools: set.tools.clone(),
    }
}

fn policy_toolset(set: ToolsetConfig) -> ToolsetDefinition {
    ToolsetDefinition {
        id: set.id,
        temperature: match set.temperature {
            ToolsetTemperature::Hot => ToolTemperature::Hot,
            ToolsetTemperature::Warm => ToolTemperature::Warm,
            ToolsetTemperature::Cold => ToolTemperature::Cold,
        },
        provider: set.provider,
        requires_provider_health: set.requires_provider_health,
        tools: set.tools,
    }
}

fn validate_catalog_tools(catalog_dir: &Path, manifest: &ToolsetManifest) -> Result<()> {
    let catalog = crate::sealed_schema_reader::read_manifest_pinned_catalog(catalog_dir)?;
    for set in &manifest.sets {
        for tool in &set.tools {
            let mut parts = tool.split('.');
            anyhow::ensure!(
                parts.next() == Some("plugin"),
                "invalid typed MCP tool name"
            );
            let plugin = parts.next().context("typed MCP tool has no plugin")?;
            let method = parts.next().context("typed MCP tool has no method")?;
            let schema = catalog
                .plugins
                .get(plugin)
                .with_context(|| format!("MCP toolset references absent plugin '{plugin}'"))?;
            anyhow::ensure!(
                schema.schema.methods.contains_key(method),
                "MCP toolset references absent method '{tool}'"
            );
        }
    }
    Ok(())
}

fn atomic_write_protected(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let parent = path.parent().context("MCP toolset policy has no parent")?;
    let name = path
        .file_name()
        .context("MCP toolset policy has no filename")?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        name.to_string_lossy(),
        uuid::Uuid::new_v4()
    ));
    let result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// Commit the one canonical MCP toolset policy with exact-generation CAS.
/// MutationEngine serializes calls and swaps the returned policy into the live
/// gateway before publishing its receipt.
pub(crate) fn configure_tool_groups_at(
    toolsets_path: &Path,
    catalog_dir: &Path,
    input: ConfigureToolGroupsInput,
) -> Result<(ConfigureToolGroupsOutput, ToolsetManifest)> {
    configure_tool_groups_at_for_uid(toolsets_path, catalog_dir, input, 0)
}

fn configure_tool_groups_at_for_uid(
    toolsets_path: &Path,
    catalog_dir: &Path,
    input: ConfigureToolGroupsInput,
    required_uid: u32,
) -> Result<(ConfigureToolGroupsOutput, ToolsetManifest)> {
    let current_bytes = read_protected_for_uid(toolsets_path, required_uid)?;
    let (output, checked, encoded) = prepare_tool_groups(&current_bytes, catalog_dir, input)?;
    atomic_write_protected(toolsets_path, &encoded)?;
    let readback = parse_toolset_manifest(&read_protected_for_uid(toolsets_path, required_uid)?)?;
    anyhow::ensure!(readback == checked, "MCP toolset durable readback differs");
    Ok((output, readback))
}

fn prepare_tool_groups(
    current_bytes: &[u8],
    catalog_dir: &Path,
    input: ConfigureToolGroupsInput,
) -> Result<(ConfigureToolGroupsOutput, ToolsetManifest, Vec<u8>)> {
    let current = parse_toolset_manifest(current_bytes)?;
    anyhow::ensure!(
        current.generation == input.expected_generation,
        "stale MCP toolset generation"
    );
    let generation = current
        .generation
        .checked_add(1)
        .context("MCP toolset generation overflow")?;
    let candidate = ToolsetManifest {
        generation,
        hot: HOT_TOOL_NAMES
            .iter()
            .map(|name| (*name).to_owned())
            .collect(),
        sets: input.toolsets.into_iter().map(policy_toolset).collect(),
    };
    let encoded = serde_json::to_vec_pretty(&candidate)?;
    let checked = parse_toolset_manifest(&encoded)?;
    validate_catalog_tools(catalog_dir, &checked)?;
    let output = ConfigureToolGroupsOutput {
        generation,
        toolsets: checked.sets.iter().map(api_toolset).collect(),
    };
    Ok((output, checked, encoded))
}

fn read_protected(path: &Path) -> Result<Vec<u8>> {
    read_protected_for_uid(path, 0)
}

fn read_protected_for_uid(path: &Path, required_uid: u32) -> Result<Vec<u8>> {
    let metadata = fs::metadata(path)
        .with_context(|| format!("MCP policy file is unreadable: {}", path.display()))?;
    if !metadata.is_file() {
        bail!("MCP policy path is not a regular file: {}", path.display());
    }
    if metadata.uid() != required_uid {
        bail!("MCP policy file has the wrong owner: {}", path.display());
    }
    let mode = metadata.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        bail!(
            "MCP policy file must not be group/world accessible (mode {:o}): {}",
            mode,
            path.display()
        );
    }
    fs::read(path).with_context(|| format!("read MCP policy file {}", path.display()))
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn valid_provider_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-'
        })
}

fn valid_typed_tool_name(value: &str) -> bool {
    let mut parts = value.split('.');
    parts.next() == Some("plugin") && parts.clone().count() == 2 && parts.all(valid_identifier)
}

fn prohibited_external_tool(value: &str) -> bool {
    let leaf = value.rsplit('.').next().unwrap_or(value);
    COMPACT_TOOL_NAMES.contains(&leaf)
        || matches!(leaf, "invoke_tool" | "respond" | "register_tool")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_hot_manifest_is_accepted() {
        let manifest = parse_toolset_manifest(
            br#"{
              "generation": 7,
              "hot": ["memory_recall", "memory_store", "memory_query", "memory_retrieve", "memory_delete", "memory_list_namespaces", "memory_semantic_query", "oscal_subids", "rust_pro", "python_pro", "typescript_pro", "sequential_thinking", "context_manager", "code_review_orchestrator", "specialist", "schema_read", "workflow_query", "workflow_run", "toolsets"],
              "sets": [{
                "id": "emqx_ops",
                "temperature": "warm",
                "provider": "emqx",
                "requires_provider_health": true,
                "tools": ["plugin.emqx.get_status"]
              }]
            }"#,
        )
        .unwrap();
        assert_eq!(manifest.generation, 7);
    }

    #[test]
    fn frequency_or_generic_tools_cannot_change_the_surface() {
        let dynamic_hot = br#"{
          "generation": 1,
          "hot": ["memory_recall", "memory_store", "memory_query", "memory_retrieve", "memory_delete", "memory_list_namespaces", "memory_semantic_query", "oscal_subids", "rust_pro", "python_pro", "typescript_pro", "sequential_thinking", "context_manager", "code_review_orchestrator", "specialist", "schema_read", "workflow_query", "invoke_tool", "toolsets"],
          "sets": []
        }"#;
        assert!(parse_toolset_manifest(dynamic_hot).is_err());

        let generic_set = br#"{
          "generation": 1,
          "hot": ["memory_recall", "memory_store", "memory_query", "memory_retrieve", "memory_delete", "memory_list_namespaces", "memory_semantic_query", "oscal_subids", "rust_pro", "python_pro", "typescript_pro", "sequential_thinking", "context_manager", "code_review_orchestrator", "specialist", "schema_read", "workflow_query", "workflow_run", "toolsets"],
          "sets": [{
            "id": "escape",
            "temperature": "cold",
            "provider": "emqx",
            "requires_provider_health": false,
            "tools": ["plugin.cognitive_mcp.invoke_tool"]
          }]
        }"#;
        assert!(parse_toolset_manifest(generic_set).is_err());
    }

    #[test]
    fn tool_group_update_is_exact_generation_and_catalog_bound() {
        use op_state::StatePlugin;
        let directory = tempfile::tempdir().unwrap();
        let mut catalog = op_blob::BlobStore::open(directory.path()).unwrap();
        let schema = op_plugins::state_plugins::emqx::EmqxPlugin::new()
            .schema()
            .unwrap();
        catalog
            .write(&op_blob::blobify_plugin_schema("emqx", schema))
            .unwrap();
        let current = serde_json::to_vec(&ToolsetManifest {
            generation: 4,
            hot: HOT_TOOL_NAMES
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
            sets: vec![],
        })
        .unwrap();
        let input = ConfigureToolGroupsInput {
            expected_generation: 4,
            toolsets: vec![ToolsetConfig {
                id: "broker_ops".into(),
                temperature: ToolsetTemperature::Warm,
                provider: "emqx".into(),
                requires_provider_health: true,
                tools: vec!["plugin.emqx.get_status".into()],
            }],
        };
        let (output, checked, bytes) =
            prepare_tool_groups(&current, directory.path(), input.clone()).unwrap();
        assert_eq!(output.generation, 5);
        assert_eq!(checked.generation, 5);
        assert_eq!(parse_toolset_manifest(&bytes).unwrap(), checked);
        let mut stale = input;
        stale.expected_generation = 3;
        assert!(prepare_tool_groups(&current, directory.path(), stale).is_err());
        let absent = ConfigureToolGroupsInput {
            expected_generation: 4,
            toolsets: vec![ToolsetConfig {
                id: "escape".into(),
                temperature: ToolsetTemperature::Cold,
                provider: "missing".into(),
                requires_provider_health: false,
                tools: vec!["plugin.missing.invoke".into()],
            }],
        };
        assert!(prepare_tool_groups(&current, directory.path(), absent).is_err());
    }

    #[test]
    fn audience_is_one_exact_principal() {
        let policy = parse_audience_policy(
            br#"{
              "version": 1,
              "rotation_epoch": 2,
              "singleton_chatbot_principal_id": "1262e982-14c1-44d4-0771-7ce74213211b"
            }"#,
        )
        .unwrap();
        assert_eq!(policy.rotation_epoch, 2);

        assert!(parse_audience_policy(
            br#"{
              "version": 1,
              "rotation_epoch": 2,
              "singleton_chatbot_principal_id": "chatbot-*"
            }"#
        )
        .is_err());
    }

    #[test]
    fn deployed_manifest_keeps_cognitive_and_notebooklm_toolsets_hot() {
        let manifest = parse_toolset_manifest(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../deploy/config/mcp-toolsets.json"
        )))
        .unwrap();
        assert_eq!(manifest.hot.as_slice(), HOT_TOOL_NAMES);
        assert_eq!(manifest.generation, 8);
        for id in [
            "context_code",
            "context_knowledge",
            "notebooklm_auth",
            "notebooklm_research",
        ] {
            assert!(manifest
                .sets
                .iter()
                .any(|set| set.id == id && set.temperature == ToolTemperature::Hot));
        }
        assert!(manifest
            .sets
            .iter()
            .any(|set| set.id == "notebooklm_research"
                && set.provider == "notebooklm-mcp-authenticated"));
        assert!(manifest.sets.iter().any(|set| set.id == "mongodb_data"
            && set.provider == "mongodb-mcp-server-authenticated"
            && set.temperature == ToolTemperature::Warm));
        assert!(manifest.sets.iter().all(|set| {
            set.tools
                .iter()
                .all(|tool| !manifest.hot.iter().any(|hot| hot == tool))
        }));

        let policy = McpProjectionPolicy {
            audience: AudiencePolicy {
                version: 1,
                rotation_epoch: 1,
                singleton_chatbot_principal_id: "1262e982-14c1-44d4-0771-7ce74213211b".to_string(),
            },
            toolsets: manifest,
        };
        assert!(policy.is_hot_tool("rust_pro"));
        assert!(policy.is_hot_tool("schema_read"));
        assert!(policy.is_hot_tool("oscal_subids"));
        assert!(policy.is_hot_tool("plugin.cognitive_mcp.code_context"));
        assert!(policy.is_hot_tool("plugin.notebooklm.notebook_query"));
        assert!(!policy.is_hot_tool("plugin.mongodb_mcp.find"));
    }

    #[test]
    fn deployed_catalog_covers_every_agent_and_notebook_method() {
        let manifest = parse_toolset_manifest(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../deploy/config/mcp-toolsets.json"
        )))
        .unwrap();
        let all: Vec<_> = manifest
            .sets
            .iter()
            .flat_map(|set| set.tools.iter())
            .collect();
        for agent in op_plugins::state_plugins::cognitive_agents::catalog_agent_methods() {
            let name = format!("plugin.cognitive_mcp.{}", agent.declaration.name);
            assert_eq!(
                all.iter().filter(|tool| tool.as_str() == name).count(),
                1,
                "{name}"
            );
        }
        use op_state::StatePlugin;
        let notebook = op_plugins::state_plugins::notebooklm::NotebookLmPlugin::new()
            .schema()
            .unwrap();
        for method in notebook.methods.keys() {
            let name = format!("plugin.notebooklm.{method}");
            assert_eq!(
                all.iter().filter(|tool| tool.as_str() == name).count(),
                1,
                "{name}"
            );
        }
    }

    #[test]
    fn singleton_chatbot_direct_tool_guard_is_leaf_exact_and_human_neutral() {
        let policy = McpProjectionPolicy {
            audience: AudiencePolicy {
                version: 1,
                rotation_epoch: 1,
                singleton_chatbot_principal_id: "1262e982-14c1-44d4-0771-7ce74213211b".to_string(),
            },
            toolsets: ToolsetManifest {
                generation: 1,
                hot: HOT_TOOL_NAMES
                    .iter()
                    .map(|name| (*name).to_string())
                    .collect(),
                sets: Vec::new(),
            },
        };
        let chatbot = "1262e982-14c1-44d4-0771-7ce74213211b";
        let human = "0b7933b7-c8d5-1c01-814e-0c3066d19bbd";

        for name in [
            "shell_execute",
            "shell_read",
            "file_read",
            "file_write",
            "dbus_call",
            "plugin.example.shell_execute",
            "plugin.example.file_read",
            "plugin.example.dbus_call",
        ] {
            assert!(
                policy.denies_singleton_chatbot_direct_tool(chatbot, name),
                "{name}"
            );
        }
        for name in [
            "Shell_execute",
            "plugin.example.SHELL_EXECUTE",
            "dbus_call_extra",
            "plugin.example.dbus_call_extra",
            "memory_recall",
        ] {
            assert!(
                !policy.denies_singleton_chatbot_direct_tool(chatbot, name),
                "{name}"
            );
        }
        assert!(!policy.denies_singleton_chatbot_direct_tool(human, "shell_execute"));
    }

    #[test]
    fn identity_inspection_stays_read_only_with_human_storage_grants() {
        let manifest = parse_toolset_manifest(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../deploy/config/mcp-toolsets.json"
        )))
        .unwrap();
        let set = manifest
            .sets
            .iter()
            .find(|set| set.id == "identity_read")
            .unwrap();
        assert_eq!(set.temperature, ToolTemperature::Warm);
        assert!(!set.requires_provider_health);
        assert_eq!(
            set.tools,
            [
                "plugin.identity_sled.get_identity",
                "plugin.identity_sled.get_session_history",
            ]
        );
        let grants: serde_json::Value = serde_json::from_slice(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../deploy/security/capability-grants.json"
        )))
        .unwrap();
        let human = &grants["c9bdf32e-811d-2495-b688-277c9e5c4d36"]["capabilities"];
        let service = &grants["1262e982-14c1-44d4-0771-7ce74213211b"]["capabilities"];
        // Storage authorization belongs to the human principal. It does not
        // add mutation methods to the read-only inspection toolset above.
        for capability in [
            "identity_sled.read",
            "identity_sled.write",
            "cap.identity.sled.root_repair@v1",
        ] {
            assert!(human
                .as_array()
                .unwrap()
                .iter()
                .any(|cap| cap == capability));
            assert!(!service
                .as_array()
                .unwrap()
                .iter()
                .any(|cap| cap == capability));
        }
    }
}
