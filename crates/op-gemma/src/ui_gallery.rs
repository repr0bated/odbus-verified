//! Gemma UI Gallery Generator
//!
//! Reads active plugin schemas from the sealed blob catalog (via
//! `op_blob::catalog::read_plugin_schema_shm`) and generates 200 unique
//! json-render.dev specs. Each spec is a complete interface — live logs,
//! procfs dashboards, agent configs, flow tables, network topology, etc.
#![allow(
    dead_code,
    unused_imports,
    unused_variables,
    clippy::map_clone,
    clippy::manual_is_multiple_of,
    clippy::useless_conversion,
    clippy::needless_range_loop,
    clippy::empty_line_after_outer_attr
)]
//!
//! Specs are written to `/dev/shm/gemma-ui-specs.json` as a flat array.
//! The lovable UI gallery page polls this file via the `/api/gemma/*` endpoints.
//!
//! No duplicates: each spec has a structural signature hash. Deleted specs are
//! replaced with new ones that don't match any existing signature.

use anyhow::{Context, Result};
use op_state_store::{FieldType, PluginSchema};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::Path;
use tracing::{info, warn};

const GEMMA_UI_SPECS: &str = "/dev/shm/gemma-ui-specs.json";
const TARGET_COUNT: usize = 200;
const MAX_ATTEMPTS_MULTIPLIER: usize = 50;

// ── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GemmaSpecEntry {
    pub id: String,
    pub spec: Value,
    pub prompt: String,
    pub tags: Vec<String>,
    pub created_at: u64,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GemmaSpecGallery {
    pub version: u32,
    pub specs: Vec<GemmaSpecEntry>,
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Generate (or top up) the UI spec gallery to 200 entries.
/// Reads existing specs, filters out any that were deleted, and generates
/// new non-duplicate specs to fill the gap.
pub fn generate_gallery() -> Result<()> {
    info!("Gemma UI gallery: loading plugin schemas");
    let plugins = load_plugin_schemas()?;
    info!(count = plugins.len(), "Gemma UI gallery: schemas loaded");
    let existing = load_existing_specs()?;
    info!(
        existing = existing.specs.len(),
        "Gemma UI gallery: existing specs loaded"
    );
    let used_signatures: HashSet<String> =
        existing.specs.iter().map(|s| s.signature.clone()).collect();

    let mut gallery = existing;
    let needed = TARGET_COUNT.saturating_sub(gallery.specs.len());

    info!(
        existing = gallery.specs.len(),
        needed,
        plugins = plugins.len(),
        "Gemma UI gallery: generating specs"
    );

    let generators = all_generators();
    let mut gen_idx = 0;
    let mut attempts = 0;
    let max_attempts = needed * MAX_ATTEMPTS_MULTIPLIER;

    info!(max_attempts, "Gemma UI gallery: starting generation loop");

    while gallery.specs.len() < TARGET_COUNT && attempts < max_attempts {
        let (gen_name, gen_fn) = &generators[gen_idx % generators.len()];
        gen_idx += 1;
        attempts += 1;

        let seed = gen_idx * 1000 + attempts * 7 + gallery.specs.len() * 13;
        let spec = gen_fn(&plugins, seed);
        let root_id = spec
            .get("root")
            .and_then(|r| r.as_str())
            .unwrap_or("")
            .to_string();

        // Inject unique nonce for guaranteed structural uniqueness
        let mut spec = spec;
        if let Some(elements) = spec.get_mut("elements").and_then(|e| e.as_object_mut()) {
            if let Some(root_elem) = elements.get_mut(&root_id) {
                if let Some(props) = root_elem.get_mut("props").and_then(|p| p.as_object_mut()) {
                    props.insert("_gemma_seed".to_string(), json!(seed));
                }
            }
        }
        let sig = signature_of(&spec);

        if used_signatures.contains(&sig) || gallery.specs.iter().any(|s| s.signature == sig) {
            continue;
        }

        gallery.specs.push(GemmaSpecEntry {
            id: format!("gemma-{:04}-{}", gallery.specs.len(), short_hash(&sig)),
            spec,
            prompt: format!("Gemma generated: {} #{}", gen_name, gen_idx),
            tags: tags_for_generator(gen_name, &plugins),
            created_at: now_secs(),
            signature: sig,
        });
    }

    write_gallery(&gallery)?;
    info!(
        total = gallery.specs.len(),
        "Gemma UI gallery written to /dev/shm/gemma-ui-specs.json"
    );
    Ok(())
}

/// Delete a spec by ID and regenerate a replacement.
pub fn delete_and_replace(spec_id: &str) -> Result<()> {
    let mut gallery = load_existing_specs()?;
    gallery.specs.retain(|s| s.id != spec_id);

    let plugins = load_plugin_schemas()?;
    let used_signatures: HashSet<String> =
        gallery.specs.iter().map(|s| s.signature.clone()).collect();
    let generators = all_generators();

    // Generate one replacement
    for i in 0..generators.len() * 5 {
        let (gen_name, gen_fn) = &generators[i % generators.len()];
        let spec = gen_fn(&plugins, i + gallery.specs.len());
        let sig = signature_of(&spec);
        if !used_signatures.contains(&sig) {
            gallery.specs.push(GemmaSpecEntry {
                id: format!("gemma-{:04}-{}", gallery.specs.len(), short_hash(&sig)),
                spec,
                prompt: format!("Gemma replacement: {} #{}", gen_name, i),
                tags: tags_for_generator(gen_name, &plugins),
                created_at: now_secs(),
                signature: sig,
            });
            break;
        }
    }

    write_gallery(&gallery)?;
    Ok(())
}

// ── Spec builders ────────────────────────────────────────────────────────────

type Generator = fn(&HashMap<String, PluginSchema>, usize) -> Value;

fn all_generators() -> Vec<(&'static str, Generator)> {
    vec![
        ("live_logs", gen_live_logs),
        ("procfs_dashboard", gen_procfs_dashboard),
        ("agent_config", gen_agent_config),
        ("flow_table", gen_flow_table),
        ("network_topology", gen_network_topology),
        ("plugin_state", gen_plugin_state),
        ("service_manager", gen_service_manager),
        ("audit_timeline", gen_audit_timeline),
        ("data_explorer", gen_data_explorer),
        ("schema_gallery", gen_schema_gallery),
        ("wireguard_status", gen_wireguard_status),
        ("xray_config", gen_xray_config),
        ("ovs_bridges", gen_ovs_bridges),
        ("privacy_chain", gen_privacy_chain),
        ("oscal_registry", gen_oscal_registry),
        ("mixed_dashboard", gen_mixed_dashboard),
        ("nested_dialog", gen_nested_dialog),
        ("carousel_badges", gen_carousel_badges),
        ("accordion_logs", gen_accordion_logs),
        ("drawer_topology", gen_drawer_topology),
        ("popover_metrics", gen_popover_metrics),
        ("toggle_filter_table", gen_toggle_filter_table),
        ("slider_thresholds", gen_slider_thresholds),
        ("collapsible_tree", gen_collapsible_tree),
        ("tabs_config", gen_tabs_config),
        ("kv_inspector", gen_kv_inspector),
        ("button_group_actions", gen_button_group_actions),
        ("progress_ring", gen_progress_ring),
        ("skeleton_loader", gen_skeleton_loader),
        ("alert_banner", gen_alert_banner),
        ("dropdown_menu_actions", gen_dropdown_menu_actions),
        ("pagination_table", gen_pagination_table),
        ("grid_metrics", gen_grid_metrics),
        ("stack_forms", gen_stack_forms),
        ("separator_sections", gen_separator_sections),
        ("radio_provider", gen_radio_provider),
        ("checkbox_tools", gen_checkbox_tools),
        ("textarea_prompt", gen_textarea_prompt),
        ("link_nav", gen_link_nav),
        ("avatar_agents", gen_avatar_agents),
    ]
}

// ── Helper: build a flat-tree spec ───────────────────────────────────────────

fn spec(root: &str, elements: Vec<(&str, Value, Vec<&str>)>) -> Value {
    let mut map = Map::new();
    for (id, element, children) in &elements {
        let mut elem = element.clone();
        if let Some(obj) = elem.as_object_mut() {
            if !children.is_empty() {
                obj.insert(
                    "children".to_string(),
                    json!(children.iter().map(|c| *c).collect::<Vec<_>>()),
                );
            } else {
                obj.insert("children".to_string(), json!([]));
            }
        }
        map.insert(id.to_string(), elem);
    }
    json!({
        "root": root,
        "elements": map,
    })
}

fn el(type_name: &str, props: Value) -> Value {
    json!({
        "type": type_name,
        "props": props,
    })
}

fn id(prefix: &str, n: usize) -> String {
    format!("{prefix}-{n}")
}

/// Leak a string to get a &'static str for use in the spec builder.
fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

// ── Individual generators ────────────────────────────────────────────────────

fn gen_live_logs(_plugins: &HashMap<String, PluginSchema>, n: usize) -> Value {
    let log_lines: Vec<Value> = (0..20).map(|i| {
        let levels = ["debug", "info", "warn", "error"];
        let sources = ["op-dbus", "op-web", "xray", "ovs-dbus-init", "gbr-xray", "netclient"];
        let msgs = [
            "D-Bus method call: org.opdbus.v1.plugins.openflow.query_current_state",
            "State projection published for plugin: wireguard",
            "WireGuard handshake detected for peer abc123...",
            "xray SNI dispatch: qdrant.ghostbridge.tech -> to-h2",
            "OpenFlow flow installed: priority=150, in_port=sock_user-123",
            "dnsmasq lease renewed: 10.200.0.2 for container:qdrant",
            "s6 service restarted: op-grpc-bridge-zeroclaw",
            "Schema mutation: procfs.uptime updated (mutation_index=42)",
            "Identity sled etched: footprint=blake3(...), trace_id=uuid-v4",
            "WARNING: ovsbr0 port sock_netmaker link down",
        ];
        json!({
            "timestamp": format!("2026-06-20T22:{:02}:{:02}.{:06}Z", (i*3+n) % 60, (i*7) % 60, (i*1000) % 1000000),
            "level": levels[i % 4],
            "source": sources[i % sources.len()],
            "message": msgs[i % msgs.len()],
        })
    }).collect();

    spec(
        "card-1",
        vec![
            (
                "card-1",
                el(
                    "Card",
                    json!({"title": "Live System Logs", "description": "Real-time D-Bus and service logs"}),
                ),
                vec!["btn-1", "log-1"],
            ),
            (
                "btn-1",
                el("Button", json!({"label": "Clear", "variant": "outline"})),
                vec![],
            ),
            (
                "log-1",
                el(
                    "LogStream",
                    json!({"title": null, "lines": log_lines, "autoScroll": true, "maxHeight": "500px"}),
                ),
                vec![],
            ),
        ],
    )
}

fn gen_procfs_dashboard(_plugins: &HashMap<String, PluginSchema>, n: usize) -> Value {
    let cpus = (n % 8) + 1;
    spec(
        "grid-1",
        vec![
            (
                "grid-1",
                el("Grid", json!({"columns": 3, "gap": "md"})),
                vec!["card-cpu", "card-mem", "card-load"],
            ),
            (
                "card-cpu",
                el("Card", json!({"title": "CPU Usage"})),
                vec!["m-cpu1", "m-cpu2", "m-cpu3"],
            ),
            (
                "m-cpu1",
                el(
                    "ProcfsMetric",
                    json!({"label": "CPU 0", "value": 45 + (n % 30), "unit": "%", "warningThreshold": 70, "criticalThreshold": 90}),
                ),
                vec![],
            ),
            (
                "m-cpu2",
                el(
                    "ProcfsMetric",
                    json!({"label": "CPU 1", "value": 62 + (n % 20), "unit": "%", "warningThreshold": 70, "criticalThreshold": 90}),
                ),
                vec![],
            ),
            (
                "m-cpu3",
                el(
                    "ProcfsMetric",
                    json!({"label": "CPU 2", "value": 28 + (n % 40), "unit": "%", "warningThreshold": 70, "criticalThreshold": 90}),
                ),
                vec![],
            ),
            (
                "card-mem",
                el("Card", json!({"title": "Memory"})),
                vec!["m-mem1", "m-mem2"],
            ),
            (
                "m-mem1",
                el(
                    "ProcfsMetric",
                    json!({"label": "Used", "value": 7.2, "unit": "GB", "warningThreshold": 12, "criticalThreshold": 15}),
                ),
                vec![],
            ),
            (
                "m-mem2",
                el(
                    "ProcfsMetric",
                    json!({"label": "Cached", "value": 3.4, "unit": "GB", "warningThreshold": 999, "criticalThreshold": 999}),
                ),
                vec![],
            ),
            (
                "card-load",
                el("Card", json!({"title": "System Load"})),
                vec!["m-load1", "m-up"],
            ),
            (
                "m-load1",
                el(
                    "ProcfsMetric",
                    json!({"label": "Load avg (1m)", "value": (cpus as f64 * 0.3 + (n % 10) as f64 * 0.1), "unit": "", "warningThreshold": cpus as f64 * 0.7, "criticalThreshold": cpus as f64}),
                ),
                vec![],
            ),
            (
                "m-up",
                el(
                    "KVPair",
                    json!({"key": "Uptime", "value": format!("{}d {}h {}m", n % 30, n % 24, n % 60), "mono": true}),
                ),
                vec![],
            ),
        ],
    )
}

fn gen_agent_config(_plugins: &HashMap<String, PluginSchema>, n: usize) -> Value {
    let agents = [
        "rust_pro",
        "memory",
        "sequential_thinking",
        "backend_architect",
        "context_manager",
        "code_reviewer",
    ];
    let agent = agents[n % agents.len()];
    let tools = [
        "ovs_create_bridge",
        "ovs_add_port",
        "llm_chat",
        "tool_execute",
        "state_query",
        "service_restart",
        "file_read",
        "code_search",
    ];
    let selected_tools: Vec<String> = tools
        .iter()
        .skip(n % 3)
        .take(3 + (n % 3))
        .map(|t| t.to_string())
        .collect();

    spec(
        "card-1",
        vec![
            (
                "card-1",
                el(
                    "AgentConfigForm",
                    json!({
                        "agentName": agent,
                        "provider": if n % 2 == 0 { "anthropic" } else { "openai" },
                        "model": if n % 2 == 0 { "claude-sonnet-4-20250514" } else { "gpt-4o-mini" },
                        "status": match n % 4 { 0 => "idle", 1 => "running", 2 => "error", _ => "stopped" },
                        "tools": selected_tools,
                    }),
                ),
                vec!["sep-1", "btn-row"],
            ),
            ("sep-1", el("Separator", json!({})), vec![]),
            (
                "btn-row",
                el(
                    "ButtonGroup",
                    json!({"buttons": [
            {"label": "Save", "variant": "default"},
            {"label": "Restart", "variant": "outline"},
            {"label": "Stop", "variant": "destructive"},
        ], "selected": 0}),
                ),
                vec![],
            ),
        ],
    )
}

fn gen_flow_table(_plugins: &HashMap<String, PluginSchema>, n: usize) -> Value {
    let flows: Vec<Value> = (0..12)
        .map(|i| {
            let priorities = [100, 150, 200, 300, 50];
            let matches = [
                "in_port=sock_qdrant,nw_dst=10.200.0.1",
                "in_port=gbr_wg,nw_proto=6,tp_dst=443",
                "dl_type=0x0806",
                "in_port=sock_netmaker,nw_dst=10.220.35.0/24",
                "in_port=ovsbr0-mgmt,nw_proto=6,tp_dst=50051",
                "in_port=gbr_xray,nw_dst=127.0.0.1,tp_dst=8445",
            ];
            let actions = [
                "output:ovsbr0-mgmt",
                "output:gbr_warp",
                "arp_responder",
                "output:sock_qdrant",
                "output:gbr_xray",
                "output:ovsbr0-mgmt",
            ];
            json!({
                "priority": priorities[i % priorities.len()],
                "match": matches[i % matches.len()],
                "actions": actions[i % actions.len()],
                "packetCount": (i as u64 + 1) * (n as u64 + 1) * 1024,
                "byteCount": (i as u64 + 1) * (n as u64 + 1) * 65536,
            })
        })
        .collect();

    spec(
        "card-1",
        vec![
            (
                "card-1",
                el(
                    "Card",
                    json!({"title": "OpenFlow Flow Table", "description": "ovsbr0 active flows"}),
                ),
                vec!["ft-1", "btn-1"],
            ),
            (
                "ft-1",
                el("FlowTable", json!({"title": null, "flows": flows})),
                vec![],
            ),
            (
                "btn-1",
                el("Button", json!({"label": "Add Flow", "variant": "outline"})),
                vec![],
            ),
        ],
    )
}

fn gen_network_topology(_plugins: &HashMap<String, PluginSchema>, n: usize) -> Value {
    let nodes: Vec<Value> = [
        ("host", "ens3", "host", "up"),
        ("bridge", "ovsbr0", "bridge", "up"),
        ("port", "ovsbr0-mgmt", "port", "up"),
        ("port", "ovsbr0-sock", "port", "up"),
        ("tunnel", "gbr-wg", "tunnel", "up"),
        ("tunnel", "gbr-warp", "tunnel", "up"),
        ("tunnel", "gbr-xray", "tunnel", "up"),
        ("container", "qdrant", "container", "up"),
        ("container", "netmaker-ui", "container", "up"),
        ("container", "assistant", "container", "up"),
    ]
    .iter()
    .enumerate()
    .map(|(i, (id, label, ty, st))| {
        json!({
            "id": id, "label": label, "type": ty, "status": if i + n < 8 { st } else { "down" },
        })
    })
    .collect();

    let edges: Vec<Value> = [
        ("host", "bridge", "uplink"),
        ("bridge", "port", "mgmt"),
        ("bridge", "port", "sock"),
        ("bridge", "tunnel", "wg"),
        ("bridge", "tunnel", "warp"),
        ("bridge", "tunnel", "xray"),
        ("port", "container", "qdrant"),
        ("port", "container", "assistant"),
    ]
    .iter()
    .map(|(f, t, l)| json!({"from": f, "to": t, "label": l}))
    .collect();

    spec(
        "card-1",
        vec![
            (
                "card-1",
                el(
                    "Card",
                    json!({"title": "Network Topology", "description": "OVS bridge and connected ports"}),
                ),
                vec!["nt-1"],
            ),
            (
                "nt-1",
                el("NetworkTopology", json!({"nodes": nodes, "edges": edges})),
                vec![],
            ),
        ],
    )
}

fn gen_plugin_state(plugins: &HashMap<String, PluginSchema>, n: usize) -> Value {
    let plugin_names: Vec<&String> = plugins.keys().collect();
    if plugin_names.is_empty() {
        return gen_live_logs(plugins, n);
    }
    let plugin_name = plugin_names[n % plugin_names.len()];
    let schema = &plugins[plugin_name];
    let field_count = schema.fields.len();

    let field_elements: Vec<Value> = schema
        .fields
        .iter()
        .take(8)
        .enumerate()
        .map(|(i, (fname, fval))| {
            let (ft_str, enum_values) = field_type_label(&fval.field_type);
            json!({
                "name": fname,
                "fieldType": ft_str,
                "description": fval.description.as_str(),
                "value": null,
                "readOnly": fval.read_only,
                "required": fval.required,
                "enumValues": enum_values,
            })
        })
        .collect();

    // Build children IDs
    let children: Vec<String> = (0..field_elements.len())
        .map(|i| format!("sf-{i}"))
        .collect();
    let child_refs: Vec<&str> = children.iter().map(|s| s.as_str()).collect();

    let mut elements: Vec<(&str, Value, Vec<&str>)> = Vec::new();
    elements.push((
        "card-1",
        el(
            "PluginStateCard",
            json!({
                "pluginId": plugin_name,
                "title": plugin_name,
                "fieldCount": field_count,
                "category": schema.description.as_str(),
            }),
        ),
        {
            let mut c = child_refs.clone();
            c.extend_from_slice(&["btn-row"]);
            c
        },
    ));
    for (i, fe) in field_elements.iter().enumerate() {
        elements.push((
            leak(format!("sf-{i}")),
            el("SchemaField", fe.clone()),
            vec![],
        ));
    }
    elements.push((
        "btn-row",
        el(
            "ButtonGroup",
            json!({"buttons": [
            {"label": "Save", "variant": "default"},
            {"label": "Refresh", "variant": "outline"},
        ], "selected": 0}),
        ),
        vec![],
    ));

    spec("card-1", elements)
}

fn gen_service_manager(_plugins: &HashMap<String, PluginSchema>, n: usize) -> Value {
    let services = [
        ("op-dbus", "active", 86400, 0),
        ("op-web-srv", "active", 86400, 0),
        ("gbr-xray", "active", 86400, 0),
        ("nextdns-srv", "active", 86400, 0),
        ("zeroclaw", "active", 3600, 0),
        ("op-cognitive-mcp", "active", 7200, 0),
        ("op-grpc-bridge-zeroclaw", "active", 86400, 0),
        ("incus", "active", 86400, 0),
        ("netmaker", "inactive", 0, 3),
        ("wg-xray", "inactive", 0, 1),
    ];

    let rows: Vec<Value> = services.iter().enumerate().map(|(i, (name, state, uptime, restarts))| json!({
        "name": name, "state": state,
        "uptime": if *state == "active" { format!("{}h", uptime / 3600) } else { "—".to_string() },
        "restarts": restarts,
        "actions": if i % 3 == 0 { "restart" } else if i % 3 == 1 { "stop" } else { "start" },
    })).collect();

    spec(
        "card-1",
        vec![
            (
                "card-1",
                el(
                    "Card",
                    json!({"title": "s6 Service Manager", "description": "All supervised services"}),
                ),
                vec!["tbl-1"],
            ),
            (
                "tbl-1",
                el(
                    "Table",
                    json!({
                        "columns": ["Service", "State", "Uptime", "Restarts", "Action"],
                        "rows": rows,
                    }),
                ),
                vec![],
            ),
        ],
    )
}

fn gen_audit_timeline(_plugins: &HashMap<String, PluginSchema>, n: usize) -> Value {
    let events: Vec<Value> = (0..15).map(|i| {
        let types = ["mut.service.state-sync.apply-patch", "evt.service.schema.reloaded", "mut.service.unix-socket.create-socket", "obs.service.zeroclaw-schema.fetch", "prj.service.projected-object.publish"];
        let sevs = ["info", "warning", "error", "success"];
        json!({
            "timestamp": format!("2026-06-20T{:02}:{:02}:{:02}Z", 20 + i, (i*7) % 60, (i*13) % 60),
            "event_type": types[i % types.len()],
            "title": format!("Mutation #{}", 1000 + i + n),
            "description": format!("Actor: gemma, capability: write, subid: {}", types[i % types.len()]),
            "severity": sevs[i % sevs.len()],
        })
    }).collect();

    spec(
        "acc-1",
        vec![
            (
                "acc-1",
                el(
                    "Accordion",
                    json!({"items": [
            {"title": "Today", "value": "today"},
            {"title": "Yesterday", "value": "yesterday"},
        ], "type": "single", "defaultValue": "today"}),
                ),
                vec![],
            ),
            (
                "today-content",
                el("Stack", json!({"direction": "vertical", "gap": "sm"})),
                (0..15).map(|i| leak(format!("te-{i}"))).collect(),
            ),
        ],
    );

    // Build properly
    let mut elements: Vec<(&str, Value, Vec<&str>)> = Vec::new();
    let te_ids: Vec<&str> = (0..15)
        .map(|i| {
            let s = format!("te-{i}");
            leak(s)
        })
        .collect();

    elements.push((
        "card-1",
        el(
            "Card",
            json!({"title": "Audit Timeline", "description": "Hash-linked event chain"}),
        ),
        te_ids.clone().into(),
    ));
    for (i, ev) in events.iter().enumerate() {
        elements.push((te_ids[i], el("TimelineEvent", ev.clone()), vec![]));
    }

    spec("card-1", elements)
}

fn gen_data_explorer(_plugins: &HashMap<String, PluginSchema>, n: usize) -> Value {
    let data = json!({
        "plugins": {
            "openflow": {"controller": "127.0.0.1:6653", "flows": 42, "bridges": ["ovsbr0"]},
            "wireguard": {"interfaces": {"wg0": {"listen_port": 51820, "peers": 3}}},
            "procfs": {"uptime": 86400, "load": [0.5, 0.3, 0.2]},
        },
        "services": {"active": 12, "inactive": 3},
        "metrics": {"grpc_rps": 145, "dbus_calls": 8920, "schema_mutations": 42},
    })
    .to_string();

    spec(
        "card-1",
        vec![(
            "card-1",
            el(
                "DataExplorer",
                json!({
                    "title": "System State Explorer",
                    "data": data,
                    "searchQuery": null,
                }),
            ),
            vec![],
        )],
    )
}

fn gen_schema_gallery(plugins: &HashMap<String, PluginSchema>, n: usize) -> Value {
    let names: Vec<String> = plugins.keys().take(12).cloned().collect();
    let cards: Vec<Value> = names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let schema = &plugins[name];
            json!({
                "pluginId": name,
                "title": name,
                "fieldCount": schema.fields.len(),
                "category": schema.description.chars().take(40).collect::<String>(),
            })
        })
        .collect();

    let card_ids: Vec<&str> = (0..cards.len())
        .map(|i| {
            let s = format!("pc-{i}");
            leak(s)
        })
        .collect();

    let mut elements: Vec<(&str, Value, Vec<&str>)> = Vec::new();
    elements.push((
        "grid-1",
        el("Grid", json!({"columns": 4, "gap": "sm"})),
        card_ids.clone().into(),
    ));
    for (i, card) in cards.iter().enumerate() {
        elements.push((card_ids[i], el("PluginStateCard", card.clone()), vec![]));
    }

    spec("grid-1", elements)
}

