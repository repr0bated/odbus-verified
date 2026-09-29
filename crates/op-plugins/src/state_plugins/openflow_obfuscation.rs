//! OpenFlow Traffic Obfuscation Plugin
//!
//! Legacy proposals for three levels of traffic obfuscation:
//! - Level 1: Basic security (drop invalid, rate limiting, connection tracking)
//! - Level 2: Pattern hiding (TTL normalization, packet padding, timing jitter)
//! - Level 3: Advanced obfuscation (protocol mimicry, decoy traffic, morphing)
//!
//! Version 2 methods install explicit TTL-normalization flows through the
//! canonical native controller with compare/readback receipts. Historical
//! levels below remain test-only proposals, not advertised privacy effects.

#![allow(clippy::vec_init_then_push)]

use anyhow::Result;
use async_trait::async_trait;
use op_state::{
    ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateAction, StateDiff, StatePlugin,
};
use op_state_store::{PluginSchema, SideEffect};
use serde::{Deserialize, Serialize};
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};

/// OpenFlow obfuscation configuration
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.network.openflow-obfuscation.schema@v1"))]
#[schemars(extend("x-oscal-category" = "network"))]
pub struct OpenFlowObfuscationConfig {
    /// OVS bridge to apply flows to
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.network.openflow-obfuscation.bridge-name@v1"))]
    pub bridge_name: String,

    /// Obfuscation level (0-3)
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.network.openflow-obfuscation.level@v1"))]
    pub obfuscation_level: u8,

    /// Enable security flows (always recommended)
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.network.openflow-obfuscation.security-flows@v1"))]
    pub enable_security_flows: bool,

    /// Privacy socket ports for the tunnel chain
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.network.openflow-obfuscation.privacy-ports@v1"))]
    pub privacy_ports: Vec<String>,

    /// Additional custom flows
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.network.openflow-obfuscation.custom-flows@v1"))]
    pub custom_flows: Vec<OpenFlowRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.network.openflow-obfuscation.rule.schema@v1"))]
pub struct OpenFlowRule {
    /// Flow table (0-254)
    #[schemars(extend("x-oscal-subid" = "obs.network.openflow-obfuscation.rule.table@v1"))]
    pub table: u8,

    /// Priority (0-65535)
    #[schemars(extend("x-oscal-subid" = "obs.network.openflow-obfuscation.rule.priority@v1"))]
    pub priority: u16,

    /// Match criteria (e.g., "in_port=1,ip,tcp_dst=80")
    #[schemars(extend("x-oscal-subid" = "exp.network.openflow-obfuscation.rule.match@v1"))]
    pub match_spec: String,

    /// Actions (e.g., "output:2,mod_nw_ttl:64")
    #[schemars(extend("x-oscal-subid" = "exp.network.openflow-obfuscation.rule.actions@v1"))]
    pub actions: String,

