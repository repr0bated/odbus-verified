use anyhow::Result;
use async_trait::async_trait;
use op_state::{ApplyResult, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{CapabilityDecl, PluginSchema};
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;

use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;

use super::xray_config_types::{
    APIConfig, DNSConfig, PolicyConfig, XrayInboundProtocolCatalog, XrayOutboundProtocolCatalog,
};

/// Xray proxy configuration.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.xray.config.schema@v1"))]
pub struct XrayConfig {
    /// Whether the xray proxy is enabled.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.software.plugin.xray.config.enabled@v1"))]
    pub enabled: bool,
    /// Xray socket tag/port name.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.software.plugin.xray.config.socket-port@v1"))]
    pub socket_port: String,
    /// Path to the xray JSON config.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.software.plugin.xray.config.config-path@v1"))]
    pub config_path: String,
    /// Xray commander API block (`api`), including HandlerService for runtime mutations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("x-oscal-subid" = "sch.software.plugin.xray.config.api@v1"))]
    pub api: Option<APIConfig>,
    /// Xray DNS block (`dns`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("x-oscal-subid" = "sch.software.plugin.xray.config.dns@v1"))]
    pub dns: Option<DNSConfig>,
    /// Xray policy block (`policy`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("x-oscal-subid" = "sch.software.plugin.xray.config.policy@v1"))]
    pub policy: Option<PolicyConfig>,
    /// Reference catalog of every inbound-protocol settings shape xray-core defines (dokodemo/http/socks/vmess/vless/trojan/shadowsocks/freedom).
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.software.plugin.xray.config.inbound-protocols@v1"))]
    pub inbound_protocols: XrayInboundProtocolCatalog,
    /// Reference catalog of every outbound-protocol settings shape xray-core defines (blackhole/freedom/http/socks/vmess/vless/trojan/shadowsocks).
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.software.plugin.xray.config.outbound-protocols@v1"))]
    pub outbound_protocols: XrayOutboundProtocolCatalog,
}

impl Default for XrayConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            socket_port: "gbr_xray".to_string(),
            config_path: "/etc/xray/xray_config.json".to_string(),
            api: None,
            dns: None,
            policy: None,
            inbound_protocols: XrayInboundProtocolCatalog::default(),
            outbound_protocols: XrayOutboundProtocolCatalog::default(),
        }
    }
}

/// Runtime state of the xray plugin.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.xray.schema@v1"))]
#[schemars(extend("x-oscal-category" = "network"))]
pub struct XrayState {
    /// Software identifier.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.xray.software@v1"))]
    pub software: String,
    /// Software version.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.xray.version@v1"))]
    pub version: String,
    /// Runtime dependencies.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.xray.dependencies@v1"))]
    pub dependencies: Vec<String>,
    /// OSCAL subid registry source path.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "src.software.plugin.xray.oscal-source@v1"))]
    pub oscal_source: Option<String>,
    /// Xray configuration.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.software.plugin.xray.config@v1"))]
    pub config: XrayConfig,
    /// Whether an xray process is currently running (host-native).
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.xray.running@v1"))]
    pub running: bool,
    /// MCP tool definitions exposed by this plugin.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.plugin.xray.tools@v1"))]
    pub tools: serde_json::Value,
}

impl Default for XrayState {
    fn default() -> Self {
        Self {
            software: "xray-core".to_string(),
            version: "1.0.0".to_string(),
            dependencies: vec!["incus".to_string()],
            oscal_source: Some("/org/opdbus/v1/plugins/oscal_subid_registry".to_string()),
            config: XrayConfig::default(),
            running: false,
            tools: serde_json::json!([
                {
                    "name": "xray.run",
                    "description": "Run the Xray daemon",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "config": {
                                "type": "string",
                                "description": "The path to the config file"
                            }
                        },
                        "required": ["config"]
                    }
                }
            ]),
        }
    }
}

/// Find running `xray` process PIDs by scanning `/proc` directly — no
/// `pgrep`/`pkill` subprocess spawning (CLAUDE.md: no `Command::new(...)`
/// subprocesses in plugin/service code).
fn find_xray_pids() -> Vec<nix::unistd::Pid> {
    let mut pids = Vec::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return pids;
    };
    for entry in entries.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<i32>() else {
            continue;
        };
        let comm_path = entry.path().join("comm");
        if let Ok(comm) = std::fs::read_to_string(&comm_path) {
            if comm.trim() == "xray" {
                pids.push(nix::unistd::Pid::from_raw(pid));
            }
        }
    }
    pids
}

/// Is an xray process currently running on the host?
fn xray_running() -> bool {
    !find_xray_pids().is_empty()
}

