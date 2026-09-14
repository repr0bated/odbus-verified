//! Per-Method Typed gRPC Services - Frozen at D-Bus Object Creation
//!
//! This module generates typed gRPC services from plugin schemas at the moment
//! a D-Bus object is created. Each D-Bus method becomes its own gRPC service.
//! The services are "frozen" - they reflect the schema exactly as it existed
//! at creation time and never change.
//!
//! Architecture:
//! ```text
//!     SchemaEngine ──► PluginSchema ──► PerMethodGrpcService (frozen)
//!           │                               │
//!           │                               ▼
//!           │                    gRPC Reflection (one service per method)
//!           │                               │
//!           ▼                               ▼
//!      /dev/shm/opdbus/plugin-blobs  Dynamic gRPC dispatch per method
//! ```
//!
//! Each method gets its own gRPC service with a single RPC. The reflection
//! service shows the exact typed interface for each method.

use std::collections::HashMap;
use std::sync::Arc;

use op_state_store::{MethodDecl, PluginSchema};
use prost::Message;
use prost_types::{
    field_descriptor_proto, DescriptorProto, FieldDescriptorProto, FileDescriptorProto,
    FileDescriptorSet, MethodDescriptorProto, ServiceDescriptorProto,
};
use tokio::sync::{broadcast, RwLock};

/// Manages frozen per-method gRPC services.
///
/// Once a plugin's D-Bus object is created, each of its methods gets its own
/// gRPC service generated and frozen. This ensures stable API contracts for
/// clients - each method is independently versioned and typed.
pub struct PerMethodGrpcServices {
    /// method_path -> ServiceRegistry (e.g., "tched_router.RegisterAgent")
    services: Arc<RwLock<HashMap<String, MethodServiceRegistry>>>,
    /// Broadcast channel for service lifecycle events
    lifecycle_tx: broadcast::Sender<MethodServiceLifecycleEvent>,
    /// Frozen per-method descriptor set created alongside D-Bus object registration.
    descriptor_bytes: std::sync::RwLock<Vec<u8>>,
    /// Original sealed descriptor sections, keyed by their pinned plugin.
    sealed_descriptor_bytes: std::sync::RwLock<std::collections::BTreeMap<String, Vec<u8>>>,
}

/// A frozen gRPC service for a single method.
#[derive(Clone)]
pub struct MethodServiceRegistry {
    /// Full method path: "plugin_id.method_name"
    pub method_path: String,
    /// Plugin ID (e.g., "tched_router")
    pub plugin_id: String,
    /// Method name (e.g., "RegisterAgent")
    pub method_name: String,
    /// Schema version when this service was created
    pub schema_version: String,
    /// The subid for this method
    pub subid: String,
    /// Input JSON Schema
    pub input_schema: simd_json::OwnedValue,
    /// Output JSON Schema  
    pub output_schema: simd_json::OwnedValue,
    /// Required capability (if any)
    pub required_capability: Option<String>,
    /// When this service was created
    pub frozen_at: std::time::Instant,
}

/// Events emitted when method services are created or removed.
#[derive(Clone, Debug)]
pub enum MethodServiceLifecycleEvent {
    Created {
        method_path: String,
        plugin_id: String,
    },
    Removed {
        method_path: String,
    },
}

impl PerMethodGrpcServices {
    /// Create a new registry.
    pub fn new() -> Arc<Self> {
        let (lifecycle_tx, _) = broadcast::channel(64);
        Arc::new(Self {
            services: Arc::new(RwLock::new(HashMap::new())),
            lifecycle_tx,
            descriptor_bytes: std::sync::RwLock::new(Vec::new()),
            sealed_descriptor_bytes: std::sync::RwLock::new(std::collections::BTreeMap::new()),
        })
    }

    /// Get the lifecycle event receiver.
    pub fn lifecycle_rx(&self) -> broadcast::Receiver<MethodServiceLifecycleEvent> {
        self.lifecycle_tx.subscribe()
    }

    /// Create frozen gRPC services for all methods in a plugin schema.
    ///
    /// This is called at D-Bus object creation time. Each method gets its own
    /// service - the services are frozen and will never change.
    pub async fn create_frozen_services(
        &self,
        plugin_id: String,
        schema: &PluginSchema,
    ) -> Result<Vec<MethodServiceRegistry>, anyhow::Error> {
        let registries = self
            .create_service_metadata(plugin_id.clone(), schema)
            .await?;
        self.freeze_descriptor_set(plugin_id.as_str(), schema);
        Ok(registries)
    }

