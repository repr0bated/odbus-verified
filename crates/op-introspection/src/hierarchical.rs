// Hierarchical D-Bus Introspection with JSON Caching
// Implements comprehensive D-Bus discovery using all methods from the guide:
// - Recursive object path traversal with zbus_xml::Node
// - ObjectManager.GetManagedObjects for bulk discovery
// - Proper handling of non-introspectable objects
// - Full interface, method, signal, and property introspection
// - JSON caching to BTRFS @cache/introspection/ subvolume

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{debug, info, warn};
use zbus::zvariant;
use zbus::{Connection, Proxy};
use zbus_xml::Node;

/// Hierarchical D-Bus introspection snapshot
/// Stored as JSON in @cache/introspection/{timestamp}.json
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HierarchicalIntrospection {
    /// Timestamp of snapshot
    pub timestamp: String,

    /// System bus services
    pub system_bus: BusIntrospection,

    /// Session bus services
    pub session_bus: BusIntrospection,

    /// Summary statistics
    pub summary: IntrospectionSummary,
}

/// Introspection data for a single bus
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BusIntrospection {
    /// All services on this bus (indexed by service name)
    pub services: HashMap<String, ServiceIntrospection>,

    /// Total object count across all services
    pub total_objects: usize,

    /// Total interface count
    pub total_interfaces: usize,
}

/// Complete introspection data for a D-Bus service
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ServiceIntrospection {
    /// Service name (e.g., "org.freedesktop.NetworkManager")
    pub name: String,

    /// Bus type ("system" or "session")
    pub bus_type: String,

    /// All object paths in this service
    pub objects: HashMap<String, ObjectIntrospection>,

    /// Whether ObjectManager was used for discovery
    pub used_object_manager: bool,

    /// Root object path (typically / or /org/freedesktop/ServiceName)
    pub root_path: String,
}

/// Complete introspection data for a D-Bus object
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ObjectIntrospection {
    /// Object path (e.g., "/org/freedesktop/NetworkManager")
    pub path: String,

    /// Interfaces implemented by this object
    pub interfaces: Vec<InterfaceIntrospection>,

    /// Child object paths (for tree traversal)
    pub children: Vec<String>,

    /// Whether this object is introspectable
    pub introspectable: bool,

    /// Error message if introspection failed
    pub error: Option<String>,
}

/// Complete interface introspection data
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InterfaceIntrospection {
    /// Interface name (e.g., "org.freedesktop.NetworkManager")
    pub name: String,

    /// Methods on this interface
    pub methods: Vec<MethodIntrospection>,

    /// Properties on this interface
    pub properties: Vec<PropertyIntrospection>,

    /// Signals on this interface
    pub signals: Vec<SignalIntrospection>,
}

/// Method introspection data
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MethodIntrospection {
    /// Method name (e.g., "GetDevices")
    pub name: String,

    /// Input arguments
    pub inputs: Vec<ArgumentIntrospection>,

    /// Output arguments
    pub outputs: Vec<ArgumentIntrospection>,
}

/// Property introspection data
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PropertyIntrospection {
    /// Property name
    pub name: String,

    /// D-Bus type signature
    pub type_: String,

    /// Access mode ("read", "write", "readwrite")
    pub access: String,
}

/// Signal introspection data
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SignalIntrospection {
    /// Signal name
    pub name: String,

    /// Signal arguments
    pub args: Vec<ArgumentIntrospection>,
}

/// Method/Signal argument
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ArgumentIntrospection {
    /// Argument name
    pub name: Option<String>,

    /// D-Bus type signature
    pub type_: String,

    /// Direction ("in" or "out")
    pub direction: Option<String>,
}

/// Summary statistics
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IntrospectionSummary {
    pub total_services: usize,
    pub total_objects: usize,
    pub total_interfaces: usize,
    pub total_methods: usize,
    pub non_introspectable_objects: usize,
    pub services_with_object_manager: usize,
}

/// Hierarchical D-Bus introspector
pub struct HierarchicalIntrospector {
    cache_dir: PathBuf,
}

impl HierarchicalIntrospector {
    /// Create new introspector with cache directory
    pub async fn new(cache_dir: PathBuf) -> Result<Self> {
        // Create @cache/introspection subvolume if needed
        let introspection_cache = cache_dir.join("introspection");

        // Check if parent @cache exists
        if !cache_dir.exists() {
            tokio::fs::create_dir_all(&cache_dir).await?;
        }

        // Create introspection subdirectory
        tokio::fs::create_dir_all(&introspection_cache).await?;

        info!(
            "Hierarchical introspection cache: {}",
            introspection_cache.display()
        );

        Ok(Self { cache_dir })
    }

