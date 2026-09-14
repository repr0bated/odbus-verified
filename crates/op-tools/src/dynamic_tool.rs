use crate::tool::Tool;
use anyhow::Result;
use async_trait::async_trait;
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};

/// A dynamically generated tool wrapping a specific D-Bus method
#[derive(Clone)]
pub struct DynamicDbusTool {
    pub name: String,
    pub service: String,
    pub path: String,
    pub interface: String,
    pub method: String,
    pub signature: String,
    pub arg_names: Vec<String>,
}

impl DynamicDbusTool {
    pub fn new(
        service: String,
        path: String,
        interface: String,
        method: String,
        signature: String,
        arg_names: Vec<String>,
    ) -> Self {
        let name = Self::compute_name(&service, &interface, &method);
        Self {
            name,
            service,
            path,
            interface,
            method,
            signature,
            arg_names,
        }
    }

    fn compute_name(service: &str, interface: &str, method: &str) -> String {
        let svc_short = service.split('.').next_back().unwrap_or(service);
        let iface_short = interface.split('.').next_back().unwrap_or(interface);

        let method_snake = method
            .chars()
            .enumerate()
            .map(|(i, c)| {
                if i > 0 && c.is_uppercase() {
                    format!("_{}", c.to_lowercase())
                } else {
                    c.to_lowercase().to_string()
                }
            })
            .collect::<String>();

        format!(
            "{}.{}.{}",
            svc_short,
            iface_short.to_lowercase(),
            method_snake
        )
    }
}

#[async_trait]
impl Tool for DynamicDbusTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "Dynamically projected D-Bus method"
    }

    fn input_schema(&self) -> Value {
        let mut props = simd_json::value::owned::Object::new();
        for arg in &self.arg_names {
            props.insert(arg.clone(), json!({"type": "string"}));
        }

        json!({
            "type": "object",
            "properties": props,
            "required": self.arg_names
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let connection = zbus::Connection::system()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to connect to system bus: {}", e))?;

        let proxy: zbus::Proxy = zbus::proxy::Builder::new(&connection)
            .destination(self.service.as_str())?
            .path(self.path.as_str())?
            .interface(self.interface.as_str())?
            .build()
            .await?;

        // Convert input map to ordered arguments based on arg_names
        let mut args = Vec::new();
        for name in &self.arg_names {
            let val = input
                .get(name)
                .ok_or_else(|| anyhow::anyhow!("Missing argument: {}", name))?;

            // Basic conversion - use zbus::zvariant::Value<'static>
            let zval: zbus::zvariant::Value<'static> = if let Some(s) = val.as_str() {
                zbus::zvariant::Value::new(s.to_string())
            } else if let Some(b) = val.as_bool() {
                zbus::zvariant::Value::new(b)
            } else if let Some(i) = val.as_i64() {
                zbus::zvariant::Value::new(i)
            } else if let Some(u) = val.as_u64() {
                zbus::zvariant::Value::new(u)
            } else if let Some(f) = val.as_f64() {
                zbus::zvariant::Value::new(f)
            } else {
                return Err(anyhow::anyhow!("Unsupported argument type for {}", name));
            };
            args.push(zval);
        }

        let result: zbus::zvariant::OwnedValue = proxy.call(self.method.as_str(), &args).await?;
        let result_json = simd_json::serde::to_owned_value(&result)?;

        Ok(result_json)
    }
}
