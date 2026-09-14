//! DBus service scanning

use std::collections::HashMap;
use tracing::debug;

use op_core::{
    BusType, Error, InterfaceInfo, MethodInfo, ObjectInfo, PropertyInfo, Result, ServiceInfo,
    SignalInfo,
};

/// Service scanner for DBus
pub struct ServiceScanner {
    _cache: HashMap<(BusType, String), Vec<ServiceInfo>>,
}

impl ServiceScanner {
    pub fn new() -> Self {
        Self {
            _cache: HashMap::new(),
        }
    }

    /// List all services on a bus
    pub async fn list_services(&self, bus_type: BusType) -> Result<Vec<ServiceInfo>> {
        let connection = match bus_type {
            BusType::System => zbus::Connection::system().await?,
            BusType::Session => zbus::Connection::session().await?,
        };

        let proxy = zbus::fdo::DBusProxy::new(&connection).await?;
        let names = proxy.list_names().await?;

        let mut services = Vec::new();
        for name in names {
            let name_str = name.to_string();
            // Skip private names
            if name_str.starts_with(':') {
                continue;
            }

            services.push(ServiceInfo {
                name: name_str.clone(),
                bus_type,
                activatable: false,
                active: true,
                pid: None,
                uid: None,
            });
        }

        debug!("Found {} services on {:?} bus", services.len(), bus_type);
        Ok(services)
    }

    /// Introspect a specific service/path
    pub async fn introspect(
        &self,
        bus_type: BusType,
        service: &str,
        path: &str,
    ) -> Result<ObjectInfo> {
        let connection = match bus_type {
            BusType::System => zbus::Connection::system().await?,
            BusType::Session => zbus::Connection::session().await?,
        };

        let proxy = zbus::fdo::IntrospectableProxy::builder(&connection)
            .destination(service)?
            .path(path)?
            .build()
            .await?;

        let xml = proxy.introspect().await?;

        // Parse the XML
        let obj_info = parse_introspection_xml(&xml, path)?;

        debug!(
            "Introspected {} {} with {} interfaces",
            service,
            path,
            obj_info.interfaces.len()
        );
        Ok(obj_info)
    }
}

impl Default for ServiceScanner {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse DBus introspection XML
fn parse_introspection_xml(xml: &str, path: &str) -> Result<ObjectInfo> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();

    let mut interfaces = Vec::new();
    let mut children = Vec::new();
    let mut node_depth: usize = 0;

    let mut current_interface: Option<InterfaceInfo> = None;
    let mut current_method: Option<MethodInfo> = None;
    let mut current_signal: Option<SignalInfo> = None;
    let _current_property: Option<PropertyInfo> = None;
    let mut in_method = false;
    let mut in_signal = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name_bytes = e.name();
                let name = std::str::from_utf8(name_bytes.as_ref()).unwrap_or("");

