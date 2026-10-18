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
use std::cmp::Reverse;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

// Bound allocations made from untrusted/configured identifiers while allowing
// dynamic masks well beyond libc's fixed cpu_set_t and one machine word.
const MAX_MASK_BITS: usize = 1 << 20;

fn bitmask(ids: &[u32]) -> Result<Vec<libc::c_ulong>> {
    let highest = ids.iter().copied().max().context("empty identifier mask")? as usize;
    anyhow::ensure!(
        highest < MAX_MASK_BITS,
        "identifier exceeds mask limit: {highest}"
    );
    let word_bits = libc::c_ulong::BITS as usize;
    let mut words = vec![0; highest / word_bits + 1];
    for &id in ids {
        words[id as usize / word_bits] |= (1 as libc::c_ulong) << (id as usize % word_bits);
    }
    Ok(words)
}

fn memory_nodemask(nodes: &[u32]) -> Result<(Vec<libc::c_ulong>, usize)> {
    let words = bitmask(nodes)?;
    // Linux get_nodes() excludes the final maxnode bit. Include the highest
    // requested node by passing its id + 2 (also used by MPOL_PREFERRED).
    let maxnode = nodes
        .iter()
        .copied()
        .max()
        .context("empty NUMA node mask")? as usize
        + 2;
    Ok((words, maxnode))
}

fn permitted_cpus(requested: &[u32], allowed: &[u32]) -> Result<Vec<u32>> {
    let _ = bitmask(requested)?;
    let mut effective: Vec<_> = requested
        .iter()
        .copied()
        .filter(|cpu| allowed.contains(cpu))
        .collect();
    effective.sort_unstable();
    effective.dedup();
    anyhow::ensure!(
        !effective.is_empty(),
        "requested CPUs {requested:?} do not intersect the calling thread's allowed CPUs {allowed:?}"
    );
    Ok(effective)
}

#[cfg(target_os = "linux")]
fn read_cpu_affinity_mask() -> Result<Vec<libc::c_ulong>> {
    let word_bytes = std::mem::size_of::<libc::c_ulong>();
    let mut words = vec![0 as libc::c_ulong; 128 / word_bytes];
    loop {
        let result = unsafe {
            libc::sched_getaffinity(0, words.len() * word_bytes, words.as_mut_ptr().cast())
        };
        if result == 0 {
            return Ok(words);
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EINVAL)
            || words.len() * libc::c_ulong::BITS as usize >= MAX_MASK_BITS
        {
            return Err(error).context("reading calling thread CPU affinity");
        }
        words.resize(words.len() * 2, 0);
    }
}

