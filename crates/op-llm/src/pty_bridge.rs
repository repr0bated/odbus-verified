//! PTY Authentication Bridge
//!
//! Wraps CLI tools in a pseudo-terminal to handle interactive authentication
//! flows on headless servers.
//!
//! ## Use Cases
//!
//! - Running `gemini` CLI on servers without browsers
//! - Using `gh` (GitHub CLI) device code flow
//! - Any CLI tool with interactive OAuth
//!
//! ## How It Works
//!
//! 1. Spawn the CLI in a PTY (pseudo-terminal)
//! 2. Monitor output for auth patterns (URLs, device codes, prompts)
//! 3. When auth is detected, emit notification (webhook, D-Bus signal, web UI)
//! 4. User completes auth on their device
//! 5. Bridge detects completion and continues execution

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, info, warn};

// =============================================================================
// AUTH PATTERNS
// =============================================================================

/// Patterns to detect in CLI output that indicate auth is required
const AUTH_URL_PATTERNS: &[&str] = &[
    "https://accounts.google.com",
    "https://github.com/login/device",
    "https://login.microsoftonline.com",
    "https://oauth.example.com",
    "Open this URL",
    "Visit this URL",
    "Go to",
    "authenticate at",
];

const DEVICE_CODE_PATTERNS: &[&str] = &[
    "Enter code:",
    "Your code:",
    "Device code:",
    "one-time code",
    "verification code",
];

const PROMPT_PATTERNS: &[&str] = &[
    "Press Enter",
    "press any key",
    "Password:",
    "Enter MFA",
    "2FA code",
    "OTP:",
];

// =============================================================================
// TYPES
// =============================================================================

/// Detected authentication requirement
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthRequirement {
    /// Unique ID for this auth request
    pub id: String,
    /// Type of auth detected
    pub auth_type: AuthType,
    /// URL to visit (if applicable)
    pub url: Option<String>,
    /// Device code to enter (if applicable)
    pub device_code: Option<String>,
    /// Human-readable message
    pub message: String,
    /// Timestamp when detected
    pub detected_at: i64,
    /// Whether this auth has been completed
    pub completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AuthType {
    /// Browser-based OAuth (open URL)
    BrowserOAuth,
    /// Device code flow (enter code at URL)
    DeviceCode,
    /// Interactive prompt (password, MFA, etc.)
    InteractivePrompt,
    /// Press Enter to continue
    Confirmation,
}

/// Result of executing a command through the PTY bridge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtyExecutionResult {
    /// Command exit code
    pub exit_code: i32,
    /// Captured stdout
    pub stdout: String,
    /// Captured stderr
    pub stderr: String,
    /// Whether auth was required during execution
    pub auth_required: bool,
    /// Auth details if required
    pub auth_details: Option<AuthRequirement>,
}

/// Notification handler for auth requirements
#[async_trait::async_trait]
pub trait AuthNotificationHandler: Send + Sync {
    /// Called when auth is required
    async fn notify(&self, auth: &AuthRequirement) -> Result<()>;

    /// Called when auth is completed
    async fn auth_completed(&self, auth_id: &str, success: bool) -> Result<()>;
}

// =============================================================================
// PTY BRIDGE
// =============================================================================

/// PTY Authentication Bridge
pub struct PtyAuthBridge {
    /// Pending auth requirements
    pending_auths: Arc<RwLock<HashMap<String, AuthRequirement>>>,
    /// Notification handlers
    handlers: Arc<RwLock<Vec<Arc<dyn AuthNotificationHandler>>>>,
    /// Broadcast channel for auth events
    auth_tx: broadcast::Sender<AuthRequirement>,
    /// Session store path
    _session_store: PathBuf,
}

impl PtyAuthBridge {
    /// Create a new PTY bridge
    pub fn new() -> Self {
        let (auth_tx, _) = broadcast::channel(16);

        Self {
            pending_auths: Arc::new(RwLock::new(HashMap::new())),
            handlers: Arc::new(RwLock::new(Vec::new())),
            auth_tx,
            _session_store: dirs::config_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join("pty-auth-bridge")
                .join("sessions"),
        }
    }

