//! Request Context - Per-Request Tool Loading
//!
//! Tools are loaded when a request starts and unloaded when it completes.
//! This ensures:
//! - All tools available during request (no eviction)
//! - Memory freed between requests
//! - Clean isolation per request
//! - max_turns enforced per request (not session)
//! - **Security blocklist enforced at the single choke point** (audit item #7)

use anyhow::Result;
use simd_json::OwnedValue;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::compact::ToolDefinition;
use crate::tool_registry::{BoxedTool, Tool};

// =============================================================================
// SECURITY BLOCKLIST (audit item #7)
//
// `meta_execute_tool` in `request_handler.rs` previously routed user-controlled
// `tool_name` straight into `ctx.execute_tool`, turning the compact-mode API
// (advertised as 5 meta-tools) into an unauthenticated control plane for the
// full ~30-tool backing registry, including `shell_execute`, `write_file`,
// every `systemd_*` mutation, and every OVS mutation.
//
// The fix is enforced HERE rather than in the handler so that *both* the
// verbose `tools/call` path and the compact `execute_tool` path are gated by
// the same check. Adding new entry points in the future automatically
// inherits the protection.
//
// A controller session (gateway-authenticated, `is_controller == true`) may
// invoke blocked tools \u2014 it represents the operator. Anonymous / regular
// sessions cannot. Response tools are always allowed because they have no
// system effect and the LLM needs them to terminate a turn.
// =============================================================================

/// Tool-name substring patterns that require controller privileges to execute.
const BLOCKED_PATTERNS: &[&str] = &[
    // Shell / arbitrary write
    "shell_execute",
    "write_file",
    // Systemd mutations
    "systemd_start",
    "systemd_stop",
    "systemd_restart",
    "systemd_reload",
    "systemd_enable",
    "systemd_disable",
    "systemd_apply",
    // OVS mutations
    "ovs_create",
    "ovs_delete",
    "ovs_add",
    "ovs_set",
    "ovs_del",
    // Plugin mutations (matches any *_apply pattern)
    "_apply",
    // Btrfs mutations
    "btrfs_create",
    "btrfs_delete",
    "btrfs_snapshot",
];

/// Tool names that are always permitted, regardless of session privilege.
/// These tools have no system side effects and are required for the LLM to
/// communicate with the user at the end of a turn.
const ALWAYS_ALLOWED: &[&str] = &[
    "respond_to_user",
    "cannot_perform",
    "request_clarification",
];

fn is_response_tool(name: &str) -> bool {
    ALWAYS_ALLOWED.contains(&name)
}

fn is_tool_blocked(name: &str) -> bool {
    if is_response_tool(name) {
        return false;
    }
    BLOCKED_PATTERNS.iter().any(|pat| name.contains(pat))
}

/// Configuration for request handling
#[derive(Debug, Clone)]
pub struct RequestConfig {
    /// Maximum tool calls per REQUEST (not session)
    pub max_turns: u32,
    /// Request timeout in seconds
    pub timeout_secs: u64,
    /// Whether to preload all tools at request start
    pub preload_all: bool,
}

impl Default for RequestConfig {
    fn default() -> Self {
        Self {
            max_turns: 75,
            timeout_secs: 300, // 5 minutes per request
            preload_all: true,
        }
    }
}

/// Per-request context that holds loaded tools
/// 
/// Created at request start, dropped at request end.
/// All tools are loaded into this context and remain available
/// for the entire duration of the request.
pub struct RequestContext {
    /// Request ID for tracking
    pub request_id: String,
    /// Session ID for auth continuity
    pub session_id: Option<String>,
    /// Is this a controller (you/chatbot) with full access
    pub is_controller: bool,
    /// WireGuard peer public key (if authenticated)
    pub peer_pubkey: Option<String>,
    /// When request started
    pub started_at: Instant,
    /// Configuration
    pub config: RequestConfig,
    /// Loaded tools (owned for this request)
    tools: HashMap<String, BoxedTool>,
    /// Tool definitions (for list/search)
    definitions: HashMap<String, ToolDefinition>,
    /// Turn counter for this request
    turn_count: AtomicU32,
    /// Request-scoped variables
    variables: RwLock<HashMap<String, Value>>,
}

