//! Fail2ban control-plane schema.
//!
//! Runtime calls use the daemon's configured Unix control socket through the
//! native adapter in `fail2ban_methods`; this module never invents daemon state.

use anyhow::{bail, Result};
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::PluginSchema;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;

#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.security.plugin.fail2ban.schema@v2"))]
#[schemars(extend("x-oscal-category" = "security"))]
pub struct Fail2banState {
    pub jails: Vec<JailStatusOutput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JailInput {
    pub jail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BanIpInput {
    pub jail: String,
    pub ip: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MonitorSourceKind {
    Files,
    Journal,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct JailStatusOutput {
    pub jail: String,
    pub currently_failed: u64,
    pub total_failed: u64,
    pub currently_banned: u64,
    pub total_banned: u64,
    pub banned_ips: Vec<String>,
    pub monitor_source_kind: MonitorSourceKind,
    pub monitored_sources: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BanIpOutput {
    pub jail: String,
    pub ip: String,
    /// Number of bans the daemon reports it applied.
    pub affected: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UnbanIpOutput {
    pub jail: String,
    pub ip: String,
    /// Number of bans the daemon reports it removed.
    pub affected: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReloadOutput {
    pub scope: String,
    pub reloaded: bool,
}

pub struct Fail2banPlugin;

impl Fail2banPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Fail2banPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StatePlugin for Fail2banPlugin {
    fn name(&self) -> &str {
        "fail2ban"
    }

    fn version(&self) -> &str {
        "2.0.0"
    }

    fn schema(&self) -> Option<PluginSchema> {
        Some(fail2ban_schema())
    }

    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        Ok(StateDiff {
            plugin: self.name().to_string(),
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
        bail!("Fail2ban StatePlugin verification is unsupported")
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        bail!("Fail2ban StatePlugin checkpoints are unsupported")
    }

    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        bail!("Fail2ban StatePlugin rollback is unsupported")
    }

    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: false,
            supports_checkpoints: false,
            supports_verification: false,
            atomic_operations: false,
        }
    }
}

pub(crate) fn fail2ban_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(Fail2banState))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "fail2ban",
        "2.0.0",
        "Fail2ban jail status and daemon-native ban control",
        &root,
    );
    schema.category = "security".to_string();

    use super::plugin_scaffold_helpers::{method_decl_from_schemars_with_output, EmptyInput};
    use op_state_store::SideEffect;

    schema.methods.insert(
        "ban_ip".to_string(),
        method_decl_from_schemars_with_output::<BanIpInput, BanIpOutput>(
            "ban_ip",
            SideEffect::Mutation,
            false,
            "fail2ban.write",
            "mut.security.fail2ban.ip.ban@v2",
        ),
    );
    schema.methods.insert(
        "unban_ip".to_string(),
        method_decl_from_schemars_with_output::<BanIpInput, UnbanIpOutput>(
            "unban_ip",
            SideEffect::Mutation,
            false,
            "fail2ban.write",
            "mut.security.fail2ban.ip.unban@v2",
        ),
    );
    schema.methods.insert(
        "get_jail_status".to_string(),
        method_decl_from_schemars_with_output::<JailInput, JailStatusOutput>(
            "get_jail_status",
            SideEffect::Read,
            true,
            "fail2ban.read",
            "obs.security.fail2ban.jail.status@v2",
        ),
    );
    schema.methods.insert(
        "reload".to_string(),
        method_decl_from_schemars_with_output::<EmptyInput, ReloadOutput>(
            "reload",
            SideEffect::Mutation,
            false,
            "fail2ban.write",
            "mut.security.fail2ban.service.reload@v2",
        ),
    );

    schema.capabilities.insert(
        "fail2ban.read".to_string(),
        op_state_store::CapabilityDecl {
            id: "fail2ban.read".to_string(),
            description: "Grants: get_jail_status.".to_string(),
        },
    );
    schema.capabilities.insert(
        "fail2ban.write".to_string(),
        op_state_store::CapabilityDecl {
            id: "fail2ban.write".to_string(),
            description: "Grants daemon-native ban_ip and unban_ip. Reload remains unavailable until a typed configuration-stream producer is installed.".to_string(),
        },
    );
    schema
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn methods_are_typed_and_names_match_keys() {
        let schema = fail2ban_schema();
        assert_eq!(schema.version, "2.0.0");
        for name in ["ban_ip", "unban_ip", "get_jail_status", "reload"] {
            let method = &schema.methods[name];
            assert_eq!(method.name, name);
            assert!(method.returns.is_some());
        }
        assert!(schema.example.is_none());
    }
}

inventory::submit! {
    crate::default_registry::PluginReg::new("fail2ban", |_ctx| std::sync::Arc::new(Fail2banPlugin::new()))
}