                match name {
                    "node" => {
                        // Capture only direct children of the current node.
                        if node_depth == 1 {
                            for attr in e.attributes().flatten() {
                                if attr.key.as_ref() == b"name" {
                                    let child_name =
                                        String::from_utf8_lossy(&attr.value).to_string();
                                    let child_path = if child_name.starts_with('/') {
                                        child_name
                                    } else if path == "/" {
                                        format!("/{}", child_name)
                                    } else {
                                        format!("{}/{}", path, child_name)
                                    };
                                    children.push(child_path);
                                }
                            }
                        }
                        node_depth += 1;
                    }
                    "interface" => {
                        if node_depth != 1 {
                            continue;
                        }
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"name" {
                                let iface_name = String::from_utf8_lossy(&attr.value).to_string();
                                current_interface = Some(InterfaceInfo {
                                    name: iface_name,
                                    methods: Vec::new(),
                                    properties: Vec::new(),
                                    signals: Vec::new(),
                                });
                            }
                        }
                    }
                    "method" => {
                        if node_depth != 1 {
                            continue;
                        }
                        in_method = true;
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"name" {
                                current_method = Some(MethodInfo {
                                    name: String::from_utf8_lossy(&attr.value).to_string(),
                                    in_args: Vec::new(),
                                    out_args: Vec::new(),
                                    annotations: std::collections::HashMap::new(),
                                });
                            }
                        }
                    }
                    "signal" => {
                        if node_depth != 1 {
                            continue;
                        }
                        in_signal = true;
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"name" {
                                current_signal = Some(SignalInfo {
                                    name: String::from_utf8_lossy(&attr.value).to_string(),
                                    args: Vec::new(),
                                });
                            }
                        }
                    }
                    "property" => {
                        if node_depth != 1 {
                            continue;
                        }
                        let mut prop_name = String::new();
                        let mut prop_type = String::new();
                        let mut prop_access = String::new();

                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"name" => {
                                    prop_name = String::from_utf8_lossy(&attr.value).to_string()
                                }
                                b"type" => {
                                    prop_type = String::from_utf8_lossy(&attr.value).to_string()
                                }
                                b"access" => {
                                    prop_access = String::from_utf8_lossy(&attr.value).to_string()
                                }
                                _ => {}
                            }
                        }

                        let access = match prop_access.as_str() {
                            "read" => op_core::PropertyAccess::Read,
                            "write" => op_core::PropertyAccess::Write,
                            "readwrite" => op_core::PropertyAccess::ReadWrite,
                            _ => op_core::PropertyAccess::Read,
                        };

                        if let Some(ref mut iface) = current_interface {
                            iface.properties.push(PropertyInfo {
                                name: prop_name,
                                signature: prop_type,
                                access,
                            });
                        }
                    }
                    "arg" => {
                        if node_depth != 1 {
                            continue;
                        }
                        let mut arg_name = String::new();
                        let mut arg_type = String::new();
                        let mut arg_direction = "in".to_string();

                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"name" => {
                                    arg_name = String::from_utf8_lossy(&attr.value).to_string()
                                }
                                b"type" => {
                                    arg_type = String::from_utf8_lossy(&attr.value).to_string()
                                }
                                b"direction" => {
                                    arg_direction = String::from_utf8_lossy(&attr.value).to_string()
                                }
                                _ => {}
                            }
                        }

                        let arg = op_core::ArgInfo {
                            name: if arg_name.is_empty() {
                                None
                            } else {
                                Some(arg_name)
                            },
                            signature: arg_type,
                            direction: if arg_direction == "out" {
                                op_core::ArgDirection::Out
                            } else {
                                op_core::ArgDirection::In
                            },
                        };

                        if in_method {
                            if let Some(ref mut method) = current_method {
                                if arg_direction == "out" {
                                    method.out_args.push(arg);
                                } else {
                                    method.in_args.push(arg);
                                }
                            }
                        } else if in_signal {
                            if let Some(ref mut signal) = current_signal {
                                signal.args.push(arg);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                let name_bytes = e.name();
                let name = std::str::from_utf8(name_bytes.as_ref()).unwrap_or("");

                match name {
                    "node" if node_depth == 1 => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"name" {
                                let child_name = String::from_utf8_lossy(&attr.value).to_string();
                                let child_path = if child_name.starts_with('/') {
                                    child_name
                                } else if path == "/" {
                                    format!("/{}", child_name)
                                } else {
                                    format!("{}/{}", path, child_name)
                                };
                                children.push(child_path);
                            }
                        }
                    }
                    "node" => {}
                    "property" => {
                        if node_depth != 1 {
                            continue;
                        }
                        let mut prop_name = String::new();
                        let mut prop_type = String::new();
                        let mut prop_access = String::new();

                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"name" => {
                                    prop_name = String::from_utf8_lossy(&attr.value).to_string()
                                }
                                b"type" => {
                                    prop_type = String::from_utf8_lossy(&attr.value).to_string()
                                }
                                b"access" => {
                                    prop_access = String::from_utf8_lossy(&attr.value).to_string()
                                }
                                _ => {}
                            }
                        }

                        let access = match prop_access.as_str() {
                            "read" => op_core::PropertyAccess::Read,
                            "write" => op_core::PropertyAccess::Write,
                            "readwrite" => op_core::PropertyAccess::ReadWrite,
                            _ => op_core::PropertyAccess::Read,
                        };

                        if let Some(ref mut iface) = current_interface {
                            iface.properties.push(PropertyInfo {
                                name: prop_name,
                                signature: prop_type,
                                access,
                            });
                        }
                    }
                    "arg" => {
                        if node_depth != 1 {
                            continue;
                        }
                        let mut arg_name = String::new();
                        let mut arg_type = String::new();
                        let mut arg_direction = "in".to_string();

                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"name" => {
                                    arg_name = String::from_utf8_lossy(&attr.value).to_string()
                                }
                                b"type" => {
                                    arg_type = String::from_utf8_lossy(&attr.value).to_string()
                                }
                                b"direction" => {
                                    arg_direction = String::from_utf8_lossy(&attr.value).to_string()
                                }
                                _ => {}
                            }
                        }

                        let arg = op_core::ArgInfo {
                            name: if arg_name.is_empty() {
                                None
                            } else {
                                Some(arg_name)
                            },
                            signature: arg_type,
                            direction: if arg_direction == "out" {
                                op_core::ArgDirection::Out
                            } else {
                                op_core::ArgDirection::In
                            },
                        };

                        if in_method {
                            if let Some(ref mut method) = current_method {
                                if arg_direction == "out" {
                                    method.out_args.push(arg);
                                } else {
                                    method.in_args.push(arg);
                                }
                            }
                        } else if in_signal {
                            if let Some(ref mut signal) = current_signal {
                                signal.args.push(arg);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let name_bytes = e.name();
                let name = std::str::from_utf8(name_bytes.as_ref()).unwrap_or("");

                match name {
                    "node" => {
                        node_depth = node_depth.saturating_sub(1);
                    }
                    "interface" => {
                        if let Some(iface) = current_interface.take() {
                            interfaces.push(iface);
                        }
                    }
                    "method" => {
                        in_method = false;
                        if let Some(method) = current_method.take() {
                            if let Some(ref mut iface) = current_interface {
                                iface.methods.push(method);
                            }
                        }
                    }
                    "signal" => {
                        in_signal = false;
                        if let Some(signal) = current_signal.take() {
                            if let Some(ref mut iface) = current_interface {
                                iface.signals.push(signal);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(Error::introspection(format!("XML parse error: {}", e))),
            _ => {}
        }
        buf.clear();
    }

    children.sort();
    children.dedup();

    Ok(ObjectInfo {
        path: path.to_string(),
        interfaces,
        children,
    })
}
