use anyhow::Result;
use futures::{stream::iter, StreamExt};
use sha2::{Digest, Sha256};
use simd_json::OwnedValue as Value;
use std::sync::Arc;
use tokio::sync::RwLock as AsyncRwLock;

use crate::IntrospectionService;
pub use op_core::types::BusType;

use op_core::types::ObjectSchemaRef;
use op_snowball::StreamingSnowball;

// ============================================================================
// D-BUS PROJECTION
// ============================================================================

/// D-Bus Projection - delegates to op-introspection for all introspection
///
/// All results are JSON-serializable. No raw XML exposed.
///
/// For restorable system configs:
/// - Uses StreamingSnowball.write_state() for BTRFS state subvolume
/// - Triggers snowball block to signal state change for backup
#[derive(Clone)]
pub struct DbusProjection {
    introspection: Arc<IntrospectionService>,
    snowball: Option<Arc<AsyncRwLock<StreamingSnowball>>>,
}

impl DbusProjection {
    /// Create a new D-Bus projection
    pub fn new() -> Self {
        Self {
            introspection: Arc::new(IntrospectionService::new()),
            snowball: None,
        }
    }

    /// Create with shared introspection service
    pub fn with_service(introspection: Arc<IntrospectionService>) -> Self {
        Self {
            introspection,
            snowball: None,
        }
    }

    /// Attach a StreamingSnowball for restorable state persistence
    /// JSON writes go to state_subvol (BTRFS) and trigger snowball backup
    pub fn with_snowball(mut self, snowball: Arc<AsyncRwLock<StreamingSnowball>>) -> Self {
        self.snowball = Some(snowball);
        self
    }

    /// List services on a bus - returns JSON
    pub async fn list_services(&self, bus_type: BusType) -> Result<Value> {
        let json = self.introspection.list_services_json(bus_type).await?;
        Ok(json)
    }

    /// Introspect a service/object - returns JSON
    ///
    /// XML is parsed internally by op-introspection; this returns pure JSON
    pub async fn introspect(&self, bus_type: BusType, service: &str, path: &str) -> Result<Value> {
        let json = self
            .introspection
            .introspect_json(bus_type, service, path)
            .await?;
        Ok(json)
    }

    /// Introspect and get structured ObjectInfo (for plugin schema linking)
    pub async fn introspect_object(
        &self,
        bus_type: BusType,
        service: &str,
        path: &str,
    ) -> Result<op_core::types::ObjectInfo> {
        let info = self
            .introspection
            .introspect(bus_type, service, path)
            .await?;
        Ok(info)
    }

    /// Introspect and persist to BTRFS state subvolume (restorable system config)
    ///
    /// This writes JSON to the snowball's state_subvol AND triggers a snowball
    /// block to signal that restorable state has changed (for backup)
    ///
    /// Only use this for managed services that should be restored in disaster recovery.
    pub async fn introspect_and_persist(
        &self,
        bus_type: BusType,
        service: &str,
        path: &str,
    ) -> Result<ObjectSchemaRef> {
        let json = self.introspect(bus_type, service, path).await?;

        // Compute schema hash
        let json_str = simd_json::to_string_pretty(&json)?;
        let schema_hash = {
            let mut hasher = Sha256::new();
            hasher.update(json_str.as_bytes());
            hex::encode(hasher.finalize())
        };

        let state_key = format!(
            "dbus/{}/{}",
            service.replace('.', "_"),
            path.replace('/', "_")
        );

        // Write to BTRFS state subvolume AND trigger snowball block
        if let Some(snowball) = &self.snowball {
            let bc = snowball.read().await;

            // Write JSON to state_subvol (restorable system config)
            bc.write_state(&state_key, &json).await?;

            // Trigger snowball block to signal state change for backup
            bc.add_event(op_snowball::BlockEvent::new(
                "dbus.schema.update",
                &schema_hash,
                simd_json::json!({"service": service, "path": path}),
            ))
            .await?;

            tracing::debug!(
                "Persisted D-Bus schema to BTRFS state subvol: {}",
                state_key
            );
        }

        Ok(ObjectSchemaRef::new(
            "dbus_interface",
            service,
            path,
            schema_hash,
        ))
    }

    /// Discover and persist all interfaces for a managed service
    /// (e.g., PackageKit, systemd, NetworkManager)
    pub async fn discover_service(
        &self,
        bus_type: BusType,
        service: &str,
    ) -> Result<Vec<ObjectSchemaRef>> {
        let root_info = self.introspect_object(bus_type, service, "/").await?;
        let schemas = Arc::new(tokio::sync::Mutex::new(Vec::new()));

        // Persist root
        if let Ok(schema) = self.introspect_and_persist(bus_type, service, "/").await {
            schemas.lock().await.push(schema);
        }

        let self_clone = self.clone();

        // Recursively discover children in parallel
        iter(root_info.children)
            .for_each_concurrent(None, |child: String| {
                let child_path = if child.starts_with('/') {
                    child.clone()
                } else {
                    format!("/{}", child)
                };
                let schemas = schemas.clone();
                let self_clone = self_clone.clone();

                async move {
                    if let Ok(schema) = self_clone
                        .introspect_and_persist(bus_type, service, &child_path)
                        .await
                    {
                        schemas.lock().await.push(schema);
                    }
                }
            })
            .await;

        let final_schemas = Arc::try_unwrap(schemas).unwrap().into_inner();
        tracing::info!(
            "Discovered {} schemas for service {} (BTRFS state + snowball trigger)",
            final_schemas.len(),
            service
        );
        Ok(final_schemas)
    }

    /// Get access to underlying introspection service
    pub fn introspection_service(&self) -> Arc<IntrospectionService> {
        Arc::clone(&self.introspection)
    }
}

impl Default for DbusProjection {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bus_type_display() {
        assert_eq!(format!("{}", BusType::System), "system");
        assert_eq!(format!("{}", BusType::Session), "session");
    }
}