impl RequestContext {
    /// Create a new request context
    pub fn new(request_id: String, config: RequestConfig) -> Self {
        info!(request_id = %request_id, "Creating request context");
        Self {
            request_id,
            session_id: None,
            is_controller: false,
            peer_pubkey: None,
            started_at: Instant::now(),
            config,
            tools: HashMap::new(),
            definitions: HashMap::new(),
            turn_count: AtomicU32::new(0),
            variables: RwLock::new(HashMap::new()),
        }
    }

    /// Create with session info (from gateway auth)
    pub fn with_session(
        request_id: String,
        config: RequestConfig,
        session_id: String,
        is_controller: bool,
        peer_pubkey: Option<String>,
    ) -> Self {
        info!(
            request_id = %request_id,
            session_id = %session_id,
            is_controller = %is_controller,
            "Creating authenticated request context"
        );
        Self {
            request_id,
            session_id: Some(session_id),
            is_controller,
            peer_pubkey,
            started_at: Instant::now(),
            config,
            tools: HashMap::new(),
            definitions: HashMap::new(),
            turn_count: AtomicU32::new(0),
            variables: RwLock::new(HashMap::new()),
        }
    }

    /// Check if caller can access controller-only tools
    pub fn can_access_controller_tools(&self) -> bool {
        self.is_controller
    }

    /// Check if caller has any valid session
    pub fn is_authenticated(&self) -> bool {
        self.session_id.is_some()
    }

    /// Load a tool into this request context
    pub fn load_tool(&mut self, tool: BoxedTool) {
        let name = tool.name().to_string();
        let definition = ToolDefinition {
            name: name.clone(),
            description: tool.description().to_string(),
            input_schema: tool.input_schema(),
            category: tool.category().to_string(),
            tags: tool.tags(),
        };
        
        self.tools.insert(name.clone(), tool);
        self.definitions.insert(name.clone(), definition);
        debug!("Loaded tool into request context: {}", name);
    }

    /// Load all tools from a factory function
    pub async fn load_all_tools<F, Fut>(&mut self, factory: F) -> Result<usize>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<Vec<BoxedTool>>>,
    {
        let tools = factory().await?;
        let count = tools.len();
        
        for tool in tools {
            self.load_tool(tool);
        }
        
        info!(
            request_id = %self.request_id,
            tool_count = count,
            "Loaded all tools for request"
        );
        
        Ok(count)
    }

    /// Get current turn count
    pub fn turn_count(&self) -> u32 {
        self.turn_count.load(Ordering::Relaxed)
    }

    /// Increment turn count and check limit
    /// Returns Err if max_turns exceeded
    pub fn increment_turn(&self) -> Result<u32, TurnLimitError> {
        let current = self.turn_count.fetch_add(1, Ordering::Relaxed) + 1;
        
        if current > self.config.max_turns {
            warn!(
                request_id = %self.request_id,
                current = current,
                max = self.config.max_turns,
                "Turn limit exceeded"
            );
            return Err(TurnLimitError {
                current,
                max: self.config.max_turns,
            });
        }
        
        debug!(
            request_id = %self.request_id,
            turn = current,
            remaining = self.config.max_turns - current,
            "Turn {} of {}",
            current,
            self.config.max_turns
        );
        
        Ok(current)
    }

    /// Check if request has timed out
    pub fn is_timed_out(&self) -> bool {
        self.started_at.elapsed().as_secs() > self.config.timeout_secs
    }

    /// Get remaining turns
    pub fn remaining_turns(&self) -> u32 {
        self.config.max_turns.saturating_sub(self.turn_count())
    }

    /// Get a tool by name
    pub fn get_tool(&self, name: &str) -> Option<&BoxedTool> {
        self.tools.get(name)
    }

    /// Get tool definition
    pub fn get_definition(&self, name: &str) -> Option<&ToolDefinition> {
        self.definitions.get(name)
    }

