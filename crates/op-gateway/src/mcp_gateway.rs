//! Legacy MCP gateway route selection
//!
//! This facade has no verified caller principal or grant at its ingress. It
//! cannot route callers to MCP services until canonical admission is wired.

use serde::{Deserialize, Serialize};
use simd_json::{json, OwnedValue as Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};
use uuid::Uuid;

use crate::wireguard_auth::WireGuardAuthManager;
use anyhow::Result;

/// Client routing decision
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingDecision {
    pub endpoint: String,
    pub allowed_tools: Vec<String>,
    pub capabilities: Vec<String>,
    pub has_full_access: bool,
    pub session_id: String,
    pub access_level: AccessLevel,
}

/// Access level for MCP clients
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AccessLevel {
    /// Full access to all tools (Compact + Cognitive)
    Full,
    /// Restricted access to cognitive tools only
    CognitiveOnly,
    /// No access (blocked)
    Blocked,
}

/// Client information for routing decisions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpClientInfo {
    pub name: String,
    pub version: Option<String>,
    pub user_agent: Option<String>,
    pub ip_address: Option<String>,
    pub auth_token: Option<String>,
    pub peer_pubkey: Option<String>,
}

/// MCP session information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpSession {
    pub session_id: String,
    pub client_info: McpClientInfo,
    pub routing_decision: RoutingDecision,
    pub created_at: u64,
    pub last_used: u64,
    pub is_active: bool,
}

/// MCP Gateway Manager - handles authentication and routing for MCP clients
pub struct McpGatewayManager {
    /// WireGuard authentication manager
    wireguard_auth: Arc<WireGuardAuthManager>,
    /// Active MCP sessions
    sessions: Arc<RwLock<HashMap<String, McpSession>>>,
    /// Client routing cache
    routing_cache: Arc<RwLock<HashMap<String, RoutingDecision>>>,
}