    /// Consumer registration preserves the sealer's descriptor bytes. It does
    /// not generate a new service package, artifact identity, or schema hash.
    pub async fn register_sealed_services(
        &self,
        plugin_id: String,
        schema: &PluginSchema,
        descriptors: &[u8],
    ) -> Result<Vec<MethodServiceRegistry>, anyhow::Error> {
        FileDescriptorSet::decode(descriptors)?;
        self.remove_services_for_plugin(&plugin_id).await;
        let registries = self
            .create_service_metadata(plugin_id.clone(), schema)
            .await?;
        self.sealed_descriptor_bytes
            .write()
            .map_err(|_| anyhow::anyhow!("sealed descriptor cache lock poisoned"))?
            .insert(plugin_id, descriptors.to_vec());
        Ok(registries)
    }

    async fn create_service_metadata(
        &self,
        plugin_id: String,
        schema: &PluginSchema,
    ) -> Result<Vec<MethodServiceRegistry>, anyhow::Error> {
        let mut registries = Vec::new();

        for (method_name, method_decl) in &schema.methods {
            let method_path = format!("{}.{}", plugin_id, method_name);

            let registry = MethodServiceRegistry {
                method_path: method_path.clone(),
                plugin_id: plugin_id.clone(),
                method_name: method_name.clone(),
                schema_version: schema.version.clone(),
                subid: method_decl.subid.clone(),
                input_schema: method_decl.args.clone(),
                output_schema: method_decl
                    .returns
                    .clone()
                    .unwrap_or_else(|| simd_json::json!({})),
                required_capability: method_decl.required_capability.clone(),
                frozen_at: std::time::Instant::now(),
            };

            // Store each method as its own service
            let mut services = self.services.write().await;
            services.insert(method_path.clone(), registry.clone());

            registries.push(registry.clone());

            // Emit lifecycle event
            let _ = self
                .lifecycle_tx
                .send(MethodServiceLifecycleEvent::Created {
                    method_path,
                    plugin_id: plugin_id.clone(),
                });
        }

        Ok(registries)
    }

    fn freeze_descriptor_set(&self, plugin_id: &str, schema: &PluginSchema) {
        let mut set = self
            .descriptor_bytes
            .read()
            .ok()
            .and_then(|bytes| FileDescriptorSet::decode(bytes.as_slice()).ok())
            .unwrap_or_default();

        for (method_name, method_decl) in &schema.methods {
            if let Some(file) =
                generate_method_file_descriptor_proto(plugin_id, method_name, method_decl)
            {
                set.file.push(file);
            }
        }

        let mut bytes = Vec::new();
        if set.encode(&mut bytes).is_ok() {
            if let Ok(mut stored) = self.descriptor_bytes.write() {
                *stored = bytes;
            }
        }
    }

    /// Return the frozen FileDescriptorSet bytes for all registered method services.
    ///
    /// This is a static snapshot. It is produced when plugin methods are registered
    /// alongside D-Bus object creation and is not mutated by the reflection service.
    pub fn frozen_descriptor_set_bytes(&self) -> Vec<u8> {
        let mut bytes = self
            .descriptor_bytes
            .read()
            .map(|bytes| bytes.clone())
            .unwrap_or_default();
        if let Ok(sealed) = self.sealed_descriptor_bytes.read() {
            // FileDescriptorSet contains repeated FileDescriptorProto fields;
            // concatenation preserves their original wire bytes without a
            // deserialize/reserialize or descriptor synthesis step.
            for descriptor in sealed.values() {
                bytes.extend_from_slice(descriptor);
            }
        }
        bytes
    }

    /// Get a method service registry by method path.
    pub async fn get_service(&self, method_path: &str) -> Option<MethodServiceRegistry> {
        let services = self.services.read().await;
        services.get(method_path).cloned()
    }

    /// Get all services for a plugin.
    pub async fn get_services_for_plugin(&self, plugin_id: &str) -> Vec<MethodServiceRegistry> {
        let services = self.services.read().await;
        services
            .values()
            .filter(|r| r.plugin_id == plugin_id)
            .cloned()
            .collect()
    }

    /// List all registered method paths.
    pub async fn list_methods(&self) -> Vec<String> {
        let services = self.services.read().await;
        services.keys().cloned().collect()
    }

