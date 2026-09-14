//! Security Module for Tool Execution - Access Level Control
//!
//! This module provides security controls based on ACCESS LEVELS, not command blocking.
//! The chatbot is designed to be a full system administrator, so it needs full access.
//!
//! ## Philosophy
//!
//! Security is enforced at the ACCESS level:
//! - **Who** can use the chatbot (authentication)
//! - **What** is logged (audit trail)
//! - **How fast** they can execute (rate limiting)
//! - **Anti-hallucination** (LLM must actually do what it claims)
//!
//! NOT at the command level - that would defeat the purpose of an admin chatbot.
//!
//! ## Access Levels
//!
//! - `Unrestricted`: Full admin access - can run any command (default)
//! - `Restricted`: Limited read-only access for untrusted users
//! - `Custom`: User-defined access with specific allowlist
//!
//! ## Native Protocol Preference
//!
//! We PREFER native protocols (D-Bus, OVSDB, rtnetlink) over shell commands because:
//! - Better error handling
//! - Structured responses
//! - No parsing issues
//!
//! But we don't BLOCK shell commands - the admin chatbot needs full access.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::info;

// ============================================================================
// SECURITY ERRORS
// ============================================================================

/// Security-related errors
#[derive(Debug, Error)]
pub enum SecurityError {
    #[error("Access denied: {0}")]
    AccessDenied(String),

    #[error("Rate limit exceeded: {0}")]
    RateLimitExceeded(String),

    #[error("Path '{0}' is forbidden for this access level")]
    PathForbidden(PathBuf),

    #[error("Path traversal detected in '{0}'")]
    PathTraversal(String),

    #[error("Input validation failed: {0}")]
    ValidationFailed(String),

    #[error("Operation requires higher access level")]
    InsufficientAccess,

    #[error("Session not authenticated")]
    NotAuthenticated,
}

// ============================================================================
// ACCESS LEVELS
// ============================================================================

/// Access level for a session
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AccessLevel {
    /// Full admin access - can run any command
    /// This is the DEFAULT for authenticated admin users
    #[default]
    Unrestricted,

    /// Limited access - read-only safe commands only
    /// For untrusted/guest users
    Restricted,

    /// Custom access level with specific permissions
    Custom,
}

impl AccessLevel {
    /// Check if this level can execute shell commands
    pub fn can_execute_shell(&self) -> bool {
        matches!(self, AccessLevel::Unrestricted | AccessLevel::Custom)
    }

    /// Check if this level can write files
    pub fn can_write_files(&self) -> bool {
        matches!(self, AccessLevel::Unrestricted | AccessLevel::Custom)
    }

    /// Check if this level can manage system services
    pub fn can_manage_services(&self) -> bool {
        matches!(self, AccessLevel::Unrestricted)
    }
}

// ============================================================================
// SECURITY PROFILE - ACCESS LEVEL BASED
// ============================================================================

/// Security profile for tool execution based on access level
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSecurityProfile {
    /// Name of this profile
    pub name: String,

    /// Access level
    pub access_level: AccessLevel,

    /// For Custom level: specific commands allowed
    #[serde(default)]
    pub custom_allowed_commands: Option<HashSet<String>>,

    /// Paths that are always forbidden (even for Unrestricted)
    /// Only the most critical system files
    #[serde(default)]
    pub critical_forbidden_paths: Vec<PathBuf>,

    /// Maximum command execution time in seconds
    #[serde(default = "default_max_timeout")]
    pub max_timeout_secs: u64,

    /// Maximum output size in bytes
    #[serde(default = "default_max_output")]
    pub max_output_bytes: usize,

    /// Rate limit: max executions per minute per session
    #[serde(default = "default_rate_limit")]
    pub rate_limit_per_minute: u32,

    /// Whether to log commands (for audit)
    #[serde(default = "default_true")]
    pub audit_logging: bool,

    /// Whether to warn about native protocol alternatives
    #[serde(default = "default_true")]
    pub warn_on_cli_alternatives: bool,
}

fn default_max_timeout() -> u64 {
    300
} // 5 minutes for admin tasks
fn default_max_output() -> usize {
    10_000_000
} // 10MB for large outputs
fn default_rate_limit() -> u32 {
    120
} // 2 per second average
fn default_true() -> bool {
    true
}

impl Default for ToolSecurityProfile {
    fn default() -> Self {
        Self::admin()
    }
}

