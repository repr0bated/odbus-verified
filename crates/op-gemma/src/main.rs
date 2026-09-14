//! Gemma — the subid → tag → OpenFlow + xray routing brain.
//!
//! Reads the canonical subid registry from `/etc/ghostbridge/subid-registry.json`
//! and writes two tmpfs artifacts:
//!   - `/dev/shm/xray-routes.json`    — routes for the Xray configuration owner
//!   - `/dev/shm/openflow-routes.json` — rules for the OpenFlow controller
//!
//! Per AGENTS.md §4: D-Bus is the control plane; Gemma is a host service that
//! produces structured state, not a CLI bypass. It never shells out to network
//! tools. OpenFlow rules may be applied via D-Bus by a separate controller.

mod ui_gallery;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use tracing::{info, warn};

const SUBID_REGISTRY: &str = "/etc/ghostbridge/subid-registry.json";
const XRAY_ROUTES: &str = "/dev/shm/xray-routes.json";
const OPENFLOW_ROUTES: &str = "/dev/shm/openflow-routes.json";

#[derive(Debug, Clone, Deserialize, Serialize)]
struct SubidRegistry {
    version: u32,
    entries: Vec<RegistryEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct RegistryEntry {
    uuid: String,
    subid: String,
    #[serde(rename = "category")]
    category: String,
    #[serde(rename = "component_type")]
    component_type: String,
    subject: String,
    verb: String,
    version: u8,
    tags: Vec<String>,
    subdomains: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    gemma: Option<GemmaConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct GemmaConfig {
    backend: BackendSpec,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
enum BackendSpec {
    #[serde(rename = "ingress")]
    Ingress,
    #[serde(rename = "tcp")]
    Tcp { host: String, port: u16 },
    #[serde(rename = "grpc")]
    Grpc {
        host: String,
        port: u16,
        #[serde(rename = "service_name")]
        service_name: String,
    },
    #[serde(rename = "unix")]
    Unix { path: String },
    #[serde(rename = "dns")]
    Dns { host: String, port: u16 },
    /// Internal metadata entry — not xray-routable, produces no outbound.
    #[serde(rename = "file")]
    File {
        #[allow(dead_code)]
        path: String,
    },
    /// Internal service — not xray-routable, produces no outbound.
    #[serde(rename = "internal")]
    Internal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct XrayRoutingMap {
    version: u32,
    xray_routes: Vec<XrayRoute>,
    openflow_rules: Vec<OpenFlowRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct XrayRoute {
    tag: String,
    subdomains: Vec<String>,
    backend: BackendSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenFlowRule {
    priority: u16,
    #[serde(rename = "match_fields")]
    match_fields: HashMap<String, String>,
    actions: Vec<String>,
    description: Option<String>,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    info!("Gemma routing brain starting");

    let registry = load_registry()?;
    let routes = derive_routes(&registry);

    write_json(XRAY_ROUTES, &routes)?;
    write_json(OPENFLOW_ROUTES, &routes)?;

    info!(
        xray_routes = routes.xray_routes.len(),
        openflow_rules = routes.openflow_rules.len(),
        "Gemma wrote routing artifacts"
    );

    // Generate the json-render.dev UI spec gallery (200 unique specs)
    if let Err(e) = ui_gallery::generate_gallery() {
        warn!("Gemma UI gallery generation failed: {e}");
    }

    Ok(())
}

fn load_registry() -> Result<SubidRegistry> {
    let raw = fs::read_to_string(SUBID_REGISTRY)
        .with_context(|| format!("failed to read subid registry: {SUBID_REGISTRY}"))?;
    let parsed: SubidRegistry =
        serde_json::from_str(&raw).with_context(|| "failed to parse subid registry as JSON")?;
    Ok(parsed)
}

fn derive_routes(registry: &SubidRegistry) -> XrayRoutingMap {
    let mut xray_routes = Vec::new();
    let mut openflow_rules = default_privacy_chain();

    for entry in &registry.entries {
        let Some(gemma) = &entry.gemma else { continue };

        // Skip non-routable backends — they are metadata (schema files,
        // internal services) that should not produce xray outbounds.
        if matches!(
            gemma.backend,
            BackendSpec::File { .. } | BackendSpec::Internal
        ) {
            continue;
        }

        let tag = entry
            .tags
            .first()
            .cloned()
            .unwrap_or_else(|| entry.subject.clone());

        // Skip duplicate tags — first declaration wins.
        if xray_routes.iter().any(|r: &XrayRoute| r.tag == tag) {
            continue;
        }

        let route = XrayRoute {
            tag: tag.clone(),
            subdomains: entry.subdomains.clone(),
            backend: gemma.backend.clone(),
        };
        xray_routes.push(route);

        if let Some(rule) = backend_to_openflow_rule(&tag, &gemma.backend) {
            openflow_rules.push(rule);
        }
    }

    XrayRoutingMap {
        version: 1,
        xray_routes,
        openflow_rules,
    }
}

/// Default privacy chain flows for the GhostBridge OVS bridge.
/// These match the conventions in `crates/op-plugins/src/state_plugins/privacy_router.rs`.
fn default_privacy_chain() -> Vec<OpenFlowRule> {
    vec![
        OpenFlowRule {
            priority: 100,
            match_fields: HashMap::from([("in_port".to_string(), "gbr_wg".to_string())]),
            actions: vec!["output:gbr_warp".to_string()],
            description: Some("gbr_wg -> gbr_warp".to_string()),
        },
        OpenFlowRule {
            priority: 100,
            match_fields: HashMap::from([("in_port".to_string(), "gbr_warp".to_string())]),
            actions: vec!["output:gbr_xray".to_string()],
            description: Some("gbr_warp -> gbr_xray".to_string()),
        },
        OpenFlowRule {
            priority: 100,
            match_fields: HashMap::from([("in_port".to_string(), "gbr_xray".to_string())]),
            actions: vec!["output:gbr_warp".to_string()],
            description: Some("gbr_xray -> gbr_warp".to_string()),
        },
        OpenFlowRule {
            priority: 200,
            match_fields: HashMap::from([("dl_type".to_string(), "0x0806".to_string())]),
            actions: vec!["arp_responder".to_string()],
            description: Some("ARP Responder for privacy network".to_string()),
        },
    ]
}

/// Translate a Gemma backend into an OpenFlow rule for the bridge.
/// Only TCP/GRPC backends produce IP/Port matchable rules here; Unix sockets
/// route through the tonic-web gRPC bridge (reflection demuxes) so no OVS
/// bridge match is needed.
fn backend_to_openflow_rule(tag: &str, backend: &BackendSpec) -> Option<OpenFlowRule> {
    let (host, port) = match backend {
        BackendSpec::Tcp { host, port } => (host.clone(), *port),
        BackendSpec::Grpc { host, port, .. } => (host.clone(), *port),
        BackendSpec::Ingress
        | BackendSpec::Unix { .. }
        | BackendSpec::Dns { .. }
        | BackendSpec::File { .. }
        | BackendSpec::Internal => return None,
    };

    let mut match_fields = HashMap::new();
    match_fields.insert("dl_type".to_string(), "0x0800".to_string());
    match_fields.insert("nw_proto".to_string(), "6".to_string()); // TCP
    match_fields.insert("nw_dst".to_string(), host.clone());
    match_fields.insert("tp_dst".to_string(), port.to_string());

    Some(OpenFlowRule {
        priority: 150,
        match_fields,
        actions: vec!["output:ovsbr0-mgmt".to_string()],
        description: Some(format!("Gemma: {tag} -> host backend {host}:{port}")),
    })
}

fn write_json<P: AsRef<Path>>(path: P, value: &XrayRoutingMap) -> Result<()> {
    let tmp = format!("{}.tmp", path.as_ref().display());
    let mut f =
        fs::File::create(&tmp).with_context(|| format!("failed to create tmp file: {tmp}"))?;
    let json =
        serde_json::to_string_pretty(value).with_context(|| "failed to serialize routing map")?;
    f.write_all(json.as_bytes())
        .with_context(|| format!("failed to write tmp file: {tmp}"))?;
    f.sync_data()
        .with_context(|| format!("failed to sync tmp file: {tmp}"))?;
    fs::rename(&tmp, path.as_ref())
        .with_context(|| format!("failed to rename {tmp} to {}", path.as_ref().display()))?;
    Ok(())
}