    /// List all plugins that have registered method services.
    pub async fn list_plugins(&self) -> Vec<String> {
        let services = self.services.read().await;
        let mut plugins: Vec<String> = services.values().map(|r| r.plugin_id.clone()).collect();
        plugins.sort();
        plugins.dedup();
        plugins
    }

    /// Remove all method services for a plugin when its D-Bus object is removed.
    pub async fn remove_services_for_plugin(&self, plugin_id: &str) -> usize {
        if let Ok(mut descriptors) = self.sealed_descriptor_bytes.write() {
            descriptors.remove(plugin_id);
        }
        let mut services = self.services.write().await;
        let to_remove: Vec<String> = services
            .iter()
            .filter(|(_, r)| r.plugin_id == plugin_id)
            .map(|(k, _)| k.clone())
            .collect();

        let count = to_remove.len();
        for key in &to_remove {
            if let Some(registry) = services.remove(key) {
                let _ = self
                    .lifecycle_tx
                    .send(MethodServiceLifecycleEvent::Removed {
                        method_path: registry.method_path,
                    });
            }
        }

        count
    }
}

/// Default implementation for testing.
impl Default for PerMethodGrpcServices {
    fn default() -> Self {
        let (lifecycle_tx, _) = broadcast::channel(64);
        Self {
            services: Arc::new(RwLock::new(HashMap::new())),
            lifecycle_tx,
            descriptor_bytes: std::sync::RwLock::new(Vec::new()),
            sealed_descriptor_bytes: std::sync::RwLock::new(std::collections::BTreeMap::new()),
        }
    }
}

// =============================================================================
// Reflection Support
// =============================================================================

/// Generate protobuf definition for a single method's gRPC service.
///
/// This creates the proto3 syntax needed to compile a service with one RPC
/// corresponding to the D-Bus method.
pub fn generate_method_proto(
    plugin_id: &str,
    method_name: &str,
    method_decl: &MethodDecl,
) -> String {
    let mut proto = String::new();

    // Header
    proto.push_str("syntax = \"proto3\";\n\n");
    proto.push_str(&format!(
        "package operation.method.{}.{};\n\n",
        sanitize_proto_ident(plugin_id).to_lowercase(),
        sanitize_proto_ident(method_name).to_lowercase()
    ));
    proto.push_str("import \"google/protobuf/descriptor.proto\";\n");
    proto.push_str("import \"google/protobuf/struct.proto\";\n\n");

    // Extend descriptor for custom options
    proto.push_str("extend google.protobuf.MethodOptions {\n");
    proto.push_str("  string plugin_subid = 50000;\n");
    proto.push_str("  string required_capability = 50001;\n");
    proto.push_str("}\n\n");

    // Input message
    let input_name = format!("{}Input", to_message_name(method_name));
    proto.push_str(&schema_message_proto(&input_name, &method_decl.args));
    proto.push('\n');

    // Output message
    let output_name = format!("{}Output", to_message_name(method_name));
    let empty_output = simd_json::json!({});
    let output_schema = method_decl.returns.as_ref().unwrap_or(&empty_output);
    proto.push_str(&schema_message_proto(&output_name, output_schema));
    proto.push('\n');

    // Generate service definition (one service per method)
    let service_name = format!("{}Service", to_message_name(method_name));
    proto.push_str(&format!("service {} {{\n", service_name));

    let subid = &method_decl.subid;
    let capability = method_decl.required_capability.as_deref().unwrap_or("");

    proto.push_str(&format!(
        "  // subid: {}, capability: {}\n",
        subid, capability
    ));
    proto.push_str(&format!(
        "  rpc {}({}) returns ({});\n",
        sanitize_proto_ident(method_name),
        input_name,
        output_name
    ));

    proto.push_str("}\n");

    proto
}

/// Generate protobuf definitions for all methods in a plugin schema.
pub fn generate_plugin_method_protos(plugin_id: &str, schema: &PluginSchema) -> String {
    let mut combined = String::new();

    combined.push_str("// ================================================\n");
    combined.push_str(&format!("// Plugin: {} (v{})\n", plugin_id, schema.version));
    combined.push_str("// ================================================\n");
    combined.push('\n');

    for (method_name, method_decl) in &schema.methods {
        combined.push_str(&generate_method_proto(plugin_id, method_name, method_decl));
        combined.push_str("\n\n");
    }

    combined
}

