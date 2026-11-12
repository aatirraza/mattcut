//! MattCut's look: a near-black editing room, floating rounded panels, the system UI font and
//! the Mattrics orange as the accent.

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle, Visuals};

pub const BG: Color32 = Color32::from_rgb(0x0b, 0x0b, 0x0d);
pub const PANEL: Color32 = Color32::from_rgb(0x17, 0x17, 0x1a);
pub const PANEL_2: Color32 = Color32::from_rgb(0x1f, 0x1f, 0x23);
pub const PANEL_3: Color32 = Color32::from_rgb(0x28, 0x28, 0x2d);
pub const HOVER: Color32 = Color32::from_rgb(0x30, 0x30, 0x36);
pub const BORDER: Color32 = Color32::from_rgb(0x2a, 0x2a, 0x30);
pub const TEXT: Color32 = Color32::from_rgb(0xee, 0xee, 0xf0);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x98, 0x98, 0xa2);
pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x62, 0x62, 0x6c);
/// Mattrics orange (the logo).
pub const ACCENT: Color32 = Color32::from_rgb(0xe4, 0x58, 0x3a);
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0xf2, 0x6b, 0x4c);
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(0x3d, 0x22, 0x1c);
pub const OK: Color32 = Color32::from_rgb(0x3c, 0xd0, 0x8f);
pub const WARN: Color32 = Color32::from_rgb(0xf5, 0xb7, 0x2b);
pub const ERR: Color32 = Color32::from_rgb(0xf2, 0x5f, 0x5c);

pub const CLIP_VIDEO: Color32 = Color32::from_rgb(0x1d, 0x4f, 0x5c);
pub const CLIP_MAIN: Color32 = Color32::from_rgb(0x22, 0x46, 0x66);
pub const CLIP_AUDIO: Color32 = Color32::from_rgb(0x1b, 0x4a, 0x38);
pub const CLIP_TEXT: Color32 = Color32::from_rgb(0x7a, 0x4a, 0x16);
pub const CLIP_STILL: Color32 = Color32::from_rgb(0x55, 0x37, 0x78);
pub const PLAYHEAD: Color32 = Color32::from_rgb(0xff, 0xff, 0xff);

pub const R_PANEL: u8 = 10;
pub const R_CTRL: u8 = 6;

static LOGO_PNG: &[u8] = include_bytes!("../../assets/logo.png");

/// The Mattrics logo, decoded (RGBA8, width, height).
pub fn logo_rgba() -> Option<(Vec<u8>, u32, u32)> {
    let img = image::load_from_memory_with_format(LOGO_PNG, image::ImageFormat::Png).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    Some((img.into_raw(), w, h))
}

/// The system UI font: SF on macOS, Segoe UI on Windows (read from the OS at run time, not
/// shipped). Falls back to egui's bundled font.
fn system_font() -> Option<Vec<u8>> {
    let candidates: &[&str] = if cfg!(target_os = "macos") {
        &["/System/Library/Fonts/SFNS.ttf", "/System/Library/Fonts/HelveticaNeue.ttc"]
    } else if cfg!(windows) {
        &["C:\\Windows\\Fonts\\segoeui.ttf", "C:\\Windows\\Fonts\\arial.ttf"]
    } else {
        &["/usr/share/fonts/truetype/inter/Inter-Regular.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"]
    };
    candidates.iter().find_map(|p| std::fs::read(p).ok())
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    if let Some(data) = system_font() {
        fonts.font_data.insert("system-ui".into(), std::sync::Arc::new(FontData::from_owned(data)));
        if let Some(fam) = fonts.families.get_mut(&FontFamily::Proportional) {
            fam.insert(0, "system-ui".into());
        }
    }
    // symbols that only the monospace face has (arrows, media keys)
    if let Some(fam) = fonts.families.get_mut(&FontFamily::Proportional) {
        fam.push("Hack".into());
    }
    ctx.set_fonts(fonts);
}

pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);
    let mut v = Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = PANEL_2;
    v.extreme_bg_color = Color32::from_rgb(0x12, 0x12, 0x15);
    v.faint_bg_color = PANEL_2;
    v.code_bg_color = PANEL_3;
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.selection.bg_fill = ACCENT;
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    v.hyperlink_color = ACCENT;
    v.widgets.noninteractive.bg_fill = PANEL;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.inactive.bg_fill = PANEL_3;
    v.widgets.inactive.weak_bg_fill = PANEL_3;
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.hovered.bg_fill = HOVER;
    v.widgets.hovered.weak_bg_fill = HOVER;
    v.widgets.hovered.bg_stroke = Stroke::NONE;
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    v.widgets.active.bg_fill = ACCENT;
    v.widgets.active.weak_bg_fill = ACCENT_SOFT;
    v.widgets.active.bg_stroke = Stroke::NONE;
    v.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    v.widgets.open = v.widgets.hovered;
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = CornerRadius::same(R_CTRL);
        w.expansion = 0.0;
    }
    v.window_corner_radius = CornerRadius::same(12);
    v.menu_corner_radius = CornerRadius::same(8);
    v.slider_trailing_fill = true;
    ctx.set_theme(egui::Theme::Dark);
    ctx.set_visuals_of(egui::Theme::Dark, v.clone());
    ctx.set_visuals_of(egui::Theme::Light, v);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 5.0);
        s.spacing.interact_size.y = 26.0;
        s.spacing.slider_rail_height = 3.0;
        s.spacing.slider_width = 120.0;
        s.spacing.menu_margin = egui::Margin::same(6);
        s.spacing.window_margin = egui::Margin::same(20);
        s.text_styles.insert(TextStyle::Body, FontId::proportional(13.0));
        s.text_styles.insert(TextStyle::Button, FontId::proportional(13.0));
        s.text_styles.insert(TextStyle::Small, FontId::proportional(11.0));
        s.text_styles.insert(TextStyle::Heading, FontId::proportional(19.0));
        s.text_styles.insert(TextStyle::Monospace, FontId::monospace(12.5));
    });
}

/// The orange call-to-action button.
pub fn primary(text: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text.to_string()).color(Color32::WHITE).strong()).fill(ACCENT).corner_radius(CornerRadius::same(R_CTRL))
}

/// A quiet button: no fill until hovered.
pub fn ghost(text: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text.to_string()).color(TEXT)).fill(Color32::TRANSPARENT).corner_radius(CornerRadius::same(R_CTRL))
}

pub fn card() -> egui::Frame {
    egui::Frame::new().fill(PANEL).stroke(Stroke::new(1.0, BORDER)).corner_radius(CornerRadius::same(14)).inner_margin(egui::Margin::same(24))
}

/// A floating editor panel.
pub fn panel_frame() -> egui::Frame {
    egui::Frame::new().fill(PANEL).corner_radius(CornerRadius::same(R_PANEL))
}

/// Small grey section caption.
pub fn caption(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(11.5).color(TEXT_DIM));
}

/// A small filled circle (connection status).
pub fn status_dot(ui: &mut egui::Ui, color: Color32) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
    ui.painter().circle_filled(r.center(), 4.0, color);
    resp
}

/// Underlined text tabs. Returns the clicked index.
pub fn tabs(ui: &mut egui::Ui, labels: &[&str], selected: usize) -> Option<usize> {
    let mut clicked = None;
    let left = ui.max_rect().min.x;
    let right = ui.max_rect().max.x;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 18.0;
        for (i, l) in labels.iter().enumerate() {
            let sel = i == selected;
            let galley = ui.painter().layout_no_wrap((*l).to_string(), FontId::proportional(13.0), TEXT);
            let (r, resp) = ui.allocate_exact_size(galley.size() + egui::vec2(0.0, 10.0), egui::Sense::click());
            let col = if sel || resp.hovered() { TEXT } else { TEXT_DIM };
            ui.painter().galley_with_override_text_color(r.left_top(), galley, col);
            if sel {
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(egui::pos2(r.min.x, r.max.y - 2.0), egui::vec2(r.width(), 2.0)),
                    CornerRadius::same(1),
                    ACCENT,
                );
            }
            if resp.clicked() {
                clicked = Some(i);
            }
        }
    });
    let y = ui.cursor().min.y;
    ui.painter().line_segment([egui::pos2(left, y), egui::pos2(right, y)], Stroke::new(1.0, BORDER));
    ui.add_space(10.0);
    clicked
}
