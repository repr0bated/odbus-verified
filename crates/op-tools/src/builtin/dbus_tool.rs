//! D-Bus RPC tools
//! Dynamically generated tools from D-Bus introspection with full argument support

use async_trait::async_trait;
use op_core::{BusType, MethodInfo, ToolDefinition, ToolRequest, ToolResult};
use simd_json::{json, OwnedValue as Value};
use tracing::debug;
use zbus::zvariant::OwnedValue;
use zbus::Connection;

use crate::Tool;

/// A tool that calls a D-Bus method with full argument support
pub struct DbusMethodTool {
    pub bus_type: BusType,
    pub service: String,
    pub path: String,
    pub interface: String,
    pub method: MethodInfo,
    tool_name: String,
}

impl DbusMethodTool {
    /// Create a new D-Bus method tool
    pub fn new(
        bus_type: BusType,
        service: String,
        path: String,
        interface: String,
        method: MethodInfo,
    ) -> Self {
        let tool_name = Self::generate_tool_name(&service, &interface, &method.name);
        Self {
            bus_type,
            service,
            path,
            interface,
            method,
            tool_name,
        }
    }

    /// Generate a unique tool name from D-Bus identifiers
    fn generate_tool_name(service: &str, interface: &str, method: &str) -> String {
        let service_short = service.split('.').next_back().unwrap_or(service);
        let interface_short = interface.split('.').next_back().unwrap_or(interface);
        format!(
            "dbus_{}_{}_{}",
            service_short.replace('-', "_"),
            interface_short.replace('-', "_"),
            method.replace('-', "_")
        )
    }

    /// Convert D-Bus signature to JSON schema type
    /// Note: Keep schema simple for LLM compatibility - avoid complex constraints
    fn signature_to_schema(signature: &str, arg_name: Option<&str>) -> Value {
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

    /// Build the input argument signature string
    fn build_input_signature(&self) -> String {
        self.method
            .in_args
            .iter()
            .map(|arg| arg.signature.as_str())
            .collect()
    }
}

#[async_trait]
impl Tool for DbusMethodTool {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn definition(&self) -> ToolDefinition {
        let mut properties = simd_json::value::owned::Object::new();
        let mut required = Vec::new();

        for (idx, arg) in self.method.in_args.iter().enumerate() {
            let arg_name = arg.name.clone().unwrap_or_else(|| format!("arg{}", idx));
            properties.insert(
                arg_name.clone(),
                Self::signature_to_schema(&arg.signature, Some(&arg_name)),
            );
            required.push(arg_name);
        }

        let return_info = if self.method.out_args.is_empty() {
            "Returns: nothing".to_string()
        } else {
            let out_types: Vec<String> = self
                .method
                .out_args
                .iter()
                .map(|a| {
                    let name = a.name.as_deref().unwrap_or("result");
                    format!("{}: {}", name, a.signature)
                })
                .collect();
            format!("Returns: {}", out_types.join(", "))
        };

        ToolDefinition {
            name: self.tool_name.clone(),
            description: format!(
                "D-Bus: {}.{} on {}. {}",
                self.interface, self.method.name, self.service, return_info
            ),
            input_schema: json!({
                "type": "object",
                "properties": properties,
                "required": required
            }),
            category: Some("dbus".to_string()),
            tags: vec!["dbus".to_string(), self.service.clone()],
        }
    }

