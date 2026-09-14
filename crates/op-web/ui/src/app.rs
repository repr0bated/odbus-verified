//! Root app — topbar + collapsible sidebar + routed content.
use crate::auth::AuthState;
use crate::chat::ChatTransport;
use crate::grpc::{InvokeHandle, ReflectionRegistry};
use crate::views::ExplorerState;
use crate::nav::{route_title, Route, TopTab, GROUPS};
use crate::theme::*;
use crate::views;
use eframe::CreationContext;
use egui::{
    Align, Color32, FontId, Frame, Layout, Margin, RichText, Rounding, Sense, Stroke, TextEdit, Vec2,
};
use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::Mutex;

pub struct ZeroClawApp {
    pub route: Route,
    pub top_tab: TopTab,
    pub nav_collapsed: bool,
    pub collapsed_groups: HashMap<String, bool>,
    pub connected: bool,
    pub theme_mode: ThemeMode,
    pub registry: ReflectionRegistry,
    pub invoke: InvokeHandle,
    pub explorer: ExplorerState,
    pub auth: AuthState,
    pub pairing_code_input: String,
    pub error_msg: Option<String>,
    /// gRPC ChatService transport. None if not yet connected.
    pub chat_transport: Option<Arc<ChatTransport>>,
    /// Shared slot for passing the stream receiver from a spawned task
    /// back to the egui main thread.
    pub chat_rx_slot: Arc<Mutex<Option<tokio::sync::mpsc::Receiver<crate::chat::ChatFrameEvent>>>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode { Dark, Light, System }

impl ZeroClawApp {
    pub fn new(_cc: &CreationContext<'_>) -> Self {
        Self::new_with_registry(_cc, ReflectionRegistry::new())
    }

    pub fn new_with_registry(_cc: &CreationContext<'_>, registry: ReflectionRegistry) -> Self {
        let auth = AuthState::load();
        let connected = auth.token.is_some();
        Self {
            route: Route::Overview,
            top_tab: TopTab::Console,
            nav_collapsed: false,
            collapsed_groups: HashMap::new(),
            connected,
            theme_mode: ThemeMode::Dark,
            registry,
            invoke: InvokeHandle::new(),
            explorer: ExplorerState::default(),
            auth,
            pairing_code_input: String::new(),
            error_msg: None,
            chat_transport: None,
            chat_rx_slot: Arc::new(Mutex::new(None)),
        }
    }