    /// Description
    #[schemars(extend("x-oscal-subid" = "exp.network.openflow-obfuscation.rule.description@v1"))]
    pub description: String,
}

impl Default for OpenFlowObfuscationConfig {
    fn default() -> Self {
        Self {
            bridge_name: "ovs-br0".to_string(),
            obfuscation_level: 2,
            enable_security_flows: true,
            privacy_ports: vec![
                "gbr_wg".to_string(),
                "gbr_warp".to_string(),
                "gbr_xray".to_string(),
            ],
            custom_flows: vec![],
        }
    }
}

pub struct OpenFlowObfuscationPlugin {
    config: OpenFlowObfuscationConfig,
}

impl OpenFlowObfuscationPlugin {
    pub fn new(config: OpenFlowObfuscationConfig) -> Self {
        Self { config }
    }
}

#[cfg(test)]
impl OpenFlowObfuscationPlugin {
    /// Generate Level 1 flows: Basic security
    fn generate_level1_flows(&self) -> Vec<OpenFlowRule> {
        let mut flows = vec![];

        // Table 0: Security screening (11 flows)

        // 1. Drop invalid TCP flags
        flows.push(OpenFlowRule {
            table: 0,
            priority: 500,
            match_spec: "tcp,tcp_flags=+syn+fin".to_string(),
            actions: "drop".to_string(),
            description: "Drop SYN+FIN packets (invalid)".to_string(),
        });

        // 2. Drop NULL scan packets
        flows.push(OpenFlowRule {
            table: 0,
            priority: 500,
            match_spec: "tcp,tcp_flags=0".to_string(),
            actions: "drop".to_string(),
            description: "Drop NULL scan packets".to_string(),
        });

        // 3. Drop XMAS scan packets
        flows.push(OpenFlowRule {
            table: 0,
            priority: 500,
            match_spec: "tcp,tcp_flags=+fin+urg+psh".to_string(),
            actions: "drop".to_string(),
            description: "Drop XMAS scan packets".to_string(),
        });

        // 4. Drop fragmented packets (potential evasion)
        flows.push(OpenFlowRule {
            table: 0,
            priority: 490,
            match_spec: "ip,ip_frag=first".to_string(),
            actions: "drop".to_string(),
            description: "Drop fragmented packets".to_string(),
        });

        // 5. Rate limit ICMP (DDoS protection)
        flows.push(OpenFlowRule {
            table: 0,
            priority: 480,
            match_spec: "icmp".to_string(),
            actions: "meter:1,resubmit(,10)".to_string(),
            description: "Rate limit ICMP to 100pps".to_string(),
        });

        // 6. Rate limit DNS (DDoS protection)
        flows.push(OpenFlowRule {
            table: 0,
            priority: 480,
            match_spec: "udp,tp_dst=53".to_string(),
            actions: "meter:2,resubmit(,10)".to_string(),
            description: "Rate limit DNS queries to 1000pps".to_string(),
        });

        // 7. Connection tracking for stateful filtering
        flows.push(OpenFlowRule {
            table: 0,
            priority: 470,
            match_spec: "ip".to_string(),
            actions: "ct(table=10)".to_string(),
            description: "Connection tracking for stateful filtering".to_string(),
        });

        // 8. Drop invalid connection states
        flows.push(OpenFlowRule {
            table: 10,
            priority: 500,
            match_spec: "ct_state=-trk".to_string(),
            actions: "drop".to_string(),
            description: "Drop untracked connections".to_string(),
        });

        flows.push(OpenFlowRule {
            table: 10,
            priority: 500,
            match_spec: "ct_state=+inv".to_string(),
            actions: "drop".to_string(),
            description: "Drop invalid connection states".to_string(),
        });

        // 9. Allow established connections
        flows.push(OpenFlowRule {
            table: 10,
            priority: 400,
            match_spec: "ct_state=+est".to_string(),
            actions: "resubmit(,20)".to_string(),
            description: "Allow established connections".to_string(),
        });

        // 10. Allow new connections
        flows.push(OpenFlowRule {
            table: 10,
            priority: 390,
            match_spec: "ct_state=+new".to_string(),
            actions: "resubmit(,20)".to_string(),
            description: "Allow new connections".to_string(),
        });

        // 11. Default drop for table 0
        flows.push(OpenFlowRule {
            table: 0,
            priority: 1,
            match_spec: "".to_string(),
            actions: "drop".to_string(),
            description: "Default drop for security".to_string(),
        });

        flows
    }

    /// Generate Level 2 flows: Pattern hiding
    fn generate_level2_flows(&self) -> Vec<OpenFlowRule> {
        let mut flows = vec![];

        // Table 20: Pattern obfuscation (3 flows)

        // 1. TTL normalization - set all outbound packets to TTL 64
        flows.push(OpenFlowRule {
            table: 20,
            priority: 300,
            match_spec: "ip".to_string(),
            actions: "mod_nw_ttl:64,resubmit(,30)".to_string(),
            description: "TTL normalization (set to 64)".to_string(),
        });

        // 2. Packet size padding - pad small packets to reduce size-based fingerprinting
        // Note: OVS doesn't directly support padding, but we can use meters with burst
        // to introduce timing variations that achieve similar anti-fingerprinting
        flows.push(OpenFlowRule {
            table: 20,
            priority: 290,
            match_spec: "tcp".to_string(),
            actions: "meter:3,resubmit(,30)".to_string(),
            description: "Timing jitter for TCP (anti-fingerprinting)".to_string(),
        });

        // 3. Window size normalization for TCP
        flows.push(OpenFlowRule {
            table: 20,
            priority: 280,
            match_spec: "tcp".to_string(),
            actions: "mod_tp_src:0x1234,resubmit(,30)".to_string(),
            description: "TCP source port randomization".to_string(),
        });

        flows
    }

