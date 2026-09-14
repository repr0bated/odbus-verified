//! D-Bus Hybrid Tools - Direct D-Bus protocol access without CLI tools
//!
//! This module provides tools that communicate directly with D-Bus services
//! using the native protocol, eliminating the need for CLI wrappers.

use anyhow::Result;
use async_trait::async_trait;
use simd_json::OwnedValue;
use std::sync::Arc;

use crate::tool::{BoxedTool, Tool};

/// A tool that calls a specific D-Bus method
pub struct DbusMethodTool {
    /// Tool name (e.g., "dbus_systemd_manager_startunit")
    name: String,
    /// Human-readable description
    description: String,
    /// D-Bus service name
    service: String,
    /// D-Bus object path
    path: String,
    /// D-Bus interface name
    interface: String,
    /// D-Bus method name
    method: String,
    /// Input signature (D-Bus type string)
    input_signature: String,
    /// Output signature (D-Bus type string)
    output_signature: String,
    /// Use system bus (true) or session bus (false)
    use_system_bus: bool,
    /// JSON schema for input validation
    input_schema: Value,
}

impl DbusMethodTool {
    /// Create a new D-Bus method tool
    pub fn new(
        service: &str,
        path: &str,
        interface: &str,
        method: &str,
        input_signature: &str,
        output_signature: &str,
        use_system_bus: bool,
    ) -> Self {
        let name = format!(
            "dbus_{}_{}",
            interface.replace('.', "_").to_lowercase(),
            method.to_lowercase()
        );

        let description = format!(
            "Call D-Bus method {}.{} on service {}",
            interface, method, service
        );

        let input_schema = Self::generate_schema_from_signature(input_signature);

        Self {
            name,
            description,
            service: service.to_string(),
            path: path.to_string(),
            interface: interface.to_string(),
            method: method.to_string(),
            input_signature: input_signature.to_string(),
            output_signature: output_signature.to_string(),
            use_system_bus,
            input_schema,
        }
    }

    /// Generate JSON schema from D-Bus signature
    pub fn generate_schema_from_signature(signature: &str) -> Value {
        let mut properties = simd_json::value::owned::Object::new();
        let mut required = Vec::new();
        let mut param_idx = 0;

        for c in signature.chars() {
            let (param_name, schema) = match c {
                's' => (
                    format!("arg{}", param_idx),
                    simd_json::json!({"type": "string"}),
                ),
                'i' | 'n' => (
                    format!("arg{}", param_idx),
                    simd_json::json!({"type": "integer"}),
                ),
                'u' | 'q' | 't' | 'x' => (
                    format!("arg{}", param_idx),
                    simd_json::json!({"type": "integer", "minimum": 0}),
                ),
                'b' => (
                    format!("arg{}", param_idx),
                    simd_json::json!({"type": "boolean"}),
                ),
                'd' => (
                    format!("arg{}", param_idx),
                    simd_json::json!({"type": "number"}),
                ),
                'o' => (
                    format!("arg{}", param_idx),
                    simd_json::json!({
                        "type": "string",
                        "description": "D-Bus object path"
                    }),
                ),
                'a' | '(' | ')' | '{' | '}' | 'v' => continue, // Complex types - skip for now
                _ => continue,
            };

            required.push(param_name.clone());
            properties.insert(param_name, schema);
            param_idx += 1;
        }

        simd_json::json!({
            "type": "object",
            "properties": properties,
            "required": required
        })
    }
}

#[async_trait]
impl Tool for DbusMethodTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        self.input_schema.clone()
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        // Build D-Bus connection
        let connection = if self.use_system_bus {
            zbus::Connection::system().await?
        } else {
            zbus::Connection::session().await?
        };

        // Use Proxy for method calls (correct zbus API)
        let proxy = zbus::Proxy::new(
            &connection,
            self.service.as_str(),
            self.path.as_str(),
            self.interface.as_str(),
        )
        .await?;

        // Build arguments based on input signature
        let result = self.call_method_with_proxy(&proxy, &input).await?;

        Ok(simd_json::json!({
            "success": true,
            "service": self.service,
            "interface": self.interface,
            "method": self.method,
            "result": result
        }))
    }

    fn category(&self) -> &str {
        "dbus"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "dbus".to_string(),
            self.service.clone(),
            self.interface.clone(),
        ]
    }
}

