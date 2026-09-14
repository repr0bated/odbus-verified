//! Privacy Router User Storage — CozoDB backend.
//!
//! No JSON flat file, no drift. Desired state = CozoDB graph.
//! Magic links remain ephemeral (in-memory only).

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use op_cozo_store::{CozoGraphShuttle, DataValue, Num};
use op_identity::generate_magic_link_token;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::RwLock;
use tracing::{info, warn};

/// A privacy router user account
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyUser {
    pub id: String,
    pub email: String,
    pub email_verified: bool,
    pub created_at: DateTime<Utc>,
    pub wg_public_key: String,
    pub wg_private_key_encrypted: String,
    pub assigned_ip: String,
    /// Allocated privacy network quota in bytes (default: 1 GiB)
    #[serde(default = "default_privacy_quota_bytes")]
    pub privacy_quota_bytes: u64,
    /// Used privacy network quota in bytes
    #[serde(default)]
    pub privacy_quota_used_bytes: u64,
    /// Provisioned per-user privacy container name (if any)
    #[serde(default)]
    pub privacy_container_name: Option<String>,
    /// Stable published privacy route object identifier
    #[serde(default)]
    pub privacy_route_id: Option<String>,
    /// True when a user container has been provisioned and started
    #[serde(default)]
    pub privacy_network_connected: bool,
    /// Timestamp when user was connected to the privacy network
    #[serde(default)]
    pub privacy_network_connected_at: Option<DateTime<Utc>>,
    /// Google OAuth identity (optional)
    pub google_id: Option<String>,
    pub google_email: Option<String>,
    /// User API credentials for AI services
    pub api_credentials: Option<UserApiCredentials>,
}

/// User-specific API credentials for AI services
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserApiCredentials {
    pub token: String,
    pub gemini_api_key: Option<String>,
    pub anthropic_api_key: Option<String>,
    pub openai_api_key: Option<String>,
    pub preferred_provider: Option<String>,
}

/// A magic link for email verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MagicLink {
    pub token: String,
    pub user_id: String,
    pub expires_at: DateTime<Utc>,
}

/// User storage backed by CozoDB — no JSON, no drift.
pub struct UserStore {
    cozo: CozoGraphShuttle,
    magic_links: RwLock<HashMap<String, MagicLink>>,
    next_ip: RwLock<u8>, // Last octet for IP assignment (10.100.0.x)
}

fn default_privacy_quota_bytes() -> u64 {
    1024 * 1024 * 1024 // 1 GiB
}

/// Extract &str from DataValue
fn dv_str(dv: &DataValue) -> Option<&str> {
    if let DataValue::Str(s) = dv {
        Some(s.as_str())
    } else {
        None
    }
}

fn dv_bool(dv: &DataValue) -> bool {
    dv_str(dv) == Some("true")
}

fn dv_int(dv: &DataValue) -> Option<i64> {
    if let DataValue::Num(Num::Int(i)) = dv {
        Some(*i)
    } else {
        None
    }
}

/// Parse a PrivacyUser from a CozoDB row vector.
/// Row order must match the query column order in get_privacy_user / list_privacy_users.
fn parse_user_row(id: &str, row: &[DataValue]) -> Option<PrivacyUser> {
    if row.len() < 15 {
        return None;
    }
    let api_json = dv_str(&row[13]).unwrap_or("null");
    let api_credentials = if api_json == "null" || api_json.is_empty() {
        None
    } else {
        serde_json::from_str(api_json).ok()
    };

    Some(PrivacyUser {
        id: id.to_string(),
        email: dv_str(&row[0])?.to_string(),
        email_verified: dv_bool(&row[1]),
        wg_public_key: dv_str(&row[2])?.to_string(),
        wg_private_key_encrypted: dv_str(&row[3])?.to_string(),
        assigned_ip: dv_str(&row[4])?.to_string(),
        privacy_quota_bytes: dv_int(&row[5]).unwrap_or(1_073_741_824) as u64,
        privacy_quota_used_bytes: dv_int(&row[6]).unwrap_or(0) as u64,
        privacy_container_name: {
            let s = dv_str(&row[7])?;
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        },
        privacy_route_id: {
            let s = dv_str(&row[8])?;
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        },
        privacy_network_connected: dv_bool(&row[9]),
        privacy_network_connected_at: {
            let s = dv_str(&row[10])?;
            if s.is_empty() {
                None
            } else {
                DateTime::parse_from_rfc3339(s)
                    .ok()
                    .map(|d| d.with_timezone(&Utc))
            }
        },
        google_id: {
            let s = dv_str(&row[11])?;
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        },
        google_email: {
            let s = dv_str(&row[12])?;
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        },
        api_credentials,
        created_at: DateTime::parse_from_rfc3339(dv_str(&row[14])?)
            .ok()?
            .with_timezone(&Utc),
    })
}