    async fn execute(&self, request: ToolRequest) -> ToolResult {
        let start = std::time::Instant::now();

        // Connect to D-Bus
        let connection = match self.bus_type {
            BusType::System => Connection::system().await,
            BusType::Session => Connection::session().await,
        };

        let connection = match connection {
            Ok(c) => c,
            Err(e) => {
                return ToolResult::error(
                    &request.id,
                    format!("Failed to connect to D-Bus: {}", e),
                    start.elapsed().as_millis() as u64,
                )
            }
        };

        // Convert arguments based on method signature
        let in_sig = self.build_input_signature();
        debug!(
            "D-Bus call: {}.{} sig='{}' args={:?}",
            self.interface, self.method.name, in_sig, request.arguments
        );

        // Call method based on number and type of arguments
        let result = self.execute_call(&connection, &request.arguments).await;

        match result {
            Ok(json_result) => ToolResult::success(
                &request.id,
                json!({
                    "success": true,
                    "service": self.service,
                    "interface": self.interface,
                    "method": self.method.name,
                    "path": self.path,
                    "result": json_result
                }),
                start.elapsed().as_millis() as u64,
            ),
            Err(e) => {
                let error_msg = format!("{}", e);
                let detailed_error = if error_msg.contains("InvalidArgs") {
                    format!(
                        "Invalid arguments. Expected: {}. Error: {}",
                        in_sig, error_msg
                    )
                } else if error_msg.contains("AccessDenied") {
                    format!("Access denied - may require root. Error: {}", error_msg)
                } else {
                    error_msg
                };
                ToolResult::error(
                    &request.id,
                    detailed_error,
                    start.elapsed().as_millis() as u64,
                )
            }
        }
    }
}

impl DbusMethodTool {
    /// Execute the D-Bus call using low-level connection API for dynamic return types
    async fn execute_call(
        &self,
        connection: &Connection,
        args: &Value,
    ) -> Result<Value, zbus::Error> {
        use zbus::zvariant::ObjectPath;

        let service: zbus::names::BusName = self.service.as_str().try_into()?;
        let path: ObjectPath = self.path.as_str().try_into()?;
        let interface: zbus::names::InterfaceName = self.interface.as_str().try_into()?;
        let method: zbus::names::MemberName = self.method.name.as_str().try_into()?;

        let num_args = self.method.in_args.len();
        debug!(
            "D-Bus call {}.{} with {} args",
            self.interface, self.method.name, num_args
        );

        // Use connection.call_method for dynamic return types
        let reply = if num_args == 0 {
            connection
                .call_method(Some(service), path, Some(interface), method, &())
                .await?
        } else {
            // Get argument values in order
            let arg_values: Vec<Value> = self
                .method
                .in_args
                .iter()
                .enumerate()
                .map(|(idx, arg_info)| {
                    let name = arg_info
                        .name
                        .clone()
                        .unwrap_or_else(|| format!("arg{}", idx));
                    args.get(&name)
                        .cloned()
                        .or_else(|| args.get(&format!("arg{}", idx)).cloned())
                        .unwrap_or(Value::Null)
                })
                .collect();

            // Get signatures for type-specific handling
            let sigs: Vec<&str> = self
                .method
                .in_args
                .iter()
                .map(|a| a.signature.as_str())
                .collect();

            self.call_with_args(
                connection,
                &service,
                &path,
                &interface,
                &method,
                &sigs,
                &arg_values,
            )
            .await?
        };

        // Convert reply to JSON using our robust converter
        Self::message_to_json(&reply)
    }

    async fn call_with_args(
        &self,
        connection: &Connection,
        service: &zbus::names::BusName<'_>,
        path: &zbus::zvariant::ObjectPath<'_>,
        interface: &zbus::names::InterfaceName<'_>,
        method: &zbus::names::MemberName<'_>,
        sigs: &[&str],
        vals: &[Value],
    ) -> Result<zbus::message::Message, zbus::Error> {
        match sigs.len() {
            1 => {
                self.call_1_arg(connection, service, path, interface, method, sigs, vals)
                    .await
            }
            2 => {
                self.call_2_args(connection, service, path, interface, method, sigs, vals)
                    .await
            }
            3 => {
                self.call_3_args(connection, service, path, interface, method, sigs, vals)
                    .await
            }
            4 => {
                self.call_4_args(connection, service, path, interface, method, sigs, vals)
                    .await
            }
            5 => {
                self.call_5_args(connection, service, path, interface, method, sigs, vals)
                    .await
            }
            n => Err(zbus::Error::Failure(format!(
                "Methods with {} arguments not yet supported",
                n
            ))),
        }
    }