impl DbusMethodTool {
    /// Call method using zbus Proxy
    async fn call_method_with_proxy(
        &self,
        proxy: &zbus::Proxy<'_>,
        input: &Value,
    ) -> Result<Value> {
        // Handle different signatures
        match self.input_signature.as_str() {
            "" => {
                let result: zbus::zvariant::OwnedValue = proxy.call(self.method.as_str(), &()).await?;                self.owned_value_to_json(result)
            }
            "s" => {
                let arg0 = input
                    .get("arg0")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let result: zbus::zvariant::OwnedValue =
                    proxy.call(self.method.as_str(), &(arg0,)).await?;
                self.owned_value_to_json(result)
            }
            "ss" => {
                let arg0 = input
                    .get("arg0")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let arg1 = input
                    .get("arg1")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let result: zbus::zvariant::OwnedValue =
                    proxy.call(self.method.as_str(), &(arg0, arg1)).await?;
                self.owned_value_to_json(result)
            }
            "o" => {
                let arg0 = input
                    .get("arg0")
                    .and_then(|v| v.as_str())
                    .unwrap_or("/");
                let result: zbus::zvariant::OwnedValue =
                    proxy.call(self.method.as_str(), &(arg0,)).await?;
                self.owned_value_to_json(result)
            }
            "ooo" => {
                let arg0 = input
                    .get("arg0")
                    .and_then(|v| v.as_str())
                    .unwrap_or("/");
                let arg1 = input
                    .get("arg1")
                    .and_then(|v| v.as_str())
                    .unwrap_or("/");
                let arg2 = input
                    .get("arg2")
                    .and_then(|v| v.as_str())
                    .unwrap_or("/");
                let result: zbus::zvariant::OwnedValue =
                    proxy.call(self.method.as_str(), &(arg0, arg1, arg2)).await?;
                self.owned_value_to_json(result)
            }
            _ => {
                // Generic fallback - try no args
                let result: zbus::zvariant::OwnedValue = proxy.call(self.method.as_str(), &()).await?;
                self.owned_value_to_json(result)
            }
        }
    }

    /// Convert OwnedValue to JSON
    fn owned_value_to_json(&self, value: zbus::zvariant::OwnedValue) -> Result<Value> {
        // Try common conversions
        if let Ok(s) = <String as TryFrom<zbus::zvariant::OwnedValue>>::try_from(value.clone()) {
            return Ok(Value::String(s));
        }
        if let Ok(b) = <bool as TryFrom<zbus::zvariant::OwnedValue>>::try_from(value.clone()) {
            return Ok(Value::Bool(b));
        }
        if let Ok(n) = <i32 as TryFrom<zbus::zvariant::OwnedValue>>::try_from(value.clone()) {
            return Ok(Value::Number(n.into()));
        }
        if let Ok(n) = <u32 as TryFrom<zbus::zvariant::OwnedValue>>::try_from(value.clone()) {
            return Ok(Value::Number(n.into()));
        }
        if let Ok(n) = <i64 as TryFrom<zbus::zvariant::OwnedValue>>::try_from(value.clone()) {
            return Ok(Value::Number(n.into()));
        }
        if let Ok(n) = <u64 as TryFrom<zbus::zvariant::OwnedValue>>::try_from(value.clone()) {
            return Ok(Value::Number(n.into()));
        }

        // For object paths
        if let Ok(path) =
            <zbus::zvariant::OwnedObjectPath as TryFrom<zbus::zvariant::OwnedValue>>::try_from(
                value.clone(),
            )
        {
            return Ok(Value::String(path.to_string()));
        }

        // Fallback: return signature info
        Ok(simd_json::json!({
            "type": "complex",
            "signature": self.output_signature,
            "raw": format!("{:?}", value)
        }))
    }
}

/// Create a D-Bus method tool
pub fn create_dbus_method_tool(
    service: &str,
    path: &str,
    interface: &str,
    method: &str,
    input_signature: &str,
    output_signature: &str,
    use_system_bus: bool,
) -> Result<BoxedTool> {
    Ok(Arc::new(DbusMethodTool::new(
        service,
        path,
        interface,
        method,
        input_signature,
        output_signature,
        use_system_bus,
    )))
}

