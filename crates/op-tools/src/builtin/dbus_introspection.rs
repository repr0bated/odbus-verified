//! D-Bus introspection tools (granular APIs).
//!
//! These tools provide the public-facing D-Bus and introspection helpers that
//! show up in the tool registry.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use op_core::{BusType, InterfaceInfo, ObjectInfo};
use op_introspection::IntrospectionService;
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use zbus::Connection;

use crate::{Tool, ToolRegistry};

fn parse_bus(input: &Value, key: &str) -> BusType {
    match input.get(key).and_then(|v| v.as_str()).unwrap_or("system") {
        "session" => BusType::Session,
        _ => BusType::System,
    }
}

fn bus_str(bus: BusType) -> &'static str {
    match bus {
        BusType::System => "system",
        BusType::Session => "session",
    }
}

fn find_interface<'a>(info: &'a ObjectInfo, interface: &str) -> Result<&'a InterfaceInfo> {
    info.interfaces
        .iter()
        .find(|iface| iface.name == interface)
        .ok_or_else(|| anyhow!("Interface not found: {}", interface))
}

fn parse_required_str(input: &Value, key: &str) -> Result<String> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("Missing required parameter: {}", key))
}

fn parse_bool(input: &Value, key: &str, default: bool) -> bool {
    input.get(key).and_then(|v| v.as_bool()).unwrap_or(default)
}

fn parse_bounded_usize(input: &Value, key: &str, default: usize, min: usize, max: usize) -> usize {
    let parsed = input
        .get(key)
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .or_else(|| {
            input
                .get(key)
                .and_then(|v| v.as_i64())
                .map(|v| v.max(0) as usize)
        });

    parsed.unwrap_or(default).clamp(min, max)
}