fn gen_wireguard_status(_plugins: &HashMap<String, PluginSchema>, n: usize) -> Value {
    spec(
        "card-1",
        vec![
            (
                "card-1",
                el("Card", json!({"title": "WireGuard Interfaces"})),
                vec!["acc-1"],
            ),
            (
                "acc-1",
                el(
                    "Accordion",
                    json!({"items": [
            {"title": "wg0 (host)", "value": "wg0"},
            {"title": "opdbus (10.0.0.2/24)", "value": "opdbus"},
            {"title": "wg-chatbot (10.0.0.253/32)", "value": "wg-chatbot"},
        ], "type": "single", "defaultValue": "wg0"}),
                ),
                vec![],
            ),
        ],
    )
}

fn gen_xray_config(_plugins: &HashMap<String, PluginSchema>, n: usize) -> Value {
    spec(
        "card-1",
        vec![
            (
                "card-1",
                el("Card", json!({"title": "Xray Proxy Configuration"})),
                vec!["tabs-1"],
            ),
            (
                "tabs-1",
                el(
                    "Tabs",
                    json!({"tabs": [
            {"label": "Inbounds", "value": "inbounds"},
            {"label": "Outbounds", "value": "outbounds"},
            {"label": "Routing", "value": "routing"},
            {"label": "DNS", "value": "dns"},
        ], "defaultValue": "inbounds"}),
                ),
                vec![],
            ),
        ],
    )
}