    /// Perform comprehensive introspection of both buses
    pub async fn introspect_all(&self) -> Result<HierarchicalIntrospection> {
        info!("Starting comprehensive D-Bus introspection");

        let timestamp = chrono::Utc::now().to_rfc3339();

        // Introspect system bus
        info!("Introspecting system bus...");
        let system_bus = self.introspect_bus("system").await?;

        // Introspect session bus (may not be available in all contexts)
        info!("Introspecting session bus...");
        let session_bus = match self.introspect_bus("session").await {
            Ok(bus) => bus,
            Err(e) => {
                warn!("Session bus not available: {}", e);
                BusIntrospection {
                    services: HashMap::new(),
                    total_objects: 0,
                    total_interfaces: 0,
                }
            }
        };

        // Calculate summary
        let summary = Self::calculate_summary(&system_bus, &session_bus);

        let introspection = HierarchicalIntrospection {
            timestamp,
            system_bus,
            session_bus,
            summary,
        };

        // Save to cache
        self.save_to_cache(&introspection).await?;

        info!(
            "Introspection complete: {} services, {} objects, {} interfaces",
            introspection.summary.total_services,
            introspection.summary.total_objects,
            introspection.summary.total_interfaces
        );

        Ok(introspection)
    }

    /// Introspect a single bus (system or session)
    async fn introspect_bus(&self, bus_type: &str) -> Result<BusIntrospection> {
        // Connect to bus
        let connection = match bus_type {
            "system" => Connection::system().await?,
            "session" => Connection::session().await?,
            _ => anyhow::bail!("Invalid bus type: {}", bus_type),
        };

        // Get list of all services on the bus
        let service_names = self.list_services(&connection).await?;

        info!("Found {} services on {} bus", service_names.len(), bus_type);

        let mut services = HashMap::new();
        let mut total_objects = 0;
        let mut total_interfaces = 0;

        for service_name in service_names {
            debug!("Introspecting service: {}", service_name);

            match self
                .introspect_service(&connection, &service_name, bus_type)
                .await
            {
                Ok(service_data) => {
                    total_objects += service_data.objects.len();
                    total_interfaces += service_data
                        .objects
                        .values()
                        .map(|obj| obj.interfaces.len())
                        .sum::<usize>();

                    services.insert(service_name.clone(), service_data);
                }
                Err(e) => {
                    warn!("Failed to introspect {}: {}", service_name, e);
                }
            }
        }

        Ok(BusIntrospection {
            services,
            total_objects,
            total_interfaces,
        })
    }

    /// List all service names on a bus
    async fn list_services(&self, conn: &Connection) -> Result<Vec<String>> {
        use zbus::fdo::DBusProxy;

        let proxy = DBusProxy::new(conn).await?;
        let names = proxy.list_names().await?;

        // Filter out unique names (starting with :) and org.freedesktop.DBus itself
        Ok(names
            .into_iter()
            .filter(|name| !name.starts_with(':'))
            .filter(|name| name.as_str() != "org.freedesktop.DBus")
            .map(|name| name.to_string())
            .collect())
    }

    /// Introspect a single service completely
    async fn introspect_service(
        &self,
        conn: &Connection,
        service_name: &str,
        bus_type: &str,
    ) -> Result<ServiceIntrospection> {
        let mut objects = HashMap::new();
        let mut used_object_manager = false;

        // Try ObjectManager first (most efficient)
        let root_path = Self::guess_root_path(service_name);

        if let Ok(managed_objects) = self
            .try_object_manager(conn, service_name, &root_path)
            .await
        {
            info!("Service {} provides ObjectManager", service_name);
            used_object_manager = true;

            // Parse managed objects into our format
            for (path, iface_data) in managed_objects {
                let obj_data = self
                    .introspect_object_by_path(conn, service_name, &path)
                    .await?;

                objects.insert(path.to_string(), obj_data);
            }
        } else {
            // Fall back to recursive introspection
            debug!(
                "ObjectManager not available for {}, using recursive introspection",
                service_name
            );

            self.introspect_recursively(conn, service_name, &root_path, &mut objects)
                .await?;
        }

        Ok(ServiceIntrospection {
            name: service_name.to_string(),
            bus_type: bus_type.to_string(),
            objects,
            used_object_manager,
            root_path,
        })
    }

