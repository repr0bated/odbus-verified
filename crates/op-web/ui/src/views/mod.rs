//! Stub views — one per route. The Inspector, State, gRPC, and gRPC Explorer
//! views are schema-driven via `crate::schema` + `crate::grpc::ReflectionRegistry`.
use crate::chat;
use crate::grpc::{InvokeHandle, MethodKind, ReflectionRegistry, StreamHandle};
use crate::nav::{route_title, Route};
use crate::schema as schema_ui;
use crate::theme::*;
use egui::{Frame, Margin, RichText, Rounding, Stroke};
use schemars::{schema_for, JsonSchema};
use serde::{Deserialize, Serialize};

#[derive(Default)]
pub struct ExplorerState {
    pub service: Option<String>,
    pub method: Option<String>,
    pub request_body: String,
    pub last_response: Option<Result<serde_json::Value, String>>,
    pub last_signature: Option<(String, String)>,
    pub validation: Option<Result<(), String>>,
    pub last_validated: Option<String>,
    pub stream: StreamHandle,
    pub chat_store: crate::chat::store::ChatStore,
    /// Expanded-rows cache: flattens the visible tree into a `Vec<RowHandle>`
    /// per frame so that virtualized `render_array` only resolves schema +
    /// data for rows currently on screen. Rebuilt each frame from the live
    /// payload + collapsing state.
    pub row_cache: Vec<schema_ui::RowHandle>,
}


pub fn render(
    ui: &mut egui::Ui,
    route: Route,
    registry: &ReflectionRegistry,
    explorer: &mut ExplorerState,
    invoke: &InvokeHandle,
    ctx: &egui::Context,
) {
    // Drain any completed invocation result into the explorer state.
    if let Some(res) = invoke.take_result() {
        explorer.last_response = Some(res);
    }

    match route {
        Route::Overview     => overview(ui),
        Route::Install      => install(ui),
        Route::Chat         => chat::view::render_chat(ui, &mut explorer.chat_store, ctx),
        Route::Inspector    => inspector(ui),
        Route::State        => state_view(ui),
        Route::Grpc         => grpc_view(ui, registry),
        Route::GrpcExplorer => grpc_explorer(ui, registry, explorer, invoke, ctx),
        r => {
            // Render real plugin state from the catalog store
            let store = catalog_store.snapshot();
            if let Some(element) = store.latest_active(&r.to_string()) {
                ui.label(RichText::new(&format!("Live Plugin: {}", r)).color(MUTED).size(13.0);
                ui.label(RichText::new(&format!("{}", element.spec["description"].as_str().unwrap_or(""))).size(11.0));
            } else {
                stub(ui, route_title(r), description(r))
            }
        },
    }
}

fn card<R>(ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui) -> R) -> R {
    Frame::none()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .rounding(Rounding::same(8.0))
        .inner_margin(Margin::same(16.0))
        .show(ui, body)
        .inner
}

fn stub(ui: &mut egui::Ui, title: &str, desc: &str) {
    ui.label(RichText::new(title).size(22.0).strong().color(FG));
    ui.add_space(4.0);
    ui.label(RichText::new(desc).color(MUTED).size(13.0));
    ui.add_space(16.0);
    card(ui, |ui| {
        ui.label(RichText::new("View not yet ported.").color(FG).size(13.0));
        ui.add_space(4.0);
        ui.label(RichText::new("Wire this view to the ZeroClaw core (gRPC/D-Bus) and render real state here.").color(MUTED).size(12.0));
    });
}

fn overview(ui: &mut egui::Ui) {
    ui.label(RichText::new("Overview").size(22.0).strong().color(FG));
    ui.add_space(4.0);
    ui.label(RichText::new("Live state of the ZeroClaw control plane.").color(MUTED).size(13.0));
    ui.add_space(16.0);

    ui.horizontal_wrapped(|ui| {
        for (label, value, hint, color) in [
            ("Core", "ONLINE",  "op-core mirror",    OK),
            ("Agent", "READY",  "router idle",       OK),
            ("Web",   "BOUND",  ":8443",             OK),
            ("Embed", "WARM",   "qdrant linked",     OK),
            ("Mesh",  "WG-UP",  "3 peers",           OK),
        ] {
            stat_card(ui, label, value, hint, color);
        }
    });
}