fn normalize_path(path: &str) -> String {
    let parts: Vec<&str> = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    if parts.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", parts.join("/"))
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

fn normalize_object_info(mut info: ObjectInfo) -> ObjectInfo {
    info.path = normalize_path(&info.path);

    let mut normalized_children: Vec<String> = info
        .children
        .iter()
        .map(|child| join_child_path(&info.path, child))
        .collect();
    normalized_children.sort();
    normalized_children.dedup();
    info.children = normalized_children;

    info
}

struct ServiceTraversal {
    objects: Vec<ObjectInfo>,
    errors: Vec<String>,
    truncated: bool,
}

async fn collect_service_objects(
    introspection: &IntrospectionService,
    bus: BusType,
    service: &str,
    root_path: &str,
    max_depth: usize,
    max_objects: usize,
) -> ServiceTraversal {
    let mut queue: VecDeque<(String, usize)> = VecDeque::new();
    let mut visited: HashSet<String> = HashSet::new();
    let mut objects = Vec::new();
    let mut errors = Vec::new();
    let mut truncated = false;

    queue.push_back((normalize_path(root_path), 0));

    while let Some((path, depth)) = queue.pop_front() {
        if objects.len() >= max_objects {
            truncated = true;
            break;
        }

        let path = normalize_path(&path);
        if !visited.insert(path.clone()) {
            continue;
        }

        match introspection.introspect(bus, service, &path).await {
            Ok(info) => {
                let info = normalize_object_info(info);
                let normalized_children = info.children.clone();

                if depth < max_depth {
                    for child_path in normalized_children {
                        if visited.contains(&child_path) {
                            continue;
                        }
                        if visited.len() + queue.len() >= max_objects {
                            truncated = true;
                            break;
                        }
                        queue.push_back((child_path, depth + 1));
                    }
                }

                objects.push(info);
            }
            Err(e) => {
                errors.push(format!("{}: {}", path, e));
            }
        }
    }

    objects.sort_by(|a, b| a.path.cmp(&b.path));

    if errors.len() > 200 {
        let omitted = errors.len() - 200;
        errors.truncate(200);
        errors.push(format!("... {} additional errors omitted", omitted));
    }

    ServiceTraversal {
        objects,
        errors,
        truncated,
    }
}

fn service_summary(objects: &[ObjectInfo]) -> Value {
    let mut unique_interfaces = HashSet::new();
    let mut unique_method_endpoints = HashSet::new();
    let mut unique_signal_endpoints = HashSet::new();
    let mut unique_property_endpoints = HashSet::new();

    let mut total_interfaces = 0usize;
    let mut total_methods = 0usize;
    let mut total_signals = 0usize;
    let mut total_properties = 0usize;

    for obj in objects {
        total_interfaces += obj.interfaces.len();
        for iface in &obj.interfaces {
            unique_interfaces.insert(iface.name.clone());
            total_methods += iface.methods.len();
            total_signals += iface.signals.len();
            total_properties += iface.properties.len();

            for method in &iface.methods {
                unique_method_endpoints
                    .insert(format!("{}|{}|{}", obj.path, iface.name, method.name));
            }
            for signal in &iface.signals {
                unique_signal_endpoints
                    .insert(format!("{}|{}|{}", obj.path, iface.name, signal.name));
            }
            for property in &iface.properties {
                unique_property_endpoints
                    .insert(format!("{}|{}|{}", obj.path, iface.name, property.name));
            }
        }
    }

    json!({
        "objects": objects.len(),
        "interfaces": total_interfaces,
        "methods": total_methods,
        "signals": total_signals,
        "properties": total_properties,
        "unique_interfaces": unique_interfaces.len(),
        "unique_method_endpoints": unique_method_endpoints.len(),
        "unique_signal_endpoints": unique_signal_endpoints.len(),
        "unique_property_endpoints": unique_property_endpoints.len()
    })
}

fn summary_count(summary: &Value, key: &str) -> usize {
    summary
        .get(key)
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .or_else(|| {
            summary
                .get(key)
                .and_then(|v| v.as_i64())
                .map(|v| v.max(0) as usize)
        })
        .unwrap_or(0)
}

fn json_to_owned_value(value: &Value) -> Result<zbus::zvariant::OwnedValue> {
    use zbus::zvariant::Str as ZStr;

    if let Some(s) = value.as_str() {
        Ok(zbus::zvariant::OwnedValue::from(ZStr::from(s)))
    } else if let Some(b) = value.as_bool() {
        Ok(zbus::zvariant::OwnedValue::from(b))
    } else if let Some(i) = value.as_i64() {
        Ok(zbus::zvariant::OwnedValue::from(i))
    } else if let Some(u) = value.as_u64() {
        Ok(zbus::zvariant::OwnedValue::from(u))
    } else if let Some(f) = value.as_f64() {
        Ok(zbus::zvariant::OwnedValue::from(f))
    } else {
        Err(anyhow!("Unsupported argument type; use string/number/bool"))
    }
}

pub async fn register_dbus_introspection_tools(registry: &ToolRegistry) -> Result<()> {
    let introspection = Arc::new(IntrospectionService::new());

    registry
        .register_tool(Arc::new(DbusListServicesTool::new(introspection.clone())))
        .await?;
    registry
        .register_tool(Arc::new(DbusDiscoverSystemTool::new(introspection.clone())))
        .await?;
    registry
        .register_tool(Arc::new(DbusIntrospectServiceTool::new(
            introspection.clone(),
        )))
        .await?;
    registry
        .register_tool(Arc::new(DbusListObjectsTool::new(introspection.clone())))
        .await?;
    registry
        .register_tool(Arc::new(DbusIntrospectObjectTool::new(
            introspection.clone(),
        )))
        .await?;
    registry
        .register_tool(Arc::new(DbusListInterfacesTool::new(introspection.clone())))
        .await?;
    registry
        .register_tool(Arc::new(DbusListMethodsTool::new(introspection.clone())))
        .await?;
    registry
        .register_tool(Arc::new(DbusListPropertiesTool::new(introspection.clone())))
        .await?;
    registry
        .register_tool(Arc::new(DbusListSignalsTool::new(introspection.clone())))
        .await?;
    registry.register_tool(Arc::new(DbusCallMethodTool)).await?;
    registry
        .register_tool(Arc::new(DbusGetPropertyTool))
        .await?;
    registry
        .register_tool(Arc::new(DbusSetPropertyTool))
        .await?;
    registry
        .register_tool(Arc::new(DbusGetAllPropertiesTool::new(introspection)))
        .await?;

    Ok(())
}

struct DbusListServicesTool {
    introspection: Arc<IntrospectionService>,
}

impl DbusListServicesTool {
    fn new(introspection: Arc<IntrospectionService>) -> Self {
        Self { introspection }
    }
}

#[async_trait]
impl Tool for DbusListServicesTool {
    fn name(&self) -> &str {
        "dbus_list_services"
    }

    fn description(&self) -> &str {
        "List all available D-Bus services on system or session bus"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "bus": {
                    "type": "string",
                    "enum": ["system", "session"],
                    "default": "system"
                },
                "filter": {
                    "type": "string",
                    "description": "Optional filter pattern (e.g., 'org.freedesktop')"
                }
            }
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let bus = parse_bus(&input, "bus");
        let filter = input.get("filter").and_then(|v| v.as_str());
        let services = self.introspection.list_services(bus).await?;
        let mut names: Vec<String> = services.into_iter().map(|s| s.name).collect();

        names.retain(|name| !name.starts_with(':'));
        if let Some(pattern) = filter {
            names.retain(|name| name.contains(pattern));
        }

        Ok(json!({
            "bus": bus_str(bus),
            "count": names.len(),
            "services": names
        }))
    }

    fn category(&self) -> &str {
        "dbus"
    }
}

