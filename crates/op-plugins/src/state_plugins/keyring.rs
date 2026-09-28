//! GNOME Keyring plugin - freedesktop.org Secret Service integration
#![allow(dead_code)]
//!
//! Implements the org.freedesktop.secrets D-Bus service for secure credential storage.
//! This allows applications like Cursor to store and retrieve passwords, API keys, etc.
//!
//! The Secret Service API provides:
//! - Collections (like "default", "login")
//! - Items (individual secrets with attributes)
//! - Secure storage with optional encryption

use anyhow::Result;
use async_trait::async_trait;
use op_state::{
    ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateAction, StateDiff, StatePlugin,
};
use op_state_store::PluginSchema;
use serde::{Deserialize, Serialize};
use simd_json::prelude::*;
use simd_json::{OwnedValue as Value, json};
use zbus::{
    Connection, Proxy,
    zvariant::{ObjectPath, OwnedObjectPath},
};

/// Keyring state representation
#[derive(Debug, Clone, Serialize, Deserialize, Default, schemars::JsonSchema)]
#[schemars(extend("x-oscal-category" = "security"))]
pub struct KeyringState {
    /// Available collections
    pub collections: Vec<CollectionInfo>,
    /// Default collection path
    pub default_collection: Option<String>,
}

/// Information about a secret collection
#[derive(Debug, Clone, Serialize, Deserialize, Default, schemars::JsonSchema)]
pub struct CollectionInfo {
    pub path: String,
    pub label: String,
    pub locked: bool,
    pub created: u64,
    pub modified: u64,
}

/// GNOME Keyring plugin implementing freedesktop.org Secret Service
pub struct KeyringPlugin {
    connection: Option<Connection>,
}

impl KeyringPlugin {
    pub fn new() -> Self {
        Self { connection: None }
    }

    #[cfg(test)]
    fn with_connection(connection: Connection) -> Self {
        Self {
            connection: Some(connection),
        }
    }

    /// Connect to the upstream Secret Service owner on the session bus.
    async fn connect_service(&self) -> Result<Proxy<'static>> {
        let conn = match &self.connection {
            Some(connection) => connection.clone(),
            None => Connection::session().await?,
        };
        let proxy = Proxy::new(
            &conn,
            "org.freedesktop.secrets",
            "/org/freedesktop/secrets",
            "org.freedesktop.Secret.Service",
        )
        .await?;
        Ok(proxy)
    }

    /// Get available collections from the upstream Secret Service.
    async fn get_collections(&self) -> Result<Vec<CollectionInfo>> {
        let service = self.connect_service().await?;
        let paths: Vec<OwnedObjectPath> = service.get_property("Collections").await?;
        let mut result = Vec::with_capacity(paths.len());
        for path in paths {
            result.push(self.collection_info(&path).await?);
        }
        Ok(result)
    }

    /// Get typed metadata for one upstream Secret Service collection.
    async fn get_collection_info(&self, path: &ObjectPath<'_>) -> Result<CollectionInfo> {
        self.collection_info(path).await
    }

    /// Resolve the upstream Secret Service default alias.
    async fn get_default_collection(&self) -> Result<Option<String>> {
        let service = self.connect_service().await?;
        let path: OwnedObjectPath = service.call("ReadAlias", &"default").await?;
        Ok((path.as_str() != "/").then(|| path.to_string()))
    }

    async fn collection_info(&self, path: &ObjectPath<'_>) -> Result<CollectionInfo> {
        let conn = match &self.connection {
            Some(connection) => connection.clone(),
            None => Connection::session().await?,
        };
        let proxy = Proxy::new(
            &conn,
            "org.freedesktop.secrets",
            path,
            "org.freedesktop.Secret.Collection",
        )
        .await?;
        Ok(CollectionInfo {
            path: path.to_string(),
            label: proxy.get_property("Label").await?,
            locked: proxy.get_property("Locked").await?,
            created: proxy.get_property("Created").await?,
            modified: proxy.get_property("Modified").await?,
        })
    }
}

impl Default for KeyringPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyringPlugin {
    /// Check whether the session bus is available for Secret Service access.
    /// NOTE: is_available() must NOT spawn subprocesses or do blocking I/O
    /// because it runs during daemon initialization before the D-Bus name is claimed.
    fn check_service_available(&self) -> bool {
        // Check if the op-dbus session bus socket exists, which means the
        // The actual Secret Service call remains authoritative and propagates
        // errors; this is only a cheap availability hint for registration.
        std::path::Path::new(op_core::config::SESSION_BUS_SOCKET_PATH).exists()
    }
}