    /// Add a notification handler
    pub async fn add_handler(&self, handler: Arc<dyn AuthNotificationHandler>) {
        self.handlers.write().await.push(handler);
    }

    /// Subscribe to auth events
    pub fn subscribe(&self) -> broadcast::Receiver<AuthRequirement> {
        self.auth_tx.subscribe()
    }

    /// Get pending auth requirements
    pub async fn get_pending_auths(&self) -> Vec<AuthRequirement> {
        self.pending_auths.read().await.values().cloned().collect()
    }

    /// Mark an auth as completed
    pub async fn complete_auth(&self, auth_id: &str, _response: Option<&str>) -> Result<()> {
        let mut auths = self.pending_auths.write().await;
        if let Some(auth) = auths.get_mut(auth_id) {
            auth.completed = true;
            info!(auth_id = %auth_id, "Auth marked as completed");

            // Notify handlers
            let handlers = self.handlers.read().await;
            for handler in handlers.iter() {
                handler.auth_completed(auth_id, true).await.ok();
            }
        }
        Ok(())
    }

    /// Execute a command through the PTY bridge
    pub async fn execute(
        &self,
        command: &str,
        args: &[&str],
        timeout_secs: u64,
    ) -> Result<PtyExecutionResult> {
        info!(command = %command, args = ?args, "Executing via PTY bridge");

        // For now, use regular process execution with output capture
        // Full PTY implementation would use `pty` crate
        let mut cmd = Command::new(command);
        cmd.args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::piped());

        // Pass through gcloud credentials for Gemini CLI
        // Priority: GOOGLE_APPLICATION_CREDENTIALS env var > service account > ADC
        if let Ok(creds) = std::env::var("GOOGLE_APPLICATION_CREDENTIALS") {
            debug!("Using GOOGLE_APPLICATION_CREDENTIALS from env: {}", creds);
            cmd.env("GOOGLE_APPLICATION_CREDENTIALS", creds);
        } else if let Some(home) = dirs::home_dir() {
            // Try service account key first (preferred for service accounts)
            let gcloud_creds = home.join(".config/gcloud/gemini-cli.json");
            if gcloud_creds.exists() {
                let creds_path = gcloud_creds.to_string_lossy().to_string();
                cmd.env("GOOGLE_APPLICATION_CREDENTIALS", &creds_path);
                debug!("Using gcloud service account: {}", creds_path);
            } else {
                // Fall back to Application Default Credentials
                let adc_creds = home.join(".config/gcloud/application_default_credentials.json");
                if adc_creds.exists() {
                    let creds_path = adc_creds.to_string_lossy().to_string();
                    cmd.env("GOOGLE_APPLICATION_CREDENTIALS", &creds_path);
                    debug!("Using gcloud ADC: {}", creds_path);
                }
            }
        }
        // Pass through project ID if set
        if let Ok(project) = std::env::var("GOOGLE_CLOUD_PROJECT") {
            cmd.env("GOOGLE_CLOUD_PROJECT", project);
        } else if let Ok(project) = std::env::var("GCLOUD_PROJECT") {
            cmd.env("GOOGLE_CLOUD_PROJECT", project);
        }

        let mut child = cmd.spawn().context("Failed to spawn command")?;

        let stdout = child.stdout.take().expect("stdout");
        let stderr = child.stderr.take().expect("stderr");

        let mut stdout_reader = BufReader::new(stdout).lines();
        let mut stderr_reader = BufReader::new(stderr).lines();

        let mut stdout_buf = String::new();
        let mut stderr_buf = String::new();
        let mut auth_required = false;
        let mut auth_details = None;