struct DbusDiscoverSystemTool {
    introspection: Arc<IntrospectionService>,
}

impl DbusDiscoverSystemTool {
    fn new(introspection: Arc<IntrospectionService>) -> Self {
        Self { introspection }
    }
}

#[async_trait]
impl Tool for DbusDiscoverSystemTool {
    fn name(&self) -> &str {
        "dbus_discover_system"
    }

    fn description(&self) -> &str {
        "Recursively discover all D-Bus services, objects, methods, properties, and signals"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "bus": {
                    "type": "string",
                    "enum": ["system", "session"],
                    "default": "system"
                },
                "path": {
                    "type": "string",
                    "default": "/"
                },
                "filter": {
                    "type": "string",
                    "description": "Optional service name substring filter"
                },
                "recursive": {
                    "type": "boolean",
                    "default": true
                },
                "max_services": {
                    "type": "integer",
                    "default": 256,
                    "minimum": 1,
                    "maximum": 5000
                },
                "max_depth": {
                    "type": "integer",
                    "default": 16,
                    "minimum": 0,
                    "maximum": 128
                },
                "max_objects_per_service": {
                    "type": "integer",
                    "default": 20000,
                    "minimum": 1,
                    "maximum": 200000
                },
                "include_objects": {
                    "type": "boolean",
                    "default": false
                }
            }
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let bus = parse_bus(&input, "bus");
        let path = input.get("path").and_then(|v| v.as_str()).unwrap_or("/");
        let filter = input.get("filter").and_then(|v| v.as_str());
        let recursive = parse_bool(&input, "recursive", true);
        let max_services = parse_bounded_usize(&input, "max_services", 256, 1, 5000);
        let max_depth = parse_bounded_usize(&input, "max_depth", 16, 0, 128);
        let max_objects_per_service =
            parse_bounded_usize(&input, "max_objects_per_service", 20000, 1, 200000);
        let include_objects = parse_bool(&input, "include_objects", false);
        let normalized_path = normalize_path(path);

        let services = self.introspection.list_services(bus).await?;
        let mut service_names: Vec<String> = services.into_iter().map(|s| s.name).collect();
        service_names.retain(|name| !name.starts_with(':'));
        if let Some(pattern) = filter {
            service_names.retain(|name| name.contains(pattern));
        }
        service_names.sort();
        service_names.dedup();

        let available_services = service_names.len();
        let services_truncated = available_services > max_services;
        if services_truncated {
            service_names.truncate(max_services);
        }

        let mut total_objects = 0usize;
        let mut total_interfaces = 0usize;
        let mut total_methods = 0usize;
        let mut total_signals = 0usize;
        let mut total_properties = 0usize;
        let mut total_errors = 0usize;

        let mut unique_interfaces = HashSet::new();
        let mut unique_method_endpoints = HashSet::new();
        let mut unique_signal_endpoints = HashSet::new();
        let mut unique_property_endpoints = HashSet::new();

        let mut failed_services = Vec::new();
        let mut truncated_services = Vec::new();
        let mut service_results = Vec::new();