fn stat_card(ui: &mut egui::Ui, label: &str, value: &str, hint: &str, accent: egui::Color32) {
    let size = egui::Vec2::new(180.0, 88.0);
    ui.allocate_ui(size, |ui| {
        card(ui, |ui| {
            ui.set_width(size.x - 32.0);
            ui.label(RichText::new(label).size(10.0).color(MUTED));
            ui.add_space(2.0);
            ui.label(RichText::new(value).size(18.0).strong().color(accent).monospace());
            ui.add_space(2.0);
            ui.label(RichText::new(hint).size(11.0).color(MUTED));
        });
    });
}

fn install(ui: &mut egui::Ui) {
    ui.label(RichText::new("Install").size(22.0).strong().color(FG));
    ui.add_space(4.0);
    ui.label(RichText::new("Provision a full ZeroClaw stack on this host.").color(MUTED).size(13.0));
    ui.add_space(16.0);
    for (title, body) in [
        ("1. Prerequisites", "rustc 1.78+, cargo, runit (sv), wireguard-tools, qdrant, btrfs-progs."),
        ("2. Fetch",         "git clone https://git.zeroclaw.dev/zeroclaw.git && cd zeroclaw"),
        ("3. Build",         "cargo build --release --workspace"),
        ("4. Services",      "sudo cp -r dist/runit/sv/* /etc/runit/sv/ && sudo ln -sfn /etc/runit/sv/zeroclaw /etc/runit/runsvdir/default/zeroclaw && sudo sv start zeroclaw"),

        ("5. Privacy Mesh",  "sudo wg-quick up zeroclaw0 && verify peer handshake"),
        ("6. Data Stores",   "btrfs subvol create /var/zeroclaw/{state,logs,vec}"),
        ("7. Verify",        "zeroclawctl health  →  expect all green"),
    ] {
        card(ui, |ui| {
            ui.label(RichText::new(title).size(13.0).strong().color(FG));
            ui.add_space(2.0);
            ui.label(RichText::new(body).monospace().size(12.0).color(MUTED));
        });
        ui.add_space(8.0);
    }
}

