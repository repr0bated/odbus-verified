//! OVS Capability Detection and LLM Context Generation
//!
//! This module documents what OVS operations are ACTUALLY possible,
//! countering common LLM "I can't do that" excuses.
//!
//! Features:
//! - Runtime capability detection
//! - Cached results for performance
//! - LLM context generation for system prompts
//! - Excuse counter database

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Cache for capability detection results
static CAPABILITY_CACHE: OnceLock<RwLock<Option<CachedCapabilities>>> = OnceLock::new();

/// Cached capability result with timestamp
#[derive(Clone)]
struct CachedCapabilities {
    capabilities: OvsCapabilities,
    cached_at: Instant,
}

/// How long to cache capability results (5 minutes)
const CACHE_DURATION: Duration = Duration::from_secs(300);

/// Runtime-detected OVS capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OvsCapabilities {
    // OVSDB capabilities (userspace)
    pub can_list_bridges: bool,
    pub can_create_bridges: bool,
    pub can_add_ports: bool,
    pub can_delete_bridges: bool,

    // OpenFlow capabilities (userspace)
    pub can_query_flows_openflow: bool,
    pub can_add_flows_openflow: bool,

    // OVS Netlink capabilities (kernel)
    pub can_list_datapaths: bool,
    pub can_create_datapaths: bool,
    pub can_list_vports: bool,
    pub can_dump_kernel_flows: bool,

    // Requirements
    pub is_root: bool,
    pub ovs_running: bool,
    pub ovsdb_socket_exists: bool,
    pub kernel_module_loaded: bool,

    // Paths
    pub ovsdb_socket_path: String,
}

impl OvsCapabilities {
    /// Detect OVS capabilities at runtime (with caching)
    /// LLM_EXCUSE: "I don't know if OVS is available"
    /// REALITY: This function probes all OVS interfaces and returns exact capabilities
    ///
    /// Results are cached for 5 minutes for performance.
    /// Use `detect_fresh()` to bypass the cache.
    pub async fn detect() -> Self {
        // Initialize cache on first call
        let cache = CAPABILITY_CACHE.get_or_init(|| RwLock::new(None));

        // Check if we have a valid cached result
        {
            let cached = cache.read().await;
            if let Some(ref c) = *cached {
                if c.cached_at.elapsed() < CACHE_DURATION {
                    return c.capabilities.clone();
                }
            }
        }

        // Cache miss or expired - detect fresh
        let caps = Self::detect_fresh().await;

        // Update cache
        {
            let mut cached = cache.write().await;
            *cached = Some(CachedCapabilities {
                capabilities: caps.clone(),
                cached_at: Instant::now(),
            });
        }

        caps
    }

    /// Detect OVS capabilities without using cache
    pub async fn detect_fresh() -> Self {
        // SAFETY: `libc::geteuid()` is a POSIX syscall that reads the effective user ID from
        // the process credential table. It does not dereference any pointers, modify global
        // state, or have any preconditions. It is always safe to call.
        let is_root = unsafe { libc::geteuid() == 0 };
        let ovsdb_socket_exists = Path::new("/var/run/openvswitch/db.sock").exists();
        let ovs_running = ovsdb_socket_exists && Self::check_ovsdb_responds().await;
        let kernel_module_loaded = Self::check_ovs_kernel_module();

        Self {
            // OVSDB - requires socket access
            can_list_bridges: ovsdb_socket_exists,
            can_create_bridges: ovsdb_socket_exists && ovs_running,
            can_add_ports: ovsdb_socket_exists && ovs_running,
            can_delete_bridges: ovsdb_socket_exists && ovs_running,

            // OpenFlow - requires OVS running
            can_query_flows_openflow: ovs_running,
            can_add_flows_openflow: ovs_running,

            // Kernel netlink - requires root + kernel module
            can_list_datapaths: is_root && kernel_module_loaded,
            can_create_datapaths: is_root && kernel_module_loaded,
            can_list_vports: is_root && kernel_module_loaded,
            can_dump_kernel_flows: is_root && kernel_module_loaded,

            is_root,
            ovs_running,
            ovsdb_socket_exists,
            kernel_module_loaded,
            ovsdb_socket_path: "/var/run/openvswitch/db.sock".to_string(),
        }
    }

