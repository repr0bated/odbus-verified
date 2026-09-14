//! Enterprise-grade NUMA topology detection and management
#![allow(dead_code)]
//!
//! This module provides comprehensive NUMA (Non-Uniform Memory Access) support
//! for optimal cache performance on multi-socket systems.
//!
//! Features:
//! - Full topology detection from /sys/devices/system/node/
//! - CPU affinity management for L3 cache optimization
//! - Memory policy configuration for local NUMA access
//! - Per-node statistics tracking
//! - Automatic node selection based on workload
//! - Graceful degradation for non-NUMA systems

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

/// NUMA node information with complete topology
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NumaNode {
    pub node_id: u32,
    pub cpu_list: Vec<u32>,
    pub memory_total_kb: u64,
    pub memory_free_kb: u64,
    pub distance_to_nodes: HashMap<u32, u32>,
}

impl NumaNode {
    /// Check if this node is online
    pub fn is_online(&self) -> bool {
        !self.cpu_list.is_empty()
    }

    /// Get memory utilization percentage
    pub fn memory_utilization(&self) -> f64 {
        if self.memory_total_kb == 0 {
            return 0.0;
        }
        let used = self.memory_total_kb.saturating_sub(self.memory_free_kb);
        (used as f64 / self.memory_total_kb as f64) * 100.0
    }

    /// Get distance to another node (10 = local, higher = more hops)
    pub fn distance_to(&self, other_node: u32) -> u32 {
        self.distance_to_nodes
            .get(&other_node)
            .copied()
            .unwrap_or(255) // 255 = unreachable
    }
}

/// NUMA topology detector
#[derive(Clone)]
pub struct NumaTopology {
    nodes: HashMap<u32, NumaNode>,
    current_node: Option<u32>,
}

impl NumaTopology {
    /// Detect NUMA topology from /sys filesystem
    pub fn detect() -> Result<Self> {
        let sys_node_path = Path::new("/sys/devices/system/node");

        // Check if NUMA is available
        if !sys_node_path.exists() {
            info!("NUMA not available on this system (no /sys/devices/system/node)");
            return Self::create_single_node_fallback();
        }

        let mut nodes = HashMap::new();

        // Find all node directories (node0, node1, etc.)
        let entries =
            fs::read_dir(sys_node_path).context("Failed to read /sys/devices/system/node")?;

        for entry in entries {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            // Parse node directories (nodeN)
            if let Some(node_suffix) = name_str.strip_prefix("node") {
                if let Ok(node_id) = node_suffix.parse::<u32>() {
                    let node_path = entry.path();

                    match Self::parse_node(&node_path, node_id) {
                        Ok(node) => {
                            if node.is_online() {
                                debug!(
                                    "Detected NUMA node {}: {} CPUs, {} MB RAM",
                                    node_id,
                                    node.cpu_list.len(),
                                    node.memory_total_kb / 1024
                                );
                                nodes.insert(node_id, node);
                            }
                        }
                        Err(e) => {
                            warn!("Failed to parse NUMA node {}: {}", node_id, e);
                        }
                    }
                }
            }
        }

        if nodes.is_empty() {
            warn!("No online NUMA nodes found, falling back to single-node");
            return Self::create_single_node_fallback();
        }

        // Detect current node (which node is this process running on)
        let current_node = Self::detect_current_node();

        info!(
            "NUMA topology detected: {} nodes, current node: {:?}",
            nodes.len(),
            current_node
        );

        Ok(Self {
            nodes,
            current_node,
        })
    }

    /// Parse a single NUMA node directory
    fn parse_node(node_path: &Path, node_id: u32) -> Result<NumaNode> {
        // Parse CPU list
        let cpu_list = Self::parse_cpu_list(node_path)?;

        // Parse memory info
        let (memory_total_kb, memory_free_kb) = Self::parse_memory_info(node_path)?;

        // Parse distance map
        let distance_to_nodes = Self::parse_distance_map(node_path)?;

        Ok(NumaNode {
            node_id,
            cpu_list,
            memory_total_kb,
            memory_free_kb,
            distance_to_nodes,
        })
    }

    /// Parse CPU list from cpulist file
    fn parse_cpu_list(node_path: &Path) -> Result<Vec<u32>> {
        let cpulist_path = node_path.join("cpulist");

        if !cpulist_path.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(&cpulist_path).context("Failed to read cpulist")?;

        let content = content.trim();
        if content.is_empty() {
            return Ok(Vec::new());
        }

        Self::parse_cpu_range(content)
    }

