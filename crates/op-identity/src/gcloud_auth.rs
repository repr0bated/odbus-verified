//! Google Cloud authentication for cloudaicompanion.googleapis.com
//!
//! Supports multiple token sources:
//! 1. Environment variable (GCLOUD_TOKEN)
//! 2. Cached token from antigravity-server
//! 3. gcloud CLI
//! 4. Application Default Credentials

use std::path::PathBuf;
use std::process::Command;

use chrono::{DateTime, Duration, Utc};
use tracing::{debug, info, warn};

/// OAuth scopes required for Cloud AI Companion
pub const OAUTH_SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/cloud-platform",
    "https://www.googleapis.com/auth/cloud-ide",
];
const OAUTH_SCOPES_FALLBACK: &[&str] = &["https://www.googleapis.com/auth/cloud-platform"];

fn adc_fallback_enabled() -> bool {
    std::env::var("OP_ENABLE_ADC_FALLBACK")
        .ok()
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

/// GCloud authentication provider
#[derive(Clone)]
pub struct GCloudAuth {
    /// Path to cached token from antigravity-server
    antigravity_token_path: Option<PathBuf>,
}

impl GCloudAuth {
    pub fn new() -> Self {
        // Look for antigravity token
        let antigravity_token_path = dirs::home_dir()
            .map(|h| h.join(".antigravity-server"))
            .and_then(|dir| {
                // Find any .token file in the directory
                std::fs::read_dir(&dir)
                    .ok()?
                    .filter_map(|e| e.ok())
                    .find(|e| {
                        e.path()
                            .extension()
                            .map(|ext| ext == "token")
                            .unwrap_or(false)
                    })
                    .map(|e| e.path())
            });

        if let Some(ref path) = antigravity_token_path {
            debug!("Found antigravity token at: {:?}", path);
        }

        Self {
            antigravity_token_path,
        }
    }

    /// Get a valid OAuth token and its expiration time
    pub async fn get_token(&self) -> anyhow::Result<(String, DateTime<Utc>)> {
        // Try sources in order of preference

        // 1. Environment variable (for testing)
        if let Ok(token) = std::env::var("GCLOUD_TOKEN") {
            info!("Using token from GCLOUD_TOKEN env var");
            // Assume 1 hour validity
            return Ok((token, Utc::now() + Duration::hours(1)));
        }

        // 2. Antigravity cached token
        if let Some(token) = self.try_antigravity_token().await {
            info!("Using token from antigravity cache");
            // These tokens are typically valid for 1 hour
            return Ok((token, Utc::now() + Duration::minutes(55)));
        }

        // 3. gcloud CLI
        if let Some((token, expires)) = self.try_gcloud_cli().await {
            info!("Using token from gcloud CLI");
            return Ok((token, expires));
        }

        // 4. Application Default Credentials via gcloud (opt-in).
        if adc_fallback_enabled() {
            if let Some((token, expires)) = self.try_adc().await {
                info!("Using Application Default Credentials");
                return Ok((token, expires));
            }
        } else {
            debug!("ADC fallback disabled (set OP_ENABLE_ADC_FALLBACK=1 to enable)");
        }

        anyhow::bail!(
            "Could not obtain OAuth token from GCLOUD_TOKEN, cached token file, or gcloud CLI credentials"
        )
    }

    async fn try_antigravity_token(&self) -> Option<String> {
        let path = self.antigravity_token_path.as_ref()?;

        let content = std::fs::read_to_string(path).ok()?;
        let token = content.trim().to_string();

        if token.is_empty() {
            return None;
        }

        // Basic validation - OAuth tokens start with "ya29."
        if token.starts_with("ya29.") {
            Some(token)
        } else {
            warn!("Antigravity token doesn't look like an OAuth token");
            None
        }
    }

    async fn try_gcloud_cli(&self) -> Option<(String, DateTime<Utc>)> {
        if let Some(token) = run_gcloud_access_token(&["auth", "print-access-token"], OAUTH_SCOPES)
        {
            return Some((token, Utc::now() + Duration::minutes(55)));
        }
        warn!("Preferred scopes failed; retrying gcloud CLI token with cloud-platform only");
        if let Some(token) =
            run_gcloud_access_token(&["auth", "print-access-token"], OAUTH_SCOPES_FALLBACK)
        {
            return Some((token, Utc::now() + Duration::minutes(55)));
        }
        // Final fallback: let gcloud decide default scopes.
        if let Some(token) = run_gcloud_access_token_no_scopes(&["auth", "print-access-token"]) {
            return Some((token, Utc::now() + Duration::minutes(55)));
        }
        None
    }

    async fn try_adc(&self) -> Option<(String, DateTime<Utc>)> {
        if let Some(token) = run_gcloud_access_token(
            &["auth", "application-default", "print-access-token"],
            OAUTH_SCOPES,
        ) {
            return Some((token, Utc::now() + Duration::minutes(55)));
        }
        warn!("Preferred scopes failed; retrying ADC token with cloud-platform only");
        if let Some(token) = run_gcloud_access_token(
            &["auth", "application-default", "print-access-token"],
            OAUTH_SCOPES_FALLBACK,
        ) {
            return Some((token, Utc::now() + Duration::minutes(55)));
        }
        // Final fallback: let gcloud decide default scopes.
        if let Some(token) = run_gcloud_access_token_no_scopes(&[
            "auth",
            "application-default",
            "print-access-token",
        ]) {
            return Some((token, Utc::now() + Duration::minutes(55)));
        }
        None
    }

    /// Force a token refresh via gcloud
    pub async fn refresh_token(&self) -> anyhow::Result<(String, DateTime<Utc>)> {
        if let Some(token) = run_gcloud_access_token(&["auth", "print-access-token"], OAUTH_SCOPES)
        {
            return Ok((token, Utc::now() + Duration::minutes(55)));
        }
        if let Some(token) =
            run_gcloud_access_token(&["auth", "print-access-token"], OAUTH_SCOPES_FALLBACK)
        {
            return Ok((token, Utc::now() + Duration::minutes(55)));
        }
        if let Some(token) = run_gcloud_access_token_no_scopes(&["auth", "print-access-token"]) {
            return Ok((token, Utc::now() + Duration::minutes(55)));
        }

        anyhow::bail!("gcloud auth failed for preferred, fallback, and default scope sets")
    }

    /// Check if gcloud is available and authenticated
    pub fn is_authenticated(&self) -> bool {
        if run_gcloud_access_token_no_scopes(&["auth", "print-access-token"]).is_some() {
            return true;
        }
        if adc_fallback_enabled()
            && run_gcloud_access_token_no_scopes(&[
                "auth",
                "application-default",
                "print-access-token",
            ])
            .is_some()
        {
            return true;
        }
        false
    }
}

impl Default for GCloudAuth {
    fn default() -> Self {
        Self::new()
    }
}

fn run_gcloud_access_token(base_args: &[&str], scopes: &[&str]) -> Option<String> {
    let mut args: Vec<String> = base_args.iter().map(|s| s.to_string()).collect();
    args.push(format!("--scopes={}", scopes.join(",")));

    let output = Command::new("gcloud").args(args).output().ok()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        debug!("gcloud {:?} failed: {}", base_args, stderr);
        return None;
    }

    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if token.is_empty() {
        return None;
    }
    Some(token)
}

fn run_gcloud_access_token_no_scopes(base_args: &[&str]) -> Option<String> {
    let output = Command::new("gcloud").args(base_args).output().ok()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        debug!("gcloud {:?} without scopes failed: {}", base_args, stderr);
        return None;
    }

    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if token.is_empty() {
        return None;
    }
    Some(token)
}
