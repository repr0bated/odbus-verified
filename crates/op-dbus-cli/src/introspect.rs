//! D-Bus Tree Introspection Adapter for org.opdbus.v1.plugins
//!
//! Connects directly to the opdbus session bus at /run/opdbus/session-bus.sock,
//! walks the plugin tree, and exposes methods, properties, and signals.

use anyhow::{Context, Result};
use op_core::types::{
    ArgDirection, ArgInfo, InterfaceInfo, MethodInfo, PropertyAccess, PropertyInfo, SignalInfo,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tracing::{debug, info};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Well-known service name for the plugin tree
const SERVICE_NAME: &str = "org.opdbus.v1.plugins";

/// Base object path for plugins
const BASE_PATH: &str = "/org/opdbus/v1/plugins";

/// Default session bus socket
const SESSION_BUS_SOCK: &str = "unix:path=/run/opdbus/session-bus.sock";

/// Standard freedesktop interfaces to skip in output
const SKIP_INTERFACES: &[&str] = &[
    "org.freedesktop.DBus.Introspectable",
    "org.freedesktop.DBus.Peer",
    "org.freedesktop.DBus.Properties",
];

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Complete introspection of the org.opdbus.v1.plugins tree
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginTree {
    pub plugins: BTreeMap<String, PluginIntrospection>,
    pub stats: TreeStats,
}

/// Introspection data for a single plugin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginIntrospection {
    pub name: String,
    pub service_name: String,
    pub object_path: String,
    pub interfaces: Vec<InterfaceDetail>,
    pub children: Vec<String>,
}

/// Detailed interface with methods, properties, signals
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceDetail {
    pub name: String,
    pub methods: Vec<MethodDetail>,
    pub properties: Vec<PropertyDetail>,
    pub signals: Vec<SignalDetail>,
}

/// A callable method with typed arguments
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MethodDetail {
    pub name: String,
    pub in_args: Vec<ArgDetail>,
    pub out_args: Vec<ArgDetail>,
    pub signature_display: String,
}

/// A readable/writable property
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyDetail {
    pub name: String,
    pub signature: String,
    pub access: String,
    pub type_display: String,
}

/// A signal the service can emit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalDetail {
    pub name: String,
    pub args: Vec<ArgDetail>,
}

/// A method/signal argument
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArgDetail {
    pub name: String,
    pub signature: String,
    pub direction: String,
    pub type_display: String,
}

/// Summary statistics for the tree
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TreeStats {
    pub plugin_count: usize,
    pub total_methods: usize,
    pub total_properties: usize,
    pub total_signals: usize,
    pub total_interfaces: usize,
}

// ---------------------------------------------------------------------------
// Adapter implementation
// ---------------------------------------------------------------------------

/// The main adapter that introspects the D-Bus tree
pub struct PluginTreeAdapter;

