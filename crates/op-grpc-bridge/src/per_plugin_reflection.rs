//! Per-Method gRPC Reflection Integration
//!
//! This module integrates per-method typed gRPC services with reflection.
//! When a D-Bus object is created for a plugin, each of its methods gets its
//! own gRPC service generated and registered with the reflection service so
//! clients can discover the exact typed methods.

use std::collections::HashMap;
use std::sync::Arc;

use op_state_store::PluginSchema;
use prost::Message;
use prost_types::FileDescriptorSet;
use tokio::sync::RwLock;

/// Per-method service configuration for reflection.
#[derive(Clone)]
pub struct PerMethodReflectionConfig {
    /// Whether to enable per-method reflection
    pub enabled: bool,
    /// Base package name for method services
    pub base_package: String,
}

impl Default for PerMethodReflectionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            base_package: "operation.method".to_string(),
        }
    }
}

/// Manages per-method gRPC service metadata for reflection.
///
/// This keeps track of which methods have been registered for gRPC and their
/// schemas, so the reflection service can advertise them correctly.
#[derive(Default)]
pub struct PerMethodReflectionRegistry {
    /// method_path -> MethodServiceMeta
    services: Arc<RwLock<HashMap<String, MethodReflectionMeta>>>,
}

/// Metadata about a registered method gRPC service.
#[derive(Clone, Debug)]
pub struct MethodReflectionMeta {
    /// Full method path: "plugin_id.method_name"
    pub method_path: String,
    /// Plugin ID (e.g., "chat_router")
    pub plugin_id: String,
    /// Method name (e.g., "RegisterAgent")
    pub method_name: String,
    /// gRPC service name (e.g., "RegisterAgentService")
    pub service_name: String,
    /// Schema version when registered
    pub schema_version: String,
    /// subid for this method
    pub subid: String,
    /// Required capability (if any)
    pub required_capability: Option<String>,
    /// When registered
    pub registered_at: std::time::Instant,
}

impl PerMethodReflectionRegistry {
    /// Import the exact service identity and authority metadata from a pinned
    /// sealed manifest; do not synthesize a consumer-side service name.
    pub async fn register_sealed(
        &self,
        plugin_id: String,
        method: &op_blob::BlobMethod,
        schema_version: String,
    ) -> MethodReflectionMeta {
        let method_path = format!("{plugin_id}.{}", method.schema_name);
        let meta = MethodReflectionMeta {
            method_path: method_path.clone(),
            plugin_id,
            method_name: method.schema_name.clone(),
            service_name: method.grpc_service.clone(),
            schema_version,
            subid: method.subid.clone(),
            required_capability: method.required_capability.clone(),
            registered_at: std::time::Instant::now(),
        };
        self.services
            .write()
            .await
            .insert(method_path, meta.clone());
        meta
    }

    /// Create a new registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a method's gRPC service metadata.
    pub async fn register(
        &self,
        plugin_id: String,
        method_name: String,
        subid: String,
        required_capability: Option<String>,
        schema_version: String,
    ) -> MethodReflectionMeta {
        let method_path = format!("{}.{}", plugin_id, method_name);
        let service_name = to_method_service_name(&method_name);

        let meta = MethodReflectionMeta {
            method_path: method_path.clone(),
            plugin_id: plugin_id.clone(),
            method_name: method_name.clone(),
            service_name,
            schema_version: schema_version.clone(),
            subid,
            required_capability,
            registered_at: std::time::Instant::now(),
        };

        let mut services = self.services.write().await;
        services.insert(method_path, meta.clone());

        meta
    }

    /// Get metadata for a method.
    pub async fn get(&self, method_path: &str) -> Option<MethodReflectionMeta> {
        let services = self.services.read().await;
        services.get(method_path).cloned()
    }

    /// Get all methods for a plugin.
    pub async fn get_for_plugin(&self, plugin_id: &str) -> Vec<MethodReflectionMeta> {
        let services = self.services.read().await;
        services
            .values()
            .filter(|m| m.plugin_id == plugin_id)
            .cloned()
            .collect()
    }

    /// List all registered method services.
    pub async fn list(&self) -> Vec<MethodReflectionMeta> {
        let services = self.services.read().await;
        services.values().cloned().collect()
    }

    /// List all registered plugin IDs.
    pub async fn list_plugins(&self) -> Vec<String> {
        let services = self.services.read().await;
        let mut plugins: Vec<String> = services.values().map(|m| m.plugin_id.clone()).collect();
        plugins.sort();
        plugins.dedup();
        plugins
    }

    /// Unregister a method's gRPC service.
    pub async fn unregister(&self, method_path: &str) -> bool {
        let mut services = self.services.write().await;
        services.remove(method_path).is_some()
    }

    /// Unregister all methods for a plugin.
    pub async fn unregister_plugin(&self, plugin_id: &str) -> usize {
        let mut services = self.services.write().await;
        let to_remove: Vec<String> = services
            .iter()
            .filter(|(_, m)| m.plugin_id == plugin_id)
            .map(|(k, _)| k.clone())
            .collect();

        let count = to_remove.len();
        for key in to_remove {
            services.remove(&key);
        }

        count
    }
}