    /// Generate Level 3 flows: Advanced obfuscation
    fn generate_level3_flows(&self) -> Vec<OpenFlowRule> {
        let mut flows = vec![];

        // Table 30: Advanced obfuscation (4 flows)

        // 1. Protocol mimicry - make VPN traffic look like HTTPS
        flows.push(OpenFlowRule {
            table: 30,
            priority: 200,
            match_spec: "udp,tp_dst=51820".to_string(),
            actions: "mod_tp_dst:443,resubmit(,40)".to_string(),
            description: "WireGuard port mimicry (51820→443)".to_string(),
        });

        // 2. Decoy traffic generation trigger
        // This flow matches low-traffic periods and triggers decoy generation
        flows.push(OpenFlowRule {
            table: 30,
            priority: 190,
            match_spec: "ip".to_string(),
            actions: "meter:4,resubmit(,40)".to_string(),
            description: "Decoy traffic trigger (low bandwidth detection)".to_string(),
        });

        // 3. Traffic morphing - randomize packet ordering
        flows.push(OpenFlowRule {
            table: 30,
            priority: 180,
            match_spec: "tcp".to_string(),
            actions: "meter:5,resubmit(,40)".to_string(),
            description: "Packet timing randomization (morphing)".to_string(),
        });

        // 4. Deep packet inspection evasion - fragment large packets
        flows.push(OpenFlowRule {
            table: 30,
            priority: 170,
            match_spec: "tcp,dl_vlan=100".to_string(),
            actions: "strip_vlan,resubmit(,40)".to_string(),
            description: "DPI evasion (VLAN stripping)".to_string(),
        });

        flows
    }

    /// Generate forwarding flows for privacy tunnel
    fn generate_forwarding_flows(&self) -> Vec<OpenFlowRule> {
        let mut flows = vec![];

        // Table 40: Final forwarding

        // Forward through privacy chain: gbr_wg → gbr_warp → gbr_xray
        for (idx, port) in self.config.privacy_ports.iter().enumerate() {
            if idx < self.config.privacy_ports.len() - 1 {
                let next_port = &self.config.privacy_ports[idx + 1];
                flows.push(OpenFlowRule {
                    table: 40,
                    priority: 100,
                    match_spec: format!("in_port={}", port),
                    actions: format!("output:{}", next_port),
                    description: format!("Forward {} → {}", port, next_port),
                });
            }
        }

        // Return path: gbr_xray → gbr_warp → gbr_wg
        let ports: Vec<_> = self.config.privacy_ports.iter().collect();
        for (idx, port) in ports.into_iter().enumerate().rev() {
            if idx > 0 {
                let prev_port = &self.config.privacy_ports[idx - 1];
                flows.push(OpenFlowRule {
                    table: 40,
                    priority: 100,
                    match_spec: format!("in_port={}", port),
                    actions: format!("output:{}", prev_port),
                    description: format!("Return {} → {}", port, prev_port),
                });
            }
        }

        // Normal forwarding for non-privacy ports
        flows.push(OpenFlowRule {
            table: 40,
            priority: 1,
            match_spec: "".to_string(),
            actions: "NORMAL".to_string(),
            description: "Normal L2/L3 forwarding".to_string(),
        });

        flows
    }

    /// Generate all flows based on obfuscation level
    fn generate_all_flows(&self) -> Vec<OpenFlowRule> {
        let mut all_flows = vec![];

        // Always include forwarding flows
        all_flows.extend(self.generate_forwarding_flows());

        // Add security flows if enabled (Level 1+)
        if self.config.enable_security_flows && self.config.obfuscation_level >= 1 {
            all_flows.extend(self.generate_level1_flows());
        }

        // Add pattern hiding flows (Level 2+)
        if self.config.obfuscation_level >= 2 {
            all_flows.extend(self.generate_level2_flows());
        }

        // Add advanced obfuscation flows (Level 3)
        if self.config.obfuscation_level >= 3 {
            all_flows.extend(self.generate_level3_flows());
        }

        // Add custom flows
        all_flows.extend(self.config.custom_flows.clone());

        all_flows
    }