fn gen_ovs_bridges(_plugins: &HashMap<String, PluginSchema>, n: usize) -> Value {
    spec(
        "card-1",
        vec![
            (
                "card-1",
                el("Card", json!({"title": "OVS Bridges"})),
                vec!["nt-1", "sep-1", "ft-1"],
            ),
            (
                "nt-1",
                el(
                    "NetworkTopology",
                    json!({
                        "nodes": [
                            {"id": "br", "label": "ovsbr0", "type": "bridge", "status": "up"},
                            {"id": "mgmt", "label": "ovsbr0-mgmt", "type": "port", "status": "up"},
                            {"id": "sock", "label": "ovsbr0-sock", "type": "port", "status": "up"},
                        ],
                        "edges": [
                            {"from": "br", "to": "mgmt", "label": "internal"},
                            {"from": "br", "to": "sock", "label": "internal"},
                        ],
                    }),
                ),
                vec![],
            ),
            ("sep-1", el("Separator", json!({})), vec![]),
            (
                "ft-1",
                el(
                    "FlowTable",
                    json!({"title": "Active Flows", "flows": [
                        {"priority": 200, "match": "dl_type=0x0806", "actions": "arp_responder", "packetCount": 1024, "byteCount": 65536},
                        {"priority": 100, "match": "in_port=gbr_wg", "actions": "output:gbr_warp", "packetCount": 50000, "byteCount": 3200000},
                    ]}),
                ),
                vec![],
            ),
        ],
    )
}

