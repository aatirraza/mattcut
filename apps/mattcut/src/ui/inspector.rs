//! The inspector (right panel): the selected clip's video, audio, speed, colour and text settings.

use std::collections::HashMap;

use egui::{RichText, vec2};
use serde_json::{Value, json};

use super::{MattCut, theme};
use crate::model::{ClipView, Kind, short_duration};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tab {
    #[default]
    Video,
    Audio,
    Speed,
    Adjust,
    Text,
}

#[derive(Default)]
pub struct InspectorState {
    pub tab: Tab,
    /// Values being dragged, applied when the drag ends (one undo step per change).
    drafts: HashMap<String, f64>,
    text: Option<(u64, String)>,
}

/// Lumetri basic and creative controls: (param, label, min, max, default).
const ADJUST: &[(&str, &str, f64, f64, f64)] = &[
    ("exposure", "Exposure", -5.0, 5.0, 0.0),
    ("contrast", "Contrast", -100.0, 100.0, 0.0),
    ("highlights", "Highlights", -100.0, 100.0, 0.0),
    ("shadows", "Shadows", -100.0, 100.0, 0.0),
    ("whites", "Whites", -100.0, 100.0, 0.0),
    ("blacks", "Blacks", -100.0, 100.0, 0.0),
    ("saturation", "Saturation", 0.0, 200.0, 100.0),
    ("vibrance", "Vibrance", -100.0, 100.0, 0.0),
    ("temperature", "Temperature", -100.0, 100.0, 0.0),
    ("tint", "Tint", -100.0, 100.0, 0.0),
    ("sharpen", "Sharpen", 0.0, 100.0, 0.0),
    ("faded_film", "Fade", 0.0, 100.0, 0.0),
];

pub fn show(app: &mut MattCut, ui: &mut egui::Ui) {
    let Some(view) = app.view.clone() else { return };
    let sel = view.selection.clone();
    egui::Frame::new().inner_margin(egui::Margin::same(14)).show(ui, |ui| {
        let Some((track, clip)) = sel.first().and_then(|id| view.clip(*id)).map(|(t, c)| (t.clone(), c.clone())) else {
            project_details(app, ui);
            return;
        };
        let media_kind = app.media.iter().find(|m| m.id == clip.item).map(|m| m.kind.clone()).unwrap_or_default();
        let is_text = media_kind == "Graphic";
        let is_audio = track.kind == Kind::Audio;
        ui.label(RichText::new(&clip.name).size(15.0).color(theme::TEXT));
        ui.label(
            RichText::new(format!(
                "{} · {}",
                short_duration(clip.duration()),
                if is_text {
                    "Text"
                } else if is_audio {
                    "Audio"
                } else {
                    media_kind.as_str()
                }
            ))
            .small()
            .color(theme::TEXT_DIM),
        );
        if sel.len() > 1 {
            ui.label(RichText::new(format!("{} clips selected — settings apply to this one", sel.len())).small().color(theme::WARN));
        }
        ui.add_space(6.0);
        let tabs: Vec<(Tab, &str)> = if is_audio {
            vec![(Tab::Audio, "Audio"), (Tab::Speed, "Speed")]
        } else if is_text {
            vec![(Tab::Text, "Text"), (Tab::Video, "Video")]
        } else {
            vec![(Tab::Video, "Video"), (Tab::Audio, "Audio"), (Tab::Speed, "Speed"), (Tab::Adjust, "Adjust")]
        };
        if !tabs.iter().any(|(t, _)| *t == app.insp.tab)
            && let Some((t, _)) = tabs.first()
        {
            app.insp.tab = *t;
        }
        let labels: Vec<&str> = tabs.iter().map(|(_, l)| *l).collect();
        let sel = tabs.iter().position(|(t, _)| *t == app.insp.tab).unwrap_or(0);
        if let Some(i) = theme::tabs(ui, &labels, sel)
            && let Some((t, _)) = tabs.get(i)
        {
            app.insp.tab = *t;
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match app.insp.tab {
            Tab::Video => video_tab(app, ui, &clip),
            Tab::Audio => audio_tab(app, ui, &clip, is_audio),
            Tab::Speed => speed_tab(app, ui, &clip),
            Tab::Adjust => adjust_tab(app, ui, &clip),
            Tab::Text => text_tab(app, ui, &clip),
        });
    });
}