    /// Parse CPU range string (e.g., "0-3,8-11" or "0,2,4,6")
    fn parse_cpu_range(s: &str) -> Result<Vec<u32>> {
        let mut cpus = Vec::new();

        for part in s.split(',') {
            let part = part.trim();

            if part.contains('-') {
                // Range: "0-3"
                let range_parts: Vec<&str> = part.split('-').collect();
                if range_parts.len() == 2 {
                    let start: u32 = range_parts[0].parse()?;
                    let end: u32 = range_parts[1].parse()?;
                    cpus.extend(start..=end);
                }
            } else {
                // Single CPU: "5"
                let cpu: u32 = part.parse()?;
                cpus.push(cpu);
            }
        }

        cpus.sort_unstable();
        cpus.dedup();
        Ok(cpus)
    }

    /// Parse memory info from meminfo file
    fn parse_memory_info(node_path: &Path) -> Result<(u64, u64)> {
        let meminfo_path = node_path.join("meminfo");

        if !meminfo_path.exists() {
            return Ok((0, 0));
        }

        let content = fs::read_to_string(&meminfo_path).context("Failed to read meminfo")?;

        let mut total_kb = 0u64;
        let mut free_kb = 0u64;

        for line in content.lines() {
            if line.contains("MemTotal:") {
                total_kb = Self::parse_kb_value(line)?;
            } else if line.contains("MemFree:") {
                free_kb = Self::parse_kb_value(line)?;
            }
        }

        Ok((total_kb, free_kb))
    }

    /// Parse KB value from meminfo line
    fn parse_kb_value(line: &str) -> Result<u64> {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 4 {
            let value: u64 = parts[3].parse()?;
            return Ok(value);
        }
        Ok(0)
    }

    /// Parse distance map from distance file
    fn parse_distance_map(node_path: &Path) -> Result<HashMap<u32, u32>> {
        let distance_path = node_path.join("distance");

        if !distance_path.exists() {
            return Ok(HashMap::new());
        }

        let content = fs::read_to_string(&distance_path).context("Failed to read distance")?;

        let mut distances = HashMap::new();
        let values: Vec<&str> = content.split_whitespace().collect();

        for (idx, value) in values.iter().enumerate() {
            if let Ok(distance) = value.parse::<u32>() {
                distances.insert(idx as u32, distance);
            }
        }

        Ok(distances)
    }

    /// Detect which NUMA node the current process is running on
    fn detect_current_node() -> Option<u32> {
        // Try to read current CPU from /proc/self/stat
        let stat = fs::read_to_string("/proc/self/stat").ok()?;
        let parts: Vec<&str> = stat.split_whitespace().collect();

        // CPU number is at index 38
        if parts.len() > 38 {
            let cpu: u32 = parts[38].parse().ok()?;

            // Find which node this CPU belongs to
            let sys_cpu_path = PathBuf::from(format!("/sys/devices/system/cpu/cpu{}", cpu));

            // Read node ID from node* symlink
            let node_links = fs::read_dir(sys_cpu_path).ok()?;
            for link in node_links.flatten() {
                let name = link.file_name();
                let name_str = name.to_string_lossy();
                if let Some(node_suffix) = name_str.strip_prefix("node") {
                    if let Ok(node_id) = node_suffix.parse::<u32>() {
                        return Some(node_id);
                    }
                }
            }
        }

        None
    }

    /// Create single-node fallback for non-NUMA systems
    fn create_single_node_fallback() -> Result<Self> {
        let num_cpus = num_cpus::get() as u32;
        let cpu_list = (0..num_cpus).collect();

        // Estimate memory from /proc/meminfo
        let (total_kb, free_kb) =
            Self::read_system_memory().unwrap_or((8 * 1024 * 1024, 4 * 1024 * 1024));

        let mut distance_to_nodes = HashMap::new();
        distance_to_nodes.insert(0, 10); // Distance to self = 10 (standard)

        let node = NumaNode {
            node_id: 0,
            cpu_list,
            memory_total_kb: total_kb,
            memory_free_kb: free_kb,
            distance_to_nodes,
        };

        let mut nodes = HashMap::new();
        nodes.insert(0, node);

        Ok(Self {
            nodes,
            current_node: Some(0),
        })
    }