        // Read output with timeout
        let _result = tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), async {
            loop {
                tokio::select! {
                    line = stdout_reader.next_line() => {
                        match line {
                            Ok(Some(line)) => {
                                debug!(line = %line, "stdout");
                                stdout_buf.push_str(&line);
                                stdout_buf.push('\n');

                                // Check for auth patterns
                                if let Some(auth) = self.detect_auth(&line).await {
                                    auth_required = true;
                                    auth_details = Some(auth.clone());

                                    // Notify handlers
                                    let handlers = self.handlers.read().await;
                                    for handler in handlers.iter() {
                                        handler.notify(&auth).await.ok();
                                    }

                                    // Broadcast
                                    self.auth_tx.send(auth).ok();
                                }
                            }
                            Ok(None) => break,
                            Err(e) => {
                                warn!(error = %e, "Error reading stdout");
                                break;
                            }
                        }
                    }
                    line = stderr_reader.next_line() => {
                        match line {
                            Ok(Some(line)) => {
                                debug!(line = %line, "stderr");
                                stderr_buf.push_str(&line);
                                stderr_buf.push('\n');

                                // Also check stderr for auth patterns
                                if let Some(auth) = self.detect_auth(&line).await {
                                    auth_required = true;
                                    auth_details = Some(auth);
                                }
                            }
                            Ok(None) => {}
                            Err(e) => {
                                warn!(error = %e, "Error reading stderr");
                            }
                        }
                    }
                }
            }
        })
        .await;

        let exit_code = match child.wait().await {
            Ok(status) => status.code().unwrap_or(-1),
            Err(_) => -1,
        };

        Ok(PtyExecutionResult {
            exit_code,
            stdout: stdout_buf,
            stderr: stderr_buf,
            auth_required,
            auth_details,
        })
    }

    /// Detect auth requirements in output line
    async fn detect_auth(&self, line: &str) -> Option<AuthRequirement> {
        let line_lower = line.to_lowercase();

        // Check for URLs
        for pattern in AUTH_URL_PATTERNS {
            if line.contains(pattern) {
                let url = extract_url(line);
                let auth = AuthRequirement {
                    id: uuid::Uuid::new_v4().to_string(),
                    auth_type: AuthType::BrowserOAuth,
                    url,
                    device_code: None,
                    message: line.to_string(),
                    detected_at: chrono::Utc::now().timestamp(),
                    completed: false,
                };

                // Store pending auth
                self.pending_auths
                    .write()
                    .await
                    .insert(auth.id.clone(), auth.clone());

                return Some(auth);
            }
        }

        // Check for device codes
        for pattern in DEVICE_CODE_PATTERNS {
            if line_lower.contains(&pattern.to_lowercase()) {
                let code = extract_device_code(line);
                let auth = AuthRequirement {
                    id: uuid::Uuid::new_v4().to_string(),
                    auth_type: AuthType::DeviceCode,
                    url: extract_url(line),
                    device_code: code,
                    message: line.to_string(),
                    detected_at: chrono::Utc::now().timestamp(),
                    completed: false,
                };

                self.pending_auths
                    .write()
                    .await
                    .insert(auth.id.clone(), auth.clone());
                return Some(auth);
            }
        }

        // Check for prompts
        for pattern in PROMPT_PATTERNS {
            if line_lower.contains(&pattern.to_lowercase()) {
                let auth = AuthRequirement {
                    id: uuid::Uuid::new_v4().to_string(),
                    auth_type: if line_lower.contains("enter") && line_lower.contains("continue") {
                        AuthType::Confirmation
                    } else {
                        AuthType::InteractivePrompt
                    },
                    url: None,
                    device_code: None,
                    message: line.to_string(),
                    detected_at: chrono::Utc::now().timestamp(),
                    completed: false,
                };

                self.pending_auths
                    .write()
                    .await
                    .insert(auth.id.clone(), auth.clone());
                return Some(auth);
            }
        }

        None
    }
}