impl McpGatewayManager {
    /// Create new MCP Gateway Manager
    #[tracing::instrument(skip(wireguard_auth))]
    pub async fn new(wireguard_auth: Arc<WireGuardAuthManager>) -> Result<Self> {
        info!("Initializing MCP Gateway Manager");

        Ok(Self {
            wireguard_auth,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            routing_cache: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Route client to appropriate MCP backend based on authentication
    #[tracing::instrument(skip(self))]
    pub async fn route_client(&self, client_info: McpClientInfo) -> Result<RoutingDecision> {
        debug!("Routing client: {}", client_info.name);

        // Neither a caller-supplied public key nor a wristband minted from one
        // proves peer possession or conveys the canonical verified principal.
        // Keep this legacy route blocked until that attribution is available.
        let routing_decision = RoutingDecision {
            endpoint: String::new(),
            allowed_tools: Vec::new(),
            capabilities: Vec::new(),
            has_full_access: false,
            session_id: Uuid::new_v4().to_string(),
            access_level: AccessLevel::Blocked,
        };

        // Cache routing decision
        {
            let mut cache = self.routing_cache.write().await;
            let cache_key = self.generate_cache_key(&client_info);
            cache.insert(cache_key, routing_decision.clone());
        }

        info!(
            client = %client_info.name,
            authenticated = false,
            endpoint = %routing_decision.endpoint,
            tools = %routing_decision.allowed_tools.len(),
            "Client routed"
        );

        Ok(routing_decision)
    }

    /// Create MCP session for client
    #[tracing::instrument(skip(self))]
    pub async fn create_session(&self, client_info: McpClientInfo) -> Result<McpSession> {
        let routing_decision = self.route_client(client_info.clone()).await?;

        let session = McpSession {
            session_id: routing_decision.session_id.clone(),
            client_info,
            routing_decision,
            created_at: Self::current_timestamp(),
            last_used: Self::current_timestamp(),
            is_active: true,
        };

        // Store session
        {
            let mut sessions = self.sessions.write().await;
            sessions.insert(session.session_id.clone(), session.clone());
        }

        info!(session_id = %session.session_id, "MCP session created");
        Ok(session)
    }

    /// Validate MCP session
    #[tracing::instrument(skip(self))]
    pub async fn validate_session(&self, session_id: &str) -> Result<bool> {
        let sessions = self.sessions.read().await;
        if let Some(session) = sessions.get(session_id) {
            if !session.is_active
                || matches!(session.routing_decision.access_level, AccessLevel::Blocked)
            {
                return Ok(false);
            }

            // Check if underlying WireGuard session is still valid
            if let Some(ref auth_token) = session.client_info.auth_token {
                return self.wireguard_auth.validate_session(auth_token).await;
            }

            // For non-authenticated sessions, just check if session exists and is active
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Get session information
    #[tracing::instrument(skip(self))]
    pub async fn get_session(&self, session_id: &str) -> Result<Option<McpSession>> {
        let sessions = self.sessions.read().await;
        Ok(sessions.get(session_id).cloned())
    }

    /// List active sessions
    #[tracing::instrument(skip(self))]
    pub async fn list_sessions(&self) -> Result<Vec<McpSession>> {
        let sessions = self.sessions.read().await;
        Ok(sessions.values().cloned().collect())
    }

    /// Get client capabilities based on session
    pub async fn get_client_capabilities(&self, session_id: &str) -> Result<Vec<String>> {
        if let Some(session) = self.get_session(session_id).await? {
            Ok(session.routing_decision.capabilities)
        } else {
            Ok(Vec::new())
        }
    }

    /// Generate cache key for routing decisions
    fn generate_cache_key(&self, client_info: &McpClientInfo) -> String {
        let key_parts = vec![
            client_info.name.clone(),
            client_info.peer_pubkey.clone().unwrap_or_default(),
            client_info.auth_token.clone().unwrap_or_default(),
        ];

        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        key_parts.hash(&mut hasher);
        format!("mcp_route_{:x}", hasher.finish())
    }

    /// Get current Unix timestamp
    fn current_timestamp() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    /// Clean up expired sessions
    #[tracing::instrument(skip(self))]
    pub async fn cleanup_expired_sessions(&self) -> Result<usize> {
        let now = Self::current_timestamp();
        let mut expired_sessions = Vec::new();

        // Find expired sessions
        {
            let sessions = self.sessions.read().await;
            for (session_id, session) in sessions.iter() {
                // Sessions expire after 1 hour of inactivity
                if now - session.last_used > 3600 {
                    expired_sessions.push(session_id.clone());
                }
            }
        }

        // Remove expired sessions
        let expired_count = expired_sessions.len();
        if expired_count > 0 {
            let mut sessions = self.sessions.write().await;
            for session_id in expired_sessions {
                sessions.remove(&session_id);
            }

            info!(expired = %expired_count, "Cleaned up expired MCP sessions");
        }

        Ok(expired_count)
    }
}

/// D-Bus interface implementation for MCP Gateway
impl McpGatewayManager {
    /// Handle D-Bus method call for client routing
    pub async fn dbus_route_client(
        &self,
        client_name: &str,
        auth_token: Option<&str>,
        peer_pubkey: Option<&str>,
    ) -> Result<Value> {
        let client_info = McpClientInfo {
            name: client_name.to_string(),
            version: None,
            user_agent: None,
            ip_address: None,
            auth_token: auth_token.map(String::from),
            peer_pubkey: peer_pubkey.map(String::from),
        };

        let routing_decision = self.route_client(client_info).await?;

        Ok(json!({
            "endpoint": routing_decision.endpoint,
            "allowed_tools": routing_decision.allowed_tools,
            "capabilities": routing_decision.capabilities,
            "has_full_access": routing_decision.has_full_access,
            "session_id": routing_decision.session_id,
            "access_level": match routing_decision.access_level {
                AccessLevel::Full => "full",
                AccessLevel::CognitiveOnly => "cognitive_only",
                AccessLevel::Blocked => "blocked",
            }
        }))
    }

    /// Handle D-Bus method call for session validation
    pub async fn dbus_validate_session(&self, session_id: &str) -> Result<Value> {
        let is_valid = self.validate_session(session_id).await?;

        Ok(json!({
            "valid": is_valid,
            "session_id": session_id
        }))
    }

    /// Handle D-Bus method call for getting client capabilities
    pub async fn dbus_get_capabilities(&self, session_id: &str) -> Result<Value> {
        let capabilities = self.get_client_capabilities(session_id).await?;

        Ok(json!({
            "capabilities": capabilities,
            "session_id": session_id
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encrypted_storage::EncryptedStorageConfig;

    #[tokio::test]
    async fn stored_peer_key_or_wristband_cannot_select_full_access_route() {
        let storage_root =
            std::env::temp_dir().join(format!("op-gateway-route-{}", Uuid::new_v4()));
        let storage_path = storage_root.join("keys");
        tokio::fs::create_dir_all(&storage_path).await.unwrap();
        let storage_config = EncryptedStorageConfig {
            base_path: storage_root.clone(),
            subvolume_name: "keys".into(),
            ..Default::default()
        };

        let auth = Arc::new(
            WireGuardAuthManager::new_with_storage_config(storage_config)
                .await
                .unwrap(),
        );
        let peer_key = hex::encode([7_u8; 32]);
        let stored = auth.create_session(&peer_key, None).await.unwrap();
        let gateway = McpGatewayManager::new(auth).await.unwrap();

        for token in [None, Some(stored.session_id)] {
            let decision = gateway
                .route_client(McpClientInfo {
                    name: "unverified-caller".into(),
                    version: None,
                    user_agent: None,
                    ip_address: None,
                    auth_token: token,
                    peer_pubkey: Some(peer_key.clone()),
                })
                .await
                .unwrap();
            assert!(matches!(decision.access_level, AccessLevel::Blocked));
            assert!(!decision.has_full_access);
            assert!(decision.endpoint.is_empty());
            assert!(decision.allowed_tools.is_empty());
            assert!(decision.capabilities.is_empty());
        }

        let session = gateway
            .create_session(McpClientInfo {
                name: "unverified-caller".into(),
                version: None,
                user_agent: None,
                ip_address: None,
                auth_token: None,
                peer_pubkey: Some(peer_key),
            })
            .await
            .unwrap();
        assert!(!gateway.validate_session(&session.session_id).await.unwrap());
        assert!(gateway
            .get_client_capabilities(&session.session_id)
            .await
            .unwrap()
            .is_empty());

        tokio::fs::remove_dir_all(storage_root).await.unwrap();
    }
}