    /// Try to use ObjectManager.GetManagedObjects for bulk discovery
    async fn try_object_manager(
        &self,
        conn: &Connection,
        service_name: &str,
        root_path: &str,
    ) -> Result<HashMap<String, HashMap<String, HashMap<String, zvariant::OwnedValue>>>> {
        let proxy = Proxy::new(
            conn,
            service_name,
            root_path,
            "org.freedesktop.DBus.ObjectManager",
        )
        .await?;

        // Call GetManagedObjects
        let result: HashMap<
            zbus::zvariant::OwnedObjectPath,
            HashMap<String, HashMap<String, zbus::zvariant::OwnedValue>>,
        > = proxy.call("GetManagedObjects", &()).await?;

        // Convert to string keys
        Ok(result
            .into_iter()
            .map(|(path, ifaces)| {
                (
                    path.to_string(),
                    ifaces
                        .into_iter()
                        .map(|(iface, props)| (iface.to_string(), props))
                        .collect(),
                )
            })
            .collect())
    }

    /// Recursively introspect object tree starting from a root path
    async fn introspect_recursively(
        &self,
        conn: &Connection,
        service_name: &str,
        path: &str,
        objects: &mut HashMap<String, ObjectIntrospection>,
    ) -> Result<()> {
        // Introspect this object
        let obj_data = self
            .introspect_object_by_path(conn, service_name, path)
            .await?;

        // Collect children before inserting (to avoid borrow issues)
        let children = obj_data.children.clone();
        objects.insert(path.to_string(), obj_data);

        // Recurse into children
        for child_name in children {
            let child_path = if path == "/" {
                format!("/{}", child_name)
            } else {
                format!("{}/{}", path, child_name)
            };

            // Recursive call (boxed to avoid infinite-sized future)
            Box::pin(self.introspect_recursively(conn, service_name, &child_path, objects)).await?;
        }

        Ok(())
    }

    /// Introspect a single object by path
    async fn introspect_object_by_path(
        &self,
        conn: &Connection,
        service_name: &str,
        path: &str,
    ) -> Result<ObjectIntrospection> {
        let proxy = Proxy::new(
            conn,
            service_name,
            path,
            "org.freedesktop.DBus.Introspectable",
        )
        .await?;

        // Try to introspect
        match proxy.introspect().await {
            Ok(xml) => {
                // Parse XML with zbus_xml
                let node = Node::from_reader(xml.as_bytes())
                    .context("Failed to parse introspection XML")?;

                // Extract interfaces
                let interfaces = node
                    .interfaces()
                    .iter()
                    .map(|iface| self.parse_interface(iface))
                    .collect();

                // Extract child node names
                let children = node
                    .nodes()
                    .iter()
                    .map(|child| child.name().unwrap_or("").to_string())
                    .filter(|name| !name.is_empty())
                    .collect();

                Ok(ObjectIntrospection {
                    path: path.to_string(),
                    interfaces,
                    children,
                    introspectable: true,
                    error: None,
                })
            }
            Err(e) => {
                // Object is not introspectable
                warn!("Cannot introspect {} on {}: {}", path, service_name, e);

                Ok(ObjectIntrospection {
                    path: path.to_string(),
                    interfaces: Vec::new(),
                    children: Vec::new(),
                    introspectable: false,
                    error: Some(e.to_string()),
                })
            }
        }
    }

    /// Parse interface from zbus_xml::Interface
    fn parse_interface(&self, iface: &zbus_xml::Interface) -> InterfaceIntrospection {
        let methods = iface
            .methods()
            .iter()
            .map(|method| {
                let inputs = method
                    .args()
                    .iter()
                    .filter(|arg| {
                        arg.direction()
                            .map(|d| matches!(d, zbus_xml::ArgDirection::In))
                            .unwrap_or(true)
                    })
                    .map(|arg| ArgumentIntrospection {
                        name: arg.name().map(String::from),
                        type_: arg.ty().to_string(),
                        direction: Some("in".to_string()),
                    })
                    .collect();

                let outputs = method
                    .args()
                    .iter()
                    .filter(|arg| {
                        arg.direction()
                            .map(|d| matches!(d, zbus_xml::ArgDirection::Out))
                            .unwrap_or(false)
                    })
                    .map(|arg| ArgumentIntrospection {
                        name: arg.name().map(String::from),
                        type_: arg.ty().to_string(),
                        direction: Some("out".to_string()),
                    })
                    .collect();

                MethodIntrospection {
                    name: method.name().to_string(),
                    inputs,
                    outputs,
                }
            })
            .collect();

        let properties = iface
            .properties()
            .iter()
            .map(|prop| PropertyIntrospection {
                name: prop.name().to_string(),
                type_: prop.ty().to_string(),
                access: {
                    // In zbus 4, Access enum may have moved
                    // Convert to string representation
                    format!("{:?}", prop.access()).to_lowercase()
                },
            })
            .collect();

        let signals = iface
            .signals()
            .iter()
            .map(|signal| {
                let args = signal
                    .args()
                    .iter()
                    .map(|arg| ArgumentIntrospection {
                        name: arg.name().map(String::from),
                        type_: arg.ty().to_string(),
                        direction: None,
                    })
                    .collect();

                SignalIntrospection {
                    name: signal.name().to_string(),
                    args,
                }
            })
            .collect();

        InterfaceIntrospection {
            name: iface.name().to_string(),
            methods,
            properties,
            signals,
        }
    }