impl PluginTreeAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Connect to the opdbus session bus (hardcoded socket path, no env dependency)
    async fn connect() -> Result<zbus::Connection> {
        let addr = std::env::var("DBUS_SESSION_BUS_ADDRESS")
            .unwrap_or_else(|_| SESSION_BUS_SOCK.to_string());
        zbus::connection::Builder::address(addr.as_str())
            .context("Invalid bus address")?
            .build()
            .await
            .context("Failed to connect to opdbus session bus at /run/opdbus/session-bus.sock")
    }

    /// Discover and introspect all plugins on the tree.
    pub async fn introspect_tree(&self) -> Result<PluginTree> {
        let conn = Self::connect().await?;

        // Introspect the base path to find all child plugin nodes
        let proxy = zbus::fdo::IntrospectableProxy::builder(&conn)
            .destination(SERVICE_NAME)?
            .path(BASE_PATH)?
            .build()
            .await
            .context("Cannot reach org.opdbus.v1.plugins on session bus")?;

        let xml = proxy
            .introspect()
            .await
            .context("Failed to introspect /org/opdbus/v1/plugins")?;

        let children = parse_child_nodes(&xml);
        if children.is_empty() {
            return Ok(PluginTree {
                plugins: BTreeMap::new(),
                stats: TreeStats::default(),
            });
        }

        info!("Discovered {} plugins on session bus", children.len());

        let mut plugins = BTreeMap::new();
        let mut stats = TreeStats::default();

        for plugin_name in &children {
            let object_path = format!("{}/{}", BASE_PATH, plugin_name);
            match self.introspect_one(&conn, &object_path).await {
                Ok(plugin) => {
                    for iface in &plugin.interfaces {
                        stats.total_methods += iface.methods.len();
                        stats.total_properties += iface.properties.len();
                        stats.total_signals += iface.signals.len();
                        stats.total_interfaces += 1;
                    }
                    plugins.insert(plugin_name.clone(), plugin);
                }
                Err(e) => {
                    debug!("Skip {}: {}", plugin_name, e);
                }
            }
        }

        stats.plugin_count = plugins.len();
        Ok(PluginTree { plugins, stats })
    }

    /// Introspect a single plugin object
    async fn introspect_one(
        &self,
        conn: &zbus::Connection,
        object_path: &str,
    ) -> Result<PluginIntrospection> {
        let proxy = zbus::fdo::IntrospectableProxy::builder(conn)
            .destination(SERVICE_NAME)?
            .path(object_path)?
            .build()
            .await?;

        let xml = proxy.introspect().await?;

        let plugin_name = object_path.rsplit('/').next().unwrap_or("").to_string();
        let interfaces = parse_interfaces(&xml);
        let children: Vec<String> = parse_child_nodes(&xml)
            .iter()
            .map(|c| format!("{}/{}", object_path, c))
            .collect();

        let detail_interfaces: Vec<InterfaceDetail> = interfaces
            .iter()
            .filter(|iface| !SKIP_INTERFACES.contains(&iface.name.as_str()))
            .map(convert_interface)
            .collect();

        Ok(PluginIntrospection {
            name: plugin_name,
            service_name: SERVICE_NAME.to_string(),
            object_path: object_path.to_string(),
            interfaces: detail_interfaces,
            children,
        })
    }
}

impl Default for PluginTreeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// XML parsing (minimal, from introspection XML)
// ---------------------------------------------------------------------------

/// Parse child <node name="..."/> entries from introspection XML
fn parse_child_nodes(xml: &str) -> Vec<String> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();
    let mut children = Vec::new();
    let mut depth = 0usize;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) if e.name().as_ref() == b"node" => {
                if depth == 1 {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"name" {
                            children.push(String::from_utf8_lossy(&attr.value).to_string());
                        }
                    }
                }
                depth += 1;
            }
            Ok(Event::Empty(ref e)) if e.name().as_ref() == b"node" && depth == 1 => {
                for attr in e.attributes().flatten() {
                    if attr.key.as_ref() == b"name" {
                        children.push(String::from_utf8_lossy(&attr.value).to_string());
                    }
                }
            }
            Ok(Event::End(ref e)) if e.name().as_ref() == b"node" => {
                depth = depth.saturating_sub(1);
            }
            Ok(Event::Eof) => break,
            _ => {}
        }
        buf.clear();
    }
    children
}