/// Create common systemd tools
pub fn create_systemd_tools() -> Vec<BoxedTool> {
    vec![
        Arc::new(DbusMethodTool::new(
            "org.freedesktop.systemd1",
            "/org/freedesktop/systemd1",
            "org.freedesktop.systemd1.Manager",
            "StartUnit",
            "ss", // unit name, mode
            "o",  // job path
            true,
        )),
        Arc::new(DbusMethodTool::new(
            "org.freedesktop.systemd1",
            "/org/freedesktop/systemd1",
            "org.freedesktop.systemd1.Manager",
            "StopUnit",
            "ss", // unit name, mode
            "o",  // job path
            true,
        )),
        Arc::new(DbusMethodTool::new(
            "org.freedesktop.systemd1",
            "/org/freedesktop/systemd1",
            "org.freedesktop.systemd1.Manager",
            "RestartUnit",
            "ss", // unit name, mode
            "o",  // job path
            true,
        )),
        Arc::new(DbusMethodTool::new(
            "org.freedesktop.systemd1",
            "/org/freedesktop/systemd1",
            "org.freedesktop.systemd1.Manager",
            "GetUnit",
            "s", // unit name
            "o", // unit path
            true,
        )),
        Arc::new(DbusMethodTool::new(
            "org.freedesktop.systemd1",
            "/org/freedesktop/systemd1",
            "org.freedesktop.systemd1.Manager",
            "ListUnits",
            "",              // no args
            "a(ssssssouso)", // array of unit info
            true,
        )),
        Arc::new(DbusMethodTool::new(
            "org.freedesktop.systemd1",
            "/org/freedesktop/systemd1",
            "org.freedesktop.systemd1.Manager",
            "ListUnitFiles",
            "",      // no args
            "a(ss)", // array of (name, state)
            true,
        )),
    ]
}

/// Create NetworkManager tools
pub fn create_networkmanager_tools() -> Vec<BoxedTool> {
    vec![
        Arc::new(DbusMethodTool::new(
            "org.freedesktop.NetworkManager",
            "/org/freedesktop/NetworkManager",
            "org.freedesktop.NetworkManager",
            "GetDevices",
            "",   // no args
            "ao", // array of device paths
            true,
        )),
        Arc::new(DbusMethodTool::new(
            "org.freedesktop.NetworkManager",
            "/org/freedesktop/NetworkManager",
            "org.freedesktop.NetworkManager",
            "GetAllDevices",
            "",   // no args
            "ao", // array of device paths
            true,
        )),
        Arc::new(DbusMethodTool::new(
            "org.freedesktop.NetworkManager",
            "/org/freedesktop/NetworkManager",
            "org.freedesktop.NetworkManager",
            "ActivateConnection",
            "ooo", // connection, device, specific_object
            "o",   // active connection path
            true,
        )),
        Arc::new(DbusMethodTool::new(
            "org.freedesktop.NetworkManager",
            "/org/freedesktop/NetworkManager",
            "org.freedesktop.NetworkManager",
            "DeactivateConnection",
            "o", // active connection path
            "",  // void
            true,
        )),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_name_generation() {
        let tool = DbusMethodTool::new(
            "org.freedesktop.systemd1",
            "/org/freedesktop/systemd1",
            "org.freedesktop.systemd1.Manager",
            "StartUnit",
            "ss",
            "o",
            true,
        );

        assert_eq!(
            tool.name(),
            "dbus_org_freedesktop_systemd1_manager_startunit"
        );
        assert_eq!(tool.category(), "dbus");
    }

    #[test]
    fn test_schema_generation() {
        let tool = DbusMethodTool::new(
            "org.test",
            "/",
            "org.test.Interface",
            "Method",
            "sib", // string, int, bool
            "s",
            false,
        );

        let schema = tool.input_schema();
        let props = schema.get("properties").unwrap();

        assert!(props.get("arg0").is_some());
        assert!(props.get("arg1").is_some());
        assert!(props.get("arg2").is_some());
    }

    #[test]
    fn test_systemd_tools_creation() {
        let tools = create_systemd_tools();
        assert!(!tools.is_empty());

        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(names.iter().any(|n| n.contains("startunit")));
        assert!(names.iter().any(|n| n.contains("stopunit")));
    }
}