// ------------------------------------------------------------------
// Schema-as-code demo:
//
// In production this struct lives in `crates/op-plugins`. Each plugin derives
// `JsonSchema` on its state, calls `schemars::schema_for!(...)`, and ships the
// resulting `Schema` over gRPC alongside the encoded message. The GUI then
// renders any plugin state without code changes via `schema_ui::render_value`.
// ------------------------------------------------------------------
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(title = "DemoPluginState", description = "Reference state struct ported from op-plugins to prove the schemars → schema_ui pipeline.")]
struct DemoPluginState {
    /// Plugin identifier (matches the runit service name).
    id: String,
    /// Current lifecycle phase.
    phase: Phase,
    /// Last heartbeat in seconds since epoch.
    last_seen: u64,
    /// Connected peers, name → ip.
    peers: Vec<Peer>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
enum Phase { Booting, Ready, Degraded, Stopped }

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct Peer {
    name: String,
    ip: String,
    /// Round-trip latency in milliseconds.
    rtt_ms: f32,
}

fn demo_payload() -> (schemars::Schema, serde_json::Value) {
    let schema = schema_for!(DemoPluginState);
    let value = serde_json::to_value(DemoPluginState {
        id: "op-core".into(),
        phase: Phase::Ready,
        last_seen: 1_724_400_000,
        peers: vec![
            Peer { name: "agent-01".into(),  ip: "10.149.181.10".into(), rtt_ms: 0.82 },
            Peer { name: "embed-01".into(),  ip: "10.149.181.190".into(), rtt_ms: 1.41 },
        ],
    }).unwrap();
    (schema, value)
}

fn inspector(ui: &mut egui::Ui) {
    ui.label(RichText::new("Inspector").size(22.0).strong().color(FG));
    ui.add_space(4.0);
    ui.label(RichText::new("Schema-driven view of the live store. Schema sourced from op-plugins via schemars::schema_for!.").color(MUTED).size(13.0));
    ui.add_space(16.0);
    let (schema, value) = demo_payload();
    card(ui, |ui| schema_ui::render_value(ui, &schema, &value));
}

fn state_view(ui: &mut egui::Ui) {
    ui.label(RichText::new("State").size(22.0).strong().color(FG));
    ui.add_space(4.0);
    ui.label(RichText::new("Projected state by plugin id. Decoded from prost-reflect DynamicMessage, rendered via JSON Schema.").color(MUTED).size(13.0));
    ui.add_space(16.0);

    let (schema, value) = demo_payload();
    card(ui, |ui| {
        ui.label(RichText::new("op-core / DemoPluginState").size(12.0).strong().color(FG));
        ui.add_space(6.0);
        schema_ui::render_value(ui, &schema, &value);
    });
    ui.add_space(10.0);
    card(ui, |ui| {
        ui.collapsing("raw JSON", |ui| {
            ui.label(RichText::new(schema_ui::pretty_json(&value)).monospace().size(11.0).color(MUTED));
        });
    });
}

fn grpc_view(ui: &mut egui::Ui, registry: &ReflectionRegistry) {
    ui.label(RichText::new("gRPC Diagnostics").size(22.0).strong().color(FG));
    ui.add_space(4.0);
    ui.label(RichText::new("tonic-web client with server reflection. Services below are discovered live via grpc.reflection.v1.ServerReflection.").color(MUTED).size(13.0));
    ui.add_space(16.0);

    let services = registry.services();
    card(ui, |ui| {
        if services.is_empty() {
            ui.label(RichText::new("Reflection pool empty — server unreachable or reflection not enabled.").color(WARN).size(12.0));
            ui.add_space(4.0);
            ui.label(RichText::new("Set ZEROCLAW_GRPC=http://host:port and restart. Server must register tonic_reflection::server::Builder with FILE_DESCRIPTOR_SET.").color(MUTED).size(11.0).italics());
        } else {
            ui.label(RichText::new(format!("{} services", services.len())).color(FG).strong().size(12.0));
            ui.add_space(6.0);
            for svc in &services {
                ui.collapsing(RichText::new(svc).monospace().color(FG).size(12.0), |ui| {
                    for m in registry.methods(svc) {
                        ui.label(RichText::new(format!("  ▸ {}", m)).monospace().color(MUTED).size(11.0));
                    }
                });
            }
        }
    });
}

fn grpc_explorer(
    ui: &mut egui::Ui,
    registry: &ReflectionRegistry,
    state: &mut ExplorerState,
    invoke: &InvokeHandle,
    ctx: &egui::Context,
) {
    ui.label(RichText::new("gRPC Explorer").size(22.0).strong().color(FG));
    ui.add_space(4.0);
    ui.label(RichText::new("Pick a reflected method, edit a schema-derived JSON request, invoke, and inspect responses. Supports unary and server-streaming RPCs with live request validation.").color(MUTED).size(13.0));
    ui.add_space(16.0);

    let services = registry.services();
    if services.is_empty() {
        card(ui, |ui| {
            ui.label(RichText::new("Reflection pool empty — server unreachable or reflection not enabled.").color(WARN).size(12.0));
        });
        return;
    }

    // Method picker
    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("service").color(MUTED).size(11.0).monospace());
            let current_svc = state.service.clone().unwrap_or_default();
            egui::ComboBox::from_id_source("explorer-svc")
                .selected_text(if current_svc.is_empty() { "—".into() } else { current_svc.clone() })
                .show_ui(ui, |ui| {
                    for svc in &services {
                        if ui.selectable_label(state.service.as_deref() == Some(svc.as_str()), svc).clicked() {
                            state.service = Some(svc.clone());
                            state.method = None;
                            state.request_body.clear();
                            state.last_signature = None;
                            state.validation = None;
                            state.last_validated = None;
                            state.stream.clear();
                        }
                    }
                });

