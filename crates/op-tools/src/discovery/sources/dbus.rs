//! D-Bus Discovery Source
//!
//! Discovers tools from D-Bus services at runtime via introspection.
//! Uses op-introspection crate for actual D-Bus scanning.

use async_trait::async_trait;
use op_core::{BusType as CoreBusType, MethodInfo};
use op_introspection::IntrospectionService;
use simd_json::json;
use std::collections::HashSet;
use tracing::{debug, warn};

use crate::discovery::{SourceType, ToolDiscoverySource};
use crate::registry::ToolDefinition as RegistryToolDefinition;

/// D-Bus discovery source for runtime tool discovery
pub struct DbusDiscoverySource {
    bus_type: BusType,
    introspection_service: IntrospectionService,
    /// Well-known services to introspect
    services: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub enum BusType {
    Session,
    System,
}

impl From<BusType> for CoreBusType {
    fn from(bus_type: BusType) -> Self {
        match bus_type {
            BusType::Session => CoreBusType::Session,
            BusType::System => CoreBusType::System,
        }
    }
}

impl DbusDiscoverySource {
    pub fn new(bus_type: BusType) -> Self {
        Self {
            bus_type,
            introspection_service: IntrospectionService::new(),
            services: default_services(),
        }
    }

    pub fn session() -> Self {
        Self::new(BusType::Session)
    }

    pub fn system() -> Self {
        Self::new(BusType::System)
    }

    pub fn with_services(mut self, services: Vec<String>) -> Self {
        self.services = services;
        self
    }
}

fn default_services() -> Vec<String> {
    vec![
        "org.freedesktop.systemd1".to_string(),
        "org.freedesktop.NetworkManager".to_string(),
        "org.freedesktop.login1".to_string(),
        "org.freedesktop.PackageKit".to_string(),
        "org.freedesktop.UDisks2".to_string(),
        "org.freedesktop.ColorManager".to_string(),
        "org.freedesktop.PolicyKit1".to_string(),
        "org.freedesktop.ModemManager1".to_string(),
    ]
}

#[async_trait]
impl ToolDiscoverySource for DbusDiscoverySource {
    fn source_type(&self) -> SourceType {
        SourceType::Dbus
    }

    fn name(&self) -> &str {
        match self.bus_type {
            BusType::Session => "dbus-session",
            BusType::System => "dbus-system",
        }
    }

    fn description(&self) -> &str {
        "D-Bus services discovered via runtime introspection"
    }

    async fn discover(&self) -> anyhow::Result<Vec<RegistryToolDefinition>> {
        let mut tools = Vec::new();
        let bus_type: CoreBusType = self.bus_type.into();

        debug!("Starting D-Bus discovery on {:?} bus", self.bus_type);

        // First, discover all available services
        let services = match self.introspection_service.list_services(bus_type).await {
            Ok(services) => {
                debug!(
                    "Found {} services on {:?} bus",
                    services.len(),
                    self.bus_type
                );
                services
            }
            Err(e) => {
                warn!("Failed to list services on {:?} bus: {}", self.bus_type, e);
                return Ok(Vec::new());
            }
        };

        // Filter to well-known services (or all services if none specified)
        let target_services: HashSet<String> = if self.services.is_empty() {
            services.iter().map(|s| s.name.clone()).collect()
        } else {
            self.services.iter().cloned().collect()
        };

        // Introspect each target service
        for service_info in services {
            if !target_services.contains(&service_info.name) {
                continue;
            }

            debug!("Introspecting service: {}", service_info.name);

            // Try to introspect the root path
            match self
                .introspect_service_paths(&service_info.name, &bus_type)
                .await
            {
                Ok(service_tools) => {
                    debug!(
                        "Discovered {} tools from {}",
                        service_tools.len(),
                        service_info.name
                    );
                    tools.extend(service_tools);
                }
                Err(e) => {
                    debug!("Failed to introspect {}: {}", service_info.name, e);
                }
            }
        }

        debug!("Total D-Bus tools discovered: {}", tools.len());
        Ok(tools)
    }

    async fn is_available(&self) -> bool {
        // Check if D-Bus is available
        match self.bus_type {
            BusType::System => std::path::Path::new("/var/run/dbus/system_bus_socket").exists(),
            BusType::Session => std::env::var("DBUS_SESSION_BUS_ADDRESS").is_ok(),
        }
    }
}

impl DbusDiscoverySource {
    /// Introspect all paths for a service and generate tool definitions
    async fn introspect_service_paths(
        &self,
        service: &str,
        bus_type: &CoreBusType,
    ) -> anyhow::Result<Vec<RegistryToolDefinition>> {
        let mut tools = Vec::new();

        // Common paths to try for most services
        let paths_to_try = vec!["/".to_string(), format!("/{}", service.replace('.', "/"))];

        for path in paths_to_try {
            match self.introspect_path(service, &path, bus_type).await {
                Ok(path_tools) => {
                    tools.extend(path_tools);
                }
                Err(e) => {
                    debug!(
                        "Failed to introspect path {} for service {}: {}",
                        path, service, e
                    );
                }
            }
        }

        Ok(tools)
    }