    fn topbar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("topbar")
            .exact_height(56.0)
            .frame(Frame::none().fill(BG).stroke(Stroke::new(1.0, BORDER)).inner_margin(Margin::symmetric(20.0, 0.0)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    // Brand
                    if ui.add(egui::Button::new(RichText::new("☰").color(MUTED)).frame(false)).clicked() {
                        self.nav_collapsed = !self.nav_collapsed;
                    }
                    ui.add_space(8.0);
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(28.0, 28.0), Sense::hover());
                    ui.painter().rect_filled(rect, Rounding::same(6.0), PRIMARY);
                    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "Z", FontId::proportional(14.0), Color32::WHITE);
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        ui.add_space(8.0);
                        ui.label(RichText::new("ZEROCLAW").size(15.0).strong().color(FG));
                        ui.label(RichText::new("OPERATOR CONSOLE").size(9.0).color(MUTED));
                    });

                    // Center tabs
                    ui.with_layout(Layout::top_down(Align::Center), |ui| {
                        ui.add_space(14.0);
                        ui.horizontal(|ui| {
                            ui.add_space((ui.available_width() / 2.0) - 90.0);
                            tab_pill(ui, "Console", self.top_tab == TopTab::Console, || {
                                self.top_tab = TopTab::Console;
                                if matches!(self.route, Route::Install) { self.route = Route::Overview; }
                            });
                            tab_pill(ui, "Install", self.top_tab == TopTab::Install, || {
                                self.top_tab = TopTab::Install;
                                self.route = Route::Install;
                            });
                        });
                    });

                    // Right: health + theme
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        theme_toggle(ui, &mut self.theme_mode);
                        ui.add_space(8.0);
                        health_pill(ui, self.connected);
                    });
                });
            });
    }

    fn sidebar(&mut self, ctx: &egui::Context) {
        if self.nav_collapsed || self.top_tab == TopTab::Install {
            return;
        }
        egui::SidePanel::left("nav")
            .exact_width(220.0)
            .resizable(false)
            .frame(Frame::none().fill(BG).inner_margin(Margin::symmetric(12.0, 16.0)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for group in GROUPS {
                        let key = group.label.to_string();
                        let collapsed = *self.collapsed_groups.get(&key).unwrap_or(&false);
                        let has_active = group.items.iter().any(|i| i.route == self.route);

                        let header = ui.add(
                            egui::Button::new(RichText::new(group.label).size(11.0).color(MUTED))
                                .frame(false)
                                .min_size(Vec2::new(ui.available_width(), 22.0)),
                        );
                        if header.clicked() {
                            self.collapsed_groups.insert(key.clone(), !collapsed);
                        }

                        if !collapsed || has_active {
                            ui.add_space(2.0);
                            for item in group.items {
                                let active = item.route == self.route;
                                let label = format!("{}  {}", item.icon, item.title);
                                let text = if item.placeholder {
                                    RichText::new(label).color(MUTED.linear_multiply(0.5)).italics().size(13.0)
                                } else if active {
                                    RichText::new(label).color(FG).size(13.0).strong()
                                } else {
                                    RichText::new(label).color(MUTED).size(13.0)
                                };
                                let btn = egui::Button::new(text)
                                    .min_size(Vec2::new(ui.available_width(), 30.0))
                                    .rounding(Rounding::same(6.0))
                                    .fill(if active { PRIMARY.linear_multiply(0.12) } else { Color32::TRANSPARENT })
                                    .stroke(if active { Stroke::new(1.0, PRIMARY.linear_multiply(0.4)) } else { Stroke::NONE });
                                let resp = ui.add_enabled(!item.placeholder, btn);
                                if resp.clicked() {
                                    self.route = item.route;
                                }
                                if item.placeholder {
                                    resp.on_hover_text("Coming soon");
                                }
                            }
                        }
                        ui.add_space(14.0);
                    }

                    ui.separator();
                    ui.add_space(6.0);
                    ui.label(RichText::new("Resources").size(11.0).color(MUTED));
                    if ui.add(egui::Button::new(RichText::new("📄  Docs").color(MUTED).size(13.0)).frame(false)).clicked() {
                        ctx.open_url(egui::OpenUrl::new_tab("https://docs.zeroclaw.dev"));
                    }
                });
            });
    }

    fn content(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(Frame::none().fill(BG).inner_margin(Margin::same(20.0)))
            .show(ctx, |ui| {
                // Breadcrumb
                ui.horizontal(|ui| {
                    ui.label(RichText::new("ZeroClaw").color(MUTED).size(11.0));
                    ui.label(RichText::new("/").color(MUTED).size(11.0));
                    ui.label(RichText::new(route_title(self.route)).color(FG).size(11.0).strong());
                });
                ui.add_space(10.0);

                egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
                    views::render(ui, self.route, &self.registry, &mut self.explorer, &self.invoke, ctx);
                });
            });
    }
}

impl eframe::App for ZeroClawApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.auth.token.is_none() {
            self.login_screen(ctx);
            return;
        }
        self.connected = true;

        // Pump chat frames: drain any incoming gRPC stream events into the store.
        if self.explorer.chat_store.drain_frames() {
            ctx.request_repaint();
        }

        // Pick up a receiver that a spawned send task placed in the slot.
        {
            let mut slot = self.chat_rx_slot.lock();
            if let Some(rx) = slot.take() {
                self.explorer.chat_store.frame_rx = Some(rx);
                self.explorer.chat_store.connected = true;
            }
        }

        // Handle pending send: if the user submitted a message, kick off the
        // gRPC stream via the transport.
        if let Some(payload) = self.explorer.chat_store.pending_send.take() {
            if let Some(transport) = &self.chat_transport {
                let transport = transport.clone();
                let slot = self.chat_rx_slot.clone();
                let ctx_clone = ctx.clone();
                tokio::spawn(async move {
                    match transport.send(payload).await {
                        Ok(rx) => {
                            *slot.lock() = Some(rx);
                            ctx_clone.request_repaint();
                        }
                        Err(e) => {
                            eprintln!("[chat] transport send error: {e:#}");
                            ctx_clone.request_repaint();
                        }
                    }
                });
            } else {
                // No transport connected — show error in store.
                self.explorer.chat_store.append_part(crate::chat::store::ChatMessage {
                    id: crate::chat::store::nanoid_pub(),
                    role: "system".into(),
                    kind: "text".into(),
                    payload: serde_json::json!({
                        "content": "No gRPC connection. Set ZEROCLAW_GRPC and restart."
                    }),
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0),
                });
                self.explorer.chat_store.submitted = false;
            }
        }

        self.topbar(ctx);
        self.sidebar(ctx);
        self.content(ctx);
    }
}