    /// Guess root object path from service name
    fn guess_root_path(service_name: &str) -> String {
        // Common patterns:
        // org.freedesktop.NetworkManager -> /org/freedesktop/NetworkManager
        // org.bluez -> /

        if service_name == "org.bluez" {
            "/".to_string()
        } else {
            format!("/{}", service_name.replace('.', "/"))
        }
    }

    /// Calculate summary statistics
    fn calculate_summary(
        system: &BusIntrospection,
        session: &BusIntrospection,
    ) -> IntrospectionSummary {
        let total_services = system.services.len() + session.services.len();
        let total_objects = system.total_objects + session.total_objects;
        let total_interfaces = system.total_interfaces + session.total_interfaces;

        let total_methods = [system, session]
            .iter()
            .flat_map(|bus| bus.services.values())
            .flat_map(|svc| svc.objects.values())
            .flat_map(|obj| &obj.interfaces)
            .map(|iface| iface.methods.len())
            .sum();

        let non_introspectable_objects = [system, session]
            .iter()
            .flat_map(|bus| bus.services.values())
            .flat_map(|svc| svc.objects.values())
            .filter(|obj| !obj.introspectable)
            .count();

        let services_with_object_manager = [system, session]
            .iter()
            .flat_map(|bus| bus.services.values())
            .filter(|svc| svc.used_object_manager)
            .count();

        IntrospectionSummary {
            total_services,
            total_objects,
            total_interfaces,
            total_methods,
            non_introspectable_objects,
            services_with_object_manager,
        }
    }

    /// Save introspection to cache as JSON
    async fn save_to_cache(&self, data: &HierarchicalIntrospection) -> Result<()> {
        let cache_path = self.cache_dir.join("introspection");
        tokio::fs::create_dir_all(&cache_path).await?;

        // Save timestamped snapshot
        let filename = format!("{}.json", data.timestamp.replace(':', "-"));
        let snapshot_path = cache_path.join(&filename);

        let json = simd_json::to_string_pretty(data)?;
        tokio::fs::write(&snapshot_path, json).await?;

        info!("Saved introspection snapshot: {}", snapshot_path.display());

        // Also save as "latest.json" for easy access
        let latest_path = cache_path.join("latest.json");
        let json = simd_json::to_string_pretty(data)?;
        tokio::fs::write(&latest_path, json).await?;

        Ok(())
    }

    /// Load latest introspection from cache
    pub async fn load_latest(&self) -> Result<HierarchicalIntrospection> {
        let latest_path = self.cache_dir.join("introspection/latest.json");

        if !latest_path.exists() {
            anyhow::bail!("No cached introspection found, run introspect_all() first");
        }

        let json = tokio::fs::read_to_string(&latest_path).await?;
        let data: HierarchicalIntrospection = simd_json::from_str(&json)?;

        Ok(data)
    }

    /// Load introspection by timestamp
    pub async fn load_by_timestamp(&self, timestamp: &str) -> Result<HierarchicalIntrospection> {
        let filename = format!("{}.json", timestamp.replace(':', "-"));
        let path = self.cache_dir.join("introspection").join(&filename);

        let json = tokio::fs::read_to_string(&path).await?;
        let data: HierarchicalIntrospection = simd_json::from_str(&json)?;

        Ok(data)
    }

    /// List all cached introspection timestamps
    pub async fn list_snapshots(&self) -> Result<Vec<String>> {
        let cache_path = self.cache_dir.join("introspection");

        if !cache_path.exists() {
            return Ok(Vec::new());
        }

        let mut snapshots = Vec::new();
        let mut entries = tokio::fs::read_dir(&cache_path).await?;

        while let Some(entry) = entries.next_entry().await? {
            let filename = entry.file_name();
            let filename_str = filename.to_string_lossy();

            if filename_str.ends_with(".json") && filename_str != "latest.json" {
                // Extract timestamp from filename
                let timestamp = filename_str.trim_end_matches(".json").replace('-', ":");
                snapshots.push(timestamp);
            }
        }

        snapshots.sort();
        Ok(snapshots)
    }
}