#[allow(clippy::type_complexity)]
fn user_to_cozo_fields(
    user: &PrivacyUser,
) -> (
    String,
    String,
    String,
    String,
    String,
    i64,
    i64,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
) {
    let api_json = user
        .api_credentials
        .as_ref()
        .map(|c| serde_json::to_string(c).unwrap_or_else(|_| "null".to_string()))
        .unwrap_or_else(|| "null".to_string());
    (
        user.email.clone(),
        user.email_verified.to_string(),
        user.wg_public_key.clone(),
        user.wg_private_key_encrypted.clone(),
        user.assigned_ip.clone(),
        user.privacy_quota_bytes as i64,
        user.privacy_quota_used_bytes as i64,
        user.privacy_container_name.clone().unwrap_or_default(),
        user.privacy_route_id.clone().unwrap_or_default(),
        user.privacy_network_connected.to_string(),
        user.privacy_network_connected_at
            .map(|d| d.to_rfc3339())
            .unwrap_or_default(),
        user.google_id.clone().unwrap_or_default(),
        user.google_email.clone().unwrap_or_default(),
        api_json,
        user.created_at.to_rfc3339(),
    )
}

impl UserStore {
    /// Create a new user store backed by CozoDB.
    pub async fn new(cozo_db_path: impl Into<String>) -> Result<Self> {
        let path = cozo_db_path.into();
        let cozo = if path.is_empty() {
            CozoGraphShuttle::new_in_memory()?
        } else {
            CozoGraphShuttle::new_persistent(PathBuf::from(path))?
        };
        let store = Self {
            cozo,
            magic_links: RwLock::new(HashMap::new()),
            next_ip: RwLock::new(2),
        };
        // Load highest IP from existing users
        store.load_next_ip().await.ok();
        Ok(store)
    }

    async fn load_next_ip(&self) -> Result<()> {
        let users = self.list_users().await;
        let mut max_octet = 2u8;
        for user in users {
            if let Some(octet) = user
                .assigned_ip
                .strip_prefix("10.100.0.")
                .and_then(|s| s.strip_suffix("/32"))
                .and_then(|s| s.parse::<u8>().ok())
            {
                if octet > max_octet && octet < 255 {
                    max_octet = octet;
                }
            }
        }
        let mut next_ip = self.next_ip.write().await;
        *next_ip = max_octet.wrapping_add(1);
        if *next_ip > 254 || *next_ip < 2 {
            *next_ip = 2;
        }
        Ok(())
    }

    /// Get the next available IP address, ensuring no collisions
    pub async fn allocate_ip(&self) -> String {
        let mut next_ip_oct = self.next_ip.write().await;
        let users = self.list_users().await;
        let assigned_ips: std::collections::HashSet<String> =
            users.into_iter().map(|u| u.assigned_ip).collect();

        for _ in 0..254 {
            let ip = format!("10.100.0.{}/32", *next_ip_oct);
            *next_ip_oct = next_ip_oct.wrapping_add(1);
            if *next_ip_oct > 254 || *next_ip_oct < 2 {
                *next_ip_oct = 2;
            }
            if !assigned_ips.contains(&ip) {
                return ip;
            }
        }
        warn!("IP address range 10.100.0.x is exhausted!");
        format!("10.100.0.{}/32", *next_ip_oct)
    }

    fn cozo_clone(&self) -> CozoGraphShuttle {
        self.cozo.clone()
    }

    async fn upsert_in_cozo(&self, user: &PrivacyUser) -> Result<()> {
        let c = self.cozo_clone();
        let (
            email,
            ev,
            wg,
            wg_priv,
            ip,
            quota,
            used,
            container,
            route,
            pnc,
            pnc_at,
            gid,
            gmail,
            api_json,
            ts,
        ) = user_to_cozo_fields(user);
        let id = user.id.clone();
        tokio::task::spawn_blocking(move || {
            c.upsert_privacy_user(
                &id,
                &email,
                ev == "true",
                &wg,
                &wg_priv,
                &ip,
                quota,
                used,
                &container,
                &route,
                pnc == "true",
                &pnc_at,
                &gid,
                &gmail,
                &api_json,
                &ts,
            )
        })
        .await
        .context("spawn_blocking")??;
        Ok(())
    }