fn project_details(app: &mut MattCut, ui: &mut egui::Ui) {
    let Some(view) = app.view.clone() else { return };
    ui.label(RichText::new("Project").size(15.0).color(theme::TEXT));
    ui.add_space(6.0);
    egui::Grid::new("proj").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
        let row = |ui: &mut egui::Ui, k: &str, v: String| {
            ui.label(RichText::new(k).color(theme::TEXT_DIM));
            ui.label(v);
            ui.end_row();
        };
        if let Some(d) = &app.doc {
            row(ui, "Name", d.name.clone());
            row(ui, "Saved", if d.office { "Team server".into() } else { "This computer".into() });
        }
        row(ui, "Size", format!("{} × {}", view.width, view.height));
        row(ui, "Frame rate", format!("{:.3} fps", view.fps).replace(".000", ""));
        row(ui, "Length", short_duration(view.duration));
        let clips: usize = view.video.iter().chain(&view.audio).map(|t| t.clips.len()).sum();
        row(ui, "Clips", clips.to_string());
    });
    ui.add_space(14.0);
    ui.label(RichText::new("Select a clip on the timeline to edit its size, position, speed, volume and colour.").small().color(theme::TEXT_DIM));
}

/// A slider-like number that applies once, when the drag or typing ends.
fn number(
    app: &mut MattCut,
    ui: &mut egui::Ui,
    key: String,
    label: &str,
    current: f64,
    range: std::ops::RangeInclusive<f64>,
    speed: f64,
    suffix: &str,
) -> Option<f64> {
    let mut v = app.insp.drafts.get(&key).copied().unwrap_or(current);
    let mut out = None;
    ui.horizontal(|ui| {
        let (lr, _) = ui.allocate_exact_size(vec2(84.0, 22.0), egui::Sense::hover());
        ui.painter().text(lr.left_center(), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(12.5), theme::TEXT_DIM);
        let r = ui.add(egui::Slider::new(&mut v, range).suffix(suffix).drag_value_speed(speed).clamping(egui::SliderClamping::Never));
        if r.dragged() || r.has_focus() {
            app.insp.drafts.insert(key.clone(), v);
        }
        if (r.drag_stopped() || r.lost_focus() || (r.changed() && !r.dragged() && !r.has_focus())) && (v - current).abs() > 1e-9 {
            out = Some(v);
        }
        if !r.dragged() && !r.has_focus() {
            app.insp.drafts.remove(&key);
        }
    });
    out
}

fn set_param(app: &mut MattCut, clip: u64, effect: &str, param: &str, value: Value) {
    app.run("effects.setParam", json!({"clip": clip, "effect": effect, "param": param, "value": value}));
}

fn video_tab(app: &mut MattCut, ui: &mut egui::Ui, clip: &ClipView) {
    let id = clip.id;
    section(ui, "Transform");
    let scale = clip.float_param("motion", "scale").unwrap_or(100.0);
    if let Some(v) = number(app, ui, format!("{id}:scale"), "Scale", scale, 1.0..=400.0, 0.5, "%") {
        set_param(app, id, "motion", "scale", json!(v));
    }
    let (w, h) = app.view.as_ref().map_or((1920.0, 1080.0), |v| (f64::from(v.width), f64::from(v.height)));
    let pos = clip.vec2_param("motion", "position").unwrap_or([w / 2.0, h / 2.0]);
    if let Some(x) = number(app, ui, format!("{id}:px"), "Position X", pos[0] - w / 2.0, -w..=w, 1.0, "") {
        set_param(app, id, "motion", "position", json!([x + w / 2.0, pos[1]]));
    }
    if let Some(y) = number(app, ui, format!("{id}:py"), "Position Y", pos[1] - h / 2.0, -h..=h, 1.0, "") {
        set_param(app, id, "motion", "position", json!([pos[0], y + h / 2.0]));
    }
    let rot = clip.float_param("motion", "rotation").unwrap_or(0.0);
    if let Some(v) = number(app, ui, format!("{id}:rot"), "Rotation", rot, -360.0..=360.0, 0.5, "°") {
        set_param(app, id, "motion", "rotation", json!(v));
    }
    ui.horizontal(|ui| {
        if ui.button("Fit to frame").clicked() {
            app.run("timeline.select", json!({"clips": [id]}));
            app.run("clip.scaleToFrameSize", json!({}));
        }
        if ui.button("Reset").clicked() {
            for p in ["scale", "position", "rotation"] {
                app.run("effects.resetParam", json!({"clip": id, "effect": "motion", "param": p}));
            }
        }
    });
    ui.add_space(8.0);
    section(ui, "Blend");
    let op = clip.float_param("opacity", "opacity").unwrap_or(100.0);
    if let Some(v) = number(app, ui, format!("{id}:op"), "Opacity", op, 0.0..=100.0, 0.5, "%") {
        set_param(app, id, "opacity", "opacity", json!(v));
    }
    ui.add_space(8.0);
    effects_list(app, ui, clip);
}

