//! Projection Engine - Auto-Discovery of D-Bus APIs as tools
//!
//! This engine walks the D-Bus object tree and projects discovered
//! interfaces as executable tools in the registry.

use anyhow::Result;
use simd_json::prelude::*;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::tool::Tool;
use op_core::BusType;
use op_introspection::IntrospectionService;

fn normalize_path(path: &str) -> String {
    let mut normalized = String::with_capacity(path.len().max(1));
    let mut prev_slash = false;

    for ch in path.chars() {
        if ch == '/' {
            if !prev_slash {
                normalized.push('/');
            }
            prev_slash = true;
        } else {
            normalized.push(ch);
            prev_slash = false;
        }
    }

    if normalized.is_empty() {
        "/".to_string()
    } else if normalized.len() > 1 && normalized.ends_with('/') {
        normalized.trim_end_matches('/').to_string()
    } else {
        normalized
    }
}

fn join_child_path(parent: &str, child: &str) -> String {
    if child.starts_with('/') {
        return normalize_path(child);
    }

    let parent_norm = normalize_path(parent);
    if parent_norm == "/" {
        normalize_path(&format!("/{}", child))
    } else {
        normalize_path(&format!("{}/{}", parent_norm, child))
    }
}

/// Projection Engine - auto-discovers D-Bus APIs
pub struct ProjectionEngine {
    introspection: Arc<IntrospectionService>,
}

impl ProjectionEngine {
    pub fn new(introspection: Arc<IntrospectionService>) -> Self {
        Self { introspection }
    }

    /// Discover and register all tools for a bus
    pub async fn discover_all(
        &self,
        registry: &crate::registry::ToolRegistry,
        bus_type: BusType,
    ) -> Result<usize> {
        let services_json = self.introspection.list_services_json(bus_type).await?;
        let mut total_count = 0;

        let services: Vec<String> = if let Some(arr) = services_json.as_array() {
            arr.iter()
                .filter_map(|v| v.get("name").and_then(|n| n.as_str()).map(String::from))
                .filter(|n| !n.starts_with(':')) // Skip unique names (temporary connections)
                .collect()
        } else {
            Vec::new()
        };

        tracing::info!(
            "Discovering tools for {} services on {:?} bus",
            services.len(),
            bus_type
        );

        // Process each service
        for service in services {
            tracing::debug!(
                "Introspecting service '{}' on {:?} bus...",
                service,
                bus_type
            );

            // Discover all object paths for this service
            let paths = self.discover_paths(bus_type, &service, "/", 0).await;
            let mut service_tools = 0;

            // Process each object path
            for path in &paths {
                if let Ok(info) = self
                    .introspection
                    .introspect(bus_type, &service, path)
                    .await
                {
                    for iface in info.interfaces {
                        // Skip standard D-Bus interfaces unless they are interesting
                        if iface.name.starts_with("org.freedesktop.DBus.")
                            && !iface.name.contains("Properties")
                            && !iface.name.contains("ObjectManager")
                        {
                            continue;
                        }

                        for method in iface.methods {
                            let tool = crate::dynamic_tool::DynamicDbusTool::new(
                                service.clone(),
                                path.clone(),
                                iface.name.clone(),
                                method.name.clone(),
                                String::new(), // Signature not easily available here yet
                                method
                                    .in_args
                                    .iter()
                                    .map(|a| a.name.clone().unwrap_or_else(|| "arg".to_string()))
                                    .collect(),
                            );

                            let definition = crate::registry::ToolDefinition {
                                name: tool.name.clone(),
                                description: format!(
                                    "D-Bus method {}.{} on {} at {}",
                                    iface.name, method.name, service, path
                                ),
                                input_schema: tool.input_schema(),
                                schema_version: "https://json-schema.org/draft/next/schema"
                                    .to_string(),
                                category: "dbus-projected".to_string(),
                                namespace: "system.v1".to_string(),
                                tags: vec![
                                    "dbus".to_string(),
                                    "projected".to_string(),
                                    service.clone(),
                                ],
                            };

                            if registry
                                .register(tool.name.clone().into(), Arc::new(tool), definition)
                                .await
                                .is_ok()
                            {
                                service_tools += 1;
                            }
                        }
                    }
                }
            }

            total_count += service_tools;
            if service_tools > 0 {
                tracing::info!(
                    "  → Service {}: registered {} tools from {} paths",
                    service,
                    service_tools,
                    paths.len()
                );
            }
        }

        Ok(total_count)
    }

    /// Recursively discover all object paths for a service
    fn discover_paths<'a>(
        &'a self,
        bus_type: BusType,
        service: &'a str,
        path: &'a str,
        depth: usize,
    ) -> Pin<Box<dyn Future<Output = Vec<String>> + Send + 'a>> {
        Box::pin(async move {
            const MAX_DEPTH: usize = 10;
            if depth > MAX_DEPTH {
                return vec![];
            }

            let path = normalize_path(path);
            let mut paths = vec![path.clone()];

            // Introspect to find child nodes
            if let Ok(info) = self
                .introspection
                .introspect(bus_type, service, &path)
                .await
            {
                for child in &info.children {
                    if child.is_empty() {
                        continue;
                    }

                    let child_path = join_child_path(&path, child);
                    if child_path == path {
                        continue;
                    }

                    // Recursively discover child paths
                    let child_paths = self
                        .discover_paths(bus_type, service, &child_path, depth + 1)
                        .await;
                    paths.extend(child_paths);
                }
            }

            paths.sort();
            paths.dedup();
            paths
        })
    }
}

impl Clone for ProjectionEngine {
    fn clone(&self) -> Self {
        Self {
            introspection: self.introspection.clone(),
        }
    }
}