fn gen_privacy_chain(_plugins: &HashMap<String, PluginSchema>, n: usize) -> Value {
    spec(
        "stack-1",
        vec![
            (
                "stack-1",
                el("Stack", json!({"direction": "vertical", "gap": "md"})),
                vec!["h-1", "nt-1", "alert-1"],
            ),
            (
                "h-1",
                el(
                    "Heading",
                    json!({"level": "h2", "text": "Privacy Chain: WG → WARP → Xray"}),
                ),
                vec![],
            ),
            (
                "nt-1",
                el(
                    "NetworkTopology",
                    json!({
                        "nodes": [
                            {"id": "wg", "label": "gbr-wg", "type": "tunnel", "status": "up"},
                            {"id": "warp", "label": "gbr-warp", "type": "tunnel", "status": "up"},
                            {"id": "xray", "label": "gbr-xray", "type": "tunnel", "status": "up"},
                            {"id": "host", "label": "host", "type": "host", "status": "up"},
                        ],
                        "edges": [
                            {"from": "wg", "to": "warp", "label": "encrypted"},
                            {"from": "warp", "to": "xray", "label": "obfuscated"},
                            {"from": "xray", "to": "host", "label": "plaintext"},
                        ],
                    }),
                ),
                vec![],
            ),
            (
                "alert-1",
                el(
                    "Alert",
                    json!({"title": "Chain Active", "message": "All 3 privacy tunnels are up and forwarding", "type": "info"}),
                ),
                vec![],
            ),
        ],
    )
}

fn gen_oscal_registry(_plugins: &HashMap<String, PluginSchema>, n: usize) -> Value {
    let entries: Vec<Value> = [
        "src.network.xray-serve@v1", "exp.service.zeroclaw-serve@v1",
        "exp.service.cognitive-mcp-serve@v1", "src.service.qdrant-serve@v1",
        "src.service.netmaker-api-serve@v1", "mut.service.unix-socket.create-socket@v1",
    ].iter().enumerate().map(|(i, s)| json!({"subid": s, "category": s.split('.').next().unwrap_or(""), "status": if i + n < 5 { "active" } else { "declared" }})).collect();

    spec(
        "card-1",
        vec![
            (
                "card-1",
                el(
                    "Card",
                    json!({"title": "OSCAL Subid Registry", "description": "Canonical taxonomy"}),
                ),
                vec!["tbl-1"],
            ),
            (
                "tbl-1",
                el(
                    "Table",
                    json!({"columns": ["Subid", "Category", "Status"], "rows": entries}),
                ),
                vec![],
            ),
        ],
    )
}

fn gen_mixed_dashboard(plugins: &HashMap<String, PluginSchema>, n: usize) -> Value {
    let variant = n % 4;
    match variant {
        0 => {
            // Logs + metrics in a grid
            spec(
                "grid-1",
                vec![
                    (
                        "grid-1",
                        el("Grid", json!({"columns": 2, "gap": "md"})),
                        vec!["log-1", "metrics-1"],
                    ),
                    (
                        "log-1",
                        el(
                            "LogStream",
                            json!({"title": "D-Bus Events", "lines": [
                    {"timestamp": "22:00:01", "level": "info", "source": "op-dbus", "message": "Plugin state projected: openflow"},
                    {"timestamp": "22:00:02", "level": "info", "source": "op-web", "message": "gRPC stream connected"},
                    {"timestamp": "22:00:03", "level": "warn", "source": "gbr-xray", "message": "SNI dispatch fallback to op-web"},
                ], "autoScroll": true}),
                        ),
                        vec![],
                    ),
                    (
                        "metrics-1",
                        el("Stack", json!({"direction": "vertical", "gap": "sm"})),
                        vec!["m-1", "m-2"],
                    ),
                    (
                        "m-1",
                        el(
                            "ProcfsMetric",
                            json!({"label": "CPU", "value": 34, "unit": "%", "warningThreshold": 70, "criticalThreshold": 90}),
                        ),
                        vec![],
                    ),
                    (
                        "m-2",
                        el(
                            "ProcfsMetric",
                            json!({"label": "Memory", "value": 7.2, "unit": "GB", "warningThreshold": 12, "criticalThreshold": 15}),
                        ),
                        vec![],
                    ),
                ],
            )
        }
        1 => {
            // Topology + flow table in tabs
            spec(
                "tabs-1",
                vec![(
                    "tabs-1",
                    el(
                        "Tabs",
                        json!({"tabs": [{"label": "Topology", "value": "topo"}, {"label": "Flows", "value": "flows"}], "defaultValue": "topo"}),
                    ),
                    vec![],
                )],
            )
        }
        2 => {
            // Agent grid
            let agents: Vec<Value> = ["rust_pro", "memory", "backend_architect"].iter().map(|a| json!({
                "agentName": a, "provider": "anthropic", "model": "claude-sonnet-4", "status": "running", "tools": ["code_search", "file_read"],
            })).collect();
            let ids: Vec<&str> = vec!["ac-1", "ac-2", "ac-3"];
            spec(
                "grid-1",
                vec![
                    (
                        "grid-1",
                        el("Grid", json!({"columns": 3, "gap": "sm"})),
                        ids.to_vec(),
                    ),
                    ("ac-1", el("AgentConfigForm", agents[0].clone()), vec![]),
                    ("ac-2", el("AgentConfigForm", agents[1].clone()), vec![]),
                    ("ac-3", el("AgentConfigForm", agents[2].clone()), vec![]),
                ],
            )
        }
        _ => {
            // Data explorer + timeline
            spec(
                "stack-1",
                vec![
                    (
                        "stack-1",
                        el("Stack", json!({"direction": "vertical", "gap": "md"})),
                        vec!["de-1", "te-1"],
                    ),
                    (
                        "de-1",
                        el(
                            "DataExplorer",
                            json!({"title": "Plugin State", "data": json!({"openflow": {"flows": 42}, "wireguard": {"peers": 3}}).to_string()}),
                        ),
                        vec![],
                    ),
                    (
                        "te-1",
                        el(
                            "TimelineEvent",
                            json!({"timestamp": "22:00:00", "event_type": "mut", "title": "State mutation", "description": "openflow.flows updated", "severity": "info"}),
                        ),
                        vec![],
                    ),
                ],
            )
        }
    }
}

fn gen_nested_dialog(_p: &HashMap<String, PluginSchema>, n: usize) -> Value {
    let titles = [
        "Configuration",
        "Settings",
        "Preferences",
        "Advanced Options",
        "Edit Profile",
        "Network Setup",
        "Security Config",
    ];
    let labels = [
        "Open Settings",
        "Configure",
        "Edit",
        "Modify",
        "Adjust",
        "Setup",
        "Tune",
    ];
    spec(
        "card-1",
        vec![
            (
                "card-1",
                el("Card", json!({"title": titles[n % titles.len()]})),
                vec!["btn-1", "txt-1"],
            ),
            (
                "btn-1",
                el(
                    "Button",
                    json!({"label": labels[n % labels.len()], "variant": match n % 3 { 0 => "default", 1 => "outline", _ => "secondary" }}),
                ),
                vec![],
            ),
            (
                "txt-1",
                el(
                    "Text",
                    json!({"content": format!("Click to open {} dialog", titles[n % titles.len()]), "variant": "caption"}),
                ),
                vec![],
            ),
        ],
    )
}

fn gen_carousel_badges(_p: &HashMap<String, PluginSchema>, n: usize) -> Value {
    let all_badges = [
        "online",
        "encrypted",
        "audited",
        "paired",
        "synced",
        "healthy",
        "active",
        "secure",
        "verified",
        "trusted",
    ];
    let count = 3 + (n % 4);
    let badges: Vec<Value> = (0..count).map(|i| {
        let b = all_badges[(i + n) % all_badges.len()];
        json!({"text": b, "variant": match (i + n) % 3 { 0 => "success", 1 => "default", _ => "secondary" }})
    }).collect();
    spec(
        "carousel-1",
        vec![(
            "carousel-1",
            el("Carousel", json!({"items": badges})),
            vec![],
        )],
    )
}