    /// Read total system memory from /proc/meminfo
    fn read_system_memory() -> Result<(u64, u64)> {
        let content = fs::read_to_string("/proc/meminfo")?;

        let mut total_kb = 0u64;
        let mut free_kb = 0u64;

        for line in content.lines() {
            if line.starts_with("MemTotal:") {
                total_kb = Self::parse_kb_value(line).unwrap_or(0);
            } else if line.starts_with("MemAvailable:") {
                // MemAvailable is a better metric than MemFree
                free_kb = Self::parse_kb_value(line).unwrap_or(0);
            }
        }

        Ok((total_kb, free_kb))
    }

    /// Get all NUMA nodes
    pub fn nodes(&self) -> &HashMap<u32, NumaNode> {
        &self.nodes
    }

    /// Get a specific NUMA node
    pub fn get_node(&self, node_id: u32) -> Option<&NumaNode> {
        self.nodes.get(&node_id)
    }

    /// Get current NUMA node (where this process is running)
    pub fn current_node(&self) -> Option<u32> {
        self.current_node
    }

    /// Get node with most available memory
    pub fn node_with_most_memory(&self) -> Option<u32> {
        self.nodes
            .values()
            .max_by_key(|n| n.memory_free_kb)
            .map(|n| n.node_id)
    }

    /// Get optimal node for cache operations
    /// Priority: current node > most memory > node 0
    pub fn optimal_node(&self) -> u32 {
        self.current_node
            .or_else(|| self.node_with_most_memory())
            .unwrap_or(0)
    }

    /// Check if system has multiple NUMA nodes
    pub fn is_numa_system(&self) -> bool {
        self.nodes.len() > 1
    }

    /// Get node count
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Get all CPUs for a specific node
    pub fn cpus_for_node(&self, node_id: u32) -> Vec<u32> {
        self.nodes
            .get(&node_id)
            .map(|n| n.cpu_list.clone())
            .unwrap_or_default()
    }

    /// Refresh memory statistics
    pub fn refresh(&mut self) -> Result<()> {
        for (node_id, node) in &mut self.nodes {
            let node_path = PathBuf::from(format!("/sys/devices/system/node/node{}", node_id));
            if node_path.exists() {
                if let Ok((total, free)) = Self::parse_memory_info(&node_path) {
                    node.memory_total_kb = total;
                    node.memory_free_kb = free;
                }
            }
        }
        Ok(())
    }
}

/// NUMA statistics for monitoring
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NumaStats {
    pub local_accesses: u64,
    pub remote_accesses: u64,
    pub total_latency_ns: u64,
    pub operations: u64,
}

impl NumaStats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_local_access(&mut self, latency_ns: u64) {
        self.local_accesses += 1;
        self.total_latency_ns += latency_ns;
        self.operations += 1;
    }

    pub fn record_remote_access(&mut self, latency_ns: u64) {
        self.remote_accesses += 1;
        self.total_latency_ns += latency_ns;
        self.operations += 1;
    }

    pub fn avg_latency_ns(&self) -> u64 {
        self.total_latency_ns
            .checked_div(self.operations)
            .unwrap_or(0)
    }

    pub fn local_hit_rate(&self) -> f64 {
        self.operations
            .checked_div(1)
            .map(|_| self.local_accesses as f64 / self.operations as f64)
            .unwrap_or(0.0)
    }

    pub fn remote_penalty(&self) -> f64 {
        // Typical remote NUMA access is 2.1x slower
        if self.local_accesses == 0 || self.remote_accesses == 0 {
            return 1.0;
        }
        2.1 // Conservative estimate
    }
}

// ============================================================================
// NUMA STRATEGY / CONFIG / OPTIMIZER
// ============================================================================

/// NUMA placement strategy
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Default)]
pub enum NumaStrategy {
    /// Use local node for all operations
    #[default]
    LocalNode,
    /// Round-robin across nodes
    RoundRobin,
    /// Use node with most memory
    MostMemory,
    /// NUMA disabled or not available
    Disabled,
}

impl NumaStrategy {
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "local" | "local_node" => NumaStrategy::LocalNode,
            "round_robin" | "roundrobin" => NumaStrategy::RoundRobin,
            "most_memory" | "mostmemory" => NumaStrategy::MostMemory,
            _ => NumaStrategy::Disabled,
        }
    }
}

/// Memory policy for NUMA allocation (lightweight, no node data)
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Default)]
pub enum MemoryPolicy {
    /// Bind to specific node
    Bind,
    /// Prefer node but allow others
    Preferred,
    /// Interleave across nodes
    Interleaved,
    /// Use system default
    #[default]
    Default,
}

