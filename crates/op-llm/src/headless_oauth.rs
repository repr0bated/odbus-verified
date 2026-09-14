//! Headless OAuth Token Provider
//!
//! Loads OAuth tokens saved by the Antigravity headless service.
//! Token is captured when user logs into Antigravity via VNC.
//!
//! ## Token Flow
//!
//! 1. `antigravity-display.service` runs Antigravity IDE in virtual Wayland
//! 2. User connects via VNC and logs in with Google account
//! 3. `antigravity-extract-token.sh` copies token to standard location
//! 4. This provider loads and auto-refreshes the token
//!
//! ## Token Location
//!
//! Default: `~/.config/antigravity/token.json`
//! Override: `GOOGLE_AUTH_TOKEN_FILE` environment variable

use anyhow::{Context, Result};
use dirs;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{debug, info, warn};

/// Google OAuth token endpoints
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
#[allow(dead_code)]
const GOOGLE_USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v2/userinfo";

/// Refresh 5 minutes before expiry
const REFRESH_BUFFER_SECS: u64 = 300;

/// OAuth token from Antigravity headless service
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthToken {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub token_type: Option<String>,
    #[serde(default)]
    pub expires_in: Option<u64>,
    #[serde(default)]
    pub expires_at: Option<f64>,
    #[serde(default)]
    pub expiry: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub saved_at: Option<f64>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub client_secret: Option<String>,
}

impl OAuthToken {
    /// Check if token is expired or will expire soon
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64();
            return now > (expires_at - REFRESH_BUFFER_SECS as f64);
        }

        // No expiry info = assume valid (rely on API to reject)
        false
    }

    /// Get remaining validity in seconds
    pub fn remaining_secs(&self) -> Option<i64> {
        self.expires_at.map(|expires_at| {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64();
            (expires_at - now) as i64
        })
    }
}

/// Cached token with load time
#[derive(Debug, Clone)]
struct CachedToken {
    token: OAuthToken,
    #[allow(dead_code)]
    loaded_at: std::time::SystemTime,
}

/// Headless OAuth provider
///
/// Loads tokens captured from Antigravity headless service.
#[derive(Debug)]
pub struct HeadlessOAuthProvider {
    /// Path to token file
    token_file: PathBuf,
    /// OAuth client ID (for refresh)
    client_id: String,
    /// OAuth client secret (for refresh)
    client_secret: String,
    /// Cached token
    cached_token: RwLock<Option<CachedToken>>,
    /// HTTP client
    client: Client,
}