impl ToolSecurityProfile {
    /// Create an admin profile with FULL access
    /// This is the default for authenticated administrators
    pub fn admin() -> Self {
        Self {
            name: "admin".to_string(),
            access_level: AccessLevel::Unrestricted,
            custom_allowed_commands: None,
            critical_forbidden_paths: vec![
                // Only truly critical paths that could break the system
                // Even admins should use proper tools for these
            ],
            max_timeout_secs: 300,        // 5 minutes
            max_output_bytes: 10_000_000, // 10MB
            rate_limit_per_minute: 120,
            audit_logging: true,
            warn_on_cli_alternatives: true,
        }
    }

    /// Create a restricted profile for untrusted users
    /// Read-only access to safe commands
    pub fn restricted() -> Self {
        Self {
            name: "restricted".to_string(),
            access_level: AccessLevel::Restricted,
            custom_allowed_commands: Some(
                [
                    "ls", "cat", "head", "tail", "grep", "find", "ps", "df", "free", "date",
                    "uptime",
                ]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            ),
            critical_forbidden_paths: vec![
                PathBuf::from("/etc/shadow"),
                PathBuf::from("/etc/sudoers"),
                PathBuf::from("/root"),
            ],
            max_timeout_secs: 30,
            max_output_bytes: 100_000, // 100KB
            rate_limit_per_minute: 30,
            audit_logging: true,
            warn_on_cli_alternatives: false,
        }
    }

    /// Create a custom profile
    pub fn custom(name: &str, allowed_commands: Vec<&str>) -> Self {
        Self {
            name: name.to_string(),
            access_level: AccessLevel::Custom,
            custom_allowed_commands: Some(allowed_commands.iter().map(|s| s.to_string()).collect()),
            ..Self::admin()
        }
    }
}

// ============================================================================
// NATIVE PROTOCOL RECOMMENDATIONS
// ============================================================================

/// Commands that have native protocol alternatives
/// We RECOMMEND using native tools but don't BLOCK the CLI
pub const NATIVE_ALTERNATIVES: &[(&str, &str)] = &[
    // OVS
    (
        "ovs-vsctl",
        "Consider using ovs_* native tools for better error handling",
    ),
    (
        "ovs-ofctl",
        "Consider using ovs_* native tools for structured responses",
    ),
    // Systemd
    (
        "systemctl",
        "Consider using dbus_systemd_* tools for programmatic access",
    ),
    (
        "journalctl",
        "Consider using dbus_systemd_* tools for structured logs",
    ),
    // Network
    (
        "ip",
        "Consider using network_* native tools for structured output",
    ),
    ("nmcli", "Consider using D-Bus NetworkManager interface"),
    // Package managers
    (
        "apt",
        "Consider using packagekit_* tools for progress tracking",
    ),
    ("apt-get", "Consider using packagekit_* tools"),
    ("dnf", "Consider using packagekit_* tools"),
];

/// Get a recommendation message if a native alternative exists
pub fn get_native_recommendation(command: &str) -> Option<&'static str> {
    let base_cmd = command.split_whitespace().next()?;
    NATIVE_ALTERNATIVES
        .iter()
        .find(|(cmd, _)| *cmd == base_cmd)
        .map(|(_, msg)| *msg)
}

// ============================================================================
// SECURITY VALIDATOR
// ============================================================================

/// Security validator for access-level based security
#[derive(Debug)]
pub struct SecurityValidator {
    profile: RwLock<ToolSecurityProfile>,
    rate_limiter: RwLock<HashMap<String, RateLimitState>>,
}

#[derive(Debug)]
struct RateLimitState {
    count: u32,
    window_start: Instant,
}

impl SecurityValidator {
    /// Create a new validator with the given profile
    pub fn new(profile: ToolSecurityProfile) -> Self {
        Self {
            profile: RwLock::new(profile),
            rate_limiter: RwLock::new(HashMap::new()),
        }
    }

    /// Create with default admin profile (FULL ACCESS)
    pub fn with_admin_profile() -> Self {
        Self::new(ToolSecurityProfile::admin())
    }

    /// Create with restricted profile
    pub fn with_restricted_profile() -> Self {
        Self::new(ToolSecurityProfile::restricted())
    }