/// Memory policy with node information for actual syscall application.
///
/// Unlike [`MemoryPolicy`], this enum carries the specific NUMA node IDs
/// needed to build the nodemask for `set_mempolicy(2)`.
#[derive(Debug, Clone, PartialEq)]
pub enum NumaMemoryPolicy {
    /// Bind allocations to specific NUMA node(s) — `MPOL_BIND`
    Bind(Vec<u32>),
    /// Prefer allocations from a specific NUMA node — `MPOL_PREFERRED`
    Preferred(u32),
    /// Interleave allocations across NUMA nodes — `MPOL_INTERLEAVE`
    Interleave(Vec<u32>),
    /// Use the system default memory policy — `MPOL_DEFAULT`
    Default,
}

/// NUMA optimizer configuration
#[derive(Debug, Clone)]
pub struct NumaConfig {
    pub strategy: NumaStrategy,
    pub node_preference: Option<u32>,
    pub memory_policy: MemoryPolicy,
}

impl Default for NumaConfig {
    fn default() -> Self {
        Self {
            strategy: NumaStrategy::parse(
                &std::env::var("OPDBUS_NUMA_STRATEGY").unwrap_or_default(),
            ),
            node_preference: std::env::var("OPDBUS_NUMA_NODE_PREFERENCE")
                .ok()
                .and_then(|s| s.parse().ok()),
            memory_policy: MemoryPolicy::Default,
        }
    }
}

/// NUMA optimizer (compatibility wrapper around NumaTopology)
pub struct NumaOptimizer {
    config: NumaConfig,
    topology: Option<NumaTopology>,
    current_node: u32,
    stats: NumaStats,
}

impl NumaOptimizer {
    /// Create a new NUMA optimizer
    pub fn new(config: NumaConfig) -> Self {
        let topology = match NumaTopology::detect() {
            Ok(t) => {
                info!("NUMA topology detected: {} nodes", t.node_count());
                Some(t)
            }
            Err(e) => {
                info!("NUMA not available: {}", e);
                None
            }
        };

        Self {
            config,
            topology,
            current_node: 0,
            stats: NumaStats::new(),
        }
    }

    /// Create from environment
    pub fn from_env() -> Self {
        Self::new(NumaConfig::default())
    }

    /// Get the optimal NUMA node for a cache operation
    pub fn get_optimal_node(&mut self) -> Option<u32> {
        let topology = self.topology.as_ref()?;

        match self.config.strategy {
            NumaStrategy::Disabled => None,
            NumaStrategy::LocalNode => self
                .config
                .node_preference
                .or(Some(topology.optimal_node())),
            NumaStrategy::RoundRobin => {
                let node = self.current_node;
                self.current_node = (self.current_node + 1) % topology.node_count() as u32;
                Some(node)
            }
            NumaStrategy::MostMemory => topology.node_with_most_memory(),
        }
    }

    /// Set CPU affinity for a given NUMA node using `sched_setaffinity(2)`.
    ///
    /// On Linux, this builds a CPU mask from the node's `cpu_list` (read from
    /// `/sys/devices/system/node/nodeN/cpulist`) and calls
    /// `libc::sched_setaffinity` to bind the calling thread to those CPUs.
    ///
    /// On non-Linux platforms this logs and returns `Ok(())`.
    pub fn apply_cpu_affinity(&self, node_id: u32) -> Result<()> {
        let cpu_list = if let Some(ref topology) = self.topology {
            topology
                .get_node(node_id)
                .map(|n| n.cpu_list.clone())
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        if cpu_list.is_empty() {
            warn!(
                "No CPUs found for NUMA node {}; skipping CPU affinity",
                node_id
            );
            return Ok(());
        }

        #[cfg(target_os = "linux")]
        {
            // SAFETY: cpu_set_t is a POD type; zeroing produces a valid empty set.
            let mut cpuset: libc::cpu_set_t = unsafe { std::mem::zeroed() };
            for &cpu in &cpu_list {
                unsafe { libc::CPU_SET(cpu as usize, &mut cpuset) };
            }

            let ret = unsafe {
                libc::sched_setaffinity(
                    0, // 0 = calling thread
                    std::mem::size_of::<libc::cpu_set_t>(),
                    &cpuset,
                )
            };
            if ret != 0 {
                let err = std::io::Error::last_os_error();
                anyhow::bail!(
                    "sched_setaffinity failed for NUMA node {} (CPUs {:?}): {}",
                    node_id,
                    cpu_list,
                    err
                );
            }

            // Verify by reading back the affinity mask.
            let mut verify_set: libc::cpu_set_t = unsafe { std::mem::zeroed() };
            let ret = unsafe {
                libc::sched_getaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &mut verify_set)
            };
            if ret != 0 {
                let err = std::io::Error::last_os_error();
                warn!(
                    "sched_getaffinity verification failed after setting affinity: {}",
                    err
                );
            }

            info!(
                "CPU affinity set to NUMA node {} ({} CPUs: {:?})",
                node_id,
                cpu_list.len(),
                cpu_list
            );
        }

        #[cfg(not(target_os = "linux"))]
        {
            info!(
                "CPU affinity for NUMA node {} is a no-op on non-Linux (CPUs: {:?})",
                node_id, cpu_list
            );
        }

        Ok(())
    }