pub struct XrayPlugin {
    config: XrayConfig,
}

impl XrayPlugin {
    pub fn new(config: XrayConfig) -> Self {
        Self { config }
    }
    pub(crate) fn current_state() -> XrayState {
        XrayState {
            software: "xray-core".to_string(),
            version: "1.0.0".to_string(),
            dependencies: vec!["incus".to_string()],
            oscal_source: Some("/org/opdbus/v1/plugins/oscal_subid_registry".to_string()),
            config: XrayConfig::default(),
            running: false,
            tools: serde_json::json!([
                {
                    "name": "xray.run",
                    "description": "Run the Xray daemon",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "config": {
                                "type": "string",
                                "description": "The path to the config file"
                            }
                        },
                        "required": ["config"]
                    }
                }
            ]),
        }
    }
}

#[async_trait]
impl StatePlugin for XrayPlugin {
    fn name(&self) -> &'static str {
        "xray"
    }

    fn version(&self) -> &str {
        "4.0.0"
    }

    fn schema(&self) -> Option<op_state_store::PluginSchema> {
        let mut schema = xray_schema();
        super::common::oscal::ensure_category_metadata_fields(&mut schema);
        Some(schema)
    }

    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: false,
            supports_checkpoints: false,
            supports_verification: false,
            atomic_operations: false,
        }
    }

    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        Ok(StateDiff {
            plugin: self.name().to_string(),
            actions: vec![],
            metadata: op_state::DiffMetadata {
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_secs() as i64,
                current_hash: "".to_string(),
                desired_hash: "".to_string(),
            },
        })
    }

    async fn apply_state(&self, _diff: &StateDiff) -> Result<ApplyResult> {
        // xray control point (the ONLY projectable tree: /org/opdbus/v1/plugins/xray).
        // Host-native lifecycle — no out-of-tree `opdbus.v1.Xray` daemon.
        let mut changes = Vec::new();
        let mut errors = Vec::new();
        let pids = find_xray_pids();
        if self.config.enabled {
            // Reload (SIGHUP) so a running xray re-reads its config; if none is
            // running, the container's own supervisor (systemd, inside the
            // `xray` container) is responsible for starting it.
            if pids.is_empty() {
                changes.push("xray not running; supervisor starts it".to_string());
            } else {
                for pid in pids {
                    match nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGHUP) {
                        Ok(()) => changes.push(format!("xray reloaded (SIGHUP, pid {pid})")),
                        Err(e) => errors.push(format!("xray reload failed (pid {pid}): {e}")),
                    }
                }
            }
        } else if pids.is_empty() {
            changes.push("xray already stopped".to_string());
        } else {
            for pid in pids {
                match nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGTERM) {
                    Ok(()) => changes.push(format!("xray stopped (pid {pid})")),
                    Err(e) => errors.push(format!("xray stop failed (pid {pid}): {e}")),
                }
            }
        }
        Ok(ApplyResult {
            success: errors.is_empty(),
            changes_applied: changes,
            errors,
            checkpoint: None,
        })
    }

    async fn verify_state(&self, _desired: &Value) -> Result<bool> {
        Ok(true)
    }

    async fn create_checkpoint(&self) -> Result<op_state::Checkpoint> {
        Err(anyhow::anyhow!(
            "Checkpoints not supported by xray schema plugin"
        ))
    }

    async fn rollback(&self, _checkpoint: &op_state::Checkpoint) -> Result<()> {
        Err(anyhow::anyhow!(
            "Rollbacks not supported by xray schema plugin"
        ))
    }
}

