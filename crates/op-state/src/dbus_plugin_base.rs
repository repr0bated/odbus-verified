#![allow(async_fn_in_trait)]
//! Base trait for D-Bus state plugins
//! Provides common D-Bus operations, hash footprints, and snowball integration

// Snowball module not yet added - stub the type for now
pub struct PluginFootprint;

impl PluginFootprint {
    pub fn new(_plugin_name: String, _action: String, _diff_data: simd_json::OwnedValue) -> Self {
        PluginFootprint
    }
}
use crate::plugin::StatePlugin;
use anyhow::{Context, Result};
use simd_json::prelude::*;
use simd_json::OwnedValue as Value;
use std::collections::HashMap;
use tokio::sync::mpsc::UnboundedSender;
use zbus::{Connection, Proxy};

/// Base trait for all D-Bus-based state plugins
/// Provides common functionality for interacting with D-Bus services
#[allow(dead_code, async_fn_in_trait)]
pub trait DbusStatePluginBase: StatePlugin {
    /// D-Bus service name (e.g., "org.freedesktop.systemd1")
    fn service_name(&self) -> &str;

    /// Base object path (e.g., "/org/freedesktop/systemd1")
    fn base_path(&self) -> &str;

    /// Optional snowball footprint sender
    fn snowball_sender(&self) -> Option<&UnboundedSender<PluginFootprint>> {
        None
    }

    /// Connect to D-Bus service and create proxy
    async fn connect_dbus(&self, path: &str, interface: &str) -> Result<Proxy<'static>> {
        let conn = Connection::system()
            .await
            .context("Failed to connect to system D-Bus")?;

        // Convert to owned strings to satisfy 'static lifetime
        let service_name = self.service_name().to_string();
        let path_owned = path.to_string();
        let interface_owned = interface.to_string();

        Proxy::new(&conn, service_name, path_owned, interface_owned)
            .await
            .context(format!(
                "Failed to create D-Bus proxy for {}/{}",
                self.service_name(),
                interface
            ))
    }

    /// Get a D-Bus property value
    async fn get_property(&self, proxy: &Proxy<'_>, property: &str) -> Result<Value> {
        let value: zbus::zvariant::OwnedValue = proxy
            .get_property(property)
            .await
            .context(format!("Failed to get property {}", property))?;

        conversion::zvariant_to_json(&value)
            .with_context(|| format!("Failed to convert property {property}"))
    }

    /// Set a D-Bus property value
    async fn set_property(&self, proxy: &Proxy<'_>, property: &str, value: &Value) -> Result<()> {
        // Convert simd_json::OwnedValue to zbus::zvariant::Value (simplified)
        let zbus_value: zbus::zvariant::Value = if let Some(s) = value.as_str() {
            s.into()
        } else if let Some(b) = value.as_bool() {
            b.into()
        } else if let Some(i) = value.as_i64() {
            i.into()
        } else if let Some(f) = value.as_f64() {
            f.into()
        } else {
            return Err(anyhow::anyhow!("Unsupported value type for D-Bus property"));
        };

        proxy
            .set_property(property, zbus_value)
            .await
            .context(format!("Failed to set property {}", property))?;

        Ok(())
    }

    /// Get all properties from a D-Bus interface
    async fn get_all_properties(&self, proxy: &Proxy<'_>) -> Result<HashMap<String, Value>> {
        // Use org.freedesktop.DBus.Properties.GetAll
        let props_proxy = Proxy::new(
            proxy.connection(),
            proxy.destination(),
            proxy.path(),
            "org.freedesktop.DBus.Properties",
        )
        .await?;

        let all_props: HashMap<String, zbus::zvariant::OwnedValue> = props_proxy
            .call("GetAll", &(proxy.interface(),))
            .await
            .context("Failed to get all properties")?;

        // Convert to simd_json::OwnedValue HashMap
        let mut result = HashMap::new();
        for (key, value) in all_props {
            let converted = conversion::zvariant_to_json(&value)
                .with_context(|| format!("Failed to convert property {key}"))?;
            result.insert(key, converted);
        }

        Ok(result)
    }

    /// Call a D-Bus method (no-arg version - for methods with args, use proxy.call directly)
    async fn call_method_no_args(
        &self,
        proxy: &Proxy<'_>,
        method: &str,
    ) -> Result<zbus::zvariant::OwnedValue> {
        proxy
            .call(method, &())
            .await
            .context(format!("Failed to call method {}", method))
    }

    /// Introspect D-Bus object to get schema
    async fn introspect(&self, path: &str) -> Result<String> {
        let conn = Connection::system().await?;
        let proxy = Proxy::new(
            &conn,
            self.service_name(),
            path,
            "org.freedesktop.DBus.Introspectable",
        )
        .await?;

        let xml: String = proxy
            .call("Introspect", &())
            .await
            .context("Failed to introspect D-Bus object")?;

        Ok(xml)
    }

    /// Calculate cryptographic hash of state (SHA-256)
    fn hash_state(&self, state: &Value) -> String {
        use sha2::{Digest, Sha256};
        let json_str = simd_json::to_string(state).unwrap_or_default();
        format!("{:x}", Sha256::digest(json_str.as_bytes()))
    }

    /// Calculate diff between two states and return hash footprint
    fn calculate_footprint(
        &self,
        old_state: &Value,
        new_state: &Value,
        action: &str,
    ) -> PluginFootprint {
        let diff_data = simd_json::json!({
            "old": old_state,
            "new": new_state,
            "old_hash": self.hash_state(old_state),
            "new_hash": self.hash_state(new_state),
        });

        PluginFootprint::new(self.name().to_string(), action.to_string(), diff_data)
    }

    /// Record state change to snowball
    async fn record_footprint(&self, action: &str, data: Value) -> Result<()> {
        if let Some(sender) = self.snowball_sender() {
            let footprint = PluginFootprint::new(self.name().to_string(), action.to_string(), data);

            sender
                .send(footprint)
                .map_err(|e| anyhow::anyhow!("Failed to send footprint to snowball: {}", e))?;

            log::debug!("Recorded footprint for {} action: {}", self.name(), action);
        } else {
            log::trace!("No snowball sender configured, skipping footprint");
        }

        Ok(())
    }

    /// Record a state transition (before/after)
    async fn record_state_transition(
        &self,
        old_state: &Value,
        new_state: &Value,
        action: &str,
    ) -> Result<()> {
        let footprint_data = simd_json::json!({
            "old_state": old_state,
            "new_state": new_state,
            "old_hash": self.hash_state(old_state),
            "new_hash": self.hash_state(new_state),
            "action": action,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });

        self.record_footprint(action, footprint_data).await
    }

    /// Verify state hash matches expected
    fn verify_state_hash(&self, state: &Value, expected_hash: &str) -> bool {
        self.hash_state(state) == expected_hash
    }

    /// Get D-Bus connection
    async fn get_connection(&self) -> Result<Connection> {
        Connection::system()
            .await
            .context("Failed to connect to system D-Bus")
    }

    /// List all object paths under a base path (for enumeration)
    async fn list_objects(&self, base_path: &str) -> Result<Vec<String>> {
        // This would use D-Bus introspection to walk the object tree
        // Simplified implementation
        let xml = self.introspect(base_path).await?;

        // Parse XML and extract child nodes
        // For now, return empty - full implementation would parse introspection XML
        log::debug!("Introspection XML for {}: {}", base_path, xml);

        Ok(Vec::new())
    }
}

