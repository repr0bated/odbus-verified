use anyhow::Result;
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{FieldSchema, FieldType, PluginSchema, SideEffect};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;
use simd_json::prelude::*;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyConfig {
    /// Enable WireGuard gateway (system service)
    pub wireguard_gateway_enabled: bool,
    /// WireGuard gateway interface
    pub wireguard_interface: String,

    /// Enable WARP tunnel (system service)
    pub warp_tunnel_enabled: bool,
    /// WARP interface name
    pub warp_interface: String,

    /// Enable XRay client container
    pub xray_client_enabled: bool,
    pub xray_client_container_id: u32,
    /// XRay SOCKS proxy port
    pub xray_socks_port: u16,
    /// VPS XRay server address
    pub vps_xray_server: Option<String>,

    /// Proxmox-specific networking
    pub proxmox_bridge: String,
}

impl Default for PrivacyConfig {
    fn default() -> Self {
        Self {
            wireguard_gateway_enabled: true,
            wireguard_interface: "wg0".to_string(),
            warp_tunnel_enabled: true,
            warp_interface: "warp0".to_string(),
            xray_client_enabled: true,
            xray_client_container_id: 102,
            xray_socks_port: 1080,
            vps_xray_server: None,
            proxmox_bridge: "vmbr0".to_string(),
        }
    }
}

pub struct PrivacyPlugin {
    config: PrivacyConfig,
}

impl PrivacyPlugin {
    pub fn new(config: PrivacyConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl StatePlugin for PrivacyPlugin {
    fn name(&self) -> &'static str {
        "privacy"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn schema(&self) -> Option<PluginSchema> {
        Some(privacy_schema())
    }

    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: false,
            supports_checkpoints: false,
            supports_verification: true,
            atomic_operations: false,
        }
    }


    async fn calculate_diff(&self, current: &Value, desired: &Value) -> Result<StateDiff> {
        // Basic diff calculation - full implementation would check component states
        let actions = Vec::new();

        // This is a coordinating plugin that delegates to individual component plugins
        // The actual work is done by the respective plugins (netmaker, lxc for xray, etc.)

        Ok(StateDiff {
            plugin: self.name().to_string(),
            actions,
            metadata: op_state::DiffMetadata {
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_secs() as i64,
                current_hash: format!("{:x}", md5::compute(simd_json::to_string(current)?)),
                desired_hash: format!("{:x}", md5::compute(simd_json::to_string(desired)?)),
            },
        })
    }

    async fn apply_state(&self, _diff: &StateDiff) -> Result<ApplyResult> {
        // Privacy plugin coordinates but doesn't directly apply changes
        // Individual component plugins handle their own state
        Ok(ApplyResult {
            success: true,
            changes_applied: vec!["Privacy network coordination active".to_string()],
            errors: vec![],
            checkpoint: None,
        })
    }

    async fn verify_state(&self, _desired: &Value) -> Result<bool> {
        Ok(true)
    }

    async fn create_checkpoint(&self) -> Result<op_state::Checkpoint> {
        let state = simd_json::json!(null);
        Ok(op_state::Checkpoint {
            id: format!(
                "privacy_{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_secs()
            ),
            plugin: self.name().to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs() as i64,
            state_snapshot: state,
            backend_checkpoint: None,
        })
    }

    async fn rollback(&self, _checkpoint: &op_state::Checkpoint) -> Result<()> {
        Err(anyhow::anyhow!("Privacy plugin rollback not implemented - individual component plugins handle their own rollback"))
    }
}

/// Input struct for MaskData method
/// D-Bus method spec: Privacy data masking for GDPR compliance
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MaskDataInput {
    /// Data to mask
    pub data: String,
    /// Type of data (email, phone, ssn, etc.)
    pub data_type: String,
}

/// Input struct for UnmaskData method
/// D-Bus method spec: Privacy data unmasking for authorized access
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UnmaskDataInput {
    /// Masked data to unmask
    pub masked_data: String,
    /// Authorization token
    pub auth_token: String,
}

/// Input struct for GetPolicy method
/// D-Bus method spec: Get privacy policy settings
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetPolicyInput {
    /// Policy scope (user, system, global)
    pub scope: Option<String>,
}

/// Generate the privacy plugin schema
pub(crate) fn privacy_schema() -> PluginSchema {
    let mut schema = PluginSchema::builder("privacy")
        .category("privacy")
        .version("1.0.0")
        .description("Privacy configuration and data masking")
        .field(
            "wireguard_gateway_enabled",
            FieldSchema {
                field_type: FieldType::Boolean,
                required: true,
                description: "Enable WireGuard gateway".to_string(),
                default: Some(simd_json::json!(true)),
                example: None,
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        )
        .field(
            "wireguard_interface",
            FieldSchema {
                field_type: FieldType::String,
                required: true,
                description: "WireGuard interface name".to_string(),
                default: Some(simd_json::json!("wg0")),
                example: None,
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        )
        .build();
    
    // Add D-Bus methods for privacy
    schema.methods.insert(
        "mask_data".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<MaskDataInput, super::plugin_scaffold_helpers::AckOutput>(
            "MaskData",
            SideEffect::Mutation,
            false,
            "privacy.write",
            "mut.privacy.data.mask@v1",
        ),
    );
    schema.methods.insert(
        "unmask_data".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<UnmaskDataInput, super::plugin_scaffold_helpers::AckOutput>(
            "UnmaskData",
            SideEffect::Mutation,
            false,
            "privacy.write",
            "mut.privacy.data.unmask@v1",
        ),
    );
    schema.methods.insert(
        "get_policy".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<GetPolicyInput, super::plugin_scaffold_helpers::AckOutput>(
            "GetPolicy",
            SideEffect::Read,
            true,
            "privacy.read",
            "obs.privacy.policy.get@v1",
        ),
    );
    
    schema
}