    /// Convert OpenFlowRule to ovs-ofctl command
    fn flow_to_command(&self, flow: &OpenFlowRule) -> String {
        let mut cmd = format!("table={},priority={}", flow.table, flow.priority);

        if !flow.match_spec.is_empty() {
            cmd.push_str(&format!(",{}", flow.match_spec));
        }

        cmd.push_str(&format!(" actions={}", flow.actions));

        cmd
    }
}

#[async_trait]
impl StatePlugin for OpenFlowObfuscationPlugin {
    fn name(&self) -> &'static str {
        "openflow_obfuscation"
    }

    fn version(&self) -> &str {
        "2.0.0"
    }

    fn schema(&self) -> Option<PluginSchema> {
        let root = serde_json::to_value(schemars::schema_for!(ObfuscationState))
            .expect("schemars schema serializes to JSON");
        let mut schema = super::schemars_adapter::plugin_schema_from_json(
            "openflow_obfuscation",
            "2.0.0",
            "Explicit TTL-normalization flows with native switch readback and exact removal",
            &root,
        );

        // Add D-Bus methods for OpenFlow obfuscation - https://www.opennetworking.org/wp-content/uploads/2014/10/of_spec_1_0.pdf
        schema.methods.insert(
            "obfuscate_flow".to_string(),
            super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
                ObfuscateFlowInput,
                ObfuscationOutput,
            >(
                "ObfuscateFlow",
                SideEffect::Mutation,
                false,
                "openflow_obfuscation.write",
                "mut.network.openflow.obfuscation.obfuscate@v1",
            ),
        );
        schema.methods.insert(
            "deobfuscate_flow".to_string(),
            super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
                DeobfuscateFlowInput,
                ObfuscationOutput,
            >(
                "DeobfuscateFlow",
                SideEffect::Mutation,
                false,
                "openflow_obfuscation.write",
                "mut.network.openflow.obfuscation.deobfuscate@v1",
            ),
        );

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

    async fn calculate_diff(&self, current: &Value, desired: &Value) -> Result<StateDiff> {
        let mut actions = Vec::new();

        let current_config = current.get("config");
        let desired_config = desired.get("config");

        if current_config != desired_config {
            actions.push(StateAction::Modify {
                resource: format!("openflow_flows_{}", self.config.bridge_name),
                changes: desired.clone(),
            });
        }

        Ok(StateDiff {
            plugin: self.name().to_string(),
            actions,
            metadata: DiffMetadata {
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_secs() as i64,
                current_hash: format!("{:x}", md5::compute(simd_json::to_string(current)?)),
                desired_hash: format!("{:x}", md5::compute(simd_json::to_string(desired)?)),
            },
        })
    }

    async fn apply_state(&self, _diff: &StateDiff) -> Result<ApplyResult> {
        // This plugin has no controller binding.  The generated strings are
        // planning output only: applying them would claim an OVS mutation
        // without sending anything to the real openflow owner.  Keep the
        // state reconciliation contract honest until the typed controller can
        // represent and install this plugin's rules.
        anyhow::bail!(
            "openflow_obfuscation cannot apply state: no canonical OpenFlow backend supports its generated rules"
        )
    }

    async fn verify_state(&self, _desired: &Value) -> Result<bool> {
        anyhow::bail!(
            "openflow_obfuscation cannot verify state: no canonical backend reports installed obfuscation rules"
        )
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        anyhow::bail!(
            "openflow_obfuscation cannot checkpoint state: no canonical backend checkpoint exists"
        )
    }

    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        anyhow::bail!(
            "openflow_obfuscation cannot roll back state: no canonical backend checkpoint exists"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_level0_no_obfuscation() {
        let config = OpenFlowObfuscationConfig {
            obfuscation_level: 0,
            enable_security_flows: false,
            ..Default::default()
        };
        let plugin = OpenFlowObfuscationPlugin::new(config);
        let flows = plugin.generate_all_flows();

        // Should only have forwarding flows
        assert_eq!(flows.len(), 5); // 2*2 + 1 for default NORMAL
    }

    #[test]
    fn test_level1_security() {
        let config = OpenFlowObfuscationConfig {
            obfuscation_level: 1,
            enable_security_flows: true,
            ..Default::default()
        };
        let plugin = OpenFlowObfuscationPlugin::new(config);
        let flows = plugin.generate_all_flows();

        // Should have forwarding + security flows
        assert!(flows.len() >= 11 + 5);
    }

    #[test]
    fn test_level2_pattern_hiding() {
        let config = OpenFlowObfuscationConfig {
            obfuscation_level: 2,
            enable_security_flows: true,
            ..Default::default()
        };
        let plugin = OpenFlowObfuscationPlugin::new(config);
        let flows = plugin.generate_all_flows();

        // Should have forwarding + security + pattern hiding
        assert!(flows.len() >= 11 + 3 + 5);
    }

    #[test]
    fn test_level3_advanced() {
        let config = OpenFlowObfuscationConfig {
            obfuscation_level: 3,
            enable_security_flows: true,
            ..Default::default()
        };
        let plugin = OpenFlowObfuscationPlugin::new(config);
        let flows = plugin.generate_all_flows();

        // Should have all flow types
        assert!(flows.len() >= 11 + 3 + 4 + 5);
    }

    #[test]
    fn test_flow_command_generation() {
        let plugin = OpenFlowObfuscationPlugin::new(Default::default());
        let flow = OpenFlowRule {
            table: 0,
            priority: 500,
            match_spec: "tcp,tcp_dst=80".to_string(),
            actions: "output:1".to_string(),
            description: "Test flow".to_string(),
        };

        let cmd = plugin.flow_to_command(&flow);
        assert!(cmd.contains("table=0"));
        assert!(cmd.contains("priority=500"));
        assert!(cmd.contains("tcp,tcp_dst=80"));
        assert!(cmd.contains("actions=output:1"));
    }

    #[tokio::test]
    async fn lifecycle_rejects_without_a_canonical_backend() {
        let plugin = OpenFlowObfuscationPlugin::new(Default::default());
        let diff = StateDiff {
            plugin: "openflow_obfuscation".to_string(),
            actions: vec![],
            metadata: DiffMetadata {
                timestamp: 0,
                current_hash: String::new(),
                desired_hash: String::new(),
            },
        };

        for error in [
            plugin.apply_state(&diff).await.unwrap_err(),
            plugin
                .verify_state(&simd_json::json!(null))
                .await
                .unwrap_err(),
            plugin.create_checkpoint().await.unwrap_err(),
        ] {
            assert!(error.to_string().contains("no canonical"));
        }
        let checkpoint = Checkpoint {
            id: "test".to_string(),
            plugin: "openflow_obfuscation".to_string(),
            timestamp: 0,
            state_snapshot: simd_json::json!(null),
            backend_checkpoint: None,
        };
        assert!(plugin
            .rollback(&checkpoint)
            .await
            .unwrap_err()
            .to_string()
            .contains("no canonical backend"));
    }
}

/// Input struct for ObfuscateFlow method
/// D-Bus method spec: https://www.opennetworking.org/wp-content/uploads/2014/10/of_spec_1_0.pdf
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ObfuscateFlowInput {
    /// Bridge name
    pub bridge: String,
    /// Explicit match and forwarding actions. The entry must be absent; the
    /// owner assigns its cookie and prepends TTL normalization. No padding,
    /// timing jitter or protocol mimicry is claimed.
    pub flow: super::openflow::FlowEntry,
    #[schemars(range(min = 1, max = 255))]
    pub ttl: u8,
    pub expected_revision: u64,
}

/// Input struct for DeobfuscateFlow method
/// D-Bus method spec: https://www.opennetworking.org/wp-content/uploads/2014/10/of_spec_1_0.pdf
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DeobfuscateFlowInput {
    /// Bridge name
    pub bridge: String,
    /// Receipt ID from obfuscate_flow. Only that exact installed entry may be removed.
    pub flow_id: String,
    pub expected_revision: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ObfuscationState {
    pub revision: u64,
    #[schemars(extend("x-opdbus-exact-json-schema" = true))]
    pub flows: std::collections::BTreeMap<String, ObfuscationRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ObfuscationRecord {
    pub bridge: String,
    pub flow: super::openflow::FlowEntry,
    pub ttl: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ObfuscationOutput {
    pub flow_id: String,
    pub revision: u64,
    pub installed: bool,
    pub record: ObfuscationRecord,
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new("openflow_obfuscation", |_ctx| std::sync::Arc::new(OpenFlowObfuscationPlugin::new(Default::default())))
}