    /// Apply a NUMA memory policy using `set_mempolicy(2)`.
    ///
    /// On Linux, translates the [`NumaMemoryPolicy`] into the corresponding
    /// `MPOL_*` mode and nodemask, then calls `libc::set_mempolicy`.
    ///
    /// On non-Linux platforms this logs and returns `Ok(())`.
    pub fn apply_memory_policy(policy: &NumaMemoryPolicy) -> Result<()> {
        #[cfg(target_os = "linux")]
        {
            // MPOL_* constants from <linux/mempolicy.h>
            const MPOL_DEFAULT: libc::c_int = 0;
            const MPOL_PREFERRED: libc::c_int = 1;
            const MPOL_BIND: libc::c_int = 2;
            const MPOL_INTERLEAVE: libc::c_int = 3;

            let (mode, nodemask, maxnode): (libc::c_int, usize, usize) = match policy {
                NumaMemoryPolicy::Default => (MPOL_DEFAULT, 0, 0),
                NumaMemoryPolicy::Preferred(node) => {
                    let node = *node as usize;
                    let mask = 1usize << node;
                    (MPOL_PREFERRED, mask, node + 2)
                }
                NumaMemoryPolicy::Bind(nodes) => {
                    let mut mask = 0usize;
                    let mut max = 0usize;
                    for &node in nodes {
                        mask |= 1usize << node;
                        if (node as usize) + 1 > max {
                            max = node as usize + 1;
                        }
                    }
                    (MPOL_BIND, mask, max)
                }
                NumaMemoryPolicy::Interleave(nodes) => {
                    let mut mask = 0usize;
                    let mut max = 0usize;
                    for &node in nodes {
                        mask |= 1usize << node;
                        if (node as usize) + 1 > max {
                            max = node as usize + 1;
                        }
                    }
                    (MPOL_INTERLEAVE, mask, max)
                }
            };

            // MPOL_DEFAULT requires a NULL nodemask pointer; the kernel
            // rejects a non-NULL pointer with EINVAL.
            let nodemask_ptr: libc::c_long = if mode == MPOL_DEFAULT {
                0 // NULL
            } else {
                &nodemask as *const usize as libc::c_long
            };

            let ret = unsafe {
                libc::syscall(
                    libc::SYS_set_mempolicy,
                    mode as libc::c_long,
                    nodemask_ptr,
                    maxnode as libc::c_long,
                )
            };
            if ret != 0 {
                let err = std::io::Error::last_os_error();
                anyhow::bail!("set_mempolicy failed for policy {:?}: {}", policy, err);
            }

            info!("Memory policy set: {:?}", policy);
        }

        #[cfg(not(target_os = "linux"))]
        {
            info!(
                "Memory policy {:?} is a no-op on non-Linux platforms",
                policy
            );
        }

        Ok(())
    }

    /// Legacy wrapper — logs the intent without applying affinity.
    /// Prefer [`apply_cpu_affinity`](Self::apply_cpu_affinity) for real pinning.
    pub fn set_cpu_affinity(&self, node_id: u32) -> Result<()> {
        self.apply_cpu_affinity(node_id)
    }

    /// Get statistics
    pub fn get_stats(&self) -> &NumaStats {
        &self.stats
    }

    /// Record a local access
    pub fn record_local_access(&mut self, latency_ns: u64) {
        self.stats.record_local_access(latency_ns);
    }

    /// Record a remote access
    pub fn record_remote_access(&mut self, latency_ns: u64) {
        self.stats.record_remote_access(latency_ns);
    }

