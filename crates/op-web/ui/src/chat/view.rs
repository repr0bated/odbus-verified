//! egui rendering for the chat panel.
//!
//! Layout:
//! ```text
//! ┌──────────────────────────────┐
//! │  transcript (stick-to-bottom) │
//! │  ┌────────────────────────┐  │
//! │  │ assistant — page surface │  │
//! │  └────────────────────────┘  │
//! │           ┌──────────────┐   │
//! │           │ user — bubble │   │
//! │           └──────────────┘   │
//! │  ▸ tool card (collapsed)     │
//! └──────────────────────────────┘
//! ┌──────────────────────────────┐
//! │  composer (persistent focus)  │
//! │  [textarea]  [Submit]         │
//! │  Thinking...                  │
//! └──────────────────────────────┘
//! ```
//!
//! Identity glyph: a domain-specific operator console mark (the ZeroClaw "Z"
//! glyph), NOT Sparkles, NOT tied to any model.

use super::store::{ChatMessage, ChatStore};
use crate::theme::*;
use egui::{Align, Color32, FontId, Frame, Layout, Margin, RichText, Rounding, Sense, Stroke, Vec2};

/// Main entry point. Renders the full chat panel: transcript + composer.
pub fn render_chat(ui: &mut egui::Ui, store: &mut ChatStore, ctx: &egui::Context) {
    // Throttle repaint to ~60fps during streaming.
    if store.submitted && store.should_repaint() {
        ctx.request_repaint();
    }

    // Layout: transcript fills available space, composer pinned to bottom.
    ui.vertical(|ui| {
        // Transcript takes all available height.
        let available = ui.available_height();
        let composer_h = 88.0_f32;
        let transcript_h = (available - composer_h - 8.0).max(120.0);

        ui.allocate_ui(egui::Vec2::new(ui.available_width(), transcript_h), |ui| {
            render_transcript(ui, store);
        });

        ui.add_space(4.0);

        // Composer pinned to bottom.
        ui.allocate_ui(egui::Vec2::new(ui.available_width(), composer_h), |ui| {
            render_composer(ui, store);
        });
    });
}

/// Sticky-to-bottom scroll area with all messages.
fn render_transcript(ui: &mut egui::Ui, store: &ChatStore) {
    egui::ScrollArea::vertical()
        .id_source("chat-transcript")
        .stick_to_bottom(true)
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            if store.messages.is_empty() {
                render_empty_state(ui);
                return;
            }
            for msg in &store.messages {
                render_message(ui, msg);
            }
        });
}

/// Empty state — identity glyph + hint text.
fn render_empty_state(ui: &mut egui::Ui) {
    ui.add_space(60.0);
    ui.vertical_centered(|ui| {
        // Identity glyph: ZeroClaw "Z", NOT Sparkles.
        let (rect, _) = ui.allocate_exact_size(Vec2::new(40.0, 40.0), Sense::hover());
        ui.painter().rect_filled(rect, Rounding::same(8.0), PRIMARY.linear_multiply(0.15));
        ui.painter().rect_stroke(rect, Rounding::same(8.0), Stroke::new(1.0, PRIMARY.linear_multiply(0.5)));
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Z",
            FontId::proportional(20.0),
            PRIMARY,
        );
        ui.add_space(12.0);
        ui.label(RichText::new("ZeroClaw Chat").size(15.0).strong().color(FG));
        ui.add_space(4.0);
        ui.label(
            RichText::new("The chatbot plans, suggests, and delegates — it does not execute.")
                .color(MUTED)
                .size(12.0),
        );
        ui.add_space(2.0);
        ui.label(
            RichText::new("Ask it to orchestrate a task, analyze state, or coordinate agents.")
                .color(MUTED)
                .size(12.0),
        );
    });
}

/// Render a single message, role-styled.
///
/// - assistant: on page surface (no bubble), full width
/// - user: high-contrast bubble, right-aligned
/// - tool: collapsed card with domain icon
/// - system: muted, centered, italic
fn render_message(ui: &mut egui::Ui, msg: &ChatMessage) {
    match msg.role.as_str() {
        "user" => render_user_message(ui, msg),
        "assistant" => render_assistant_message(ui, msg),
        "tool" => render_tool_card(ui, msg),
        "system" => render_system_message(ui, msg),
        _ => render_unknown(ui, msg),
    }
}