        for service_name in service_names {
            let traversal = if recursive {
                collect_service_objects(
                    &self.introspection,
                    bus,
                    &service_name,
                    &normalized_path,
                    max_depth,
                    max_objects_per_service,
                )
                .await
            } else {
                match self
                    .introspection
                    .introspect(bus, &service_name, &normalized_path)
                    .await
                {
                    Ok(info) => ServiceTraversal {
                        objects: vec![normalize_object_info(info)],
                        errors: Vec::new(),
                        truncated: false,
                    },
                    Err(e) => ServiceTraversal {
                        objects: Vec::new(),
                        errors: vec![format!("{}: {}", normalized_path, e)],
                        truncated: false,
                    },
                }
            };

            if traversal.truncated {
                truncated_services.push(service_name.clone());
            }

            if traversal.objects.is_empty() && !traversal.errors.is_empty() {
                failed_services.push(service_name.clone());
            }

            let summary = service_summary(&traversal.objects);
            total_objects += summary_count(&summary, "objects");
            total_interfaces += summary_count(&summary, "interfaces");
            total_methods += summary_count(&summary, "methods");
            total_signals += summary_count(&summary, "signals");
            total_properties += summary_count(&summary, "properties");
            total_errors += traversal.errors.len();

            for obj in &traversal.objects {
                for iface in &obj.interfaces {
                    unique_interfaces.insert(iface.name.clone());
                    for method in &iface.methods {
                        unique_method_endpoints.insert(format!(
                            "{}|{}|{}|{}",
                            service_name, obj.path, iface.name, method.name
                        ));
                    }
                    for signal in &iface.signals {
                        unique_signal_endpoints.insert(format!(
                            "{}|{}|{}|{}",
                            service_name, obj.path, iface.name, signal.name
                        ));
                    }
                    for property in &iface.properties {
                        unique_property_endpoints.insert(format!(
                            "{}|{}|{}|{}",
                            service_name, obj.path, iface.name, property.name
                        ));
                    }
                }
            }

            let mut service_entry = json!({
                "service": service_name,
                "path": normalized_path,
                "recursive": recursive,
                "summary": summary,
                "errors": traversal.errors,
                "truncated": traversal.truncated
            });

            if include_objects {
                let objects_json = simd_json::serde::to_owned_value(&traversal.objects)
                    .unwrap_or_else(|_| Value::Array(vec![]));
                if let Some(obj) = service_entry.as_object_mut() {
                    obj.insert("objects".to_string(), objects_json);
                }
            } else if let Some(obj) = service_entry.as_object_mut() {
                obj.insert(
                    "object_count".to_string(),
                    Value::from(traversal.objects.len() as u64),
                );
            }

            service_results.push(service_entry);
        }

        Ok(json!({
            "bus": bus_str(bus),
            "path": normalized_path,
            "recursive": recursive,
            "filter": filter,
            "limits": {
                "max_services": max_services,
                "max_depth": max_depth,
                "max_objects_per_service": max_objects_per_service
            },
            "services_available": available_services,
            "services_scanned": service_results.len(),
            "services_truncated": services_truncated,
            "failed_services": failed_services,
            "truncated_services": truncated_services,
            "summary": {
                "services": service_results.len(),
                "objects": total_objects,
                "interfaces": total_interfaces,
                "methods": total_methods,
                "signals": total_signals,
                "properties": total_properties,
                "errors": total_errors,
                "unique_interfaces": unique_interfaces.len(),
                "unique_method_endpoints": unique_method_endpoints.len(),
                "unique_signal_endpoints": unique_signal_endpoints.len(),
                "unique_property_endpoints": unique_property_endpoints.len()
            },
            "services": service_results
        }))
    }

    fn category(&self) -> &str {
        "dbus"
    }
}

struct DbusIntrospectServiceTool {
    introspection: Arc<IntrospectionService>,
}

impl DbusIntrospectServiceTool {
    fn new(introspection: Arc<IntrospectionService>) -> Self {
        Self { introspection }
    }
}

#[async_trait]
impl Tool for DbusIntrospectServiceTool {
    fn name(&self) -> &str {
        "dbus_introspect_service"
    }

    fn description(&self) -> &str {
        "Get complete introspection data for a D-Bus service"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "service": { "type": "string" },
                "bus": {
                    "type": "string",
                    "enum": ["system", "session"],
                    "default": "system"
                },
                "path": {
                    "type": "string",
                    "default": "/"
                },
                "recursive": {
                    "type": "boolean",
                    "default": true
                },
                "max_depth": {
                    "type": "integer",
                    "default": 16,
                    "minimum": 0,
                    "maximum": 128
                },
                "max_objects": {
                    "type": "integer",
                    "default": 20000,
                    "minimum": 1,
                    "maximum": 200000
                },
                "include_objects": {
                    "type": "boolean",
                    "default": true
                }
            },
            "required": ["service"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let service = parse_required_str(&input, "service")?;
        let path = input.get("path").and_then(|v| v.as_str()).unwrap_or("/");
        let bus = parse_bus(&input, "bus");
        let recursive = parse_bool(&input, "recursive", true);
        let max_depth = parse_bounded_usize(&input, "max_depth", 16, 0, 128);
        let max_objects = parse_bounded_usize(&input, "max_objects", 20000, 1, 200000);
        let include_objects = parse_bool(&input, "include_objects", true);

        if !recursive {
            let data = self
                .introspection
                .introspect_json(bus, &service, path)
                .await?;

            return Ok(json!({
                "bus": bus_str(bus),
                "service": service,
                "path": normalize_path(path),
                "recursive": false,
                "data": data
            }));
        }

        let traversal = collect_service_objects(
            &self.introspection,
            bus,
            &service,
            path,
            max_depth,
            max_objects,
        )
        .await;

        let normalized_root = normalize_path(path);
        let root_from_traversal = traversal
            .objects
            .iter()
            .find(|obj| obj.path == normalized_root)
            .cloned();

        let root_data = if let Some(root_obj) = root_from_traversal {
            simd_json::serde::to_owned_value(&root_obj).unwrap_or(Value::null())
        } else {
            self.introspection
                .introspect_json(bus, &service, &normalized_root)
                .await
                .unwrap_or(Value::null())
        };

        let mut response = json!({
            "bus": bus_str(bus),
            "service": service,
            "path": normalized_root,
            "recursive": true,
            "limits": {
                "max_depth": max_depth,
                "max_objects": max_objects
            },
            "summary": service_summary(&traversal.objects),
            "data": root_data,
            "errors": traversal.errors,
            "truncated": traversal.truncated
        });

        if include_objects {
            let objects_json = simd_json::serde::to_owned_value(&traversal.objects)
                .unwrap_or_else(|_| Value::Array(vec![]));
            if let Some(obj) = response.as_object_mut() {
                obj.insert("objects".to_string(), objects_json);
            }
        }

        Ok(response)
    }