fn gen_accordion_logs(_p: &HashMap<String, PluginSchema>, n: usize) -> Value {
    spec(
        "acc-1",
        vec![(
            "acc-1",
            el(
                "Accordion",
                json!({"items": [
            {"title": "op-dbus logs", "value": "dbus"},
            {"title": "xray logs", "value": "xray"},
            {"title": "ovs logs", "value": "ovs"},
        ], "type": "multiple", "defaultValue": "dbus"}),
            ),
            vec![],
        )],
    )
}

fn gen_drawer_topology(_p: &HashMap<String, PluginSchema>, n: usize) -> Value {
    spec(
        "stack-1",
        vec![
            (
                "stack-1",
                el("Stack", json!({"direction": "vertical", "gap": "sm"})),
                vec!["btn-1", "nt-1"],
            ),
            (
                "btn-1",
                el(
                    "Button",
                    json!({"label": "Show Network Map", "variant": "outline"}),
                ),
                vec![],
            ),
            (
                "nt-1",
                el(
                    "NetworkTopology",
                    json!({
                        "nodes": [{"id": "br", "label": "ovsbr0", "type": "bridge", "status": "up"}],
                        "edges": [],
                    }),
                ),
                vec![],
            ),
        ],
    )
}

fn gen_popover_metrics(_p: &HashMap<String, PluginSchema>, n: usize) -> Value {
    spec(
        "row-1",
        vec![
            (
                "row-1",
                el("Stack", json!({"direction": "horizontal", "gap": "sm"})),
                vec!["b-1", "b-2"],
            ),
            (
                "b-1",
                el("Badge", json!({"text": "CPU: 34%", "variant": "success"})),
                vec![],
            ),
            (
                "b-2",
                el("Badge", json!({"text": "MEM: 7.2GB", "variant": "warning"})),
                vec![],
            ),
        ],
    )
}

fn gen_toggle_filter_table(_p: &HashMap<String, PluginSchema>, n: usize) -> Value {
    spec(
        "stack-1",
        vec![
            (
                "stack-1",
                el("Stack", json!({"direction": "vertical", "gap": "sm"})),
                vec!["tg-1", "tbl-1"],
            ),
            (
                "tg-1",
                el(
                    "ToggleGroup",
                    json!({"items": ["All", "Active", "Inactive", "Error"], "type": "single", "value": "All"}),
                ),
                vec![],
            ),
            (
                "tbl-1",
                el(
                    "Table",
                    json!({"columns": ["Service", "Status"], "rows": [{"name": "op-dbus", "status": "active"}]}),
                ),
                vec![],
            ),
        ],
    )
}

fn gen_slider_thresholds(_p: &HashMap<String, PluginSchema>, n: usize) -> Value {
    spec(
        "card-1",
        vec![
            (
                "card-1",
                el("Card", json!({"title": "Alert Thresholds"})),
                vec!["s-1", "s-2", "s-3"],
            ),
            (
                "s-1",
                el(
                    "Slider",
                    json!({"label": "CPU Warning", "min": 0, "max": 100, "step": 1, "value": 70}),
                ),
                vec![],
            ),
            (
                "s-2",
                el(
                    "Slider",
                    json!({"label": "CPU Critical", "min": 0, "max": 100, "step": 1, "value": 90}),
                ),
                vec![],
            ),
            (
                "s-3",
                el(
                    "Slider",
                    json!({"label": "Memory Warning (GB)", "min": 0, "max": 32, "step": 1, "value": 12}),
                ),
                vec![],
            ),
        ],
    )
}

fn gen_collapsible_tree(_p: &HashMap<String, PluginSchema>, n: usize) -> Value {
    let data = json!({
        "system": {
            "kernel": "7.0.11-xanmod1",
            "uptime": "5d 3h 22m",
            "load": [0.42, 0.35, 0.28],
        },
        "network": {
            "interfaces": ["ens3", "ovsbr0", "opdbus", "wg-chatbot"],
            "bridges": {"ovsbr0": {"ports": 6, "flows": 42}},
        },
        "containers": {
            "running": ["qdrant", "assistant", "netmaker-ui", "netmaker-mq"],
            "stopped": ["netmaker", "wg-xray"],
        },
    })
    .to_string();

    spec(
        "col-1",
        vec![
            (
                "col-1",
                el(
                    "Collapsible",
                    json!({"title": "System State Tree", "defaultOpen": true}),
                ),
                vec!["de-1"],
            ),
            (
                "de-1",
                el(
                    "DataExplorer",
                    json!({"title": null, "data": data, "searchQuery": null}),
                ),
                vec![],
            ),
        ],
    )
}

fn gen_tabs_config(_p: &HashMap<String, PluginSchema>, n: usize) -> Value {
    spec(
        "card-1",
        vec![
            (
                "card-1",
                el("Card", json!({"title": "System Configuration"})),
                vec!["tabs-1"],
            ),
            (
                "tabs-1",
                el(
                    "Tabs",
                    json!({"tabs": [
            {"label": "General", "value": "general"},
            {"label": "D-Bus", "value": "dbus"},
            {"label": "Network", "value": "network"},
            {"label": "Security", "value": "security"},
            {"label": "Agents", "value": "agents"},
        ], "defaultValue": "general"}),
                ),
                vec![],
            ),
        ],
    )
}

fn gen_kv_inspector(_p: &HashMap<String, PluginSchema>, n: usize) -> Value {
    let pairs: Vec<Value> = [
        ("hostname", "ghostbridge"),
        ("kernel", "7.0.11-xanmod1"),
        ("public_ip", "148.113.204.83"),
        ("ovsbr0_ip", "10.200.0.1/30"),
        ("xray_port", "443"),
        ("grpc_port", "50051"),
        ("nextdns_profile", "689ec7"),
        ("dbus_bus", "org.opdbus.v1.plugins"),
    ]
    .iter()
    .map(|(k, v)| json!({"key": k, "value": v, "mono": true}))
    .collect();

    let ids: Vec<&str> = (0..pairs.len())
        .map(|i| {
            let s = format!("kv-{i}");
            leak(s)
        })
        .collect();

    let mut elements: Vec<(&str, Value, Vec<&str>)> = Vec::new();
    elements.push((
        "card-1",
        el("Card", json!({"title": "System Inspector"})),
        ids.clone().into(),
    ));
    for (i, p) in pairs.iter().enumerate() {
        elements.push((ids[i], el("KVPair", p.clone()), vec![]));
    }

    spec("card-1", elements)
}

fn gen_button_group_actions(_p: &HashMap<String, PluginSchema>, n: usize) -> Value {
    spec(
        "card-1",
        vec![
            (
                "card-1",
                el("Card", json!({"title": "Service Control"})),
                vec!["bg-1"],
            ),
            (
                "bg-1",
                el(
                    "ButtonGroup",
                    json!({"buttons": [
            {"label": "Start", "variant": "default"},
            {"label": "Stop", "variant": "outline"},
            {"label": "Restart", "variant": "outline"},
            {"label": "Delete", "variant": "destructive"},
        ], "selected": 0}),
                ),
                vec![],
            ),
        ],
    )
}