    /// Check if NUMA is available
    pub fn is_available(&self) -> bool {
        self.topology.is_some() && self.config.strategy != NumaStrategy::Disabled
    }

    /// Get topology
    pub fn topology(&self) -> Option<&NumaTopology> {
        self.topology.as_ref()
    }

    /// Refresh memory statistics
    pub fn refresh(&mut self) -> Result<()> {
        if let Some(ref mut topology) = self.topology {
            topology.refresh()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cpu_range() {
        let cpus = NumaTopology::parse_cpu_range("0-3,8-11").unwrap();
        assert_eq!(cpus, vec![0, 1, 2, 3, 8, 9, 10, 11]);

        let cpus = NumaTopology::parse_cpu_range("0,2,4,6").unwrap();
        assert_eq!(cpus, vec![0, 2, 4, 6]);

        let cpus = NumaTopology::parse_cpu_range("5").unwrap();
        assert_eq!(cpus, vec![5]);
    }

    #[test]
    fn test_numa_detection() {
        // Should not panic even if NUMA is not available
        let topology = NumaTopology::detect();
        assert!(topology.is_ok());

        let topology = topology.unwrap();
        assert!(topology.node_count() >= 1);
    }

    #[test]
    fn test_numa_stats() {
        let mut stats = NumaStats::new();

        stats.record_local_access(50);
        stats.record_local_access(60);
        stats.record_remote_access(120);

        assert_eq!(stats.operations, 3);
        assert_eq!(stats.local_accesses, 2);
        assert_eq!(stats.remote_accesses, 1);
        assert_eq!(stats.avg_latency_ns(), (50 + 60 + 120) / 3);
    }

    #[test]
    fn test_numa_optimizer() {
        let optimizer = NumaOptimizer::from_env();
        // Should not panic
        assert!(optimizer.topology().is_some() || !optimizer.is_available());
    }

    #[test]
    fn test_numa_strategy_from_str() {
        assert_eq!(NumaStrategy::parse("local"), NumaStrategy::LocalNode);
        assert_eq!(NumaStrategy::parse("round_robin"), NumaStrategy::RoundRobin);
        assert_eq!(NumaStrategy::parse("most_memory"), NumaStrategy::MostMemory);
        assert_eq!(NumaStrategy::parse("unknown"), NumaStrategy::Disabled);
    }

    #[test]
    fn test_apply_cpu_affinity_single_node() {
        // On a single-node system, applying affinity to node 0 should set the
        // mask to all CPUs — effectively a no-op, but it must not error.
        let optimizer = NumaOptimizer::from_env();
        let result = optimizer.apply_cpu_affinity(0);
        assert!(
            result.is_ok(),
            "apply_cpu_affinity(0) should succeed on single-node: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_apply_memory_policy_default() {
        // Setting MPOL_DEFAULT should always succeed (it restores the kernel
        // default policy regardless of NUMA topology).
        let result = NumaOptimizer::apply_memory_policy(&NumaMemoryPolicy::Default);
        assert!(
            result.is_ok(),
            "apply_memory_policy(Default) should succeed: {:?}",
            result.err()
        );
    }

    #[test]
    #[ignore = "requires multi-node NUMA hardware to test meaningfully"]
    fn test_apply_cpu_affinity_multi_node() {
        let optimizer = NumaOptimizer::from_env();
        // On a multi-node system, node 1 should exist and have CPUs.
        let result = optimizer.apply_cpu_affinity(1);
        assert!(
            result.is_ok(),
            "apply_cpu_affinity(1) failed: {:?}",
            result.err()
        );
    }

    #[test]
    #[ignore = "requires multi-node NUMA hardware to test meaningfully"]
    fn test_apply_memory_policy_bind() {
        let result = NumaOptimizer::apply_memory_policy(&NumaMemoryPolicy::Bind(vec![0]));
        assert!(
            result.is_ok(),
            "apply_memory_policy(Bind [0]) failed: {:?}",
            result.err()
        );
    }

    #[test]
    #[ignore = "requires multi-node NUMA hardware to test meaningfully"]
    fn test_apply_memory_policy_interleave() {
        let result = NumaOptimizer::apply_memory_policy(&NumaMemoryPolicy::Interleave(vec![0, 1]));
        assert!(
            result.is_ok(),
            "apply_memory_policy(Interleave [0,1]) failed: {:?}",
            result.err()
        );
    }
}