/// Derived `xray` schema from the typed [`XrayState`] struct via schemars.
pub(crate) fn xray_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(XrayState))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "xray",
        "4.0.0",
        "Xray proxy state and execution schema",
        &root,
    );
    let state = simd_json::serde::to_owned_value(&XrayState::default())
        .expect("XrayState default serializes");
    super::schemars_adapter::apply_state_defaults(&mut schema, &state);

    use op_state_store::SideEffect;

    // Backed by a real dispatcher (op-grpc-bridge's dispatch_xray_method):
    // xray-core StatsService.GetStats over the commander API socket.
    schema.methods.insert(
        "get_stats".to_string(),
        method_decl_from_schemars_with_output::<StatsInput, GetStatsOutput>(
            "get_stats",
            SideEffect::Read,
            true,
            "xray.read",
            "obs.software.plugin.xray.stats.get@v1",
        ),
    );
    // v4 retires restart and the three tracing declarations: the pinned
    // xray-core Commander protocol has no corresponding operations. Do not
    // advertise a lifecycle or trace API until its real owner is integrated.
    // Native xray-core HandlerService operations. These mutate the running
    // process only; they do not rewrite or persist its bootstrap JSON config.
    schema.methods.insert(
        "add_user".to_string(),
        method_decl_from_schemars_with_output::<AddUserInput, AddUserOutput>(
            "add_user",
            SideEffect::Mutation,
            false,
            "xray.write",
            "mut.software.plugin.xray.user.add@v1",
        ),
    );
    schema.methods.insert(
        "remove_user".to_string(),
        method_decl_from_schemars_with_output::<RemoveUserInput, RemoveUserOutput>(
            "remove_user",
            SideEffect::Mutation,
            false,
            "xray.write",
            "mut.software.plugin.xray.user.remove@v1",
        ),
    );
    schema.methods.insert(
        "add_inbound".to_string(),
        method_decl_from_schemars_with_output::<AddInboundInput, AddInboundOutput>(
            "add_inbound",
            SideEffect::Mutation,
            false,
            "xray.write",
            "mut.software.plugin.xray.inbound.add@v1",
        ),
    );

    schema.capabilities.insert(
        "xray.read".to_string(),
        CapabilityDecl {
            id: "xray.read".to_string(),
            description: "Grants: get_stats.".to_string(),
        },
    );
    schema.capabilities.insert(
        "xray.write".to_string(),
        CapabilityDecl {
            id: "xray.write".to_string(),
            description: "Grants: add_user, remove_user, add_inbound.".to_string(),
        },
    );

    schema
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_plugins::common::oscal::validate_subid;
    use crate::state_plugins::schemars_adapter::schema_diffs;
    use serde_json::Value as JVal;

    fn collect_subids(value: &JVal, out: &mut Vec<String>) {
        if let Some(obj) = value.as_object() {
            if let Some(JVal::String(subid)) = obj.get("x-oscal-subid") {
                out.push(subid.clone());
            }
            for v in obj.values() {
                collect_subids(v, out);
            }
        }
        if let Some(arr) = value.as_array() {
            for v in arr {
                collect_subids(v, out);
            }
        }
    }

    #[test]
    fn policy_level_keys_and_null_values_keep_the_exact_derived_contract() {
        use op_state_store::FieldType;
        let raw = serde_json::to_value(schemars::schema_for!(PolicyConfig)).unwrap();
        let levels = &raw["properties"]["levels"];
        assert!(
            levels.get("$ref").is_none(),
            "explicit inline marker must survive resolution"
        );
        assert!(levels["patternProperties"].is_object());
        assert_eq!(levels["additionalProperties"], serde_json::json!(false));
        let schema = super::super::schemars_adapter::try_plugin_schema_from_json(
            "xray_policy_fixture",
            "3.0.0",
            "policy",
            &raw,
        )
        .unwrap();
        let FieldType::JsonSchema(document) = &schema.fields["levels"].field_type else {
            panic!("exact keyed policy field");
        };
        let captured = serde_json::to_value(document).unwrap();
        assert_eq!(captured["patternProperties"], levels["patternProperties"]);
        assert_eq!(
            captured["additionalProperties"],
            levels["additionalProperties"]
        );
        let valid = serde_json::json!({"levels":{"0":null,"42":{"handshake":4}}});
        schema
            .validate_exact_schema_fields(&simd_json::serde::to_owned_value(&valid).unwrap())
            .unwrap();
        let exported = serde_json::to_value(schema.to_json_schema()).unwrap();
        let validator = jsonschema::validator_for(&exported).unwrap();
        assert!(validator.is_valid(&valid));
        for invalid in [
            serde_json::json!({"levels":{"name":null}}),
            serde_json::json!({"levels":{"-1":null}}),
        ] {
            assert!(!validator.is_valid(&invalid));
            assert!(schema
                .validate_exact_schema_fields(&simd_json::serde::to_owned_value(invalid).unwrap())
                .is_err());
        }
        let canonical = xray_schema();
        assert_eq!(
            canonical.version,
            XrayPlugin::new(XrayConfig::default()).version()
        );
        let restored: PluginSchema = simd_json::serde::from_owned_value(
            simd_json::serde::to_owned_value(canonical).unwrap(),
        )
        .unwrap();
        assert_eq!(restored.version, "4.0.0");
    }

    #[test]
    fn method_surface_contains_only_backed_commander_operations() {
        let schema = xray_schema();
        let mut names: Vec<_> = schema.methods.keys().map(String::as_str).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            ["add_inbound", "add_user", "get_stats", "remove_user"]
        );
        for retired in ["restart", "start_trace", "end_trace", "record_span"] {
            assert!(!schema.capabilities["xray.write"]
                .description
                .contains(retired));
        }
    }

    #[test]
    fn all_subids_are_valid() {
        let root = serde_json::to_value(schemars::schema_for!(XrayState))
            .expect("schemars schema serializes to JSON");
        let mut subids = Vec::new();
        collect_subids(&root, &mut subids);
        assert!(!subids.is_empty(), "expected at least one subid");
        for subid in subids {
            assert!(validate_subid(&subid).is_ok(), "invalid subid: {subid}");
        }
    }
}