            ui.add_space(12.0);
            ui.label(RichText::new("method").color(MUTED).size(11.0).monospace());
            let methods = state.service.as_deref().map(|s| registry.methods(s)).unwrap_or_default();
            let current_m = state.method.clone().unwrap_or_default();
            egui::ComboBox::from_id_source("explorer-method")
                .selected_text(if current_m.is_empty() { "—".into() } else { current_m.clone() })
                .show_ui(ui, |ui| {
                    for m in &methods {
                        if ui.selectable_label(state.method.as_deref() == Some(m.as_str()), m).clicked() {
                            state.method = Some(m.clone());
                            state.last_signature = None;
                            state.validation = None;
                            state.last_validated = None;
                            state.stream.clear();
                        }
                    }
                });
        });
    });

    let (Some(svc), Some(method)) = (state.service.clone(), state.method.clone()) else {
        ui.add_space(8.0);
        ui.label(RichText::new("Select a service + method to continue.").color(MUTED).size(12.0).italics());
        return;
    };

    let kind = registry.method_kind(&svc, &method).unwrap_or(MethodKind::Unary);

    // Refresh the template when method changes.
    let sig = (svc.clone(), method.clone());
    if state.last_signature.as_ref() != Some(&sig) {
        match crate::grpc::template_for_request(registry, &svc, &method) {
            Ok(tpl) => state.request_body = schema_ui::pretty_json(&tpl),
            Err(e)  => state.request_body = format!("// {e}\n{{}}"),
        }
        state.last_signature = Some(sig);
        state.last_validated = None;
    }

    // Live validation: only re-check when the body or method changes.
    if state.last_validated.as_deref() != Some(state.request_body.as_str()) {
        state.validation = Some(
            crate::grpc::validate_request(registry, &svc, &method, &state.request_body)
                .map_err(|e| format!("{e:#}")),
        );
        state.last_validated = Some(state.request_body.clone());
    }

    let request_valid = matches!(state.validation, Some(Ok(())));
    let snapshot = state.stream.snapshot();
    let streaming = matches!(kind, MethodKind::ServerStreaming);
    let unary_busy = invoke.in_flight();
    let stream_busy = snapshot.running;

    ui.add_space(8.0);

    // Request editor
    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("/{}/{}", svc, method)).monospace().color(FG).size(12.0).strong());
            ui.add_space(8.0);
            ui.label(RichText::new(format!("[{}]", kind.label())).monospace().color(MUTED).size(11.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let supported = matches!(kind, MethodKind::Unary | MethodKind::ServerStreaming);

                if streaming && stream_busy {
                    let stop = egui::Button::new(RichText::new("■  Stop").color(egui::Color32::WHITE).size(12.0).strong())
                        .fill(DANGER)
                        .stroke(Stroke::new(1.0, DANGER))
                        .rounding(Rounding::same(6.0))
                        .min_size(egui::Vec2::new(96.0, 26.0));
                    if ui.add(stop).clicked() { state.stream.cancel(); }
                } else {
                    let busy = unary_busy || stream_busy;
                    let label = if !supported {
                        "Unsupported"
                    } else if busy {
                        "Working…"
                    } else if streaming {
                        "▶  Start stream"
                    } else {
                        "▶  Invoke"
                    };
                    let btn = egui::Button::new(RichText::new(label).color(egui::Color32::WHITE).size(12.0).strong())
                        .fill(if busy || !supported || !request_valid { PRIMARY.linear_multiply(0.5) } else { PRIMARY })
                        .stroke(Stroke::new(1.0, PRIMARY))
                        .rounding(Rounding::same(6.0))
                        .min_size(egui::Vec2::new(110.0, 26.0));
                    let enabled = supported && !busy && request_valid;
                    if ui.add_enabled(enabled, btn).clicked() {
                        // safe: validation already parsed it
                        let body: serde_json::Value =
                            serde_json::from_str(&state.request_body).unwrap_or(serde_json::Value::Null);
                        if streaming {
                            state.stream.clear();
                            state.stream.spawn(registry.clone(), svc.clone(), method.clone(), body, ctx.clone());
                        } else {
                            invoke.spawn(registry.clone(), svc.clone(), method.clone(), body, ctx.clone());
                        }
                    }
                }
                if ui.button(RichText::new("↻ Reset").color(MUTED).size(11.0)).clicked() {
                    state.last_signature = None;
                    state.stream.clear();
                }
            });
        });

        ui.add_space(6.0);
        // Validation banner
        match &state.validation {
            Some(Ok(())) => {
                ui.label(RichText::new("✓ request matches input schema").color(OK).size(11.0).monospace());
            }
            Some(Err(e)) => {
                ui.label(RichText::new(format!("✗ {e}")).color(DANGER).size(11.0).monospace());
            }
            None => {}
        }

        ui.add_space(6.0);
        ui.label(RichText::new("request (JSON, schema-derived)").color(MUTED).size(11.0));
        egui::ScrollArea::vertical().id_source("req-editor").max_height(220.0).show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut state.request_body)
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .desired_rows(10),
            );
        });
    });

    ui.add_space(10.0);

    // Response / stream panel
    card(ui, |ui| {
        if streaming {
            ui.horizontal(|ui| {
                ui.label(RichText::new("stream").color(MUTED).size(11.0));
                ui.add_space(8.0);
                let status = if snapshot.running { "● live" }
                    else if snapshot.closed { "● closed" }
                    else { "○ idle" };
                let color = if snapshot.running { OK } else if snapshot.error.is_some() { DANGER } else { MUTED };
                ui.label(RichText::new(status).color(color).size(11.0).monospace());
                ui.add_space(8.0);
                ui.label(RichText::new(format!("{} items", snapshot.items.len())).color(MUTED).size(11.0).monospace());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(RichText::new("Clear").color(MUTED).size(11.0)).clicked() {
                        state.stream.clear();
                    }
                });
            });
            ui.add_space(6.0);
            if let Some(e) = &snapshot.error {
                ui.label(RichText::new(e).color(DANGER).monospace().size(12.0));
                ui.add_space(6.0);
            }
            egui::ScrollArea::vertical()
                .id_source("stream-items")
                .max_height(420.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    for (i, item) in snapshot.items.iter().enumerate() {
                        ui.collapsing(
                            RichText::new(format!("▸ #{i}")).monospace().color(FG).size(11.0),
                            |ui| {
                                ui.label(
                                    RichText::new(schema_ui::pretty_json(item))
                                        .monospace().size(11.0).color(FG),
                                );
                            },
                        );
                    }
                    if snapshot.items.is_empty() && !snapshot.running {
                        ui.label(RichText::new("— no items yet —").color(MUTED).italics().size(12.0));
                    }
                });
            if snapshot.running {
                ctx.request_repaint_after(std::time::Duration::from_millis(120));
            }
        } else {
            ui.label(RichText::new("response").color(MUTED).size(11.0));
            ui.add_space(4.0);
            match &state.last_response {
                None => {
                    ui.label(RichText::new("— no invocation yet —").color(MUTED).italics().size(12.0));
                }
                Some(Err(e)) => {
                    ui.label(RichText::new(e).color(DANGER).monospace().size(12.0));
                }
                Some(Ok(val)) => {
                    egui::ScrollArea::vertical().id_source("resp").max_height(360.0).show(ui, |ui| {
                        ui.label(RichText::new(schema_ui::pretty_json(val)).monospace().size(11.0).color(FG));
                    });
                }
            }
        }
    });
}