    fn category(&self) -> &str {
        "dbus"
    }
}

struct DbusListObjectsTool {
    introspection: Arc<IntrospectionService>,
}

impl DbusListObjectsTool {
    fn new(introspection: Arc<IntrospectionService>) -> Self {
        Self { introspection }
    }
}

#[async_trait]
impl Tool for DbusListObjectsTool {
    fn name(&self) -> &str {
        "dbus_list_objects"
    }

    fn description(&self) -> &str {
        "List object paths for a D-Bus service"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "service": { "type": "string" },
                "bus": {
                    "type": "string",
                    "enum": ["system", "session"],
                    "default": "system"
                },
                "path": {
                    "type": "string",
                    "default": "/"
                },
                "recursive": {
                    "type": "boolean",
                    "default": true
                },
                "max_depth": {
                    "type": "integer",
                    "default": 16,
                    "minimum": 0,
                    "maximum": 128
                },
                "max_objects": {
                    "type": "integer",
                    "default": 20000,
                    "minimum": 1,
                    "maximum": 200000
                }
            },
            "required": ["service"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let service = parse_required_str(&input, "service")?;
        let path = input.get("path").and_then(|v| v.as_str()).unwrap_or("/");
        let bus = parse_bus(&input, "bus");
        let recursive = parse_bool(&input, "recursive", true);
        let max_depth = parse_bounded_usize(&input, "max_depth", 16, 0, 128);
        let max_objects = parse_bounded_usize(&input, "max_objects", 20000, 1, 200000);

        if !recursive {
            let info = self.introspection.introspect(bus, &service, path).await?;
            let mut objects: Vec<String> = info
                .children
                .iter()
                .map(|child| join_child_path(path, child))
                .collect();
            objects.sort();
            objects.dedup();

            return Ok(json!({
                "bus": bus_str(bus),
                "service": service,
                "path": normalize_path(path),
                "recursive": false,
                "count": objects.len(),
                "objects": objects
            }));
        }

        let traversal = collect_service_objects(
            &self.introspection,
            bus,
            &service,
            path,
            max_depth,
            max_objects,
        )
        .await;

        let object_paths: Vec<String> = traversal
            .objects
            .iter()
            .map(|obj| obj.path.clone())
            .collect();

        Ok(json!({
            "bus": bus_str(bus),
            "service": service,
            "path": normalize_path(path),
            "recursive": true,
            "limits": {
                "max_depth": max_depth,
                "max_objects": max_objects
            },
            "count": object_paths.len(),
            "objects": object_paths,
            "summary": service_summary(&traversal.objects),
            "errors": traversal.errors,
            "truncated": traversal.truncated
        }))
    }

    fn category(&self) -> &str {
        "dbus"
    }
}

struct DbusIntrospectObjectTool {
    introspection: Arc<IntrospectionService>,
}

impl DbusIntrospectObjectTool {
    fn new(introspection: Arc<IntrospectionService>) -> Self {
        Self { introspection }
    }
}

#[async_trait]
impl Tool for DbusIntrospectObjectTool {
    fn name(&self) -> &str {
        "dbus_introspect_object"
    }