/// Output for `get_stats`: a single xray-core StatsService counter.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GetStatsOutput {
    pub name: String,
    pub value: i64,
}

/// Supported native Xray account and inbound protocols. Other protocol/config
/// variants are not accepted by these methods and must not be silently ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum XrayHandlerProtocol {
    Vless,
    Vmess,
    Trojan,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
pub enum XrayVlessFlow {
    #[default]
    #[serde(rename = "")]
    None,
    #[serde(rename = "xtls-rprx-vision")]
    Vision,
}

/// Credentials are transient method arguments, never projection/config state.
/// This type deliberately has no Debug implementation exposing credentials.
#[derive(Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "protocol", rename_all = "snake_case", deny_unknown_fields)]
pub enum XrayHandlerAccount {
    Vless {
        /// Account UUID, validated and normalized before the native RPC.
        #[schemars(length(min = 1, max = 64))]
        id: String,
        #[serde(default)]
        flow: XrayVlessFlow,
    },
    Vmess {
        #[schemars(length(min = 1, max = 64))]
        id: String,
    },
    Trojan {
        #[schemars(length(min = 1, max = 4096))]
        password: String,
    },
}

#[derive(Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct XrayHandlerUser {
    /// Exact Xray email/user key, unique within the target inbound.
    #[schemars(length(min = 1, max = 320))]
    pub email: String,
    #[serde(default)]
    pub level: u32,
    pub account: XrayHandlerAccount,
}

#[derive(Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddUserInput {
    #[schemars(length(min = 1, max = 256))]
    pub inbound_tag: String,
    pub user: XrayHandlerUser,
}

/// Operation receipt returned after HandlerService accepts the native mutation.
/// It identifies what was applied; it does not claim disk persistence or readback.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddUserOutput {
    pub inbound_tag: String,
    pub email: String,
    pub protocol: XrayHandlerProtocol,
    pub level: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveUserInput {
    #[schemars(length(min = 1, max = 256))]
    pub inbound_tag: String,
    #[schemars(length(min = 1, max = 320))]
    pub email: String,
}

/// Exact target of a completed native RemoveUserOperation.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveUserOutput {
    pub inbound_tag: String,
    pub email: String,
}

/// TCP transport only. TLS material is supplied inline for the running handler;
/// certificate/key file paths, REALITY and other transport settings are absent
/// from this contract, so they cannot become an unchecked raw-config bypass.
#[derive(Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "security", rename_all = "snake_case", deny_unknown_fields)]
pub enum XrayInboundTransport {
    /// Explicit plaintext is restricted to a literal loopback IP address.
    LoopbackPlaintext,
    Tls {
        #[schemars(length(min = 1, max = 262144))]
        certificate_pem: String,
        #[schemars(length(min = 1, max = 65536))]
        private_key_pem: String,
        /// ALPN identifiers, each between 1 and 255 UTF-8 bytes.
        #[serde(default)]
        #[schemars(length(max = 16))]
        alpn: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum XrayInboundSecurity {
    LoopbackPlaintext,
    Tls,
}

#[derive(Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddInboundInput {
    #[schemars(length(min = 1, max = 256))]
    pub tag: String,
    /// Literal IPv4/IPv6 listener address; no DNS lookup or filesystem target.
    #[schemars(length(min = 1, max = 45))]
    pub listen_ip: String,
    #[schemars(range(min = 1, max = 65535))]
    pub port: u16,
    pub protocol: XrayHandlerProtocol,
    #[schemars(length(min = 1, max = 1024))]
    pub users: Vec<XrayHandlerUser>,
    pub transport: XrayInboundTransport,
}

/// Completed runtime AddInbound operation. No credentials are returned and no
/// persistent configuration or independent network probe is implied.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddInboundOutput {
    pub tag: String,
    pub listen_ip: String,
    pub port: u16,
    pub protocol: XrayHandlerProtocol,
    pub user_count: u32,
    pub security: XrayInboundSecurity,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct StatsInput {
    pub name: String,
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new("xray", |_ctx| std::sync::Arc::new(XrayPlugin::new(XrayConfig::default())))
}