fn effects_list(app: &mut MattCut, ui: &mut egui::Ui, clip: &ClipView) {
    let fx: Vec<(usize, String, bool)> = clip
        .effects
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let name = e.get("effect")?.as_str()?;
            (!matches!(name, "motion" | "opacity" | "time_remap" | "volume" | "channel_volume" | "panner" | "graphic_text" | "graphic"))
                .then(|| (i, name.to_string(), e.get("enabled").and_then(Value::as_bool).unwrap_or(true)))
        })
        .collect();
    if fx.is_empty() {
        return;
    }
    section(ui, "Effects on this clip");
    for (i, name, enabled) in fx {
        ui.horizontal(|ui| {
            let mut on = enabled;
            if ui.checkbox(&mut on, name.replace('_', " ")).changed() {
                app.run("effects.toggleEnabled", json!({"clip": clip.id, "index": i}));
            }
            if super::icons::button_sized(ui, super::icons::Icon::Close, "Remove", false, 22.0).clicked() {
                app.run("effects.remove", json!({"clip": clip.id, "index": i}));
            }
        });
    }
}

fn audio_tab(app: &mut MattCut, ui: &mut egui::Ui, clip: &ClipView, is_audio: bool) {
    // a video clip's sound lives on its linked audio clip
    let audio_clip = if is_audio { Some(clip.clone()) } else { clip.link.and_then(|l| app.view.as_ref().and_then(|v| v.clip(l)).map(|(_, c)| c.clone())) };
    let Some(ac) = audio_clip else {
        ui.label(RichText::new("This clip has no sound.").color(theme::TEXT_DIM));
        return;
    };
    let id = ac.id;
    section(ui, "Volume");
    let level = ac.float_param("volume", "level").unwrap_or(0.0);
    if let Some(v) = number(app, ui, format!("{id}:vol"), "Volume", level, -60.0..=15.0, 0.2, " dB") {
        set_param(app, id, "volume", "level", json!(v));
    }
    ui.horizontal(|ui| {
        if ui.button("Mute").clicked() {
            set_param(app, id, "volume", "level", json!(-96.0));
        }
        if ui.button("Reset").clicked() {
            set_param(app, id, "volume", "level", json!(0.0));
        }
    });
    ui.add_space(8.0);
    section(ui, "Fades");
    let fps = app.view.as_ref().map_or(30.0, |v| v.fps);
    ui.horizontal(|ui| {
        for (label, edge) in [("Fade in", "in"), ("Fade out", "out")] {
            if ui.button(label).clicked() {
                app.run("sequence.applyAudioTransition", json!({"clip": id, "effect": "constant_power", "edge": edge, "frames": fps.round() as i64}));
            }
        }
    });
    ui.add_space(8.0);
    section(ui, "Voice & clean-up");
    ui.horizontal_wrapped(|ui| {
        for (label, effect) in [
            ("Reduce noise", "denoise"),
            ("Enhance voice", "speech_enhance"),
            ("Remove echo", "dereverb"),
            ("Remove hum", "dehummer"),
            ("Voice changer (pitch)", "pitch_shifter"),
            ("Loudness boost", "mastering"),
        ] {
            if ui.button(label).clicked() {
                app.run("effects.apply", json!({"clips": [id], "effect": effect}));
            }
        }
    });
    effects_list(app, ui, &ac);
}

fn speed_tab(app: &mut MattCut, ui: &mut egui::Ui, clip: &ClipView) {
    let id = clip.id;
    let main = app.view.as_ref().and_then(|v| v.main_track()).is_some_and(|t| t.clips.iter().any(|c| c.id == id));
    let ripple = app.cfg.magnet && main;
    section(ui, "Speed");
    ui.label(RichText::new(format!("Now {:.2}x · {}", clip.speed, short_duration(clip.duration()))).small().color(theme::TEXT_DIM));
    if let Some(v) = number(app, ui, format!("{id}:speed"), "Speed", clip.speed, 0.1..=10.0, 0.01, "x") {
        app.run("clip.speedDuration", json!({"clips": [id], "speed": (v * 100.0).clamp(1.0, 10000.0), "reverse": clip.reverse, "ripple": ripple}));
    }
    ui.horizontal_wrapped(|ui| {
        for s in [0.25, 0.5, 1.0, 1.5, 2.0, 3.0, 5.0] {
            if ui.selectable_label((clip.speed - s).abs() < 1e-3, format!("{s}x")).clicked() {
                app.run("clip.speedDuration", json!({"clips": [id], "speed": s * 100.0, "reverse": clip.reverse, "ripple": ripple}));
            }
        }
    });
    let mut rev = clip.reverse;
    if ui.checkbox(&mut rev, "Reverse").changed() {
        app.run("clip.speedDuration", json!({"clips": [id], "speed": clip.speed * 100.0, "reverse": rev, "ripple": ripple}));
    }
    ui.add_space(6.0);
    section(ui, "Smooth slow-mo");
    ui.horizontal(|ui| {
        for (label, mode) in [("Off", "frameSampling"), ("Blend", "frameBlending"), ("Optical flow", "opticalFlow")] {
            if ui.button(label).clicked() {
                app.run(
                    "clip.speedDuration",
                    json!({"clips": [id], "speed": clip.speed * 100.0, "reverse": clip.reverse, "ripple": ripple, "interpolation": mode}),
                );
            }
        }
    });
}