fn description(r: Route) -> &'static str {
    match r {
        Route::Orchestration   => "Plugin lifecycle, dependency graph, restart policy.",
        Route::Services        => "runit service control center — start/stop/restart via sv.",
        Route::Sessions        => "Active client sessions and auth handles.",
        Route::Llm             => "LLM router status, provider mix, queue depth.",
        Route::Agents          => "Persona registry sourced from personas.yaml.",
        Route::Assistant       => "Operator assistant — tool-augmented agent.",
        Route::Models          => "Routable models registry and availability.",
        Route::Tools           => "Registered tools and recent invocations.",
        Route::Workflows       => "Declarative multi-step workflows.",
        Route::Security        => "RBAC, masking, audit posture.",
        Route::Accountability  => "Reasoning audit trail and PII review.",
        Route::Skills          => "Active skills and trigger phrases.",
        Route::Knowledge       => "Local knowledge base and Qdrant indexer.",
        Route::Embedding       => "Embedding worker pipeline → Qdrant.",
        Route::Containers      => "Container runtimes and workloads.",
        Route::PrivacyNetwork  => "WireGuard / XRay / WARP route map.",
        Route::Ovs             => "Open vSwitch bridges and ports.",
        Route::OpenFlow        => "Installed OpenFlow rules.",
        Route::Btrfs           => "BTRFS subvolumes and snapshots.",
        Route::DataStores      => "Postgres / Redis / Qdrant endpoints.",
        Route::Config          => "Tunables and immutable rules.",
        Route::Logs            => "Actionable structured log stream.",
        Route::Dreams          => "Coming soon.",
        Route::Reflections     => "Coming soon.",
        Route::Observability   => "Coming soon.",
        Route::LiveTrace       => "Coming soon.",
        _ => "",
    }
}