    /// Update the security profile
    pub async fn set_profile(&self, profile: ToolSecurityProfile) {
        info!(
            profile = %profile.name,
            access_level = ?profile.access_level,
            "Security profile updated"
        );
        *self.profile.write().await = profile;
    }

    /// Get current profile
    pub async fn get_profile(&self) -> ToolSecurityProfile {
        self.profile.read().await.clone()
    }

    /// Check if a command can be executed
    /// Returns Ok(Option<warning>) - warning is a native alternative suggestion
    pub async fn check_command(&self, command: &str) -> Result<Option<String>, SecurityError> {
        let profile = self.profile.read().await;

        match profile.access_level {
            AccessLevel::Unrestricted => {
                // Full access - just check for native alternatives to warn
                let warning = if profile.warn_on_cli_alternatives {
                    get_native_recommendation(command).map(|s| s.to_string())
                } else {
                    None
                };
                Ok(warning)
            }
            AccessLevel::Restricted => {
                // Check against the restricted allowlist
                let base_cmd = command
                    .split_whitespace()
                    .next()
                    .ok_or_else(|| SecurityError::ValidationFailed("Empty command".to_string()))?;

                if let Some(allowed) = &profile.custom_allowed_commands {
                    if !allowed.contains(base_cmd) {
                        return Err(SecurityError::AccessDenied(format!(
                            "Command '{}' not allowed in restricted mode",
                            base_cmd
                        )));
                    }
                }
                Ok(None)
            }
            AccessLevel::Custom => {
                // Check against custom allowlist
                let base_cmd = command
                    .split_whitespace()
                    .next()
                    .ok_or_else(|| SecurityError::ValidationFailed("Empty command".to_string()))?;

                if let Some(allowed) = &profile.custom_allowed_commands {
                    if !allowed.contains(base_cmd) {
                        return Err(SecurityError::AccessDenied(format!(
                            "Command '{}' not in custom allowlist",
                            base_cmd
                        )));
                    }
                }
                Ok(None)
            }
        }
    }

    /// Validate a path for reading
    pub async fn validate_read_path(&self, path: &str) -> Result<PathBuf, SecurityError> {
        let profile = self.profile.read().await;
        let path_buf = PathBuf::from(path);

        // Check for path traversal
        if path.contains("..") {
            return Err(SecurityError::PathTraversal(path.to_string()));
        }

        // Check critical forbidden paths
        for forbidden in &profile.critical_forbidden_paths {
            if path_buf.starts_with(forbidden) {
                return Err(SecurityError::PathForbidden(path_buf));
            }
        }

        // Admins can read anything else
        if profile.access_level == AccessLevel::Unrestricted {
            return Ok(path_buf);
        }

        // Restricted users have limited paths
        let allowed_read = ["/tmp", "/var/log", "/home", "/opt"];
        let is_allowed = allowed_read.iter().any(|p| path_buf.starts_with(p));

        if !is_allowed {
            return Err(SecurityError::PathForbidden(path_buf));
        }

        Ok(path_buf)
    }

    /// Validate a path for writing
    pub async fn validate_write_path(&self, path: &str) -> Result<PathBuf, SecurityError> {
        let profile = self.profile.read().await;
        let path_buf = PathBuf::from(path);

        // Check for path traversal
        if path.contains("..") {
            return Err(SecurityError::PathTraversal(path.to_string()));
        }

        // Check critical forbidden paths
        for forbidden in &profile.critical_forbidden_paths {
            if path_buf.starts_with(forbidden) {
                return Err(SecurityError::PathForbidden(path_buf));
            }
        }

        // Admins can write anywhere (except critical paths)
        if profile.access_level == AccessLevel::Unrestricted {
            return Ok(path_buf);
        }

        // Restricted users can only write to /tmp
        if !path_buf.starts_with("/tmp") {
            return Err(SecurityError::PathForbidden(path_buf));
        }

        Ok(path_buf)
    }

    /// Check rate limit for a session
    pub async fn check_rate_limit(&self, session_id: &str) -> Result<(), SecurityError> {
        let profile = self.profile.read().await;
        let limit = profile.rate_limit_per_minute;
        drop(profile);

        let mut rate_limiter = self.rate_limiter.write().await;
        let now = Instant::now();

        let state = rate_limiter
            .entry(session_id.to_string())
            .or_insert(RateLimitState {
                count: 0,
                window_start: now,
            });

        // Reset if window has passed
        if now.duration_since(state.window_start) > Duration::from_secs(60) {
            state.count = 0;
            state.window_start = now;
        }

        if state.count >= limit {
            return Err(SecurityError::RateLimitExceeded(format!(
                "Exceeded {} executions per minute",
                limit
            )));
        }

        state.count += 1;
        Ok(())
    }