    /// Create a new user (unverified)
    pub async fn create_user(
        &self,
        email: &str,
        wg_public_key: String,
        wg_private_key_encrypted: String,
    ) -> Result<PrivacyUser> {
        if self.get_user_by_email(email).await.is_some() {
            anyhow::bail!("Email already registered");
        }

        let ip = self.allocate_ip().await;
        let user = PrivacyUser {
            id: uuid::Uuid::new_v4().to_string(),
            email: email.to_string(),
            email_verified: false,
            created_at: Utc::now(),
            wg_public_key,
            wg_private_key_encrypted,
            assigned_ip: ip,
            privacy_quota_bytes: default_privacy_quota_bytes(),
            privacy_quota_used_bytes: 0,
            privacy_container_name: None,
            privacy_route_id: None,
            privacy_network_connected: false,
            privacy_network_connected_at: None,
            google_id: None,
            google_email: None,
            api_credentials: None,
        };

        self.upsert_in_cozo(&user).await?;
        info!("Created user {} with IP {}", user.id, user.assigned_ip);
        Ok(user)
    }

    /// Create a magic link for a user
    pub async fn create_magic_link(&self, user_id: &str) -> Result<MagicLink> {
        let token = generate_magic_link_token(32);
        let link = MagicLink {
            token: token.clone(),
            user_id: user_id.to_string(),
            expires_at: Utc::now() + Duration::minutes(15),
        };
        let mut links = self.magic_links.write().await;
        links.insert(token, link.clone());
        Ok(link)
    }

    /// Verify a magic link and mark user as verified
    pub async fn verify_magic_link(&self, token: &str) -> Result<PrivacyUser> {
        let link = {
            let mut links = self.magic_links.write().await;
            links.remove(token).context("Invalid or expired link")?
        };
        if link.expires_at < Utc::now() {
            anyhow::bail!("Link has expired");
        }
        let mut user = self
            .get_user(&link.user_id)
            .await
            .context("User not found")?;
        user.email_verified = true;
        self.upsert_in_cozo(&user).await?;
        info!("User {} verified via magic link", user.id);
        Ok(user)
    }

    /// Get user by ID
    pub async fn get_user(&self, user_id: &str) -> Option<PrivacyUser> {
        let c = self.cozo_clone();
        let id = user_id.to_string();
        let row = tokio::task::spawn_blocking(move || c.get_privacy_user(&id))
            .await
            .ok()?;
        let row = row.ok()?;
        row.and_then(|r| parse_user_row(user_id, &r))
    }

    /// Get user by email
    pub async fn get_user_by_email(&self, email: &str) -> Option<PrivacyUser> {
        let c = self.cozo_clone();
        let email = email.to_string();
        let row = tokio::task::spawn_blocking(move || c.get_privacy_user_by_email(&email))
            .await
            .ok()?;
        let (id, row) = row.ok()?.and_then(|mut r| {
            if r.is_empty() {
                return None;
            }
            let id = dv_str(&r.remove(0))?.to_string();
            Some((id, r))
        })?;
        parse_user_row(&id, &row)
    }

    /// Get user by Google ID
    pub async fn get_user_by_google_id(&self, google_id: &str) -> Option<PrivacyUser> {
        let c = self.cozo_clone();
        let gid = google_id.to_string();
        let row = tokio::task::spawn_blocking(move || c.get_privacy_user_by_google_id(&gid))
            .await
            .ok()?;
        let (id, row) = row.ok()?.and_then(|mut r| {
            if r.is_empty() {
                return None;
            }
            let id = dv_str(&r.remove(0))?.to_string();
            Some((id, r))
        })?;
        parse_user_row(&id, &row)
    }