    /// Execute a tool.
    ///
    /// This is the **single choke point** for all tool execution in compact
    /// mode. Both the verbose `tools/call` path and the compact-mode
    /// `execute_tool` meta-tool route through here, so the security check
    /// below covers both.
    pub async fn execute_tool(&self, name: &str, input: Value) -> Result<Value> {
        // -----------------------------------------------------------------
        // SECURITY GATE (audit item #7)
        // -----------------------------------------------------------------
        // Reject blocked tools unless the session is an authenticated
        // controller (i.e. the operator's session, validated by the gateway).
        if is_tool_blocked(name) {
            if !self.is_controller {
                warn!(
                    request_id = %self.request_id,
                    session_id = ?self.session_id,
                    tool = %name,
                    "Rejected blocked tool: non-controller session"
                );
                anyhow::bail!(
                    "Tool '{}' is restricted to controller sessions and cannot be invoked from compact mode",
                    name
                );
            }
            // Controller is allowed, but we still log every privileged
            // invocation so the audit trail records it.
            info!(
                request_id = %self.request_id,
                session_id = ?self.session_id,
                tool = %name,
                "Controller session invoking privileged tool"
            );
        }

        // Check turn limit
        self.increment_turn()?;
        
        // Check timeout
        if self.is_timed_out() {
            anyhow::bail!("Request timed out after {} seconds", self.config.timeout_secs);
        }
        
        // Get and execute tool
        let tool = self.tools.get(name)
            .ok_or_else(|| anyhow::anyhow!("Tool not found: {}", name))?;
        
        tool.execute(input).await
    }

    /// List all tools (paginated)
    pub fn list_tools(&self, offset: usize, limit: usize, category: Option<&str>) -> Vec<&ToolDefinition> {
        self.definitions.values()
            .filter(|d| category.map_or(true, |c| d.category == c))
            .skip(offset)
            .take(limit)
            .collect()
    }

    /// Search tools
    pub fn search_tools(&self, query: &str) -> Vec<&ToolDefinition> {
        let query_lower = query.to_lowercase();
        
        self.definitions.values()
            .filter(|d| {
                d.name.to_lowercase().contains(&query_lower) ||
                d.description.to_lowercase().contains(&query_lower) ||
                d.category.to_lowercase().contains(&query_lower)
            })
            .take(50)
            .collect()
    }

    /// Total tool count
    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }

    /// Set a request-scoped variable
    pub async fn set_variable(&self, key: &str, value: Value) {
        self.variables.write().await.insert(key.to_string(), value);
    }

    /// Get a request-scoped variable
    pub async fn get_variable(&self, key: &str) -> Option<Value> {
        self.variables.read().await.get(key).cloned()
    }

    /// Get elapsed time
    pub fn elapsed_secs(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }

    /// Get summary for logging
    pub fn summary(&self) -> RequestSummary {
        RequestSummary {
            request_id: self.request_id.clone(),
            tools_loaded: self.tools.len(),
            turns_used: self.turn_count(),
            max_turns: self.config.max_turns,
            elapsed_secs: self.elapsed_secs(),
        }
    }
}

impl Drop for RequestContext {
    fn drop(&mut self) {
        info!(
            request_id = %self.request_id,
            tools_loaded = self.tools.len(),
            turns_used = self.turn_count(),
            elapsed_secs = self.elapsed_secs(),
            "Request context dropped, unloading {} tools",
            self.tools.len()
        );
        // Tools are automatically dropped here, freeing memory
    }
}

/// Error when turn limit is exceeded
#[derive(Debug, Clone)]
pub struct TurnLimitError {
    pub current: u32,
    pub max: u32,
}

impl std::fmt::Display for TurnLimitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Turn limit exceeded: {} of {} maximum tool calls used",
            self.current, self.max
        )
    }
}

impl std::error::Error for TurnLimitError {}

impl From<TurnLimitError> for anyhow::Error {
    fn from(e: TurnLimitError) -> Self {
        anyhow::anyhow!(e.to_string())
    }
}

/// Request summary for logging/metrics
#[derive(Debug, Clone)]
pub struct RequestSummary {
    pub request_id: String,
    pub tools_loaded: usize,
    pub turns_used: u32,
    pub max_turns: u32,
    pub elapsed_secs: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use simd_json::json;

    // --- Test helpers -----------------------------------------------------

    struct DummyTool {
        name: &'static str,
    }

    #[async_trait]
    impl Tool for DummyTool {
        fn name(&self) -> &str {
            self.name
        }
        fn description(&self) -> &str {
            "dummy"
        }
        fn input_schema(&self) -> Value {
            json!({})
        }
        async fn execute(&self, _input: Value) -> Result<Value> {
            Ok(json!({ "ok": true }))
        }
    }