    fn description(&self) -> &str {
        "Introspect a specific D-Bus object path"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "service": { "type": "string" },
                "path": { "type": "string" },
                "bus": {
                    "type": "string",
                    "enum": ["system", "session"],
                    "default": "system"
                }
            },
            "required": ["service", "path"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let service = parse_required_str(&input, "service")?;
        let path = parse_required_str(&input, "path")?;
        let bus = parse_bus(&input, "bus");
        let data = self
            .introspection
            .introspect_json(bus, &service, &path)
            .await?;

        Ok(json!({
            "bus": bus_str(bus),
            "service": service,
            "path": path,
            "data": data
        }))
    }

    fn category(&self) -> &str {
        "dbus"
    }
}

struct DbusListInterfacesTool {
    introspection: Arc<IntrospectionService>,
}

impl DbusListInterfacesTool {
    fn new(introspection: Arc<IntrospectionService>) -> Self {
        Self { introspection }
    }
}

#[async_trait]
impl Tool for DbusListInterfacesTool {
    fn name(&self) -> &str {
        "dbus_list_interfaces"
    }

    fn description(&self) -> &str {
        "List interfaces for a D-Bus object"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "service": { "type": "string" },
                "path": { "type": "string", "default": "/" },
                "bus": {
                    "type": "string",
                    "enum": ["system", "session"],
                    "default": "system"
                }
            },
            "required": ["service"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let service = parse_required_str(&input, "service")?;
        let path = input.get("path").and_then(|v| v.as_str()).unwrap_or("/");
        let bus = parse_bus(&input, "bus");
        let info = self.introspection.introspect(bus, &service, path).await?;
        let interfaces: Vec<String> = info.interfaces.into_iter().map(|i| i.name).collect();

        Ok(json!({
            "bus": bus_str(bus),
            "service": service,
            "path": path,
            "interfaces": interfaces
        }))
    }

    fn category(&self) -> &str {
        "dbus"
    }
}

struct DbusListMethodsTool {
    introspection: Arc<IntrospectionService>,
}

impl DbusListMethodsTool {
    fn new(introspection: Arc<IntrospectionService>) -> Self {
        Self { introspection }
    }
}

#[async_trait]
impl Tool for DbusListMethodsTool {
    fn name(&self) -> &str {
        "dbus_list_methods"
    }

    fn description(&self) -> &str {
        "List methods for a D-Bus interface"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "service": { "type": "string" },
                "path": { "type": "string", "default": "/" },
                "interface": { "type": "string" },
                "bus": {
                    "type": "string",
                    "enum": ["system", "session"],
                    "default": "system"
                }
            },
            "required": ["service", "interface"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let service = parse_required_str(&input, "service")?;
        let interface = parse_required_str(&input, "interface")?;
        let path = input.get("path").and_then(|v| v.as_str()).unwrap_or("/");
        let bus = parse_bus(&input, "bus");
        let info = self.introspection.introspect(bus, &service, path).await?;
        let iface = find_interface(&info, &interface)?;
        let methods: Vec<String> = iface.methods.iter().map(|m| m.name.clone()).collect();

        Ok(json!({
            "bus": bus_str(bus),
            "service": service,
            "path": path,
            "interface": interface,
            "methods": methods
        }))
    }

    fn category(&self) -> &str {
        "dbus"
    }
}

struct DbusListPropertiesTool {
    introspection: Arc<IntrospectionService>,
}

impl DbusListPropertiesTool {
    fn new(introspection: Arc<IntrospectionService>) -> Self {
        Self { introspection }
    }
}

#[async_trait]
impl Tool for DbusListPropertiesTool {
    fn name(&self) -> &str {
        "dbus_list_properties"
    }

    fn description(&self) -> &str {
        "List properties for a D-Bus interface"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "service": { "type": "string" },
                "path": { "type": "string", "default": "/" },
                "interface": { "type": "string" },
                "bus": {
                    "type": "string",
                    "enum": ["system", "session"],
                    "default": "system"
                }
            },
            "required": ["service", "interface"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let service = parse_required_str(&input, "service")?;
        let interface = parse_required_str(&input, "interface")?;
        let path = input.get("path").and_then(|v| v.as_str()).unwrap_or("/");
        let bus = parse_bus(&input, "bus");
        let info = self.introspection.introspect(bus, &service, path).await?;
        let iface = find_interface(&info, &interface)?;
        let properties: Vec<String> = iface.properties.iter().map(|p| p.name.clone()).collect();

        Ok(json!({
            "bus": bus_str(bus),
            "service": service,
            "path": path,
            "interface": interface,
            "properties": properties
        }))
    }