impl ZeroClawApp {
    fn login_screen(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(Frame::none().fill(BG).inner_margin(Margin::same(40.0)))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(120.0);

                    let (rect, _) = ui.allocate_exact_size(Vec2::new(48.0, 48.0), Sense::hover());
                    ui.painter().rect_filled(rect, Rounding::same(10.0), PRIMARY);
                    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "Z", FontId::proportional(24.0), Color32::WHITE);
                    ui.add_space(12.0);

                    ui.label(RichText::new("ZEROCLAW").size(20.0).strong().color(FG));
                    ui.label(RichText::new("OPERATOR CONSOLE").size(11.0).color(MUTED));
                    ui.add_space(24.0);

                    Frame::none().fill(SURFACE).stroke(Stroke::new(1.0, BORDER)).rounding(Rounding::same(8.0)).inner_margin(Margin::same(16.0)).show(ui, |ui| {
                        ui.set_max_width(360.0);

                        ui.label(RichText::new("Gateway").color(MUTED).size(11.0));
                        ui.add_space(4.0);
                        ui.add_sized([320.0, 24.0], TextEdit::singleline(&mut self.auth.gateway).text_color(FG));
                        ui.add_space(12.0);

                        ui.label(RichText::new("Pairing Code").color(MUTED).size(11.0));
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.add_sized([220.0, 28.0], TextEdit::singleline(&mut self.pairing_code_input).hint_text("Enter code...").text_color(FG));
                            ui.add_space(8.0);
                            if ui.add(egui::Button::new(RichText::new("Pair").color(Color32::WHITE).size(13.0))
                                .fill(PRIMARY).rounding(Rounding::same(6.0)).min_size(Vec2::new(80.0, 28.0))).clicked()
                                && !self.pairing_code_input.is_empty() {
                                match self.auth.pair(&self.pairing_code_input) {
                                    Ok(_) => { self.error_msg = None; }
                                    Err(e) => { self.error_msg = Some(format!("{e}")); }
                                }
                            }
                        });

                        if let Some(msg) = &self.error_msg {
                            ui.add_space(8.0);
                            ui.label(RichText::new(msg).color(DANGER).size(12.0));
                        }

                        ui.add_space(12.0);
                        ui.separator();
                        ui.add_space(8.0);
                        if ui.button("Generate new code").clicked() {
                            match self.auth.generate_code() {
                                Ok(code) => { self.pairing_code_input = code; self.error_msg = None; }
                                Err(e) => { self.error_msg = Some(format!("{e}")); }
                            }
                        }
                    });
                });
            });
    }
}

fn tab_pill(ui: &mut egui::Ui, label: &str, active: bool, mut on_click: impl FnMut()) {
    let text = if active {
        RichText::new(label).color(Color32::WHITE).size(12.0).strong()
    } else {
        RichText::new(label).color(MUTED).size(12.0)
    };
    let btn = egui::Button::new(text)
        .rounding(Rounding::same(999.0))
        .fill(if active { PRIMARY } else { SURFACE })
        .stroke(Stroke::new(1.0, if active { PRIMARY } else { BORDER }))
        .min_size(Vec2::new(78.0, 26.0));
    if ui.add(btn).clicked() { on_click(); }
}

fn health_pill(ui: &mut egui::Ui, ok: bool) {
    let (bg, fg, text) = if ok {
        (OK.linear_multiply(0.18), OK, "Health  OK")
    } else {
        (DANGER.linear_multiply(0.18), DANGER, "Health  Offline")
    };
    let btn = egui::Button::new(RichText::new(text).color(fg).size(11.0).monospace())
        .rounding(Rounding::same(999.0))
        .fill(bg)
        .stroke(Stroke::new(1.0, fg.linear_multiply(0.6)))
        .sense(Sense::hover());
    ui.add(btn);
}

fn theme_toggle(ui: &mut egui::Ui, mode: &mut ThemeMode) {
    Frame::none()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .rounding(Rounding::same(999.0))
        .inner_margin(Margin::same(2.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for (m, glyph) in [(ThemeMode::Dark, "☾"), (ThemeMode::Light, "☀"), (ThemeMode::System, "▦")] {
                    let active = *mode == m;
                    let text = RichText::new(glyph).size(11.0)
                        .color(if active { Color32::WHITE } else { MUTED });
                    let btn = egui::Button::new(text)
                        .rounding(Rounding::same(999.0))
                        .fill(if active { PRIMARY } else { Color32::TRANSPARENT })
                        .stroke(Stroke::NONE)
                        .min_size(Vec2::new(22.0, 22.0));
                    if ui.add(btn).clicked() { *mode = m; }
                }
            });
        });
}