/// Helper functions for D-Bus value conversion
pub mod conversion {
    use super::Value;
    use simd_json::prelude::*;
    use simd_json::ValueBuilder;
    use zbus::zvariant;

    /// Convert simd_json::OwnedValue to zbus::zvariant::Value
    #[allow(dead_code)]
    pub fn json_to_zvariant(value: &Value) -> Result<zvariant::Value<'_>, anyhow::Error> {
        if value.is_null() {
            Ok(zvariant::Value::from(""))
        } else if let Some(b) = value.as_bool() {
            Ok(zvariant::Value::from(b))
        } else if let Some(i) = value.as_i64() {
            Ok(zvariant::Value::from(i))
        } else if let Some(u) = value.as_u64() {
            Ok(zvariant::Value::from(u))
        } else if let Some(f) = value.as_f64() {
            Ok(zvariant::Value::from(f))
        } else if let Some(s) = value.as_str() {
            Ok(zvariant::Value::from(s))
        } else {
            Err(anyhow::anyhow!("Unsupported value type for conversion"))
        }
    }

    /// Convert zbus::zvariant::Value to simd_json::OwnedValue
    #[allow(dead_code)]
    pub fn zvariant_to_json(value: &zvariant::Value) -> Result<Value, anyhow::Error> {
        use zvariant::Value as Z;
        Ok(match value {
            Z::U8(v) => Value::from(*v),
            Z::Bool(v) => Value::from(*v),
            Z::I16(v) => Value::from(*v),
            Z::U16(v) => Value::from(*v),
            Z::I32(v) => Value::from(*v),
            Z::U32(v) => Value::from(*v),
            Z::I64(v) => Value::from(*v),
            Z::U64(v) => Value::from(*v),
            Z::F64(v) if v.is_finite() => Value::from(*v),
            Z::Str(v) => Value::from(v.as_str()),
            Z::ObjectPath(v) => Value::from(v.as_str()),
            Z::Signature(v) => Value::from(v.to_string()),
            Z::Value(v) => zvariant_to_json(v)?,
            Z::Array(v) => Value::from(
                v.inner()
                    .iter()
                    .map(zvariant_to_json)
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            Z::Structure(v) => Value::from(
                v.fields()
                    .iter()
                    .map(zvariant_to_json)
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            Z::Dict(v) => {
                let mut object = Value::object();
                for (key, value) in v.iter() {
                    let key = match key {
                        Z::Str(v) => v.as_str(),
                        _ => {
                            return Err(anyhow::anyhow!(
                                "JSON object requires string D-Bus dictionary keys"
                            ))
                        }
                    };
                    object.insert(key, zvariant_to_json(value)?)?;
                }
                object
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "Unsupported D-Bus JSON value with signature {}",
                    value.value_signature()
                ))
            }
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn preserves_values_instead_of_parsing_debug_text() {
            for value in [
                zvariant::Value::from(42i32),
                zvariant::Value::from(42u32),
                zvariant::Value::from(42i64),
            ] {
                assert_eq!(zvariant_to_json(&value).unwrap().as_i64(), Some(42));
            }
            assert_eq!(
                zvariant_to_json(&zvariant::Value::from(true)).unwrap(),
                Value::from(true)
            );
            let array = zvariant::Value::from(vec![1u32, 2u32]);
            assert_eq!(zvariant_to_json(&array).unwrap(), simd_json::json!([1, 2]));
            assert!(zvariant_to_json(&zvariant::Value::from(f64::NAN)).is_err());
        }
    }
}