    /// Create or link user with Google identity
    pub async fn create_or_link_google_user(
        &self,
        google_id: &str,
        google_email: &str,
        wg_public_key: String,
        wg_private_key_encrypted: String,
    ) -> Result<PrivacyUser> {
        if let Some(existing) = self.get_user_by_google_id(google_id).await {
            return Ok(existing);
        }

        if let Some(mut existing) = self.get_user_by_email(google_email).await {
            existing.google_id = Some(google_id.to_string());
            existing.google_email = Some(google_email.to_string());
            existing.email_verified = true;
            self.upsert_in_cozo(&existing).await?;
            info!("Linked Google identity to existing user {}", existing.id);
            return Ok(existing);
        }

        let ip = self.allocate_ip().await;
        let user = PrivacyUser {
            id: uuid::Uuid::new_v4().to_string(),
            email: google_email.to_string(),
            email_verified: true,
            created_at: Utc::now(),
            wg_public_key,
            wg_private_key_encrypted,
            assigned_ip: ip,
            privacy_quota_bytes: default_privacy_quota_bytes(),
            privacy_quota_used_bytes: 0,
            privacy_container_name: None,
            privacy_route_id: None,
            privacy_network_connected: false,
            privacy_network_connected_at: None,
            google_id: Some(google_id.to_string()),
            google_email: Some(google_email.to_string()),
            api_credentials: None,
        };

        self.upsert_in_cozo(&user).await?;
        info!("Created new user {} with Google identity", user.id);
        Ok(user)
    }

    /// Clean up expired magic links
    pub async fn cleanup_expired_links(&self) {
        let mut links = self.magic_links.write().await;
        let now = Utc::now();
        let before = links.len();
        links.retain(|_, v| v.expires_at > now);
        let removed = before - links.len();
        if removed > 0 {
            warn!("Cleaned up {} expired magic links", removed);
        }
    }

    /// Set API credentials for a user
    pub async fn set_user_api_credentials(
        &self,
        user_id: &str,
        credentials: UserApiCredentials,
    ) -> Result<()> {
        let mut user = self.get_user(user_id).await.context("User not found")?;
        user.api_credentials = Some(credentials);
        self.upsert_in_cozo(&user).await?;
        info!("Updated API credentials for user {}", user_id);
        Ok(())
    }

    /// Mark a user as connected to the privacy network and persist container mapping.
    pub async fn mark_privacy_network_connected(
        &self,
        user_id: &str,
        container_name: String,
        route_id: String,
    ) -> Result<PrivacyUser> {
        let mut user = self.get_user(user_id).await.context("User not found")?;
        user.privacy_container_name = Some(container_name.clone());
        user.privacy_route_id = Some(route_id.clone());
        user.privacy_network_connected = true;
        user.privacy_network_connected_at = Some(Utc::now());
        self.upsert_in_cozo(&user).await?;
        info!(
            "User {} connected to privacy network via container {} route {}",
            user_id, container_name, route_id
        );
        Ok(user)
    }

    /// Get API credentials for a user
    pub async fn get_user_api_credentials(&self, user_id: &str) -> Option<UserApiCredentials> {
        let user = self.get_user(user_id).await?;
        user.api_credentials
    }

    /// List all users
    pub async fn list_users(&self) -> Vec<PrivacyUser> {
        let c = self.cozo_clone();
        let rows = match tokio::task::spawn_blocking(move || c.list_privacy_users())
            .await
            .ok()
        {
            Some(Ok(r)) => r,
            _ => return Vec::new(),
        };
        let mut users = Vec::new();
        for mut row in rows {
            if row.is_empty() {
                continue;
            }
            let id = match dv_str(&row.remove(0)) {
                Some(s) => s.to_string(),
                None => continue,
            };
            if let Some(user) = parse_user_row(&id, &row) {
                users.push(user);
            }
        }
        users
    }

    /// Delete a user by ID.
    ///
    /// Guardrail: refuses to delete a user that still has an attached
    /// container/privacy route. Deleting the Cozo record out from under a
    /// live container's storage is exactly the btrfs-corruption trap learned
    /// the hard way — storage must be cleanly detached (container stopped,
    /// route unpublished) before the identity record itself goes away. Call
    /// [`Self::detach_and_delete_user`] for the full teardown, or detach the
    /// container/route explicitly first if this is truly just an orphaned
    /// record with no real backing container.
    pub async fn delete_user(&self, user_id: &str) -> Result<()> {
        if let Some(user) = self.get_user(user_id).await {
            if user.privacy_container_name.is_some() || user.privacy_network_connected {
                anyhow::bail!(
                    "refusing to delete user {user_id}: privacy container/route still attached \
                     (container={:?}, connected={}) — detach it first",
                    user.privacy_container_name,
                    user.privacy_network_connected
                );
            }
        }
        let c = self.cozo_clone();
        let id = user_id.to_string();
        tokio::task::spawn_blocking(move || c.delete_privacy_user(&id))
            .await
            .context("spawn_blocking")??;
        Ok(())
    }
}