    fn category(&self) -> &str {
        "dbus"
    }
}

struct DbusListSignalsTool {
    introspection: Arc<IntrospectionService>,
}

impl DbusListSignalsTool {
    fn new(introspection: Arc<IntrospectionService>) -> Self {
        Self { introspection }
    }
}

#[async_trait]
impl Tool for DbusListSignalsTool {
    fn name(&self) -> &str {
        "dbus_list_signals"
    }

    fn description(&self) -> &str {
        "List signals for a D-Bus interface"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "service": { "type": "string" },
                "path": { "type": "string", "default": "/" },
                "interface": { "type": "string" },
                "bus": {
                    "type": "string",
                    "enum": ["system", "session"],
                    "default": "system"
                }
            },
            "required": ["service", "interface"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let service = parse_required_str(&input, "service")?;
        let interface = parse_required_str(&input, "interface")?;
        let path = input.get("path").and_then(|v| v.as_str()).unwrap_or("/");
        let bus = parse_bus(&input, "bus");
        let info = self.introspection.introspect(bus, &service, path).await?;
        let iface = find_interface(&info, &interface)?;
        let signals: Vec<String> = iface.signals.iter().map(|s| s.name.clone()).collect();

        Ok(json!({
            "bus": bus_str(bus),
            "service": service,
            "path": path,
            "interface": interface,
            "signals": signals
        }))
    }

    fn category(&self) -> &str {
        "dbus"
    }
}

struct DbusCallMethodTool;

#[async_trait]
impl Tool for DbusCallMethodTool {
    fn name(&self) -> &str {
        "dbus_call_method"
    }

    fn description(&self) -> &str {
        "Call a D-Bus method with arguments"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "service": { "type": "string" },
                "path": { "type": "string" },
                "interface": { "type": "string" },
                "method": { "type": "string" },
                "args": {
                    "type": "array",
                    "description": "Method arguments (as JSON values)"
                },
                "bus": {
                    "type": "string",
                    "enum": ["system", "session"],
                    "default": "system"
                }
            },
            "required": ["service", "path", "interface", "method"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let service = parse_required_str(&input, "service")?;
        let path = parse_required_str(&input, "path")?;
        let interface = parse_required_str(&input, "interface")?;
        let method = parse_required_str(&input, "method")?;
        let bus = parse_bus(&input, "bus");
        let args = input
            .get("args")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let connection = match bus {
            BusType::System => Connection::system().await?,
            BusType::Session => Connection::session().await?,
        };

        let proxy = zbus::Proxy::new(
            &connection,
            service.as_str(),
            path.as_str(),
            interface.as_str(),
        )
        .await?;
        let zbus_args: Vec<zbus::zvariant::OwnedValue> = args
            .iter()
            .map(json_to_owned_value)
            .collect::<Result<Vec<_>>>()?;

        let result: zbus::zvariant::OwnedValue = proxy.call(method.as_str(), &zbus_args).await?;
        let result_json = simd_json::serde::to_owned_value(&result)?;

        Ok(json!({
            "bus": bus_str(bus),
            "service": service,
            "path": path,
            "interface": interface,
            "method": method,
            "result": result_json
        }))
    }

    fn category(&self) -> &str {
        "dbus"
    }
}

struct DbusGetPropertyTool;

#[async_trait]
impl Tool for DbusGetPropertyTool {
    fn name(&self) -> &str {
        "dbus_get_property"
    }

    fn description(&self) -> &str {
        "Get the value of a D-Bus property"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "service": { "type": "string" },
                "path": { "type": "string" },
                "interface": { "type": "string" },
                "property": { "type": "string" },
                "bus": {
                    "type": "string",
                    "enum": ["system", "session"],
                    "default": "system"
                }
            },
            "required": ["service", "path", "interface", "property"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let service = parse_required_str(&input, "service")?;
        let path = parse_required_str(&input, "path")?;
        let interface = parse_required_str(&input, "interface")?;
        let property = parse_required_str(&input, "property")?;
        let bus = parse_bus(&input, "bus");

        let connection = match bus {
            BusType::System => Connection::system().await?,
            BusType::Session => Connection::session().await?,
        };

        let interface_name = zbus::names::InterfaceName::try_from(interface.as_str())?;
        let properties_proxy = zbus::fdo::PropertiesProxy::builder(&connection)
            .destination(service.as_str())?
            .path(path.as_str())?
            .build()
            .await?;

        let value: zbus::zvariant::OwnedValue = properties_proxy
            .get(interface_name, property.as_str())
            .await?;
        let value_json = simd_json::serde::to_owned_value(&value)?;

        Ok(json!({
            "bus": bus_str(bus),
            "service": service,
            "path": path,
            "interface": interface,
            "property": property,
            "value": value_json
        }))
    }