#[async_trait]
impl StatePlugin for KeyringPlugin {
    fn name(&self) -> &str {
        "keyring"
    }

    fn version(&self) -> &str {
        "0.1.0"
    }

    fn schema(&self) -> Option<PluginSchema> {
        Some(keyring_schema())
    }

    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: false, // Security reasons
            supports_checkpoints: true,
            supports_verification: true,
            atomic_operations: false, // Keyring operations are not atomic
        }
    }

    fn is_available(&self) -> bool {
        self.check_service_available()
    }

    fn unavailable_reason(&self) -> String {
        "org.freedesktop Secret Service is not available on the session bus".to_string()
    }

    async fn apply_state(&self, _diff: &StateDiff) -> Result<ApplyResult> {
        // Keyring operations are typically interactive and should not be automated
        Err(anyhow::anyhow!(
            "Keyring apply operations are not supported for security reasons"
        ))
    }

    async fn calculate_diff(&self, current: &Value, desired: &Value) -> Result<StateDiff> {
        let mut actions = Vec::new();

        if current != desired {
            actions.push(StateAction::Modify {
                resource: "keyring-collections".to_string(),
                changes: json!({
                    "current": current,
                    "desired": desired
                }),
            });
        }

        let metadata = DiffMetadata {
            timestamp: chrono::Utc::now().timestamp(),
            current_hash: format!("{:x}", md5::compute(simd_json::to_string(current)?)),
            desired_hash: format!("{:x}", md5::compute(simd_json::to_string(desired)?)),
        };

        Ok(StateDiff {
            plugin: self.name().to_string(),
            actions,
            metadata,
        })
    }

    async fn verify_state(&self, desired: &Value) -> Result<bool> {
        // For keyring, we can only verify that the structure is valid
        // We can't verify actual secrets without user interaction
        Ok(desired.is_object())
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        let state = simd_json::json!(null);
        Ok(Checkpoint {
            id: format!("keyring-{}", chrono::Utc::now().timestamp()),
            plugin: self.name().to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: state,
            backend_checkpoint: None,
        })
    }

    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        // Keyring rollback is not supported for security reasons
        Err(anyhow::anyhow!(
            "Keyring rollback is not supported for security reasons"
        ))
    }
}

pub(crate) fn keyring_schema() -> PluginSchema {
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "keyring",
        "0.1.0",
        "Secret service collections state",
        &serde_json::to_value(schemars::schema_for!(KeyringState)).unwrap(),
    );
    super::schemars_adapter::apply_state_defaults(
        &mut schema,
        &simd_json::serde::to_owned_value(&KeyringState::default()).unwrap(),
    );

    use super::plugin_scaffold_helpers::{EmptyInput, method_decl_from_schemars_with_output};
    use op_state_store::SideEffect;

    #[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
    pub struct ListCollectionsOutput {
        pub collections: Vec<CollectionInfo>,
    }
    #[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
    pub struct GetDefaultCollectionOutput {
        pub default_collection: Option<String>,
    }

    // These methods query the upstream Secret Service. Unavailable providers
    // return an error; a successful empty list means the provider has none.
    schema.methods.insert(
        "list_collections".to_string(),
        method_decl_from_schemars_with_output::<EmptyInput, ListCollectionsOutput>(
            "list_collections",
            SideEffect::Read,
            true,
            "keyring.read",
            "obs.security.plugin.keyring.collections.list@v1",
        ),
    );
    schema.methods.insert(
        "get_default_collection".to_string(),
        method_decl_from_schemars_with_output::<EmptyInput, GetDefaultCollectionOutput>(
            "get_default_collection",
            SideEffect::Read,
            true,
            "keyring.read",
            "obs.security.plugin.keyring.default-collection.get@v1",
        ),
    );

    schema.capabilities.insert(
        "keyring.read".to_string(),
        op_state_store::CapabilityDecl {
            id: "keyring.read".to_string(),
            description: "Grants: list_collections, get_default_collection.".to_string(),
        },
    );

    schema
}

