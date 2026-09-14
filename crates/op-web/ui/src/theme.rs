//! Visual tokens — graphite/slate dark with red accent, parity with web UI.
use egui::{Color32, Rounding, Stroke, Style, Visuals};

pub const BG: Color32 = Color32::from_rgb(0x0B, 0x0D, 0x10);          // background
pub const SURFACE: Color32 = Color32::from_rgb(0x11, 0x14, 0x18);     // panels
pub const SURFACE_2: Color32 = Color32::from_rgb(0x17, 0x1B, 0x21);   // raised
pub const BORDER: Color32 = Color32::from_rgb(0x24, 0x29, 0x31);
pub const FG: Color32 = Color32::from_rgb(0xE6, 0xE8, 0xEC);
pub const MUTED: Color32 = Color32::from_rgb(0x8A, 0x93, 0x9E);
pub const PRIMARY: Color32 = Color32::from_rgb(0xE5, 0x48, 0x4D);     // red accent
pub const OK: Color32 = Color32::from_rgb(0x30, 0xA4, 0x6C);
pub const WARN: Color32 = Color32::from_rgb(0xE5, 0xB1, 0x4D);
pub const DANGER: Color32 = PRIMARY;

pub fn install(ctx: &egui::Context) {
    let mut style: Style = (*ctx.style()).clone();
    let mut v = Visuals::dark();

    v.override_text_color = Some(FG);
    v.panel_fill = BG;
    v.window_fill = SURFACE;
    v.extreme_bg_color = BG;
    v.faint_bg_color = SURFACE;
    v.code_bg_color = SURFACE_2;
    v.widgets.noninteractive.bg_fill = SURFACE;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, MUTED);
    v.widgets.inactive.bg_fill = SURFACE;
    v.widgets.inactive.weak_bg_fill = SURFACE;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, MUTED);
    v.widgets.inactive.rounding = Rounding::same(6.0);
    v.widgets.hovered.bg_fill = SURFACE_2;
    v.widgets.hovered.weak_bg_fill = SURFACE_2;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, PRIMARY);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, FG);
    v.widgets.hovered.rounding = Rounding::same(6.0);
    v.widgets.active.bg_fill = PRIMARY.linear_multiply(0.18);
    v.widgets.active.weak_bg_fill = PRIMARY.linear_multiply(0.12);
    v.widgets.active.bg_stroke = Stroke::new(1.0, PRIMARY);
    v.widgets.active.fg_stroke = Stroke::new(1.0, FG);
    v.widgets.active.rounding = Rounding::same(6.0);
    v.selection.bg_fill = PRIMARY.linear_multiply(0.25);
    v.selection.stroke = Stroke::new(1.0, PRIMARY);
    v.window_stroke = Stroke::new(1.0, BORDER);

    style.visuals = v;
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    ctx.set_style(style);
}