    /// Get maximum allowed timeout
    pub async fn max_timeout(&self) -> Duration {
        Duration::from_secs(self.profile.read().await.max_timeout_secs)
    }

    /// Get maximum output size
    pub async fn max_output(&self) -> usize {
        self.profile.read().await.max_output_bytes
    }

    /// Check if audit logging is enabled
    pub async fn is_audit_enabled(&self) -> bool {
        self.profile.read().await.audit_logging
    }

    /// Clear rate limit state
    pub async fn clear_rate_limits(&self) {
        self.rate_limiter.write().await.clear();
    }
}

impl Default for SecurityValidator {
    fn default() -> Self {
        // Default to FULL ADMIN access
        Self::with_admin_profile()
    }
}

// ============================================================================
// GLOBAL VALIDATOR INSTANCE
// ============================================================================

// Global security validator instance (initialized eagerly)
static SECURITY_VALIDATOR: std::sync::OnceLock<Arc<SecurityValidator>> = std::sync::OnceLock::new();

/// Initialize the global security validator (call once at startup)
pub fn init_security_validator() {
    SECURITY_VALIDATOR
        .set(Arc::new(SecurityValidator::with_admin_profile()))
        .unwrap_or_else(|_| panic!("Security validator already initialized"));
}

/// Get the global security validator
#[cfg(test)]
pub fn get_security_validator() -> Arc<SecurityValidator> {
    // Unit tests exercise tools directly without running the application
    // bootstrap that initializes the process-global validator.
    SECURITY_VALIDATOR
        .get_or_init(|| Arc::new(SecurityValidator::with_admin_profile()))
        .clone()
}

/// Get the global security validator
#[cfg(not(test))]
pub fn get_security_validator() -> Arc<SecurityValidator> {
    SECURITY_VALIDATOR
        .get()
        .expect("Security validator not initialized")
        .clone()
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_admin_allows_everything() {
        let validator = SecurityValidator::with_admin_profile();

        // All commands should pass for admin
        assert!(validator.check_command("rm -rf /").await.is_ok());
        assert!(validator
            .check_command("systemctl restart sshd")
            .await
            .is_ok());
        assert!(validator
            .check_command("curl http://example.com")
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn test_admin_gets_native_warnings() {
        let validator = SecurityValidator::with_admin_profile();

        // Should get warning for ovs-vsctl
        let result = validator.check_command("ovs-vsctl add-br br0").await;
        assert!(result.is_ok());
        let warning = result.unwrap();
        assert!(warning.is_some());
        assert!(warning.unwrap().contains("native tools"));
    }

    #[tokio::test]
    async fn test_restricted_blocks_dangerous() {
        let validator = SecurityValidator::with_restricted_profile();

        // Should block rm
        assert!(validator.check_command("rm -rf /").await.is_err());

        // Should allow ls
        assert!(validator.check_command("ls -la").await.is_ok());
    }

    #[tokio::test]
    async fn test_path_validation_admin() {
        let validator = SecurityValidator::with_admin_profile();

        // Admin can read/write anywhere
        assert!(validator.validate_read_path("/etc/passwd").await.is_ok());
        assert!(validator.validate_write_path("/etc/hosts").await.is_ok());
    }

    #[tokio::test]
    async fn test_path_traversal_blocked() {
        let validator = SecurityValidator::with_admin_profile();

        // Path traversal always blocked
        assert!(validator
            .validate_read_path("/tmp/../etc/shadow")
            .await
            .is_err());
    }

    #[tokio::test]
    async fn test_rate_limiting() {
        let profile = ToolSecurityProfile {
            rate_limit_per_minute: 3,
            ..ToolSecurityProfile::admin()
        };
        let validator = SecurityValidator::new(profile);

        // First 3 should pass
        assert!(validator.check_rate_limit("session1").await.is_ok());
        assert!(validator.check_rate_limit("session1").await.is_ok());
        assert!(validator.check_rate_limit("session1").await.is_ok());

        // 4th should fail
        assert!(validator.check_rate_limit("session1").await.is_err());
    }
}