/// Generate a FileDescriptorSet for a single method's gRPC service.
///
/// In a full implementation, this would compile the proto and return the
/// encoded FileDescriptorSet bytes for registration with tonic-reflection.
pub fn generate_method_file_descriptor(
    plugin_id: &str,
    method_name: &str,
    method_decl: &MethodDecl,
) -> Vec<u8> {
    let Some(file) = generate_method_file_descriptor_proto(plugin_id, method_name, method_decl)
    else {
        return Vec::new();
    };

    let set = FileDescriptorSet { file: vec![file] };
    let mut bytes = Vec::new();
    if set.encode(&mut bytes).is_ok() {
        bytes
    } else {
        Vec::new()
    }
}

fn generate_method_file_descriptor_proto(
    plugin_id: &str,
    method_name: &str,
    method_decl: &MethodDecl,
) -> Option<FileDescriptorProto> {
    let package = method_package(plugin_id, method_name);
    let input_name = format!("{}Input", to_message_name(method_name));
    let output_name = format!("{}Output", to_message_name(method_name));
    let empty_output = simd_json::json!({});
    let output_schema = method_decl.returns.as_ref().unwrap_or(&empty_output);

    Some(FileDescriptorProto {
        name: Some(format!(
            "operation/method/{}/{}.proto",
            sanitize_proto_ident(plugin_id).to_lowercase(),
            sanitize_proto_ident(method_name).to_lowercase()
        )),
        package: Some(package.clone()),
        dependency: vec!["google/protobuf/struct.proto".to_string()],
        message_type: vec![
            schema_descriptor(&input_name, &method_decl.args),
            schema_descriptor(&output_name, output_schema),
        ],
        service: vec![ServiceDescriptorProto {
            name: Some(format!("{}Service", to_message_name(method_name))),
            method: vec![MethodDescriptorProto {
                name: Some(sanitize_proto_ident(method_name)),
                input_type: Some(format!(".{}.{}", package, input_name)),
                output_type: Some(format!(".{}.{}", package, output_name)),
                options: None,
                client_streaming: Some(false),
                server_streaming: Some(false),
            }],
            options: None,
        }],
        syntax: Some("proto3".to_string()),
        ..Default::default()
    })
}

fn schema_message_proto(message_name: &str, schema: &simd_json::OwnedValue) -> String {
    let fields = schema_fields(schema);
    let mut proto = format!("message {} {{\n", to_message_name(message_name));

    if fields.is_empty() {
        proto.push_str("  // Empty schema object.\n");
    }

    for (idx, field) in fields.into_iter().enumerate() {
        let number = idx + 1;
        let repeated = if field.repeated {
            "repeated "
        } else if crate::json_schema_proto::needs_scalar_presence(&field.proto_type, field.repeated)
        {
            "optional "
        } else {
            ""
        };
        proto.push_str(&format!(
            "  {}{} {} = {};\n",
            repeated, field.proto_type, field.name, number
        ));
    }

    proto.push_str("}\n");
    proto
}

fn schema_descriptor(message_name: &str, schema: &simd_json::OwnedValue) -> DescriptorProto {
    let mut oneof_decl = Vec::new();
    let field = schema_fields(schema)
        .into_iter()
        .enumerate()
        .map(|(idx, field)| {
            let number = idx as i32 + 1;
            let (field_type, type_name) = field_descriptor_type(&field.proto_type);
            let oneof_index = if crate::json_schema_proto::needs_scalar_presence(
                &field.proto_type,
                field.repeated,
            ) {
                let index = oneof_decl.len() as i32;
                oneof_decl.push(prost_types::OneofDescriptorProto {
                    name: Some(format!("_{}", field.name)),
                    ..Default::default()
                });
                Some(index)
            } else {
                None
            };
            FieldDescriptorProto {
                name: Some(field.name.clone()),
                number: Some(number),
                label: Some(if field.repeated {
                    field_descriptor_proto::Label::Repeated
                } else {
                    field_descriptor_proto::Label::Optional
                } as i32),
                r#type: Some(field_type as i32),
                type_name,
                json_name: Some(field.name),
                proto3_optional: oneof_index.map(|_| true),
                oneof_index,
                ..Default::default()
            }
        })
        .collect();

    DescriptorProto {
        name: Some(to_message_name(message_name)),
        field,
        oneof_decl,
        ..Default::default()
    }
}