    /// Introspect a specific path and generate tool definitions
    async fn introspect_path(
        &self,
        service: &str,
        path: &str,
        bus_type: &CoreBusType,
    ) -> anyhow::Result<Vec<RegistryToolDefinition>> {
        let object_info = self
            .introspection_service
            .introspect(*bus_type, service, path)
            .await?;

        let mut tools = Vec::new();

        for interface in &object_info.interfaces {
            for method in &interface.methods {
                // Skip methods with file descriptor arguments (not supported in JSON)
                let has_fd_args = method.in_args.iter().any(|arg| arg.signature.contains('h'))
                    || method
                        .out_args
                        .iter()
                        .any(|arg| arg.signature.contains('h'));

                if has_fd_args {
                    debug!(
                        "Skipping method {}.{} (has file descriptors)",
                        interface.name, method.name
                    );
                    continue;
                }

                let tool_def = self.method_to_tool_definition(service, path, interface, method)?;
                tools.push(tool_def);
            }
        }

        Ok(tools)
    }

    /// Convert a D-Bus method to a tool definition
    fn method_to_tool_definition(
        &self,
        service: &str,
        path: &str,
        interface: &op_core::InterfaceInfo,
        method: &MethodInfo,
    ) -> anyhow::Result<RegistryToolDefinition> {
        let tool_name = format!(
            "dbus_{}_{}_{}",
            service.split('.').next_back().unwrap_or(service),
            interface
                .name
                .split('.')
                .next_back()
                .unwrap_or(&interface.name),
            method.name
        );

        // Build input schema from method arguments
        let mut properties = simd_json::value::owned::Object::new();
        let mut required = Vec::new();

        for (idx, arg) in method.in_args.iter().enumerate() {
            let arg_name = arg.name.clone().unwrap_or_else(|| format!("arg{}", idx));
            let schema = self.signature_to_schema(&arg.signature, Some(&arg_name));
            properties.insert(arg_name.clone(), schema);
            required.push(arg_name);
        }

        let input_schema = json!({
            "type": "object",
            "properties": properties,
            "required": required
        });

        let description = format!(
            "D-Bus method: {}.{} on {}{}",
            interface.name,
            method.name,
            service,
            if path != "/" { path } else { "" }
        );

        Ok(RegistryToolDefinition {
            name: tool_name,
            description,
            input_schema,
            schema_version: "https://json-schema.org/draft/next/schema".to_string(),
            category: "dbus".to_string(),
            tags: vec![
                "dbus".to_string(),
                service.to_string(),
                interface.name.clone(),
            ],
            namespace: "system.v1".to_string(),
        })
    }

    /// Convert D-Bus signature to JSON schema type
    fn signature_to_schema(
        &self,
        signature: &str,
        arg_name: Option<&str>,
    ) -> simd_json::OwnedValue {
        let desc = arg_name.map(|n| format!(" ({})", n)).unwrap_or_default();
        match signature {
            "s" => json!({"type": "string", "description": format!("string{}", desc)}),
            "o" => json!({"type": "string", "description": format!("D-Bus object path{}", desc)}),
            "g" => json!({"type": "string", "description": format!("D-Bus signature{}", desc)}),
            "b" => json!({"type": "boolean", "description": format!("boolean{}", desc)}),
            "y" | "n" | "q" | "i" | "u" | "x" | "t" => {
                json!({"type": "integer", "description": format!("integer{}", desc)})
            }
            "d" => json!({"type": "number", "description": format!("number{}", desc)}),
            "v" => json!({"type": "string", "description": format!("variant{}", desc)}),
            "as" | "ao" => {
                json!({"type": "array", "items": {"type": "string"}, "description": format!("string array{}", desc)})
            }
            "ai" | "au" | "ax" | "at" => {
                json!({"type": "array", "items": {"type": "integer"}, "description": format!("integer array{}", desc)})
            }
            "ab" => {
                json!({"type": "array", "items": {"type": "boolean"}, "description": format!("boolean array{}", desc)})
            }
            // For complex types, use simple string representation to avoid schema issues
            _ => {
                json!({"type": "string", "description": format!("D-Bus type {}{}", signature, desc)})
            }
        }
    }
}