/// Assistant message: on page surface (no bubble), full width.
fn render_assistant_message(ui: &mut egui::Ui, msg: &ChatMessage) {
    ui.horizontal(|ui| {
        // Identity glyph
        let (rect, _) = ui.allocate_exact_size(Vec2::new(20.0, 20.0), Sense::hover());
        ui.painter().rect_filled(rect, Rounding::same(4.0), PRIMARY.linear_multiply(0.15));
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Z",
            FontId::proportional(11.0),
            PRIMARY,
        );
        ui.add_space(6.0);

        ui.vertical(|ui| {
            match msg.kind.as_str() {
                "reasoning" => render_reasoning(ui, msg),
                "tool-call" => render_tool_card(ui, msg),
                _ => render_text_content(ui, msg, FG),
            }
        });
    });
    ui.add_space(6.0);
}

/// User message: high-contrast bubble, right-aligned.
fn render_user_message(ui: &mut egui::Ui, msg: &ChatMessage) {
    ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
        let max_w = (ui.available_width() * 0.75).min(600.0);
        Frame::none()
            .fill(PRIMARY.linear_multiply(0.12))
            .stroke(Stroke::new(1.0, PRIMARY.linear_multiply(0.3)))
            .rounding(Rounding::same(10.0))
            .inner_margin(Margin::same(12.0))
            .show(ui, |ui| {
                ui.set_max_width(max_w);
                render_text_content(ui, msg, FG);
            });
    });
    ui.add_space(6.0);
}

/// System message: muted, centered, italic.
fn render_system_message(ui: &mut egui::Ui, msg: &ChatMessage) {
    ui.horizontal_centered(|ui| {
        ui.add_space(20.0);
        let content = msg
            .payload
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        ui.label(RichText::new(content).color(MUTED).size(11.0).italics());
    });
    ui.add_space(4.0);
}

/// Text content — monospace label for now (egui has no markdown built-in).
fn render_text_content(ui: &mut egui::Ui, msg: &ChatMessage, color: Color32) {
    let content = msg
        .payload
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if content.is_empty() {
        ui.label(RichText::new("(empty)").color(MUTED).size(12.0).italics());
    } else {
        // Monospace for now; markdown renderer will replace this.
        ui.label(RichText::new(content).color(color).size(13.0).monospace());
    }
}

/// Reasoning block: muted, slightly indented, collapsible.
fn render_reasoning(ui: &mut egui::Ui, msg: &ChatMessage) {
    let content = msg
        .payload
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    ui.collapsing(
        RichText::new("▸ reasoning").color(MUTED).size(11.0).italics(),
        |ui| {
            ui.label(RichText::new(content).color(MUTED).size(12.0).monospace());
            // Plan items if present
            if let Some(plan) = msg.payload.get("plan").and_then(|v| v.as_array()) {
                ui.add_space(4.0);
                for (i, step) in plan.iter().enumerate() {
                    let text = step.as_str().unwrap_or("");
                    ui.label(
                        RichText::new(format!("  {}. {}", i + 1, text))
                            .color(MUTED)
                            .size(11.0)
                            .monospace(),
                    );
                }
            }
        },
    );
}

/// Tool card: collapsed by default, domain icon, status indicator.
///
/// Renders for both `"tool"` role (tool-result) and assistant `"tool-call"`
/// kind. When `role == "tool"`, shows output; when `kind == "tool-call"`,
/// shows input.
fn render_tool_card(ui: &mut egui::Ui, msg: &ChatMessage) {
    let is_result = msg.role == "tool" || msg.kind == "tool-result";
    let tool_name = msg
        .payload
        .get("tool")
        .and_then(|v| v.as_str())
        .unwrap_or("tool");
    let status = if is_result {
        msg.payload
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("ok")
    } else {
        "pending"
    };

    let (icon_color, status_glyph) = match status {
        "ok" => (OK, "✓"),
        "error" => (DANGER, "✗"),
        "pending" => (WARN, "…"),
        _ => (MUTED, "•"),
    };

    Frame::none()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .rounding(Rounding::same(6.0))
        .inner_margin(Margin::same(10.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                // Domain icon (not Sparkles — a tool glyph)
                let (rect, _) = ui.allocate_exact_size(Vec2::new(18.0, 18.0), Sense::hover());
                ui.painter()
                    .rect_filled(rect, Rounding::same(4.0), icon_color.linear_multiply(0.15));
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "⚙",
                    FontId::proportional(10.0),
                    icon_color,
                );

                ui.add_space(6.0);
                ui.label(
                    RichText::new(tool_name)
                        .color(FG)
                        .size(12.0)
                        .strong()
                        .monospace(),
                );

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(status_glyph)
                            .color(icon_color)
                            .size(12.0)
                            .monospace(),
                    );
                });
            });

            // Collapsible detail
            ui.add_space(2.0);
            ui.collapsing(
                RichText::new(if is_result { "output" } else { "input" })
                    .color(MUTED)
                    .size(10.0),
                |ui| {
                    if is_result {
                        let output = msg.payload.get("output").cloned().unwrap_or(serde_json::Value::Null);
                        let pretty = serde_json::to_string_pretty(&output).unwrap_or_default();
                        ui.label(RichText::new(pretty).color(MUTED).size(11.0).monospace());
                    } else {
                        let input = msg.payload.get("input").cloned().unwrap_or(serde_json::Value::Null);
                        let pretty = serde_json::to_string_pretty(&input).unwrap_or_default();
                        ui.label(RichText::new(pretty).color(MUTED).size(11.0).monospace());
                    }
                },
            );
        });
    ui.add_space(4.0);
}