fn adjust_tab(app: &mut MattCut, ui: &mut egui::Ui, clip: &ClipView) {
    let id = clip.id;
    if !clip.has_effect("lumetri") {
        ui.label(RichText::new("Colour adjustments for this clip.").color(theme::TEXT_DIM));
        if ui.add(theme::primary("Adjust colour")).clicked() {
            app.run("effects.apply", json!({"clips": [id], "effect": "lumetri"}));
        }
        return;
    }
    section(ui, "Basic");
    for (param, label, lo, hi, default) in ADJUST {
        let cur = clip.float_param("lumetri", param).unwrap_or(*default);
        let speed = (hi - lo) / 400.0;
        if let Some(v) = number(app, ui, format!("{id}:{param}"), label, cur, *lo..=*hi, speed, "") {
            set_param(app, id, "lumetri", param, json!(v));
        }
    }
    ui.horizontal(|ui| {
        if ui.button("Reset all").clicked() {
            for (param, ..) in ADJUST {
                app.run("effects.resetParam", json!({"clip": id, "effect": "lumetri", "param": param}));
            }
        }
        if ui.button("Auto match to playhead frame").on_hover_text("Match this clip's colour to the frame under the playhead").clicked() {
            let t = app.playhead();
            app.run("lumetri.applyMatch", json!({"clip": id, "referenceTime": t}));
        }
    });
}

fn text_tab(app: &mut MattCut, ui: &mut egui::Ui, clip: &ClipView) {
    let id = clip.id;
    if app.insp.text.as_ref().is_none_or(|(c, _)| *c != id) {
        app.insp.text = Some((id, String::new()));
    }
    section(ui, "Text");
    let mut apply = None;
    if let Some((_, buf)) = app.insp.text.as_mut() {
        let r = ui.add(egui::TextEdit::multiline(buf).hint_text("Type to replace the text").desired_rows(3).desired_width(f32::INFINITY));
        if r.lost_focus() && !buf.trim().is_empty() {
            apply = Some(buf.clone());
        }
    }
    if let Some(t) = apply {
        app.run("graphics.setText", json!({"clip": id, "text": t}));
    }
    ui.add_space(6.0);
    section(ui, "Style");
    let size = app.insp.drafts.get(&format!("{id}:tsize")).copied().unwrap_or(100.0);
    if let Some(v) = number(app, ui, format!("{id}:tsize"), "Size", size, 8.0..=400.0, 0.5, "px") {
        app.run("graphics.set", json!({"clip": id, "props": {"size": v}}));
    }
    ui.horizontal_wrapped(|ui| {
        for (name, hex) in [
            ("White", "#ffffff"),
            ("Black", "#000000"),
            ("Yellow", "#ffd60a"),
            ("Red", "#ff3b30"),
            ("Violet", "#8b5cf6"),
            ("Cyan", "#22d3ee"),
            ("Green", "#34d399"),
        ] {
            let c = egui::Color32::from_hex(hex).unwrap_or(egui::Color32::WHITE);
            let (r, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), egui::Sense::click());
            ui.painter().rect_filled(r, egui::CornerRadius::same(11), c);
            ui.painter().rect_stroke(r, egui::CornerRadius::same(11), egui::Stroke::new(1.0, theme::BORDER), egui::StrokeKind::Inside);
            if resp.on_hover_text(name).clicked() {
                app.run("graphics.set", json!({"clip": id, "props": {"fill_color": hex}}));
            }
        }
    });
    ui.horizontal_wrapped(|ui| {
        for (label, props) in [
            ("Bold", json!({"font_style": "Bold"})),
            ("Regular", json!({"font_style": "Regular"})),
            ("Outline", json!({"stroke": true, "stroke_width": 8})),
            ("No outline", json!({"stroke": false})),
            ("Shadow", json!({"shadow": true})),
            ("Box", json!({"background": true})),
            ("No box", json!({"background": false})),
        ] {
            if ui.button(label).clicked() {
                app.run("graphics.set", json!({"clip": id, "props": props}));
            }
        }
    });
    ui.horizontal(|ui| {
        for (label, a) in [("Left", "left"), ("Centre", "center"), ("Right", "right")] {
            if ui.button(label).clicked() {
                app.run("graphics.set", json!({"clip": id, "props": {"align": a}}));
            }
        }
    });
}

/// A section heading inside the inspector.
fn section(ui: &mut egui::Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(text).size(12.0).color(theme::TEXT));
    ui.add_space(2.0);
}