#[derive(Debug, Clone)]
struct ProtoField {
    name: String,
    proto_type: String,
    repeated: bool,
}

fn schema_fields(schema: &simd_json::OwnedValue) -> Vec<ProtoField> {
    let Ok(json_schema) = serde_json::to_value(schema) else {
        return vec![fallback_field("payload", "google.protobuf.Value", false)];
    };

    let Some(properties) = json_schema.get("properties").and_then(|v| v.as_object()) else {
        return Vec::new();
    };

    let mut fields = properties.iter().collect::<Vec<_>>();
    fields.sort_by_key(|(name, _)| *name);

    fields
        .into_iter()
        .map(|(name, property)| {
            let (proto_type, repeated) = json_schema_type_to_proto(property);
            ProtoField {
                name: sanitize_proto_ident(name),
                proto_type,
                repeated,
            }
        })
        .collect()
}

fn fallback_field(name: &str, proto_type: &str, repeated: bool) -> ProtoField {
    ProtoField {
        name: name.to_string(),
        proto_type: proto_type.to_string(),
        repeated,
    }
}

use crate::json_schema_proto::json_schema_type_to_proto;

fn field_descriptor_type(proto_type: &str) -> (field_descriptor_proto::Type, Option<String>) {
    match proto_type {
        "string" => (field_descriptor_proto::Type::String, None),
        "bool" => (field_descriptor_proto::Type::Bool, None),
        "int64" => (field_descriptor_proto::Type::Int64, None),
        "uint64" => (field_descriptor_proto::Type::Uint64, None),
        "double" => (field_descriptor_proto::Type::Double, None),
        "google.protobuf.ListValue" => (
            field_descriptor_proto::Type::Message,
            Some(".google.protobuf.ListValue".to_string()),
        ),
        "google.protobuf.Struct" => (
            field_descriptor_proto::Type::Message,
            Some(".google.protobuf.Struct".to_string()),
        ),
        "google.protobuf.Value" => (
            field_descriptor_proto::Type::Message,
            Some(".google.protobuf.Value".to_string()),
        ),
        _ => (
            field_descriptor_proto::Type::Message,
            Some(".google.protobuf.Value".to_string()),
        ),
    }
}

fn method_package(plugin_id: &str, method_name: &str) -> String {
    format!(
        "operation.method.{}.{}",
        sanitize_proto_ident(plugin_id).to_lowercase(),
        sanitize_proto_ident(method_name).to_lowercase()
    )
}

fn to_message_name(name: &str) -> String {
    let mut output = String::new();
    let mut capitalize_next = true;

    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            if capitalize_next {
                output.push(c.to_ascii_uppercase());
                capitalize_next = false;
            } else {
                output.push(c);
            }
        } else {
            capitalize_next = true;
        }
    }

    if output.is_empty() {
        "Method".to_string()
    } else if output.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("M{}", output)
    } else {
        output
    }
}