/// Convert a method name to a gRPC service name.
///
/// Examples:
///   - "RegisterAgent" -> "RegisterAgentService"
///   - "report_status" -> "Report_statusService"
///   - "ReceiveCommand" -> "ReceiveCommandService"
pub fn to_method_service_name(method_name: &str) -> String {
    let mut chars = method_name.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => format!(
            "{}Service",
            c.to_uppercase().chain(chars).collect::<String>()
        ),
    }
}

/// Generate reflection file descriptor bytes for all registered methods.
///
/// This creates a combined FileDescriptorSet that can be registered with
/// tonic-reflection to expose all per-method services.
pub fn generate_reflection_file_descriptor_set(schemas: &HashMap<String, PluginSchema>) -> Vec<u8> {
    let mut set = FileDescriptorSet::default();
    let mut plugin_ids = schemas.keys().collect::<Vec<_>>();
    plugin_ids.sort();

    for plugin_id in plugin_ids {
        let Some(schema) = schemas.get(plugin_id) else {
            continue;
        };
        for (method_name, method_decl) in &schema.methods {
            let bytes = crate::plugin_grpc_gen::generate_method_file_descriptor(
                plugin_id,
                method_name,
                method_decl,
            );
            if bytes.is_empty() {
                continue;
            }
            if let Ok(mut method_set) = FileDescriptorSet::decode(bytes.as_slice()) {
                set.file.append(&mut method_set.file);
            }
        }
    }

    let mut bytes = Vec::new();
    if set.encode(&mut bytes).is_ok() {
        bytes
    } else {
        Vec::new()
    }
}

/// Generate all method protos as a combined string for a plugin.
pub fn generate_all_method_protos_for_plugin(plugin_id: &str, schema: &PluginSchema) -> String {
    crate::plugin_grpc_gen::generate_plugin_method_protos(plugin_id, schema)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_name_conversion() {
        assert_eq!(
            to_method_service_name("RegisterAgent"),
            "RegisterAgentService"
        );
        assert_eq!(
            to_method_service_name("report_status"),
            "Report_statusService"
        );
        assert_eq!(
            to_method_service_name("ReceiveCommand"),
            "ReceiveCommandService"
        );
    }

    #[tokio::test]
    async fn test_register_and_list() {
        let registry = PerMethodReflectionRegistry::new();

        let meta = registry
            .register(
                "chat_router".to_string(),
                "RegisterAgent".to_string(),
                "exp.service.3tched-router.register-agent@v1".to_string(),
                Some("register".to_string()),
                "1.0.0".to_string(),
            )
            .await;

        assert_eq!(meta.plugin_id, "chat_router");
        assert_eq!(meta.method_name, "RegisterAgent");
        assert_eq!(meta.service_name, "RegisterAgentService");
        assert_eq!(meta.method_path, "chat_router.RegisterAgent");
        assert_eq!(meta.schema_version, "1.0.0");

        let list = registry.list().await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].method_name, "RegisterAgent");
    }

    #[tokio::test]
    async fn test_get_for_plugin() {
        let registry = PerMethodReflectionRegistry::new();

        registry
            .register(
                "chat_router".to_string(),
                "RegisterAgent".to_string(),
                "exp.1".to_string(),
                None,
                "1.0.0".to_string(),
            )
            .await;

        registry
            .register(
                "chat_router".to_string(),
                "ReportStatus".to_string(),
                "exp.2".to_string(),
                None,
                "1.0.0".to_string(),
            )
            .await;

        let methods = registry.get_for_plugin("chat_router").await;
        assert_eq!(methods.len(), 2);
    }

    #[tokio::test]
    async fn test_unregister_plugin() {
        let registry = PerMethodReflectionRegistry::new();

        registry
            .register(
                "chat_router".to_string(),
                "RegisterAgent".to_string(),
                "exp.1".to_string(),
                None,
                "1.0.0".to_string(),
            )
            .await;

        registry
            .register(
                "chat_router".to_string(),
                "ReportStatus".to_string(),
                "exp.2".to_string(),
                None,
                "1.0.0".to_string(),
            )
            .await;

        let removed = registry.unregister_plugin("chat_router").await;
        assert_eq!(removed, 2);

        let meta = registry.get("chat_router.RegisterAgent").await;
        assert!(meta.is_none());
    }

    #[tokio::test]
    async fn test_list_plugins() {
        let registry = PerMethodReflectionRegistry::new();

        registry
            .register(
                "chat_router".to_string(),
                "RegisterAgent".to_string(),
                "exp.1".to_string(),
                None,
                "1.0.0".to_string(),
            )
            .await;

        registry
            .register(
                "unix_socket".to_string(),
                "CreateSocket".to_string(),
                "exp.2".to_string(),
                None,
                "1.0.0".to_string(),
            )
            .await;

        let plugins = registry.list_plugins().await;
        assert_eq!(plugins.len(), 2);
        assert!(plugins.contains(&"chat_router".to_string()));
        assert!(plugins.contains(&"unix_socket".to_string()));
    }
}