/// Dispatch a `keyring` schema method. Called from `op-grpc-bridge`'s
/// `MutationEngine::dispatch_method_call`.
pub async fn dispatch_keyring_method(
    method: &str,
    _args: &serde_json::Value,
) -> Result<serde_json::Value> {
    let plugin = KeyringPlugin::new();
    match method {
        "list_collections" => {
            let collections = plugin.get_collections().await?;
            Ok(serde_json::json!({ "collections": collections }))
        }
        "get_default_collection" => {
            let default_collection = plugin.get_default_collection().await?;
            Ok(serde_json::json!({ "default_collection": default_collection }))
        }
        other => Err(anyhow::anyhow!("unknown keyring method: {}", other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::connection;

    struct SecretFixture {
        default_available: bool,
    }
    #[zbus::interface(name = "org.freedesktop.Secret.Service")]
    impl SecretFixture {
        #[zbus(property)]
        async fn collections(&self) -> Vec<OwnedObjectPath> {
            vec![OwnedObjectPath::try_from("/org/freedesktop/secrets/collection/login").unwrap()]
        }
        async fn read_alias(&self, alias: &str) -> OwnedObjectPath {
            if alias == "default" && self.default_available {
                OwnedObjectPath::try_from("/org/freedesktop/secrets/collection/login").unwrap()
            } else {
                OwnedObjectPath::try_from("/").unwrap()
            }
        }
    }

    struct CollectionFixture;
    #[zbus::interface(name = "org.freedesktop.Secret.Collection")]
    impl CollectionFixture {
        #[zbus(property)]
        async fn label(&self) -> String {
            "Login".to_string()
        }
        #[zbus(property)]
        async fn locked(&self) -> bool {
            false
        }
        #[zbus(property)]
        async fn created(&self) -> u64 {
            11
        }
        #[zbus(property)]
        async fn modified(&self) -> u64 {
            22
        }
    }

    async fn fixture_connection(default_available: bool) -> (Connection, Connection) {
        let (server_stream, client_stream) = tokio::net::UnixStream::pair().unwrap();
        let guid = zbus::Guid::generate();
        let server = connection::Builder::unix_stream(server_stream)
            .server(guid)
            .unwrap()
            .p2p()
            .serve_at(
                "/org/freedesktop/secrets",
                SecretFixture { default_available },
            )
            .unwrap();
        let server = server
            .serve_at(
                "/org/freedesktop/secrets/collection/login",
                CollectionFixture,
            )
            .unwrap();
        let client = connection::Builder::unix_stream(client_stream)
            .p2p()
            .build();
        let (server, client) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            tokio::try_join!(server.build(), client)
        })
        .await
        .expect("p2p fixture handshake timed out")
        .unwrap();
        (server, client)
    }

    #[tokio::test]
    async fn test_keyring_plugin_creation() {
        let plugin = KeyringPlugin::new();
        assert_eq!(plugin.name(), "keyring");
    }

    #[tokio::test]
    async fn test_capabilities() {
        let plugin = KeyringPlugin::new();
        let caps = plugin.capabilities();
        // KeyringPlugin capabilities from op_state::PluginCapabilities
        assert!(!caps.supports_rollback); // Security reasons - no rollback
        assert!(caps.supports_checkpoints);
        assert!(caps.supports_verification);
        assert!(!caps.atomic_operations);
    }

    #[tokio::test]
    async fn secret_service_fixture_returns_collection_and_default() {
        let (_server, client) = fixture_connection(true).await;
        let plugin = KeyringPlugin::with_connection(client);
        let collections = plugin.get_collections().await.unwrap();
        assert_eq!(collections[0].label, "Login");
        assert!(!collections[0].locked);
        assert_eq!(
            plugin.get_default_collection().await.unwrap().as_deref(),
            Some("/org/freedesktop/secrets/collection/login")
        );
    }

    #[tokio::test]
    async fn secret_service_failure_is_propagated() {
        let (server, client) = fixture_connection(true).await;
        drop(server);
        let plugin = KeyringPlugin::with_connection(client);
        assert!(plugin.get_collections().await.is_err());
        assert!(plugin.get_default_collection().await.is_err());
    }

    #[tokio::test]
    async fn secret_service_missing_default_alias_is_none() {
        let (_server, client) = fixture_connection(false).await;
        let plugin = KeyringPlugin::with_connection(client);
        assert_eq!(plugin.get_default_collection().await.unwrap(), None);
    }
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new("keyring", |_ctx| std::sync::Arc::new(KeyringPlugin::new()))
}