fn sanitize_proto_ident(value: &str) -> String {
    let mut ident = String::new();
    for c in value.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            ident.push(c);
        } else {
            ident.push('_');
        }
    }

    if ident.is_empty() {
        "method".to_string()
    } else if ident.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("_{}", ident)
    } else {
        ident
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_name_conversion() {
        assert_eq!(to_message_name("RegisterAgent"), "RegisterAgent");
        assert_eq!(to_message_name("report_status"), "ReportStatus");
        assert_eq!(to_message_name("ReceiveCommand"), "ReceiveCommand");
    }

    #[test]
    fn test_generate_method_proto() {
        let method_decl = MethodDecl {
            name: "RegisterAgent".to_string(),
            args: simd_json::json!({
                "type": "object",
                "properties": {
                    "agent_id": {"type": "string"},
                    "priority": {"type": "integer"}
                }
            }),
            returns: Some(simd_json::json!({
                "type": "object",
                "properties": {
                    "success": {"type": "boolean"}
                }
            })),
            side_effect: op_state_store::SideEffect::Mutation,
            idempotent: false,
            required_capability: Some("register".to_string()),
            subid: "exp.service.3tched-router.register-agent@v1".to_string(),
        };

        let proto = generate_method_proto("tched_router", "RegisterAgent", &method_decl);

        assert!(proto.contains("service RegisterAgentService {"));
        assert!(proto.contains("rpc RegisterAgent"));
        assert!(proto.contains("RegisterAgentInput"));
        assert!(proto.contains("RegisterAgentOutput"));
        assert!(proto.contains("string agent_id = 1;"));
        assert!(proto.contains("int64 priority = 2;"));
        assert!(proto.contains("bool success = 1;"));
        assert!(proto.contains("subid: exp.service.3tched-router.register-agent@v1"));
        assert!(proto.contains("capability: register"));

        let descriptor =
            generate_method_file_descriptor("tched_router", "RegisterAgent", &method_decl);
        assert!(!descriptor.is_empty(), "expected encoded FileDescriptorSet");
    }

    #[tokio::test]
    async fn test_create_frozen_services() {
        let services = PerMethodGrpcServices::new();

        let schema = PluginSchema::builder("test_plugin")
            .version("1.0.0")
            .description("Test plugin")
            .method(MethodDecl {
                name: "TestMethod".to_string(),
                args: simd_json::json!({"type": "object"}),
                returns: Some(simd_json::json!({"type": "object"})),
                side_effect: op_state_store::SideEffect::Read,
                idempotent: true,
                required_capability: None,
                subid: "exp.service.test.method@v1".to_string(),
            })
            .build();

        let registries = services
            .create_frozen_services("test_plugin".to_string(), &schema)
            .await
            .unwrap();

        assert_eq!(registries.len(), 1);
        assert_eq!(registries[0].plugin_id, "test_plugin");
        assert_eq!(registries[0].method_name, "TestMethod");
        assert_eq!(registries[0].method_path, "test_plugin.TestMethod");
        assert_eq!(registries[0].schema_version, "1.0.0");
        assert!(!services.frozen_descriptor_set_bytes().is_empty());
    }

    #[tokio::test]
    async fn test_list_methods() {
        let services = PerMethodGrpcServices::new();

        let schema = PluginSchema::builder("plugin1")
            .version("1.0.0")
            .method(MethodDecl {
                name: "Method1".to_string(),
                args: simd_json::json!({}),
                returns: None,
                side_effect: op_state_store::SideEffect::Read,
                idempotent: true,
                required_capability: None,
                subid: "exp".to_string(),
            })
            .method(MethodDecl {
                name: "Method2".to_string(),
                args: simd_json::json!({}),
                returns: None,
                side_effect: op_state_store::SideEffect::Read,
                idempotent: true,
                required_capability: None,
                subid: "exp".to_string(),
            })
            .build();

        services
            .create_frozen_services("plugin1".to_string(), &schema)
            .await
            .unwrap();

        let methods = services.list_methods().await;
        assert_eq!(methods.len(), 2);
        assert!(methods.contains(&"plugin1.Method1".to_string()));
        assert!(methods.contains(&"plugin1.Method2".to_string()));
    }

    #[tokio::test]
    async fn test_remove_services_for_plugin() {
        let services = PerMethodGrpcServices::new();

        let schema = PluginSchema::builder("plugin1")
            .version("1.0.0")
            .method(MethodDecl {
                name: "Method1".to_string(),
                args: simd_json::json!({}),
                returns: None,
                side_effect: op_state_store::SideEffect::Read,
                idempotent: true,
                required_capability: None,
                subid: "exp".to_string(),
            })
            .build();

        services
            .create_frozen_services("plugin1".to_string(), &schema)
            .await
            .unwrap();

        let removed = services.remove_services_for_plugin("plugin1").await;
        assert_eq!(removed, 1);

        let methods = services.list_methods().await;
        assert!(methods.is_empty());
    }

    #[tokio::test]
    async fn test_get_services_for_plugin() {
        let services = PerMethodGrpcServices::new();

        let schema = PluginSchema::builder("plugin1")
            .version("1.0.0")
            .method(MethodDecl {
                name: "Method1".to_string(),
                args: simd_json::json!({}),
                returns: None,
                side_effect: op_state_store::SideEffect::Read,
                idempotent: true,
                required_capability: None,
                subid: "exp".to_string(),
            })
            .build();

        services
            .create_frozen_services("plugin1".to_string(), &schema)
            .await
            .unwrap();

        let plugin_services = services.get_services_for_plugin("plugin1").await;
        assert_eq!(plugin_services.len(), 1);
        assert_eq!(plugin_services[0].method_name, "Method1");
    }
}