/// Parse interfaces from introspection XML
fn parse_interfaces(xml: &str) -> Vec<InterfaceInfo> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();
    let mut interfaces = Vec::new();
    let mut current_iface: Option<InterfaceInfo> = None;
    let mut current_method: Option<MethodInfo> = None;
    let mut current_signal: Option<SignalInfo> = None;
    let mut in_method = false;
    let mut in_signal = false;
    let mut depth = 0usize;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name_bytes = e.name();
                let tag = std::str::from_utf8(name_bytes.as_ref()).unwrap_or("");
                match tag {
                    "node" => depth += 1,
                    "interface" if depth == 1 => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"name" {
                                current_iface = Some(InterfaceInfo {
                                    name: String::from_utf8_lossy(&attr.value).to_string(),
                                    methods: Vec::new(),
                                    signals: Vec::new(),
                                    properties: Vec::new(),
                                });
                            }
                        }
                    }
                    "method" if depth == 1 => {
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
                    "signal" if depth == 1 => {
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
                    "property" if depth == 1 => {
                        let mut name = String::new();
                        let mut sig = String::new();
                        let mut access = String::new();
                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"name" => name = String::from_utf8_lossy(&attr.value).to_string(),
                                b"type" => sig = String::from_utf8_lossy(&attr.value).to_string(),
                                b"access" => {
                                    access = String::from_utf8_lossy(&attr.value).to_string()
                                }
                                _ => {}
                            }
                        }
                        if let Some(ref mut iface) = current_iface {
                            iface.properties.push(PropertyInfo {
                                name,
                                signature: sig,
                                access: match access.as_str() {
                                    "read" => PropertyAccess::Read,
                                    "write" => PropertyAccess::Write,
                                    "readwrite" => PropertyAccess::ReadWrite,
                                    _ => PropertyAccess::Read,
                                },
                            });
                        }
                    }
                    "arg" if depth == 1 => {
                        let mut name = String::new();
                        let mut sig = String::new();
                        let mut dir = "in".to_string();
                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"name" => name = String::from_utf8_lossy(&attr.value).to_string(),
                                b"type" => sig = String::from_utf8_lossy(&attr.value).to_string(),
                                b"direction" => {
                                    dir = String::from_utf8_lossy(&attr.value).to_string()
                                }
                                _ => {}
                            }
                        }
                        let arg = ArgInfo {
                            name: if name.is_empty() { None } else { Some(name) },
                            signature: sig,
                            direction: if dir == "out" {
                                ArgDirection::Out
                            } else {
                                ArgDirection::In
                            },
                        };
                        if in_method {
                            if let Some(ref mut m) = current_method {
                                if dir == "out" {
                                    m.out_args.push(arg);
                                } else {
                                    m.in_args.push(arg);
                                }
                            }
                        } else if in_signal {
                            if let Some(ref mut s) = current_signal {
                                s.args.push(arg);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                let name_bytes = e.name();
                let tag = std::str::from_utf8(name_bytes.as_ref()).unwrap_or("");
                match tag {
                    "property" if depth == 1 => {
                        let mut name = String::new();
                        let mut sig = String::new();
                        let mut access = String::new();
                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"name" => name = String::from_utf8_lossy(&attr.value).to_string(),
                                b"type" => sig = String::from_utf8_lossy(&attr.value).to_string(),
                                b"access" => {
                                    access = String::from_utf8_lossy(&attr.value).to_string()
                                }
                                _ => {}
                            }
                        }
                        if let Some(ref mut iface) = current_iface {
                            iface.properties.push(PropertyInfo {
                                name,
                                signature: sig,
                                access: match access.as_str() {
                                    "read" => PropertyAccess::Read,
                                    "write" => PropertyAccess::Write,
                                    "readwrite" => PropertyAccess::ReadWrite,
                                    _ => PropertyAccess::Read,
                                },
                            });
                        }
                    }
                    "arg" if depth == 1 => {
                        let mut name = String::new();
                        let mut sig = String::new();
                        let mut dir = "in".to_string();
                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"name" => name = String::from_utf8_lossy(&attr.value).to_string(),
                                b"type" => sig = String::from_utf8_lossy(&attr.value).to_string(),
                                b"direction" => {
                                    dir = String::from_utf8_lossy(&attr.value).to_string()
                                }
                                _ => {}
                            }
                        }
                        let arg = ArgInfo {
                            name: if name.is_empty() { None } else { Some(name) },
                            signature: sig,
                            direction: if dir == "out" {
                                ArgDirection::Out
                            } else {
                                ArgDirection::In
                            },
                        };
                        if in_method {
                            if let Some(ref mut m) = current_method {
                                if dir == "out" {
                                    m.out_args.push(arg);
                                } else {
                                    m.in_args.push(arg);
                                }
                            }
                        } else if in_signal {
                            if let Some(ref mut s) = current_signal {
                                s.args.push(arg);
                            }
                        }
                    }
                    "node" if depth == 1 => {
                        // handled above in parse_child_nodes
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let name_bytes = e.name();
                let tag = std::str::from_utf8(name_bytes.as_ref()).unwrap_or("");
                match tag {
                    "node" => depth = depth.saturating_sub(1),
                    "interface" => {
                        if let Some(iface) = current_iface.take() {
                            interfaces.push(iface);
                        }
                    }
                    "method" => {
                        in_method = false;
                        if let Some(method) = current_method.take() {
                            if let Some(ref mut iface) = current_iface {
                                iface.methods.push(method);
                            }
                        }
                    }
                    "signal" => {
                        in_signal = false;
                        if let Some(signal) = current_signal.take() {
                            if let Some(ref mut iface) = current_iface {
                                iface.signals.push(signal);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            _ => {}
        }
        buf.clear();
    }
    interfaces
}

// ---------------------------------------------------------------------------
// Conversion helpers
// ---------------------------------------------------------------------------

fn convert_interface(iface: &InterfaceInfo) -> InterfaceDetail {
    InterfaceDetail {
        name: iface.name.clone(),
        methods: iface.methods.iter().map(convert_method).collect(),
        properties: iface.properties.iter().map(convert_property).collect(),
        signals: iface.signals.iter().map(convert_signal).collect(),
    }
}

fn convert_method(method: &MethodInfo) -> MethodDetail {
    let in_args: Vec<ArgDetail> = method
        .in_args
        .iter()
        .map(|a| convert_arg(a, "in"))
        .collect();
    let out_args: Vec<ArgDetail> = method
        .out_args
        .iter()
        .map(|a| convert_arg(a, "out"))
        .collect();

    let in_sig: String = in_args
        .iter()
        .map(|a| {
            if a.name.is_empty() {
                a.type_display.clone()
            } else {
                format!("{}: {}", a.name, a.type_display)
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    let out_sig: String = out_args
        .iter()
        .map(|a| a.type_display.clone())
        .collect::<Vec<_>>()
        .join(", ");
    let signature_display = if out_sig.is_empty() {
        format!("({}) → ()", in_sig)
    } else {
        format!("({}) → ({})", in_sig, out_sig)
    };

    MethodDetail {
        name: method.name.clone(),
        in_args,
        out_args,
        signature_display,
    }
}

fn convert_property(prop: &PropertyInfo) -> PropertyDetail {
    PropertyDetail {
        name: prop.name.clone(),
        signature: prop.signature.clone(),
        access: match prop.access {
            PropertyAccess::Read => "read".to_string(),
            PropertyAccess::Write => "write".to_string(),
            PropertyAccess::ReadWrite => "readwrite".to_string(),
        },
        type_display: dbus_signature_to_human(&prop.signature),
    }
}

fn convert_signal(signal: &SignalInfo) -> SignalDetail {
    SignalDetail {
        name: signal.name.clone(),
        args: signal
            .args
            .iter()
            .map(|a| {
                let dir = match a.direction {
                    ArgDirection::Out => "out",
                    _ => "in",
                };
                convert_arg(a, dir)
            })
            .collect(),
    }
}

fn convert_arg(arg: &ArgInfo, direction: &str) -> ArgDetail {
    ArgDetail {
        name: arg.name.clone().unwrap_or_default(),
        signature: arg.signature.clone(),
        direction: direction.to_string(),
        type_display: dbus_signature_to_human(&arg.signature),
    }
}

/// Convert a D-Bus type signature to a human-readable description
pub fn dbus_signature_to_human(sig: &str) -> String {
    match sig {
        "s" => "String".to_string(),
        "b" => "Boolean".to_string(),
        "y" => "Byte".to_string(),
        "n" => "Int16".to_string(),
        "q" => "Uint16".to_string(),
        "i" => "Int32".to_string(),
        "u" => "Uint32".to_string(),
        "x" => "Int64".to_string(),
        "t" => "Uint64".to_string(),
        "d" => "Double".to_string(),
        "o" => "ObjectPath".to_string(),
        "g" => "Signature".to_string(),
        "v" => "Variant".to_string(),
        "h" => "UnixFd".to_string(),
        "" => "()".to_string(),
        _ if sig.starts_with("a{") && sig.ends_with('}') => {
            let inner = &sig[2..sig.len() - 1];
            if inner.len() >= 2 {
                format!(
                    "Dict<{}, {}>",
                    dbus_signature_to_human(&inner[..1]),
                    dbus_signature_to_human(&inner[1..])
                )
            } else {
                format!("Dict<{}>", inner)
            }
        }
        _ if sig == "as" => "Array<String>".to_string(),
        _ if sig == "ai" => "Array<Int32>".to_string(),
        _ if sig == "au" => "Array<Uint32>".to_string(),
        _ if sig == "ao" => "Array<ObjectPath>".to_string(),
        _ if sig == "ay" => "Array<Byte>".to_string(),
        _ if sig.starts_with('a') => format!("Array<{}>", dbus_signature_to_human(&sig[1..])),
        _ if sig.starts_with('(') && sig.ends_with(')') => format!("Struct{}", sig),
        _ => sig.to_string(),
    }
}