    fn category(&self) -> &str {
        "dbus"
    }
}

struct DbusSetPropertyTool;

#[async_trait]
impl Tool for DbusSetPropertyTool {
    fn name(&self) -> &str {
        "dbus_set_property"
    }

    fn description(&self) -> &str {
        "Set the value of a D-Bus property"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "service": { "type": "string" },
                "path": { "type": "string" },
                "interface": { "type": "string" },
                "property": { "type": "string" },
                "value": { "description": "Property value (as JSON)" },
                "bus": {
                    "type": "string",
                    "enum": ["system", "session"],
                    "default": "system"
                }
            },
            "required": ["service", "path", "interface", "property", "value"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let service = parse_required_str(&input, "service")?;
        let path = parse_required_str(&input, "path")?;
        let interface = parse_required_str(&input, "interface")?;
        let property = parse_required_str(&input, "property")?;
        let value = input
            .get("value")
            .ok_or_else(|| anyhow!("Missing required parameter: value"))?;
        let bus = parse_bus(&input, "bus");

        let connection = match bus {
            BusType::System => Connection::system().await?,
            BusType::Session => Connection::session().await?,
        };

        let interface_name = zbus::names::InterfaceName::try_from(interface.as_str())?;
        let properties_proxy = zbus::fdo::PropertiesProxy::builder(&connection)
            .destination(service.as_str())?
            .path(path.as_str())?
            .build()
            .await?;

        let zbus_value = json_to_owned_value(value)?;
        properties_proxy
            .set(
                interface_name,
                property.as_str(),
                zbus::zvariant::Value::from(zbus_value),
            )
            .await
            .map_err(|e| anyhow::anyhow!("Failed to set property: {}", e))?;

        Ok(json!({
            "bus": bus_str(bus),
            "success": true,
            "service": service,
            "path": path,
            "interface": interface,
            "property": property
        }))
    }

    fn category(&self) -> &str {
        "dbus"
    }
}

struct DbusGetAllPropertiesTool {
    introspection: Arc<IntrospectionService>,
}

impl DbusGetAllPropertiesTool {
    fn new(introspection: Arc<IntrospectionService>) -> Self {
        Self { introspection }
    }
}

#[async_trait]
impl Tool for DbusGetAllPropertiesTool {
    fn name(&self) -> &str {
        "dbus_get_all_properties"
    }

    fn description(&self) -> &str {
        "Get all properties of a D-Bus object (optionally filter by interface)"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "service": { "type": "string" },
                "path": { "type": "string" },
                "interface": {
                    "type": "string",
                    "description": "Optional: specific interface, otherwise all interfaces"
                },
                "bus": {
                    "type": "string",
                    "enum": ["system", "session"],
                    "default": "system"
                }
            },
            "required": ["service", "path"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let service = parse_required_str(&input, "service")?;
        let path = parse_required_str(&input, "path")?;
        let interface_filter = input.get("interface").and_then(|v| v.as_str());
        let bus = parse_bus(&input, "bus");

        let connection = match bus {
            BusType::System => Connection::system().await?,
            BusType::Session => Connection::session().await?,
        };

        let info = self.introspection.introspect(bus, &service, &path).await?;
        let properties_proxy = zbus::fdo::PropertiesProxy::builder(&connection)
            .destination(service.as_str())?
            .path(path.as_str())?
            .build()
            .await?;

        let mut all_properties = json!({});
        for iface in info.interfaces {
            if let Some(filter) = interface_filter {
                if iface.name != filter {
                    continue;
                }
            }

            let interface_name = zbus::names::InterfaceName::try_from(iface.name.as_str())?;
            let props: HashMap<String, zbus::zvariant::OwnedValue> = properties_proxy
                .get_all(interface_name)
                .await
                .unwrap_or_default();

            let mut iface_props = simd_json::value::owned::Object::new();
            for (prop_name, prop_value) in props {
                let value_json = simd_json::serde::to_owned_value(&prop_value)?;
                iface_props.insert(prop_name, value_json);
            }
            if let Some(obj) = all_properties.as_object_mut() {
                obj.insert(iface.name.clone(), Value::Object(Box::new(iface_props)));
            }
        }

        Ok(json!({
            "bus": bus_str(bus),
            "service": service,
            "path": path,
            "properties": all_properties
        }))
    }

    fn category(&self) -> &str {
        "dbus"
    }
}