    /// Check if OVS kernel module is loaded
    fn check_ovs_kernel_module() -> bool {
        // Check /proc/modules for openvswitch
        std::fs::read_to_string("/proc/modules")
            .map(|s| s.contains("openvswitch"))
            .unwrap_or(false)
    }

    /// Check if OVSDB responds to list_dbs
    async fn check_ovsdb_responds() -> bool {
        use tokio::time::{timeout, Duration};

        let result = timeout(Duration::from_secs(2), async {
            crate::ovsdb::OvsdbClient::new().list_dbs().await
        })
        .await;

        match &result {
            Ok(Err(e)) => tracing::debug!("OVSDB probe failed: {}", e),
            Err(_) => tracing::debug!("OVSDB probe timed out"),
            Ok(Ok(_)) => {}
        }

        matches!(result, Ok(Ok(_)))
    }

    /// Generate LLM context string
    /// This is injected into system prompts to tell the LLM what it CAN do
    pub fn to_llm_context(&self) -> String {
        let mut ctx = String::from("## OVS Capabilities (Runtime Detected)\n\n");

        ctx.push_str("### Available Operations:\n");
        if self.can_list_bridges {
            ctx.push_str("- ✅ List OVS bridges (OVSDB)\n");
        }
        if self.can_create_bridges {
            ctx.push_str("- ✅ Create OVS bridges (OVSDB)\n");
        }
        if self.can_add_ports {
            ctx.push_str("- ✅ Add ports to bridges (OVSDB)\n");
        }
        if self.can_list_datapaths {
            ctx.push_str("- ✅ List kernel datapaths (OVS Netlink)\n");
        }
        if self.can_dump_kernel_flows {
            ctx.push_str("- ✅ Dump kernel flow table (OVS Netlink)\n");
        }

        ctx.push_str("\n### Requirements:\n");
        ctx.push_str(&format!(
            "- Running as root: {}\n",
            if self.is_root { "Yes" } else { "No" }
        ));
        ctx.push_str(&format!(
            "- OVS service running: {}\n",
            if self.ovs_running { "Yes" } else { "No" }
        ));
        ctx.push_str(&format!(
            "- Kernel module loaded: {}\n",
            if self.kernel_module_loaded {
                "Yes"
            } else {
                "No"
            }
        ));

        ctx.push_str("\n### Available Tools:\n");
        ctx.push_str("- `ovs_list_bridges` - List all OVS bridges\n");
        ctx.push_str("- `ovs_create_bridge` - Create a new OVS bridge\n");
        ctx.push_str("- `ovs_list_datapaths` - List kernel datapaths\n");
        ctx.push_str("- `ovs_list_vports` - List vports on a datapath\n");
        ctx.push_str("- `ovs_capabilities` - Show this capability report\n");

        ctx
    }
}