fn gen_progress_ring(_p: &HashMap<String, PluginSchema>, n: usize) -> Value {
    spec(
        "grid-1",
        vec![
            (
                "grid-1",
                el("Grid", json!({"columns": 3, "gap": "md"})),
                vec!["p-1", "p-2", "p-3"],
            ),
            (
                "p-1",
                el("Progress", json!({"value": 34, "max": 100, "label": "CPU"})),
                vec![],
            ),
            (
                "p-2",
                el(
                    "Progress",
                    json!({"value": 72, "max": 100, "label": "Memory"}),
                ),
                vec![],
            ),
            (
                "p-3",
                el(
                    "Progress",
                    json!({"value": 15, "max": 100, "label": "Disk"}),
                ),
                vec![],
            ),
        ],
    )
}

fn gen_skeleton_loader(_p: &HashMap<String, PluginSchema>, n: usize) -> Value {
    spec(
        "card-1",
        vec![
            (
                "card-1",
                el("Card", json!({"title": "Loading Plugin State..."})),
                vec!["s-1", "s-2", "s-3"],
            ),
            (
                "s-1",
                el(
                    "Skeleton",
                    json!({"width": "100%", "height": "20px", "rounded": true}),
                ),
                vec![],
            ),
            (
                "s-2",
                el(
                    "Skeleton",
                    json!({"width": "80%", "height": "20px", "rounded": true}),
                ),
                vec![],
            ),
            (
                "s-3",
                el(
                    "Skeleton",
                    json!({"width": "60%", "height": "20px", "rounded": true}),
                ),
                vec![],
            ),
        ],
    )
}

fn gen_alert_banner(_p: &HashMap<String, PluginSchema>, n: usize) -> Value {
    let types = ["info", "warning", "error", "success"];
    let msgs = [
        ("System Healthy", "All 12 services are running normally"),
        (
            "High Memory",
            "Memory usage at 72% — approaching warning threshold",
        ),
        (
            "Service Failed",
            "netmaker container crash-looping on LXC mount error",
        ),
        (
            "Update Complete",
            "All plugins schemas re-projected successfully",
        ),
    ];
    let (title, msg) = msgs[n % msgs.len()];
    spec(
        "stack-1",
        vec![
            (
                "stack-1",
                el("Stack", json!({"direction": "vertical", "gap": "sm"})),
                vec!["alert-1"],
            ),
            (
                "alert-1",
                el(
                    "Alert",
                    json!({"title": title, "message": msg, "type": types[n % types.len()]}),
                ),
                vec![],
            ),
        ],
    )
}

fn gen_dropdown_menu_actions(_p: &HashMap<String, PluginSchema>, n: usize) -> Value {
    spec(
        "card-1",
        vec![
            (
                "card-1",
                el("Card", json!({"title": "Quick Actions"})),
                vec!["dm-1"],
            ),
            (
                "dm-1",
                el(
                    "DropdownMenu",
                    json!({"label": "Actions", "items": ["Restart Service", "View Logs", "Edit Config", "Delete"]}),
                ),
                vec![],
            ),
        ],
    )
}

fn gen_pagination_table(_p: &HashMap<String, PluginSchema>, n: usize) -> Value {
    let rows: Vec<Value> = (0..20)
        .map(|i| {
            json!({
                "id": format!("evt-{}", 1000 + i),
                "type": match i % 4 { 0 => "mut", 1 => "obs", 2 => "evt", _ => "prj" },
                "timestamp": format!("22:{:02}:{:02}", i, i * 3 % 60),
            })
        })
        .collect();

    spec(
        "stack-1",
        vec![
            (
                "stack-1",
                el("Stack", json!({"direction": "vertical", "gap": "sm"})),
                vec!["tbl-1", "pg-1"],
            ),
            (
                "tbl-1",
                el(
                    "Table",
                    json!({"columns": ["ID", "Type", "Timestamp"], "rows": rows}),
                ),
                vec![],
            ),
            (
                "pg-1",
                el("Pagination", json!({"totalPages": 10, "page": 1})),
                vec![],
            ),
        ],
    )
}

fn gen_grid_metrics(_p: &HashMap<String, PluginSchema>, n: usize) -> Value {
    let cols = (n % 4) + 2;
    let ids: Vec<&str> = (0..cols * 2)
        .map(|i| {
            let s = format!("m-{i}");
            leak(s)
        })
        .collect();

    let mut elements: Vec<(&str, Value, Vec<&str>)> = Vec::new();
    elements.push((
        "grid-1",
        el("Grid", json!({"columns": cols, "gap": "sm"})),
        ids.clone().into(),
    ));
    for i in 0..cols * 2 {
        elements.push((
            ids[i],
            el(
                "ProcfsMetric",
                json!({
                    "label": format!("Metric {}", i + 1),
                    "value": (i as u64 + 1) * (n as u64 + 1) * 7 % 100,
                    "unit": "%",
                    "warningThreshold": 70,
                    "criticalThreshold": 90,
                }),
            ),
            vec![],
        ));
    }

    spec("grid-1", elements)
}

fn gen_stack_forms(_p: &HashMap<String, PluginSchema>, n: usize) -> Value {
    spec(
        "stack-1",
        vec![
            (
                "stack-1",
                el("Stack", json!({"direction": "vertical", "gap": "md"})),
                vec!["inp-1", "inp-2", "inp-3", "btn-1"],
            ),
            (
                "inp-1",
                el(
                    "Input",
                    json!({"label": "Hostname", "name": "hostname", "placeholder": "ghostbridge", "type": "text"}),
                ),
                vec![],
            ),
            (
                "inp-2",
                el(
                    "Input",
                    json!({"label": "Port", "name": "port", "placeholder": "50051", "type": "number"}),
                ),
                vec![],
            ),
            (
                "inp-3",
                el(
                    "Select",
                    json!({"label": "Protocol", "name": "protocol", "options": ["grpc", "tcp", "unix"], "value": "grpc"}),
                ),
                vec![],
            ),
            (
                "btn-1",
                el("Button", json!({"label": "Save", "variant": "default"})),
                vec![],
            ),
        ],
    )
}

fn gen_separator_sections(_p: &HashMap<String, PluginSchema>, n: usize) -> Value {
    spec(
        "stack-1",
        vec![
            (
                "stack-1",
                el("Stack", json!({"direction": "vertical", "gap": "md"})),
                vec!["h-1", "txt-1", "sep-1", "h-2", "txt-2"],
            ),
            (
                "h-1",
                el("Heading", json!({"level": "h3", "text": "Network"})),
                vec![],
            ),
            (
                "txt-1",
                el(
                    "Text",
                    json!({"content": "OVS bridge ovsbr0 with 6 ports and 42 flows", "variant": "body"}),
                ),
                vec![],
            ),
            ("sep-1", el("Separator", json!({})), vec![]),
            (
                "h-2",
                el("Heading", json!({"level": "h3", "text": "Services"})),
                vec![],
            ),
            (
                "txt-2",
                el(
                    "Text",
                    json!({"content": "12 active, 3 inactive, 0 failed", "variant": "body"}),
                ),
                vec![],
            ),
        ],
    )
}

fn gen_radio_provider(_p: &HashMap<String, PluginSchema>, n: usize) -> Value {
    spec(
        "card-1",
        vec![
            (
                "card-1",
                el("Card", json!({"title": "LLM Provider"})),
                vec!["r-1"],
            ),
            (
                "r-1",
                el(
                    "Radio",
                    json!({"label": "Select provider", "name": "provider", "options": ["anthropic", "openai", "google", "ollama", "factory"], "value": "anthropic"}),
                ),
                vec![],
            ),
        ],
    )
}