    async fn call_1_arg(
        &self,
        connection: &Connection,
        service: &zbus::names::BusName<'_>,
        path: &zbus::zvariant::ObjectPath<'_>,
        interface: &zbus::names::InterfaceName<'_>,
        method: &zbus::names::MemberName<'_>,
        sigs: &[&str],
        vals: &[Value],
    ) -> Result<zbus::message::Message, zbus::Error> {
        match sigs.first().copied() {
            Some("s") => {
                let s = vals[0].as_str().unwrap_or("");
                connection
                    .call_method(
                        Some(service.clone()),
                        path.clone(),
                        Some(interface.clone()),
                        method.clone(),
                        &(s,),
                    )
                    .await
            }
            Some("o") => {
                let p: zbus::zvariant::ObjectPath = vals[0].as_str().unwrap_or("/").try_into()?;
                connection
                    .call_method(
                        Some(service.clone()),
                        path.clone(),
                        Some(interface.clone()),
                        method.clone(),
                        &(p,),
                    )
                    .await
            }
            Some("b") => {
                let b = vals[0].as_bool().unwrap_or(false);
                connection
                    .call_method(
                        Some(service.clone()),
                        path.clone(),
                        Some(interface.clone()),
                        method.clone(),
                        &(b,),
                    )
                    .await
            }
            Some("i") => {
                let n = vals[0].as_i64().unwrap_or(0) as i32;
                connection
                    .call_method(
                        Some(service.clone()),
                        path.clone(),
                        Some(interface.clone()),
                        method.clone(),
                        &(n,),
                    )
                    .await
            }
            Some("u") => {
                let n = vals[0].as_u64().unwrap_or(0) as u32;
                connection
                    .call_method(
                        Some(service.clone()),
                        path.clone(),
                        Some(interface.clone()),
                        method.clone(),
                        &(n,),
                    )
                    .await
            }
            Some("x") => {
                let n = vals[0].as_i64().unwrap_or(0);
                connection
                    .call_method(
                        Some(service.clone()),
                        path.clone(),
                        Some(interface.clone()),
                        method.clone(),
                        &(n,),
                    )
                    .await
            }
            Some("t") => {
                let n = vals[0].as_u64().unwrap_or(0);
                connection
                    .call_method(
                        Some(service.clone()),
                        path.clone(),
                        Some(interface.clone()),
                        method.clone(),
                        &(n,),
                    )
                    .await
            }
            _ => {
                let s = vals[0].as_str().unwrap_or("").to_string();
                connection
                    .call_method(
                        Some(service.clone()),
                        path.clone(),
                        Some(interface.clone()),
                        method.clone(),
                        &(s,),
                    )
                    .await
            }
        }
    }

    async fn call_2_args(
        &self,
        connection: &Connection,
        service: &zbus::names::BusName<'_>,
        path: &zbus::zvariant::ObjectPath<'_>,
        interface: &zbus::names::InterfaceName<'_>,
        method: &zbus::names::MemberName<'_>,
        sigs: &[&str],
        vals: &[Value],
    ) -> Result<zbus::message::Message, zbus::Error> {
        match (sigs.get(0).copied(), sigs.get(1).copied()) {
            (Some("s"), Some("s")) => {
                let s1 = vals[0].as_str().unwrap_or("");
                let s2 = vals[1].as_str().unwrap_or("");
                connection
                    .call_method(
                        Some(service.clone()),
                        path.clone(),
                        Some(interface.clone()),
                        method.clone(),
                        &(s1, s2),
                    )
                    .await
            }
            (Some("s"), Some("b")) => {
                let s = vals[0].as_str().unwrap_or("");
                let b = vals[1].as_bool().unwrap_or(false);
                connection
                    .call_method(
                        Some(service.clone()),
                        path.clone(),
                        Some(interface.clone()),
                        method.clone(),
                        &(s, b),
                    )
                    .await
            }
            (Some("s"), Some("u")) => {
                let s = vals[0].as_str().unwrap_or("");
                let n = vals[1].as_u64().unwrap_or(0) as u32;
                connection
                    .call_method(
                        Some(service.clone()),
                        path.clone(),
                        Some(interface.clone()),
                        method.clone(),
                        &(s, n),
                    )
                    .await
            }
            (Some("s"), Some("i")) => {
                let s = vals[0].as_str().unwrap_or("");
                let n = vals[1].as_i64().unwrap_or(0) as i32;
                connection
                    .call_method(
                        Some(service.clone()),
                        path.clone(),
                        Some(interface.clone()),
                        method.clone(),
                        &(s, n),
                    )
                    .await
            }
            (Some("u"), Some("u")) => {
                let n1 = vals[0].as_u64().unwrap_or(0) as u32;
                let n2 = vals[1].as_u64().unwrap_or(0) as u32;
                connection
                    .call_method(
                        Some(service.clone()),
                        path.clone(),
                        Some(interface.clone()),
                        method.clone(),
                        &(n1, n2),
                    )
                    .await
            }
            _ => {
                let s1 = vals[0].as_str().unwrap_or("").to_string();
                let s2 = vals[1].as_str().unwrap_or("").to_string();
                connection
                    .call_method(
                        Some(service.clone()),
                        path.clone(),
                        Some(interface.clone()),
                        method.clone(),
                        &(s1, s2),
                    )
                    .await
            }
        }
    }