impl Default for PtyAuthBridge {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// HELPERS
// =============================================================================

/// Extract URL from a line of text
fn extract_url(line: &str) -> Option<String> {
    // Simple URL extraction - find https:// and take until whitespace
    if let Some(start) = line.find("https://") {
        let rest = &line[start..];
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let url = &rest[..end];
        let url = url.trim_end_matches(|c| ['.', ',', ')', '"', '\''].contains(&c));
        return Some(url.to_string());
    }

    if let Some(start) = line.find("http://") {
        let rest = &line[start..];
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let url = &rest[..end];
        let url = url.trim_end_matches(|c| ['.', ',', ')', '"', '\''].contains(&c));
        return Some(url.to_string());
    }

    None
}

/// Extract device code from a line of text
fn extract_device_code(line: &str) -> Option<String> {
    // Look for patterns like XXXX-XXXX or similar alphanumeric codes
    for word in line.split_whitespace() {
        let clean = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '-');
        if clean.len() >= 8
            && clean.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            && clean.chars().any(|c| c.is_ascii_uppercase())
            && clean.chars().any(|c| c.is_ascii_digit() || c == '-')
        {
            return Some(clean.to_string());
        }
    }
    None
}

// =============================================================================
// NOTIFICATION HANDLERS
// =============================================================================

/// Webhook notification handler
pub struct WebhookNotificationHandler {
    url: String,
    client: reqwest::Client,
}

impl WebhookNotificationHandler {
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait::async_trait]
impl AuthNotificationHandler for WebhookNotificationHandler {
    async fn notify(&self, auth: &AuthRequirement) -> Result<()> {
        let payload = simd_json::json!({
            "event": "auth_required",
            "auth": auth
        });

        self.client
            .post(&self.url)
            .json(&payload)
            .send()
            .await
            .context("Failed to send webhook")?;

        Ok(())
    }

    async fn auth_completed(&self, auth_id: &str, success: bool) -> Result<()> {
        let payload = simd_json::json!({
            "event": "auth_completed",
            "auth_id": auth_id,
            "success": success
        });

        self.client
            .post(&self.url)
            .json(&payload)
            .send()
            .await
            .context("Failed to send webhook")?;

        Ok(())
    }
}

/// Log notification handler (for testing/debugging)
pub struct LogNotificationHandler;

#[async_trait::async_trait]
impl AuthNotificationHandler for LogNotificationHandler {
    async fn notify(&self, auth: &AuthRequirement) -> Result<()> {
        info!(
            auth_type = ?auth.auth_type,
            url = ?auth.url,
            device_code = ?auth.device_code,
            message = %auth.message,
            "🔐 AUTH REQUIRED"
        );

        if let Some(url) = &auth.url {
            eprintln!("\n╔══════════════════════════════════════════════════════════════╗");
            eprintln!("║  🔐 AUTHENTICATION REQUIRED                                    ║");
            eprintln!("╠══════════════════════════════════════════════════════════════╣");
            eprintln!("║  Visit this URL to authenticate:                              ║");
            eprintln!("║  {}  ", url);
            if let Some(code) = &auth.device_code {
                eprintln!(
                    "║  Enter code: {}                                       ",
                    code
                );
            }
            eprintln!("╚══════════════════════════════════════════════════════════════╝\n");
        }

        Ok(())
    }

    async fn auth_completed(&self, auth_id: &str, success: bool) -> Result<()> {
        info!(auth_id = %auth_id, success = %success, "Auth completed");
        Ok(())
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_url() {
        assert_eq!(
            extract_url("Please visit https://accounts.google.com/o/oauth2/auth?client_id=123"),
            Some("https://accounts.google.com/o/oauth2/auth?client_id=123".to_string())
        );

        assert_eq!(
            extract_url("Go to https://github.com/login/device and enter code"),
            Some("https://github.com/login/device".to_string())
        );

        assert_eq!(extract_url("No URL here"), None);
    }

    #[test]
    fn test_extract_device_code() {
        assert_eq!(
            extract_device_code("Enter code: ABCD-1234"),
            Some("ABCD-1234".to_string())
        );

        assert_eq!(
            extract_device_code("Your one-time code is WXYZ5678"),
            Some("WXYZ5678".to_string())
        );
    }

    #[tokio::test]
    async fn test_pty_bridge_simple_command() {
        let bridge = PtyAuthBridge::new();
        bridge.add_handler(Arc::new(LogNotificationHandler)).await;

        let result = bridge.execute("echo", &["hello"], 10).await.unwrap();

        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("hello"));
        assert!(!result.auth_required);
    }
}