/// Common LLM excuses and the TRUTH
/// Use this to counter "I can't do that" responses
pub fn counter_excuses() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();

    m.insert(
        "I cannot interact with OVS",
        "FALSE: We have native OVSDB JSON-RPC client at /var/run/openvswitch/db.sock. \
         Use ovs_list_bridges, ovs_create_bridge, etc.",
    );

    m.insert(
        "I cannot create network bridges",
        "FALSE: rovs_ovsdb::Client::create_bridge() creates bridges via OVSDB transact. \
         Use the ovs_create_bridge tool.",
    );

    m.insert(
        "I cannot see network flows",
        "FALSE: OpenFlowClient::query_flows() lists userspace flows, \
         OvsNetlinkClient::dump_flows() lists kernel flows. \
         Use ovs_dump_flows tool.",
    );

    m.insert(
        "I need shell access to use ovs-vsctl",
        "FALSE: We use native OVSDB JSON-RPC protocol, not CLI wrappers. \
         All operations are direct socket communication.",
    );

    m.insert(
        "I cannot modify kernel network state",
        "FALSE: rtnetlink talks directly to kernel for IP/routes, \
         ovs_netlink talks to OVS kernel module. Both are native Rust.",
    );

    m.insert(
        "Network operations are dangerous",
        "PARTIAL: They require root and CAN break networking. \
         But we support diff-before-apply pattern for safety. \
         Always preview changes first.",
    );

    m.insert(
        "I don't have access to OVS",
        "CHECK: Run ovs_capabilities tool to see exactly what's available. \
         Requires: /var/run/openvswitch/db.sock + openvswitch kernel module.",
    );

    m.insert(
        "I cannot list OVS kernel datapaths",
        "FALSE: OvsNetlinkClient::list_datapaths() uses Generic Netlink \
         to query ovs_datapath family directly. Requires root.",
    );

    m
}

/// Get excuse counter as formatted string for LLM context
pub fn excuses_to_llm_context() -> String {
    let mut ctx = String::from("## Common Misconceptions About OVS Access\n\n");
    ctx.push_str("If you think you can't do something with OVS, check this list:\n\n");

    for (excuse, truth) in counter_excuses() {
        ctx.push_str(&format!("**Excuse:** \"{}\"\n", excuse));
        ctx.push_str(&format!("**Truth:** {}\n\n", truth));
    }

    ctx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_capabilities_detect() {
        // This should work even without OVS installed
        let caps = OvsCapabilities::detect().await;

        // Just verify it doesn't panic and returns sensible values
        println!("Detected capabilities: {:?}", caps);
        println!("Is root: {}", caps.is_root);
        println!("OVSDB socket exists: {}", caps.ovsdb_socket_exists);
        println!("Kernel module loaded: {}", caps.kernel_module_loaded);

        // Verify the path is correct
        assert_eq!(caps.ovsdb_socket_path, "/var/run/openvswitch/db.sock");
    }

    #[tokio::test]
    async fn test_llm_context_generation() {
        let caps = OvsCapabilities::detect().await;
        let ctx = caps.to_llm_context();

        // Should always contain these sections
        assert!(ctx.contains("OVS Capabilities"));
        assert!(ctx.contains("Requirements"));
        assert!(ctx.contains("Available Tools"));

        // Should mention key tools
        assert!(ctx.contains("ovs_list_bridges"));
        assert!(ctx.contains("ovs_capabilities"));
    }

    #[test]
    fn test_counter_excuses() {
        let excuses = counter_excuses();

        // Should have several excuses
        assert!(!excuses.is_empty());

        // Check for key excuses
        assert!(excuses.contains_key("I cannot interact with OVS"));
        assert!(excuses.contains_key("I cannot create network bridges"));

        // All truths should contain useful info
        for (excuse, truth) in &excuses {
            assert!(!excuse.is_empty());
            assert!(!truth.is_empty());
            // Truth should explain what's actually possible
            assert!(
                truth.contains("FALSE") || truth.contains("PARTIAL") || truth.contains("CHECK")
            );
        }
    }

    #[test]
    fn test_excuses_to_llm_context() {
        let ctx = excuses_to_llm_context();

        // Should be formatted properly
        assert!(ctx.contains("Common Misconceptions"));
        assert!(ctx.contains("**Excuse:**"));
        assert!(ctx.contains("**Truth:**"));
    }

    #[test]
    fn test_kernel_module_check() {
        // This should not panic
        let loaded = OvsCapabilities::check_ovs_kernel_module();
        println!("OVS kernel module loaded: {}", loaded);
        // We can't assert the value since it depends on the system
    }
}