    async fn call_3_args(
        &self,
        connection: &Connection,
        service: &zbus::names::BusName<'_>,
        path: &zbus::zvariant::ObjectPath<'_>,
        interface: &zbus::names::InterfaceName<'_>,
        method: &zbus::names::MemberName<'_>,
        sigs: &[&str],
        vals: &[Value],
    ) -> Result<zbus::message::Message, zbus::Error> {
        match (
            sigs.get(0).copied(),
            sigs.get(1).copied(),
            sigs.get(2).copied(),
        ) {
            (Some("s"), Some("s"), Some("s")) => {
                let s1 = vals[0].as_str().unwrap_or("");
                let s2 = vals[1].as_str().unwrap_or("");
                let s3 = vals[2].as_str().unwrap_or("");
                connection
                    .call_method(
                        Some(service.clone()),
                        path.clone(),
                        Some(interface.clone()),
                        method.clone(),
                        &(s1, s2, s3),
                    )
                    .await
            }
            (Some("s"), Some("s"), Some("b")) => {
                let s1 = vals[0].as_str().unwrap_or("");
                let s2 = vals[1].as_str().unwrap_or("");
                let b = vals[2].as_bool().unwrap_or(false);
                connection
                    .call_method(
                        Some(service.clone()),
                        path.clone(),
                        Some(interface.clone()),
                        method.clone(),
                        &(s1, s2, b),
                    )
                    .await
            }
            (Some("s"), Some("s"), Some("u")) => {
                let s1 = vals[0].as_str().unwrap_or("");
                let s2 = vals[1].as_str().unwrap_or("");
                let n = vals[2].as_u64().unwrap_or(0) as u32;
                connection
                    .call_method(
                        Some(service.clone()),
                        path.clone(),
                        Some(interface.clone()),
                        method.clone(),
                        &(s1, s2, n),
                    )
                    .await
            }
            _ => {
                let s1 = vals[0].as_str().unwrap_or("").to_string();
                let s2 = vals[1].as_str().unwrap_or("").to_string();
                let s3 = vals[2].as_str().unwrap_or("").to_string();
                connection
                    .call_method(
                        Some(service.clone()),
                        path.clone(),
                        Some(interface.clone()),
                        method.clone(),
                        &(s1, s2, s3),
                    )
                    .await
            }
        }
    }

    async fn call_4_args(
        &self,
        connection: &Connection,
        service: &zbus::names::BusName<'_>,
        path: &zbus::zvariant::ObjectPath<'_>,
        interface: &zbus::names::InterfaceName<'_>,
        method: &zbus::names::MemberName<'_>,
        _sigs: &[&str],
        vals: &[Value],
    ) -> Result<zbus::message::Message, zbus::Error> {
        let s1 = vals[0].as_str().unwrap_or("").to_string();
        let s2 = vals[1].as_str().unwrap_or("").to_string();
        let s3 = vals[2].as_str().unwrap_or("").to_string();
        let s4 = vals[3].as_str().unwrap_or("").to_string();
        connection
            .call_method(
                Some(service.clone()),
                path.clone(),
                Some(interface.clone()),
                method.clone(),
                &(s1, s2, s3, s4),
            )
            .await
    }

    async fn call_5_args(
        &self,
        connection: &Connection,
        service: &zbus::names::BusName<'_>,
        path: &zbus::zvariant::ObjectPath<'_>,
        interface: &zbus::names::InterfaceName<'_>,
        method: &zbus::names::MemberName<'_>,
        _sigs: &[&str],
        vals: &[Value],
    ) -> Result<zbus::message::Message, zbus::Error> {
        let s1 = vals[0].as_str().unwrap_or("").to_string();
        let s2 = vals[1].as_str().unwrap_or("").to_string();
        let s3 = vals[2].as_str().unwrap_or("").to_string();
        let s4 = vals[3].as_str().unwrap_or("").to_string();
        let s5 = vals[4].as_str().unwrap_or("").to_string();
        connection
            .call_method(
                Some(service.clone()),
                path.clone(),
                Some(interface.clone()),
                method.clone(),
                &(s1, s2, s3, s4, s5),
            )
            .await
    }