/// Fallback for unknown roles.
fn render_unknown(ui: &mut egui::Ui, msg: &ChatMessage) {
    ui.label(
        RichText::new(format!("[unknown role: {}] {}", msg.role, msg.kind))
            .color(WARN)
            .size(11.0)
            .monospace(),
    );
}

/// Composer: persistent focus, "Thinking…" indicator when submitted,
/// status-aware submit.
fn render_composer(ui: &mut egui::Ui, store: &mut ChatStore) {
    // We need a persistent text buffer. Since ChatStore doesn't hold one
    // (it's append-only messages), we use a static via egui's data store.
    // This is the egui-idiomatic way to persist widget state without
    // adding fields to the app struct.
    let draft_id = egui::Id::new("chat-composer-draft");
    let draft = ui.data_mut(|data| {
        data.get_temp_mut_or_insert_with::<String>(draft_id, || String::new())
            .clone()
    });
    let mut draft = draft;

    Frame::none()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .rounding(Rounding::same(8.0))
        .inner_margin(Margin::same(12.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                // Text area — takes available width
                let resp = ui.add_sized(
                    [ui.available_width() - 100.0, 48.0],
                    egui::TextEdit::multiline(&mut draft)
                        .desired_width(f32::INFINITY)
                        .hint_text("›  message the operator…")
                        .frame(false),
                );

                // Persist the draft
                ui.data_mut(|data| {
                    data.insert_temp(draft_id, draft.clone());
                });

                // Submit button (status-aware)
                let can_submit = !store.submitted && !draft.trim().is_empty();
                let btn_label = if store.submitted {
                    "Cancel"
                } else {
                    "Send"
                };
                let btn = egui::Button::new(
                    RichText::new(btn_label)
                        .color(Color32::WHITE)
                        .size(13.0)
                        .strong(),
                )
                .fill(if can_submit || store.submitted {
                    PRIMARY
                } else {
                    PRIMARY.linear_multiply(0.4)
                })
                .stroke(Stroke::new(1.0, PRIMARY))
                .rounding(Rounding::same(6.0))
                .min_size(Vec2::new(80.0, 48.0));

                if ui.add_enabled(can_submit || store.submitted, btn).clicked() {
                    if store.submitted {
                        // Cancel
                        store.submitted = false;
                    } else if !draft.trim().is_empty() {
                        store.append_user_text(draft.trim());
                        store.submitted = true;
                        draft.clear();
                        ui.data_mut(|data| {
                            data.insert_temp(draft_id, String::new());
                        });
                    }
                }

                // Enter to submit (Shift+Enter for newline)
                if resp.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    && !store.submitted
                    && !draft.trim().is_empty()
                {
                    store.append_user_text(draft.trim());
                    store.submitted = true;
                    draft.clear();
                    ui.data_mut(|data| {
                        data.insert_temp(draft_id, String::new());
                    });
                    resp.request_focus();
                }
            });

            // Thinking indicator
            if store.submitted {
                ui.add_space(2.0);
                render_thinking(ui);
            }
        });
}

/// Shimmer / thinking indicator. Uses a pulsing dot animation driven by
/// the current time.
fn render_thinking(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        let t = ui.input(|i| i.time);
        // Three dots with phase-shifted opacity for a shimmer effect.
        for i in 0..3 {
            let phase = (t * 2.0 + i as f64 * 0.4) % 2.0;
            let alpha = ((phase.sin() + 1.0) * 0.5) as f32;
            let color = PRIMARY.gamma_multiply(alpha * 0.6 + 0.2);
            let (rect, _) = ui.allocate_exact_size(Vec2::new(6.0, 6.0), Sense::hover());
            ui.painter().circle_filled(rect.center(), 3.0, color);
            ui.add_space(2.0);
        }
        ui.add_space(4.0);
        ui.label(
            RichText::new("Thinking…")
                .color(MUTED)
                .size(11.0)
                .italics(),
        );
    });
}