fn mask_ids(words: &[libc::c_ulong]) -> Vec<u32> {
    let bits = libc::c_ulong::BITS as usize;
    words
        .iter()
        .enumerate()
        .flat_map(|(index, word)| {
            (0..bits).filter_map(move |bit| {
                ((*word & ((1 as libc::c_ulong) << bit)) != 0)
                    .then_some((index * bits + bit) as u32)
            })
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn read_cpu_affinity() -> Result<Vec<u32>> {
    Ok(mask_ids(&read_cpu_affinity_mask()?))
}

#[cfg(target_os = "linux")]
fn prepare_memory_policy(
    policy: &NumaMemoryPolicy,
) -> Result<(libc::c_int, Vec<libc::c_ulong>, usize)> {
    let (mode, nodes) = match policy {
        NumaMemoryPolicy::Default => return Ok((0, Vec::new(), 0)),
        NumaMemoryPolicy::Preferred(node) => (1, vec![*node]),
        NumaMemoryPolicy::Bind(nodes) => (2, nodes.clone()),
        NumaMemoryPolicy::Interleave(nodes) => (3, nodes.clone()),
    };
    let (mask, maxnode) = memory_nodemask(&nodes)?;
    let allowed = allowed_memory_nodes()?;
    anyhow::ensure!(
        nodes.iter().all(|node| allowed.contains(node)),
        "NUMA policy {policy:?} includes a node outside the calling thread's allowed nodes {allowed:?}"
    );
    Ok((mode, mask, maxnode))
}

#[cfg(target_os = "linux")]
fn allowed_memory_nodes() -> Result<Vec<u32>> {
    let status =
        fs::read_to_string("/proc/thread-self/status").context("reading allowed NUMA nodes")?;
    let allowed = status
        .lines()
        .find_map(|line| line.strip_prefix("Mems_allowed_list:"))
        .context("missing allowed NUMA node list")?;
    NumaTopology::parse_cpu_range(allowed)
}

#[cfg(target_os = "linux")]
fn default_child_policy_denial_is_optional(mode: libc::c_int, errno: Option<i32>) -> bool {
    mode == 0 && matches!(errno, Some(libc::EPERM | libc::EACCES | libc::ENOSYS))
}

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
            if let Some(node_suffix) = name_str.strip_prefix("node")
                && let Ok(node_id) = node_suffix.parse::<u32>()
            {
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

        if s.trim().is_empty() {
            return Ok(cpus);
        }

        for part in s.split(',') {
            let part = part.trim();

            if part.contains('-') {
                // Range: "0-3"
                let range_parts: Vec<&str> = part.split('-').collect();
                anyhow::ensure!(range_parts.len() == 2, "invalid CPU range: {part}");
                let start: u32 = range_parts[0].trim().parse()?;
                let end: u32 = range_parts[1].trim().parse()?;
                anyhow::ensure!(
                    start <= end && (end as usize) < MAX_MASK_BITS,
                    "invalid CPU range: {part}"
                );
                anyhow::ensure!(
                    cpus.len() + ((end - start) as usize) < MAX_MASK_BITS,
                    "CPU list exceeds mask limit"
                );
                cpus.extend(start..=end);
            } else {
                // Single CPU: "5"
                let cpu: u32 = part.parse()?;
                anyhow::ensure!(
                    (cpu as usize) < MAX_MASK_BITS && cpus.len() < MAX_MASK_BITS,
                    "CPU identifier exceeds mask limit: {cpu}"
                );
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
        line.split_once(':')
            .context("missing memory field separator")?
            .1
            .split_whitespace()
            .next()
            .context("missing memory field value")?
            .parse()
            .context("invalid memory field value")
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
        let cpu = Self::current_cpu()?;

        // Find which node this CPU belongs to
        let sys_cpu_path = PathBuf::from(format!("/sys/devices/system/cpu/cpu{}", cpu));

        // Read node ID from node* symlink
        let node_links = fs::read_dir(sys_cpu_path).ok()?;
        for link in node_links.flatten() {
            let name = link.file_name();
            let name_str = name.to_string_lossy();
            if let Some(node_suffix) = name_str.strip_prefix("node")
                && let Ok(node_id) = node_suffix.parse::<u32>()
            {
                return Some(node_id);
            }
        }
        None
    }

    fn current_cpu() -> Option<u32> {
        #[cfg(target_os = "linux")]
        {
            let cpu = unsafe { libc::sched_getcpu() };
            (cpu >= 0).then_some(cpu as u32)
        }
        #[cfg(not(target_os = "linux"))]
        {
            None
        }
    }

    /// Create single-node fallback for non-NUMA systems
    fn create_single_node_fallback() -> Result<Self> {
        let cpu_list = NumaOptimizer::current_cpu_affinity()
            .filter(|cpus| !cpus.is_empty())
            .or_else(|| {
                fs::read_to_string("/sys/devices/system/cpu/online")
                    .ok()
                    .and_then(|list| Self::parse_cpu_range(&list).ok())
            })
            .unwrap_or_else(|| (0..num_cpus::get().min(MAX_MASK_BITS) as u32).collect());

        // Estimate memory from /proc/meminfo
        let (total_kb, free_kb) = Self::read_system_memory().unwrap_or_default();

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
        Self::current_cpu()
            .and_then(|cpu| {
                self.nodes
                    .values()
                    .find(|node| node.cpu_list.contains(&cpu))
                    .map(|node| node.node_id)
            })
            .or(self.current_node)
    }

    /// Get node with most available memory
    pub fn node_with_most_memory(&self) -> Option<u32> {
        self.nodes
            .values()
            .min_by_key(|n| (Reverse(n.memory_free_kb), n.node_id))
            .map(|n| n.node_id)
    }

    /// Get optimal node for cache operations
    /// Priority: current node > most memory > node 0
    pub fn optimal_node(&self) -> u32 {
        self.current_node()
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

    /// Stable node order, including sparse node IDs.
    pub fn node_ids(&self) -> Vec<u32> {
        let mut ids: Vec<_> = self.nodes.keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    #[cfg(test)]
    pub(crate) fn test_topology(nodes: Vec<NumaNode>, current_node: Option<u32>) -> Self {
        Self {
            nodes: nodes.into_iter().map(|node| (node.node_id, node)).collect(),
            current_node,
        }
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
        // Refresh hotplug topology as well as memory availability. Retain the
        // previous snapshot if discovery fails.
        let refreshed = Self::detect()?;
        *self = refreshed;
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
        if self.operations == 0 {
            0.0
        } else {
            self.local_accesses as f64 / self.operations as f64
        }
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
            strategy: std::env::var("OPDBUS_NUMA_STRATEGY")
                .map(|value| NumaStrategy::parse(&value))
                .unwrap_or_default(),
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
        if self.config.strategy == NumaStrategy::Disabled {
            return None;
        }
        if let Err(error) = self.refresh() {
            debug!("NUMA refresh failed, retaining previous topology: {error}");
        }
        self.select_node()
    }

    fn select_node(&mut self) -> Option<u32> {
        let topology = self.topology.as_ref()?;

        match self.config.strategy {
            NumaStrategy::Disabled => None,
            NumaStrategy::LocalNode => self
                .config
                .node_preference
                .filter(|id| topology.get_node(*id).is_some())
                .or(Some(topology.optimal_node())),
            NumaStrategy::RoundRobin => {
                let ids = topology.node_ids();
                let node = ids[self.current_node as usize % ids.len()];
                self.current_node = (self.current_node + 1) % ids.len() as u32;
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
    /// Explicit placement is unsupported on non-Linux platforms.
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
            anyhow::bail!("No CPUs found for NUMA node {node_id}");
        }

        Self::apply_cpu_affinity_to(&cpu_list, &format!("NUMA node {node_id}"))
    }

    /// Bind the calling thread to the requested CPUs it is currently allowed
    /// to use, and verify the effective mask. Reject an empty intersection.
    ///
    /// A single-element list pins a dedicated streaming thread to one core so
    /// its L1/L2 working set stays warm; `apply_cpu_affinity` uses the same
    /// primitive with a whole node's CPU list.
    pub fn apply_cpu_affinity_to(cpu_list: &[u32], label: &str) -> Result<()> {
        anyhow::ensure!(!cpu_list.is_empty(), "empty CPU list for {label}");

        #[cfg(target_os = "linux")]
        {
            let effective = permitted_cpus(cpu_list, &read_cpu_affinity()?)?;
            let mask = bitmask(&effective)?;

            let ret = unsafe {
                libc::sched_setaffinity(
                    0, // 0 = calling thread
                    mask.len() * std::mem::size_of::<libc::c_ulong>(),
                    mask.as_ptr().cast(),
                )
            };
            if ret != 0 {
                let err = std::io::Error::last_os_error();
                anyhow::bail!(
                    "sched_setaffinity failed for {} (CPUs {:?}): {}",
                    label,
                    cpu_list,
                    err
                );
            }

            let actual = read_cpu_affinity()?;
            anyhow::ensure!(
                actual == effective,
                "CPU affinity verification failed for {label}: requested allowed mask {effective:?}, actual {actual:?}"
            );

            info!(
                "CPU affinity set to {} ({} CPUs: {:?})",
                label,
                actual.len(),
                actual
            );
        }

        #[cfg(not(target_os = "linux"))]
        {
            anyhow::bail!("CPU affinity for {label} is unsupported on this platform");
        }

        Ok(())
    }

    /// CPUs the calling thread is currently allowed to run on
    /// (`sched_getaffinity(2)`); `None` on non-Linux.
    pub fn current_cpu_affinity() -> Option<Vec<u32>> {
        #[cfg(target_os = "linux")]
        {
            read_cpu_affinity().ok()
        }
        #[cfg(not(target_os = "linux"))]
        {
            None
        }
    }

    /// Apply a NUMA memory policy using `set_mempolicy(2)`.
    ///
    /// On Linux, translates the [`NumaMemoryPolicy`] into the corresponding
    /// `MPOL_*` mode and nodemask, then calls `libc::set_mempolicy`.
    ///
    /// Explicit placement is unsupported on non-Linux platforms.
    pub fn apply_memory_policy(policy: &NumaMemoryPolicy) -> Result<()> {
        #[cfg(target_os = "linux")]
        {
            let (mode, nodemask, maxnode) = prepare_memory_policy(policy)?;

            // MPOL_DEFAULT requires a NULL nodemask pointer; the kernel
            // rejects a non-NULL pointer with EINVAL.
            let nodemask_ptr: libc::c_long = if mode == 0 {
                0 // NULL
            } else {
                nodemask.as_ptr() as libc::c_long
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
                return Err(err)
                    .with_context(|| format!("set_mempolicy failed for policy {policy:?}"));
            }

            info!("Memory policy set: {:?}", policy);
        }

        #[cfg(not(target_os = "linux"))]
        {
            anyhow::bail!("Memory policy {policy:?} is unsupported on this platform");
        }

        Ok(())
    }

    /// Prepare placement in the parent and apply it only to the spawned child.
    /// The pre-exec hook performs raw syscalls without allocation, filesystem
    /// reads, logging or locks. Explicit placement failures stop spawn. A
    /// restricted kernel may deny resetting the default memory policy; in
    /// that case the child inherits its existing policy. The parent is untouched.
    pub fn configure_command(
        command: &mut tokio::process::Command,
        cpu_list: &[u32],
        policy: &NumaMemoryPolicy,
    ) -> Result<()> {
        #[cfg(target_os = "linux")]
        {
            let (cpu_mask, mut cpu_readback) = if cpu_list.is_empty() {
                (None, Vec::new())
            } else {
                let allowed_mask = read_cpu_affinity_mask()?;
                let effective = permitted_cpus(cpu_list, &mask_ids(&allowed_mask))?;
                let mask = bitmask(&effective)?;
                let readback = vec![0; allowed_mask.len().max(mask.len())];
                (Some(mask), readback)
            };
            let (mode, node_mask, maxnode) = prepare_memory_policy(policy)?;
            // SAFETY: all owned masks are prepared before fork. This closure
            // touches only those masks and async-signal-safe kernel syscalls.
            unsafe {
                command.pre_exec(move || {
                    if let Some(mask) = &cpu_mask {
                        if libc::sched_setaffinity(
                            0,
                            mask.len() * std::mem::size_of::<libc::c_ulong>(),
                            mask.as_ptr().cast(),
                        ) != 0
                        {
                            return Err(std::io::Error::last_os_error());
                        }
                        if libc::sched_getaffinity(
                            0,
                            cpu_readback.len() * std::mem::size_of::<libc::c_ulong>(),
                            cpu_readback.as_mut_ptr().cast(),
                        ) != 0
                        {
                            return Err(std::io::Error::last_os_error());
                        }
                        if cpu_readback
                            .iter()
                            .enumerate()
                            .any(|(index, word)| *word != mask.get(index).copied().unwrap_or(0))
                        {
                            return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
                        }
                    }
                    let pointer = if mode == 0 {
                        std::ptr::null()
                    } else {
                        node_mask.as_ptr()
                    };
                    if libc::syscall(
                        libc::SYS_set_mempolicy,
                        mode as libc::c_long,
                        pointer,
                        maxnode as libc::c_long,
                    ) != 0
                    {
                        let error = std::io::Error::last_os_error();
                        // Default is the unconfigured/best-effort policy.
                        // Seccomp or missing kernel NUMA support must not make
                        // ordinary cache transport unusable. Requested memory
                        // placement and every other kernel error remain strict.
                        if !default_child_policy_denial_is_optional(mode, error.raw_os_error()) {
                            return Err(error);
                        }
                    }
                    Ok(())
                });
            }
            Ok(())
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = command;
            anyhow::bail!(
                "Child NUMA placement is unsupported on this platform: CPUs {cpu_list:?}, policy {policy:?}"
            );
        }
    }

    /// Compatibility wrapper for applying real calling-thread affinity.
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
    fn malformed_and_unbounded_cpu_lists_are_rejected() {
        for input in ["3-1", "1-2-3", "1,,2", "-1", "0-4294967295", "4294967295"] {
            assert!(NumaTopology::parse_cpu_range(input).is_err(), "{input}");
        }
        assert_eq!(
            NumaTopology::parse_cpu_range(" 7, 2-3, 7 ").unwrap(),
            vec![2, 3, 7]
        );
    }

    #[test]
    fn system_and_node_meminfo_values_have_the_same_parser() {
        assert_eq!(
            NumaTopology::parse_kb_value("MemTotal:       32856188 kB").unwrap(),
            32856188
        );
        assert_eq!(
            NumaTopology::parse_kb_value("Node 0 MemFree: 838752 kB").unwrap(),
            838752
        );
        assert!(NumaTopology::parse_kb_value("MemTotal: unknown kB").is_err());
        assert!(NumaTopology::parse_kb_value("MemTotal:").is_err());
    }

    #[test]
    fn dynamic_nodemasks_include_machine_word_boundaries() {
        let bits = libc::c_ulong::BITS;
        let (mask, maxnode) = memory_nodemask(&[0, bits - 1, bits, 2 * bits]).unwrap();
        assert_eq!(mask.len(), 3);
        assert_eq!(mask[0], 1 | ((1 as libc::c_ulong) << (bits - 1)));
        assert_eq!(mask[1], 1);
        assert_eq!(mask[2], 1);
        assert_eq!(maxnode, 2 * bits as usize + 2);
        assert_eq!(memory_nodemask(&[0]).unwrap(), (vec![1], 2));
        assert!(memory_nodemask(&[]).is_err());
        assert!(memory_nodemask(&[u32::MAX]).is_err());
        assert!(bitmask(&[MAX_MASK_BITS as u32]).is_err());
    }

    #[test]
    fn effective_affinity_is_a_checked_allowed_intersection() {
        assert_eq!(permitted_cpus(&[7, 3, 7, 11], &[3, 7]).unwrap(), vec![3, 7]);
        assert!(permitted_cpus(&[0], &[3, 7]).is_err());
        assert!(permitted_cpus(&[], &[3]).is_err());
        assert!(permitted_cpus(&[u32::MAX], &[3]).is_err());
    }

    #[test]
    fn strategies_use_stable_sparse_node_ids_and_free_memory() {
        let mut nodes = HashMap::new();
        for id in [11, 3, 8] {
            nodes.insert(
                id,
                NumaNode {
                    node_id: id,
                    cpu_list: Vec::new(),
                    memory_total_kb: 1000,
                    memory_free_kb: if id == 8 { 900 } else { 100 },
                    distance_to_nodes: HashMap::new(),
                },
            );
        }
        let topology = NumaTopology {
            nodes,
            current_node: None,
        };
        assert_eq!(topology.node_ids(), vec![3, 8, 11]);
        assert_eq!(topology.node_with_most_memory(), Some(8));
        let mut optimizer = NumaOptimizer {
            config: NumaConfig {
                strategy: NumaStrategy::RoundRobin,
                node_preference: None,
                memory_policy: MemoryPolicy::Default,
            },
            topology: Some(topology),
            current_node: 0,
            stats: NumaStats::default(),
        };
        let round: Vec<_> = (0..5).map(|_| optimizer.select_node().unwrap()).collect();
        assert_eq!(round, vec![3, 8, 11, 3, 8]);
        optimizer.config.strategy = NumaStrategy::MostMemory;
        assert_eq!(optimizer.select_node(), Some(8));
        optimizer
            .topology
            .as_mut()
            .unwrap()
            .nodes
            .get_mut(&3)
            .unwrap()
            .memory_free_kb = 900;
        assert_eq!(optimizer.select_node(), Some(3)); // deterministic tie
        optimizer.config.strategy = NumaStrategy::Disabled;
        assert_eq!(optimizer.select_node(), None);
        optimizer.config.strategy = NumaStrategy::LocalNode;
        optimizer.config.node_preference = Some(11);
        assert_eq!(optimizer.select_node(), Some(11));
        optimizer.config.node_preference = Some(99);
        assert_eq!(optimizer.select_node(), Some(3));
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
        assert_eq!(stats.local_hit_rate(), 0.0);

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
        assert_native_memory_policy(NumaMemoryPolicy::Default, 0, vec![]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn child_default_policy_only_tolerates_restricted_or_missing_syscall() {
        for errno in [libc::EPERM, libc::EACCES, libc::ENOSYS] {
            assert!(default_child_policy_denial_is_optional(0, Some(errno)));
            for explicit_mode in [1, 2, 3] {
                assert!(!default_child_policy_denial_is_optional(
                    explicit_mode,
                    Some(errno)
                ));
            }
        }
        for errno in [libc::EINVAL, libc::ENOMEM, libc::EIO] {
            assert!(!default_child_policy_denial_is_optional(0, Some(errno)));
        }
        assert!(!default_child_policy_denial_is_optional(0, None));
    }

    /// Install a filter only after fork, never on the test runner or host.
    #[cfg(target_os = "linux")]
    fn deny_child_memory_policy(command: &mut tokio::process::Command) {
        // SAFETY: the hook uses stack-only BPF data and raw prctl syscalls.
        // It denies only set_mempolicy; every other child syscall is allowed.
        unsafe {
            command.pre_exec(|| {
                let mut instructions = [
                    libc::sock_filter {
                        code: (libc::BPF_LD | libc::BPF_W | libc::BPF_ABS) as u16,
                        jt: 0,
                        jf: 0,
                        k: 0, // seccomp_data.nr
                    },
                    libc::sock_filter {
                        code: (libc::BPF_JMP | libc::BPF_JEQ | libc::BPF_K) as u16,
                        jt: 0,
                        jf: 1,
                        k: libc::SYS_set_mempolicy as u32,
                    },
                    libc::sock_filter {
                        code: (libc::BPF_RET | libc::BPF_K) as u16,
                        jt: 0,
                        jf: 0,
                        k: libc::SECCOMP_RET_ERRNO | libc::EPERM as u32,
                    },
                    libc::sock_filter {
                        code: (libc::BPF_RET | libc::BPF_K) as u16,
                        jt: 0,
                        jf: 0,
                        k: libc::SECCOMP_RET_ALLOW,
                    },
                ];
                let program = libc::sock_fprog {
                    len: instructions.len() as u16,
                    filter: instructions.as_mut_ptr(),
                };
                if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0
                    || libc::prctl(
                        libc::PR_SET_SECCOMP,
                        libc::SECCOMP_MODE_FILTER,
                        &program as *const libc::sock_fprog,
                    ) != 0
                {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn child_seccomp_denial_keeps_default_transport_usable_and_explicit_policies_strict() {
        // Distinguish a kernel without child seccomp support from a failed
        // NUMA hook. Unsupported fixtures are reported explicitly.
        let mut availability = tokio::process::Command::new("/usr/bin/true");
        deny_child_memory_policy(&mut availability);
        match availability.output().await {
            Ok(output) => assert!(output.status.success()),
            Err(error)
                if matches!(
                    error.raw_os_error(),
                    Some(libc::EPERM | libc::EACCES | libc::EINVAL | libc::ENOSYS)
                ) =>
            {
                eprintln!("child seccomp fixture unavailable: {error}");
                return;
            }
            Err(error) => panic!("child seccomp fixture failed: {error}"),
        }
        let before = NumaOptimizer::current_cpu_affinity().unwrap();
        let memory_before = read_memory_policy();
        let node = allowed_memory_nodes().unwrap()[0];
        let policies = [
            NumaMemoryPolicy::Default,
            NumaMemoryPolicy::Bind(vec![node]),
            NumaMemoryPolicy::Interleave(vec![node]),
            NumaMemoryPolicy::Preferred(node),
        ];
        for policy in policies {
            let mut command = tokio::process::Command::new("/usr/bin/cat");
            command.arg("/proc/self/status");
            deny_child_memory_policy(&mut command);
            NumaOptimizer::configure_command(&mut command, &[before[0]], &policy).unwrap();
            match command.output().await {
                Ok(output) => {
                    assert_eq!(policy, NumaMemoryPolicy::Default);
                    assert!(output.status.success());
                    let status = String::from_utf8(output.stdout).unwrap();
                    let cpus = status
                        .lines()
                        .find_map(|line| line.strip_prefix("Cpus_allowed_list:"))
                        .unwrap();
                    assert_eq!(
                        NumaTopology::parse_cpu_range(cpus).unwrap(),
                        vec![before[0]]
                    );
                }
                Err(error) => {
                    assert_ne!(policy, NumaMemoryPolicy::Default, "{error}");
                    assert_eq!(error.raw_os_error(), Some(libc::EPERM));
                }
            }
        }
        assert_eq!(NumaOptimizer::current_cpu_affinity(), Some(before));
        assert_eq!(read_memory_policy(), memory_before);
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
    fn test_apply_memory_policy_bind() {
        assert_native_memory_policy(NumaMemoryPolicy::Bind(vec![0]), 2, vec![0]);
    }

    #[test]
    fn test_apply_memory_policy_interleave_single_node() {
        assert_native_memory_policy(NumaMemoryPolicy::Interleave(vec![0]), 3, vec![0]);
    }

    fn assert_native_memory_policy(
        policy: NumaMemoryPolicy,
        expected_mode: i32,
        expected_nodes: Vec<u32>,
    ) {
        #[cfg(target_os = "linux")]
        std::thread::spawn(move || {
            if !matches!(policy, NumaMemoryPolicy::Default)
                && !allowed_memory_nodes().unwrap().contains(&0)
            {
                eprintln!("SKIP native policy readback: node 0 is not allowed");
                return;
            }
            if let Err(error) = NumaOptimizer::apply_memory_policy(&NumaMemoryPolicy::Default) {
                let errno = error
                    .downcast_ref::<std::io::Error>()
                    .and_then(|error| error.raw_os_error());
                if matches!(errno, Some(libc::EPERM | libc::EACCES | libc::ENOSYS)) {
                    eprintln!(
                        "SKIP native policy readback: kernel denied memory policy: {error:#}"
                    );
                    return;
                }
                panic!("default memory policy failed: {error:#}");
            }
            NumaOptimizer::apply_memory_policy(&policy).unwrap();
            let (mode, nodes) = read_memory_policy();
            assert_eq!(mode, expected_mode);
            assert_eq!(nodes, expected_nodes);
        })
        .join()
        .unwrap();
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (policy, expected_mode, expected_nodes);
        }
    }

    #[cfg(target_os = "linux")]
    fn read_memory_policy() -> (i32, Vec<u32>) {
        let bits = libc::c_ulong::BITS as usize;
        let possible = NumaTopology::parse_cpu_range(
            &fs::read_to_string("/sys/devices/system/node/possible").unwrap(),
        )
        .unwrap();
        let words = possible.iter().copied().max().unwrap_or(0) as usize / bits + 1;
        let mut mask = vec![0 as libc::c_ulong; words];
        let mut mode = -1 as libc::c_int;
        let result = unsafe {
            libc::syscall(
                libc::SYS_get_mempolicy,
                &mut mode as *mut libc::c_int,
                mask.as_mut_ptr(),
                words * bits,
                std::ptr::null::<libc::c_void>(),
                0usize,
            )
        };
        assert_eq!(
            result,
            0,
            "get_mempolicy: {}",
            std::io::Error::last_os_error()
        );
        let nodes = mask
            .iter()
            .enumerate()
            .flat_map(|(index, word)| {
                (0..bits).filter_map(move |bit| {
                    ((*word & ((1 as libc::c_ulong) << bit)) != 0)
                        .then_some((index * bits + bit) as u32)
                })
            })
            .collect();
        (mode, nodes)
    }

    #[test]
    fn native_affinity_readback_is_exact_and_caller_is_unchanged() {
        let Some(before) = NumaOptimizer::current_cpu_affinity() else {
            return;
        };
        let cpu = before[0];
        std::thread::spawn(move || {
            NumaOptimizer::apply_cpu_affinity_to(&[cpu, cpu], "regression test").unwrap();
            assert_eq!(NumaOptimizer::current_cpu_affinity(), Some(vec![cpu]));
        })
        .join()
        .unwrap();
        assert_eq!(NumaOptimizer::current_cpu_affinity(), Some(before));
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn spawned_child_affinity_leaves_parent_untouched() {
        let before = NumaOptimizer::current_cpu_affinity().unwrap();
        let memory_before = read_memory_policy();
        let mut command = tokio::process::Command::new("/usr/bin/cat");
        command.arg("/proc/self/status");
        NumaOptimizer::configure_command(&mut command, &[before[0]], &NumaMemoryPolicy::Default)
            .unwrap();
        let output = command.output().await.unwrap();
        assert!(output.status.success());
        let status = String::from_utf8(output.stdout).unwrap();
        let cpus = status
            .lines()
            .find_map(|line| line.strip_prefix("Cpus_allowed_list:"))
            .unwrap();
        assert_eq!(
            NumaTopology::parse_cpu_range(cpus).unwrap(),
            vec![before[0]]
        );
        assert_eq!(NumaOptimizer::current_cpu_affinity(), Some(before));
        assert_eq!(read_memory_policy(), memory_before);
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