    /// Convert D-Bus message reply to JSON - handles all complex types
    fn message_to_json(msg: &zbus::message::Message) -> Result<Value, zbus::Error> {
        use zbus::zvariant::Value as ZValue;

        fn convert_value(v: &ZValue<'_>) -> Value {
            match v {
                ZValue::U8(n) => json!(*n),
                ZValue::Bool(b) => json!(*b),
                ZValue::I16(n) => json!(*n),
                ZValue::U16(n) => json!(*n),
                ZValue::I32(n) => json!(*n),
                ZValue::U32(n) => json!(*n),
                ZValue::I64(n) => json!(*n),
                ZValue::U64(n) => json!(*n),
                ZValue::F64(n) => json!(*n),
                ZValue::Str(s) => json!(s.as_str()),
                ZValue::Signature(s) => json!(s.to_string()),
                ZValue::ObjectPath(p) => json!(p.as_str()),
                ZValue::Value(inner) => convert_value(inner),
                ZValue::Array(arr) => {
                    let items: Vec<Value> = arr.iter().map(|item| convert_value(&item)).collect();
                    json!(items)
                }
                ZValue::Dict(dict) => {
                    let mut map = simd_json::value::owned::Object::new();
                    for (k, v) in dict.iter() {
                        let key = match &k {
                            ZValue::Str(s) => s.to_string(),
                            other => format!("{:?}", other),
                        };
                        map.insert(key, convert_value(&v));
                    }
                    Value::Object(map)
                }
                ZValue::Structure(s) => {
                    let fields: Vec<Value> = s.fields().iter().map(|f| convert_value(f)).collect();
                    json!(fields)
                }
                ZValue::Fd(_) => json!("<file descriptor>"),
            }
        }

        let sig = msg.body().signature().to_string();
        debug!("Reply signature: {}", sig);

        // Try signature-specific deserialization for known complex types
        // SystemD ListUnits: a(ssssssouso) - array of 10-tuples
        if sig == "a(ssssssouso)" {
            type UnitInfo = (
                String,
                String,
                String,
                String,
                String,
                String,
                zbus::zvariant::OwnedObjectPath,
                u32,
                String,
                zbus::zvariant::OwnedObjectPath,
            );
            if let Ok(units) = msg.body().deserialize::<Vec<UnitInfo>>() {
                let json_units: Vec<Value> = units
                    .iter()
                    .map(|u| {
                        json!({
                            "name": u.0,
                            "description": u.1,
                            "load_state": u.2,
                            "active_state": u.3,
                            "sub_state": u.4,
                            "following": u.5,
                            "unit_path": u.6.as_str(),
                            "job_id": u.7,
                            "job_type": u.8,
                            "job_path": u.9.as_str()
                        })
                    })
                    .collect();
                return Ok(json!(json_units));
            }
        }

        // Try common simple return types first
        if sig == "s" {
            if let Ok(s) = msg.body().deserialize::<String>() {
                return Ok(json!(s));
            }
        }
        if sig == "b" {
            if let Ok(b) = msg.body().deserialize::<bool>() {
                return Ok(json!(b));
            }
        }
        if sig == "o" {
            if let Ok(p) = msg.body().deserialize::<zbus::zvariant::OwnedObjectPath>() {
                return Ok(json!(p.as_str()));
            }
        }
        if sig == "as" {
            if let Ok(arr) = msg.body().deserialize::<Vec<String>>() {
                return Ok(json!(arr));
            }
        }
        if sig == "ao" {
            if let Ok(arr) = msg
                .body()
                .deserialize::<Vec<zbus::zvariant::OwnedObjectPath>>()
            {
                let strs: Vec<&str> = arr.iter().map(|p| p.as_str()).collect();
                return Ok(json!(strs));
            }
        }

        // Try OwnedValue for other types
        match msg.body().deserialize::<OwnedValue>() {
            Ok(owned) => {
                let zval: ZValue = owned.into();
                Ok(convert_value(&zval))
            }
            Err(e) => {
                debug!("Failed to deserialize as OwnedValue: {}", e);
                // Return success with signature info
                Ok(
                    json!({"_success": true, "_signature": sig, "_note": "Complex return type - call succeeded"}),
                )
            }
        }
    }
}

/// Factory for creating D-Bus tools from introspection data
pub struct DbusToolFactory;

impl DbusToolFactory {
    /// Convert introspected methods into tools
    pub fn methods_to_tools(
        bus_type: BusType,
        service: &str,
        path: &str,
        interface: &str,
        methods: &[MethodInfo],
    ) -> Vec<std::sync::Arc<dyn Tool>> {
        methods
            .iter()
            .filter(|method| {
                // Skip methods that use file descriptors
                let uses_fd = method.in_args.iter().any(|a| a.signature.contains('h'))
                    || method.out_args.iter().any(|a| a.signature.contains('h'));
                !uses_fd
            })
            .map(|method| {
                std::sync::Arc::new(DbusMethodTool::new(
                    bus_type,
                    service.to_string(),
                    path.to_string(),
                    interface.to_string(),
                    method.clone(),
                )) as std::sync::Arc<dyn Tool>
            })
            .collect()
    }
}