impl HeadlessOAuthProvider {
    /// Create from environment variables
    ///
    /// Looks for token at:
    /// 1. `GOOGLE_AUTH_TOKEN_FILE` environment variable
    /// 2. `~/.config/antigravity/token.json` (default)
    /// 3. `~/.config/gcloud/application_default_credentials.json` (fallback)
    pub fn from_env() -> Result<Self> {
        let token_file = std::env::var("GOOGLE_AUTH_TOKEN_FILE")
            .map(PathBuf::from)
            .or_else(|_| {
                // Default location
                dirs::config_dir()
                    .map(|d| d.join("antigravity").join("token.json"))
                    .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))
            })
            .or_else(|_| {
                // Fallback to gcloud ADC
                dirs::config_dir()
                    .map(|d| {
                        d.join("gcloud")
                            .join("application_default_credentials.json")
                    })
                    .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))
            })?;

        let client_id = std::env::var("GOOGLE_CLIENT_ID").unwrap_or_default();
        let client_secret = std::env::var("GOOGLE_CLIENT_SECRET").unwrap_or_default();

        Ok(Self::new(token_file, client_id, client_secret))
    }

    /// Create with explicit configuration
    pub fn new(
        token_file: impl Into<PathBuf>,
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
    ) -> Self {
        Self {
            token_file: token_file.into(),
            client_id: client_id.into(),
            client_secret: client_secret.into(),
            cached_token: RwLock::new(None),
            client: Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
        }
    }

    /// Get valid access token, refreshing if needed
    pub async fn get_token(&self) -> Result<String> {
        // Check cache
        {
            let cache = self.cached_token.read().unwrap();
            if let Some(ref cached) = *cache {
                if !cached.token.is_expired() {
                    debug!("Using cached OAuth token");
                    return Ok(cached.token.access_token.clone());
                }
            }
        }

        // Load from file
        let mut token = self.load_token().await?;

        // Refresh if expired
        if token.is_expired() {
            if let Some(ref refresh_token) = token.refresh_token {
                let client_id = if self.client_id.is_empty() {
                    token.client_id.clone().unwrap_or_default()
                } else {
                    self.client_id.clone()
                };

                let client_secret = if self.client_secret.is_empty() {
                    token.client_secret.clone().unwrap_or_default()
                } else {
                    self.client_secret.clone()
                };

                if !client_id.is_empty() {
                    info!("Token expired, refreshing...");
                    match self
                        .refresh_token(refresh_token, &client_id, &client_secret)
                        .await
                    {
                        Ok(new_token) => {
                            token = new_token;
                            if let Err(e) = self.save_token(&token).await {
                                warn!("Failed to save refreshed token: {}", e);
                            }
                        }
                        Err(e) => {
                            warn!("Token refresh failed: {}", e);
                        }
                    }
                } else {
                    warn!("Token expired, no client_id for refresh");
                }
            } else {
                warn!("Token expired, no refresh_token available");
            }
        }

        // Cache
        {
            let mut cache = self.cached_token.write().unwrap();
            *cache = Some(CachedToken {
                token: token.clone(),
                loaded_at: SystemTime::now(),
            });
        }

        Ok(token.access_token)
    }

    /// Check if token file exists and has valid token
    pub fn is_authenticated(&self) -> bool {
        if !self.token_file.exists() {
            return false;
        }

        if let Ok(contents) = std::fs::read_to_string(&self.token_file) {
            let mut contents_mut = contents;
            if let Ok(token) = unsafe { simd_json::from_str::<OAuthToken>(&mut contents_mut) } {
                return token.refresh_token.is_some() || !token.is_expired();
            }
        }

        false
    }

    /// Get token file path
    pub fn token_file(&self) -> &Path {
        &self.token_file
    }

    async fn load_token(&self) -> Result<OAuthToken> {
        let contents = tokio::fs::read_to_string(&self.token_file)
            .await
            .with_context(|| format!("Token file not found: {}\n\nTo authenticate:\n1. Start Antigravity headless: sudo systemctl start antigravity-display\n2. Connect via VNC: vncviewer localhost:5900\n3. Log in with Google account\n4. Run: ./scripts/antigravity-extract-token.sh", self.token_file.display()))?;

        let mut contents_mut = contents;
        let token: OAuthToken =
            unsafe { simd_json::from_str(&mut contents_mut) }.context("Invalid token JSON")?;

        if let Some(remaining) = token.remaining_secs() {
            debug!("Loaded token, expires in {}s", remaining);
        }

        Ok(token)
    }

    async fn save_token(&self, token: &OAuthToken) -> Result<()> {
        let contents = simd_json::to_string_pretty(token)?;
        tokio::fs::write(&self.token_file, contents).await?;
        Ok(())
    }

    async fn refresh_token(
        &self,
        refresh_token: &str,
        client_id: &str,
        client_secret: &str,
    ) -> Result<OAuthToken> {
        let response = self
            .client
            .post(GOOGLE_TOKEN_URL)
            .form(&[
                ("client_id", client_id),
                ("client_secret", client_secret),
                ("refresh_token", refresh_token),
                ("grant_type", "refresh_token"),
            ])
            .send()
            .await
            .context("Refresh request failed")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Token refresh failed {}: {}", status, body);
        }

        let mut new_token: OAuthToken =
            response.json().await.context("Invalid refresh response")?;

        // Preserve fields
        if new_token.refresh_token.is_none() {
            new_token.refresh_token = Some(refresh_token.to_string());
        }
        new_token.client_id = Some(client_id.to_string());
        new_token.client_secret = Some(client_secret.to_string());

        // Calculate expiry
        if let Some(expires_in) = new_token.expires_in {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64();
            new_token.expires_at = Some(now + expires_in as f64);
            new_token.saved_at = Some(now);
        }

        info!("Token refreshed successfully");
        Ok(new_token)
    }
}

impl Default for HeadlessOAuthProvider {
    fn default() -> Self {
        Self::from_env().unwrap_or_else(|_| {
            Self::new(
                dirs::config_dir()
                    .map(|d| d.join("antigravity").join("token.json"))
                    .unwrap_or_else(|| PathBuf::from("/tmp/antigravity-token.json")),
                "",
                "",
            )
        })
    }
}