    fn ctx(is_controller: bool) -> RequestContext {
        let mut ctx = RequestContext::with_session(
            "req-1".to_string(),
            RequestConfig::default(),
            "sess-1".to_string(),
            is_controller,
            None,
        );
        ctx.load_tool(Box::new(DummyTool { name: "shell_execute" }));
        ctx.load_tool(Box::new(DummyTool { name: "systemd_start_unit" }));
        ctx.load_tool(Box::new(DummyTool { name: "ovs_list_bridges" }));
        ctx.load_tool(Box::new(DummyTool { name: "respond_to_user" }));
        ctx
    }

    // --- Tests -------------------------------------------------------------

    #[test]
    fn test_turn_limit() {
        let config = RequestConfig {
            max_turns: 3,
            ..Default::default()
        };
        let ctx = RequestContext::new("test".to_string(), config);
        
        assert!(ctx.increment_turn().is_ok()); // 1
        assert!(ctx.increment_turn().is_ok()); // 2
        assert!(ctx.increment_turn().is_ok()); // 3
        assert!(ctx.increment_turn().is_err()); // 4 - exceeds limit
    }

    #[test]
    fn test_remaining_turns() {
        let config = RequestConfig {
            max_turns: 10,
            ..Default::default()
        };
        let ctx = RequestContext::new("test".to_string(), config);
        
        assert_eq!(ctx.remaining_turns(), 10);
        ctx.increment_turn().unwrap();
        assert_eq!(ctx.remaining_turns(), 9);
    }

    #[test]
    fn blocklist_classification_matches_audit_intent() {
        // Blocked:
        assert!(is_tool_blocked("shell_execute"));
        assert!(is_tool_blocked("write_file"));
        assert!(is_tool_blocked("systemd_start_unit"));
        assert!(is_tool_blocked("systemd_restart_unit"));
        assert!(is_tool_blocked("ovs_create_bridge"));
        assert!(is_tool_blocked("ovs_del_port"));
        assert!(is_tool_blocked("plugin_systemd_apply"));
        assert!(is_tool_blocked("btrfs_snapshot"));

        // Allowed:
        assert!(!is_tool_blocked("systemd_list_units"));
        assert!(!is_tool_blocked("systemd_unit_status"));
        assert!(!is_tool_blocked("ovs_list_bridges"));
        assert!(!is_tool_blocked("ovs_list_ports"));
        assert!(!is_tool_blocked("ovs_dump_flows"));
        assert!(!is_tool_blocked("read_file"));
        assert!(!is_tool_blocked("plugin_systemd_query"));

        // Response tools always allowed even though name structure could otherwise trip a pattern:
        assert!(!is_tool_blocked("respond_to_user"));
        assert!(!is_tool_blocked("cannot_perform"));
        assert!(!is_tool_blocked("request_clarification"));
    }

    #[tokio::test]
    async fn blocks_shell_execute_for_non_controller() {
        let c = ctx(/* is_controller */ false);
        let err = c.execute_tool("shell_execute", json!({}))
            .await
            .expect_err("non-controller must be blocked");
        let msg = err.to_string();
        assert!(msg.contains("restricted to controller sessions"), "got: {}", msg);
    }

    #[tokio::test]
    async fn blocks_systemd_mutation_for_non_controller() {
        let c = ctx(false);
        let err = c.execute_tool("systemd_start_unit", json!({}))
            .await
            .expect_err("non-controller must be blocked");
        assert!(err.to_string().contains("restricted to controller sessions"));
    }

    #[tokio::test]
    async fn allows_shell_execute_for_controller() {
        let c = ctx(true);
        let res = c.execute_tool("shell_execute", json!({})).await;
        assert!(res.is_ok(), "controller must be allowed, got: {:?}", res);
    }

    #[tokio::test]
    async fn allows_read_only_tool_for_non_controller() {
        let c = ctx(false);
        let res = c.execute_tool("ovs_list_bridges", json!({})).await;
        assert!(res.is_ok(), "read-only must always be allowed, got: {:?}", res);
    }

    #[tokio::test]
    async fn response_tools_always_allowed() {
        let c = ctx(false);
        let res = c.execute_tool("respond_to_user", json!({})).await;
        assert!(res.is_ok(), "respond_to_user must always be allowed, got: {:?}", res);
    }
}