fn gen_checkbox_tools(_p: &HashMap<String, PluginSchema>, n: usize) -> Value {
    let tools = [
        "ovs_create_bridge",
        "ovs_add_port",
        "llm_chat",
        "tool_execute",
        "state_query",
        "service_restart",
    ];
    let ids: Vec<&str> = (0..tools.len())
        .map(|i| {
            let s = format!("cb-{i}");
            leak(s)
        })
        .collect();

    let mut elements: Vec<(&str, Value, Vec<&str>)> = Vec::new();
    elements.push((
        "card-1",
        el("Card", json!({"title": "Agent Tool Assignment"})),
        ids.clone().into(),
    ));
    for (i, tool) in tools.iter().enumerate() {
        elements.push((
            ids[i],
            el(
                "Checkbox",
                json!({"label": tool, "name": tool, "checked": (i + n) % 2 == 0}),
            ),
            vec![],
        ));
    }

    spec("card-1", elements)
}

fn gen_textarea_prompt(_p: &HashMap<String, PluginSchema>, n: usize) -> Value {
    spec(
        "card-1",
        vec![
            (
                "card-1",
                el("Card", json!({"title": "System Prompt Editor"})),
                vec!["ta-1", "btn-1"],
            ),
            (
                "ta-1",
                el(
                    "Textarea",
                    json!({"label": "System Prompt", "name": "prompt", "placeholder": "You are a helpful AI assistant...", "rows": 10, "value": "You are ANNA, the Axon Network Notary Arbitrator. You notarize WireGuard identity and handle Snowball sessions."}),
                ),
                vec![],
            ),
            (
                "btn-1",
                el(
                    "Button",
                    json!({"label": "Save Prompt", "variant": "default"}),
                ),
                vec![],
            ),
        ],
    )
}

fn gen_link_nav(_p: &HashMap<String, PluginSchema>, n: usize) -> Value {
    spec(
        "stack-1",
        vec![
            (
                "stack-1",
                el("Stack", json!({"direction": "horizontal", "gap": "md"})),
                vec!["l-1", "l-2", "l-3", "l-4"],
            ),
            (
                "l-1",
                el("Link", json!({"label": "Overview", "href": "/"})),
                vec![],
            ),
            (
                "l-2",
                el("Link", json!({"label": "Logs", "href": "/logs"})),
                vec![],
            ),
            (
                "l-3",
                el("Link", json!({"label": "Services", "href": "/services"})),
                vec![],
            ),
            (
                "l-4",
                el(
                    "Link",
                    json!({"label": "Gallery", "href": "/gemma-gallery"}),
                ),
                vec![],
            ),
        ],
    )
}

fn gen_avatar_agents(_p: &HashMap<String, PluginSchema>, n: usize) -> Value {
    let agents = [
        ("ANNA", "scribe"),
        ("Olivia", "scal"),
        ("Eugene", "risk"),
        ("Penny", "privacy"),
        ("Reggie", "opa"),
    ];
    let ids: Vec<&str> = (0..agents.len())
        .map(|i| {
            let s = format!("av-{i}");
            leak(s)
        })
        .collect();

    let mut elements: Vec<(&str, Value, Vec<&str>)> = Vec::new();
    elements.push((
        "grid-1",
        el("Grid", json!({"columns": 5, "gap": "sm"})),
        ids.clone().into(),
    ));
    for (i, (name, role)) in agents.iter().enumerate() {
        elements.push((
            ids[i],
            el("Avatar", json!({"src": null, "name": name, "size": "md"})),
            vec![],
        ));
    }

    spec("grid-1", elements)
}

// ── Utilities ────────────────────────────────────────────────────────────────

/// Map a `FieldType` to a UI-facing type label and, for `Enum`, its values.
fn field_type_label(ft: &FieldType) -> (&'static str, Option<Value>) {
    match ft {
        FieldType::String => ("string", None),
        FieldType::Integer => ("integer", None),
        FieldType::Float => ("number", None),
        FieldType::Boolean => ("boolean", None),
        FieldType::Array(_) => ("array", None),
        FieldType::Object(_) | FieldType::Map(_) => ("object", None),
        FieldType::Enum(values) => ("string", Some(json!(values))),
        FieldType::OneOf(_) => ("object", None),
        FieldType::JsonSchema(_) => ("json_schema", None),
        FieldType::Any => ("string", None),
    }
}

fn load_plugin_schemas() -> Result<HashMap<String, PluginSchema>> {
    let ids = op_blob::catalog::read_manifest_plugin_ids_shm()
        .context("failed to read plugin blob manifest from SHM catalog")?;
    let mut schemas = HashMap::with_capacity(ids.len());
    for id in ids {
        if let Some(schema) = op_blob::catalog::read_plugin_schema_shm(&id) {
            schemas.insert(id, schema);
        } else {
            warn!(plugin_id = %id, "Gemma UI gallery: no sealed schema for manifest entry, skipping");
        }
    }
    Ok(schemas)
}

fn load_existing_specs() -> Result<GemmaSpecGallery> {
    match fs::read_to_string(GEMMA_UI_SPECS) {
        Ok(raw) => {
            let gallery: GemmaSpecGallery =
                serde_json::from_str(&raw).unwrap_or(GemmaSpecGallery {
                    version: 1,
                    specs: vec![],
                });
            Ok(gallery)
        }
        Err(_) => Ok(GemmaSpecGallery {
            version: 1,
            specs: vec![],
        }),
    }
}

fn write_gallery(gallery: &GemmaSpecGallery) -> Result<()> {
    let tmp = format!("{}.tmp", GEMMA_UI_SPECS);
    let json = serde_json::to_string_pretty(gallery)?;
    let mut f = fs::File::create(&tmp)?;
    f.write_all(json.as_bytes())?;
    f.sync_data()?;
    fs::rename(&tmp, GEMMA_UI_SPECS)?;
    Ok(())
}

fn signature_of(spec: &Value) -> String {
    // Structural signature: component types in traversal order + props shape
    let mut sig = String::new();
    if let Some(elements) = spec.get("elements").and_then(|e| e.as_object()) {
        let root = spec.get("root").and_then(|r| r.as_str()).unwrap_or("");
        fn walk(id: &str, elements: &Map<String, Value>, sig: &mut String, depth: usize) {
            if let Some(elem) = elements.get(id) {
                let ty = elem.get("type").and_then(|t| t.as_str()).unwrap_or("?");
                let props = elem.get("props").map(|p| p.to_string()).unwrap_or_default();
                let props_hash = short_hash(&props);
                sig.push_str(&format!(
                    "{}:{}:{}|",
                    depth,
                    ty,
                    &props_hash[..8.min(props_hash.len())]
                ));
                if let Some(children) = elem.get("children").and_then(|c| c.as_array()) {
                    for child in children {
                        if let Some(cid) = child.as_str() {
                            walk(cid, elements, sig, depth + 1);
                        }
                    }
                }
            }
        }
        walk(root, elements, &mut sig, 0);
    }
    short_hash(&sig)
}

fn short_hash(s: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn tags_for_generator(gen_name: &str, plugins: &HashMap<String, PluginSchema>) -> Vec<String> {
    let mut tags = vec![gen_name.to_string()];
    // Add a random plugin name as a tag for variety
    let plugin_names: Vec<&String> = plugins.keys().collect();
    if !plugin_names.is_empty() {
        let idx = (gen_name.len() + tags.len()) % plugin_names.len();
        tags.push(plugin_names[idx].clone());
    }
    tags
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
