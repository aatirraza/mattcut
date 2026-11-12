//! The library (left panel): media, audio, text, stickers, effects, transitions, filters and
//! captions. Drag anything with a duration onto the timeline, or click to add it at the playhead.

use egui::{Color32, CornerRadius, FontId, RichText, Sense, Stroke, vec2};
use serde_json::{Value, json};

use super::icons::{self, Icon};
use super::{MattCut, MediaDrag, editor, theme};
use crate::model::{MediaItem, short_duration};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tab {
    #[default]
    Media,
    Audio,
    Text,
    Stickers,
    Effects,
    Transitions,
    Filters,
    Captions,
}

const TABS: [(Tab, Icon, &str); 8] = [
    (Tab::Media, Icon::Media, "Media"),
    (Tab::Audio, Icon::Audio, "Audio"),
    (Tab::Text, Icon::Text, "Text"),
    (Tab::Stickers, Icon::Sticker, "Stickers"),
    (Tab::Effects, Icon::Effects, "Effects"),
    (Tab::Transitions, Icon::Transitions, "Transitions"),
    (Tab::Filters, Icon::Filters, "Filters"),
    (Tab::Captions, Icon::Captions, "Captions"),
];

#[derive(Clone, Debug)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub folder: String,
    pub description: String,
}

#[derive(Default)]
pub struct LibraryState {
    pub tab: Tab,
    pub search: String,
    effects: Option<Vec<Entry>>,
    transitions: Option<Vec<Entry>>,
    filters: Option<Vec<Entry>>,
    voices: Option<Vec<Entry>>,
    tts_text: String,
    tts_voice: String,
    caption_text: String,
    captions: Option<(u64, Vec<(u64, String, i64)>)>,
}

pub fn show(app: &mut MattCut, ui: &mut egui::Ui) {
    // tab strip: icon over label, evenly spread
    let strip = ui.max_rect();
    let tab_h = 56.0;
    let n = TABS.len() as f32;
    let tw = (strip.width() - 12.0) / n;
    let (bar, _) = ui.allocate_exact_size(vec2(strip.width(), tab_h), Sense::hover());
    for (i, (tab, icon, label)) in TABS.iter().enumerate() {
        let r = egui::Rect::from_min_size(egui::pos2(bar.min.x + 6.0 + tw * i as f32, bar.min.y + 6.0), vec2(tw, tab_h - 10.0));
        let resp = ui.interact(r, ui.scope_id().with(("lib-tab", i)), Sense::click());
        let sel = app.lib.tab == *tab;
        let p = ui.painter();
        if sel {
            p.rect_filled(r.shrink2(vec2(2.0, 0.0)), CornerRadius::same(7), theme::ACCENT_SOFT);
        } else if resp.hovered() {
            p.rect_filled(r.shrink2(vec2(2.0, 0.0)), CornerRadius::same(7), theme::PANEL_2);
        }
        let c = if sel {
            theme::ACCENT_HOVER
        } else if resp.hovered() {
            theme::TEXT
        } else {
            theme::TEXT_DIM
        };
        icons::draw(p, egui::Rect::from_center_size(r.center() - vec2(0.0, 8.0), vec2(20.0, 20.0)), *icon, c);
        p.text(r.center() + vec2(0.0, 12.0), egui::Align2::CENTER_CENTER, *label, FontId::proportional(10.5), if sel { theme::TEXT } else { theme::TEXT_DIM });
        if resp.clicked() {
            app.lib.tab = *tab;
            app.lib.search.clear();
        }
    }
    let y = bar.max.y;
    ui.painter().line_segment([egui::pos2(strip.min.x, y), egui::pos2(strip.max.x, y)], Stroke::new(1.0, theme::BORDER));
    egui::Frame::new().inner_margin(egui::Margin::same(12)).show(ui, |ui| {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match app.lib.tab {
            Tab::Media => media_tab(app, ui),
            Tab::Audio => audio_tab(app, ui),
            Tab::Text => text_tab(app, ui),
            Tab::Stickers => stickers_tab(app, ui),
            Tab::Effects => effects_tab(app, ui),
            Tab::Transitions => transitions_tab(app, ui),
            Tab::Filters => filters_tab(app, ui),
            Tab::Captions => captions_tab(app, ui),
        });
    });
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(13.5).color(theme::TEXT));
    ui.add_space(2.0);
}

fn hint(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).small().color(theme::TEXT_DIM));
}

fn search(ui: &mut egui::Ui, s: &mut String) {
    egui::Frame::new().fill(theme::PANEL_2).corner_radius(CornerRadius::same(7)).inner_margin(egui::Margin::symmetric(8, 3)).show(ui, |ui| {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
            icons::draw(ui.painter(), r, Icon::Search, theme::TEXT_FAINT);
            ui.add(egui::TextEdit::singleline(s).hint_text("Search").frame(egui::Frame::NONE).desired_width(f32::INFINITY));
        });
    });
    ui.add_space(4.0);
}

fn matches(search: &str, e: &Entry) -> bool {
    let q = search.trim().to_lowercase();
    q.is_empty() || e.name.to_lowercase().contains(&q) || e.folder.to_lowercase().contains(&q)
}

// ---- media ------------------------------------------------------------------------------------

const CARD_W: f32 = 112.0;
const CARD_H: f32 = 68.0;

fn media_card(app: &mut MattCut, ui: &mut egui::Ui, m: &MediaItem) {
    let payload = MediaDrag { item: m.id, audio: m.is_audio(), duration: m.duration, name: m.name.clone() };
    let id = egui::Id::unique(("media", m.id));
    let mut add = false;
    let at = (m.duration / 3).clamp(0, filmcraft_time::TICKS_PER_SECOND * 2);
    let thumb = if m.is_audio() { None } else { app.thumbs.get(&app.session, m.id, at) };
    ui.vertical(|ui| {
        ui.set_width(CARD_W);
        ui.spacing_mut().item_spacing.y = 4.0;
        let inner = ui.dnd_drag_source(id, payload, |ui| {
            let (r, resp) = ui.allocate_exact_size(vec2(CARD_W, CARD_H), Sense::hover());
            let p = ui.painter();
            p.rect_filled(r, CornerRadius::same(7), theme::PANEL_3);
            match &thumb {
                Some(t) => {
                    egui::Image::new((t.id(), r.size())).corner_radius(CornerRadius::same(7)).fit_to_exact_size(r.size()).paint_at(ui, r);
                }
                None => {
                    let icon = if m.is_audio() {
                        Icon::Audio
                    } else if m.kind == "Still" {
                        Icon::Sticker
                    } else {
                        Icon::Media
                    };
                    let bg = if m.is_audio() { theme::CLIP_AUDIO } else { theme::PANEL_3 };
                    p.rect_filled(r, CornerRadius::same(7), bg);
                    icons::draw(p, egui::Rect::from_center_size(r.center(), vec2(24.0, 24.0)), icon, theme::TEXT_DIM);
                }
            }
            if m.duration > 0 && !matches!(m.kind.as_str(), "Still") {
                let txt = short_duration(m.duration);
                let g = p.layout_no_wrap(txt, FontId::proportional(10.0), Color32::WHITE);
                let br = egui::Rect::from_min_size(r.right_bottom() - g.size() - vec2(10.0, 7.0), g.size() + vec2(6.0, 2.0));
                p.rect_filled(br, CornerRadius::same(4), Color32::from_black_alpha(160));
                p.galley(br.min + vec2(3.0, 1.0), g, Color32::WHITE);
            }
            resp
        });
        let r = inner.response.rect;
        let hovered = ui.rect_contains_pointer(r);
        if hovered {
            ui.painter().rect_stroke(r, CornerRadius::same(7), Stroke::new(1.5, theme::ACCENT), egui::StrokeKind::Inside);
            // the add button, bottom-left on hover
            let ar = egui::Rect::from_center_size(r.left_bottom() + vec2(14.0, -14.0), vec2(20.0, 20.0));
            let aresp = ui.interact(ar, id.with("add"), Sense::click());
            ui.painter().circle_filled(ar.center(), 10.0, if aresp.hovered() { theme::ACCENT_HOVER } else { theme::ACCENT });
            icons::draw(ui.painter(), ar.shrink(4.0), Icon::Plus, Color32::WHITE);
            if aresp.on_hover_text("Add at the playhead").clicked() {
                add = true;
            }
        }
        let name: String = if m.name.chars().count() > 18 { format!("{}…", m.name.chars().take(17).collect::<String>()) } else { m.name.clone() };
        ui.label(RichText::new(name).size(11.0).color(theme::TEXT_DIM)).on_hover_text(&m.name);
    });
    if add {
        editor::place_media(app, m.id, m.is_audio(), None, None, false);
    }
}

fn media_grid(app: &mut MattCut, ui: &mut egui::Ui, items: Vec<MediaItem>) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(10.0, 12.0);
        for m in &items {
            media_card(app, ui, m);
        }
    });
}

fn media_tab(app: &mut MattCut, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        if icons::pill(ui, Icon::Plus, "Import", theme::ACCENT, Color32::WHITE).on_hover_text("⌘I — or drop files onto the window").clicked() {
            app.import_dialog();
        }
        if let Some(share) = app.office.status().login.and_then(|l| l.media_share) {
            hint(ui, &format!("Team footage: {share}"));
        }
    });
    ui.add_space(6.0);
    search(ui, &mut app.lib.search);
    let q = app.lib.search.to_lowercase();
    let items: Vec<MediaItem> =
        app.media.iter().filter(|m| !m.is_sequence() && m.kind != "Graphic" && (q.is_empty() || m.name.to_lowercase().contains(&q))).cloned().collect();
    if items.is_empty() {
        ui.add_space(20.0);
        hint(ui, "Import videos, photos and music, or drag them onto this window. Then drag them to the timeline.");
        return;
    }
    media_grid(app, ui, items);
}

// ---- audio ------------------------------------------------------------------------------------

fn audio_tab(app: &mut MattCut, ui: &mut egui::Ui) {
    heading(ui, "Music & sound");
    if ui.button("+ Import music or sound effects").clicked() {
        app.import_dialog();
    }
    let items: Vec<MediaItem> = app.media.iter().filter(|m| m.is_audio()).cloned().collect();
    if items.is_empty() {
        hint(ui, "Imported audio appears here.");
    } else {
        media_grid(app, ui, items);
    }
    ui.add_space(14.0);
    heading(ui, "Text to speech");
    if app.lib.voices.is_none() {
        app.lib.voices = Some(
            app.session
                .execute("tts.voices", json!({}))
                .ok()
                .and_then(|v| v.get("voices").and_then(Value::as_array).cloned())
                .unwrap_or_default()
                .iter()
                .filter(|v| v.get("installed").and_then(Value::as_bool).unwrap_or(false))
                .filter_map(|v| {
                    Some(Entry {
                        id: v.get("id")?.as_str()?.into(),
                        name: v.get("name")?.as_str()?.into(),
                        folder: String::new(),
                        description: v.get("description").and_then(Value::as_str).unwrap_or_default().into(),
                    })
                })
                .collect(),
        );
    }
    let voices = app.lib.voices.clone().unwrap_or_default();
    if app.lib.tts_voice.is_empty()
        && let Some(v) = voices.first()
    {
        app.lib.tts_voice = v.id.clone();
    }
    ui.add(egui::TextEdit::multiline(&mut app.lib.tts_text).hint_text("Type what the voice should say…").desired_rows(3).desired_width(f32::INFINITY));
    ui.horizontal(|ui| {
        let current = voices.iter().find(|v| v.id == app.lib.tts_voice).map_or("Voice", |v| v.name.as_str()).to_string();
        egui::ComboBox::new("tts-voice", "").selected_text(current).show_ui(ui, |ui| {
            for v in &voices {
                ui.selectable_value(&mut app.lib.tts_voice, v.id.clone(), &v.name).on_hover_text(&v.description);
            }
        });
        let ready = !app.lib.tts_text.trim().is_empty();
        if ui.add_enabled(ready, egui::Button::new("Add narration")).clicked() {
            let (text, voice, t) = (app.lib.tts_text.clone(), app.lib.tts_voice.clone(), app.playhead());
            if app.run("tts.create", json!({"text": text, "voice": voice, "time": t})).is_some() {
                app.lib.tts_text.clear();
                app.toast("Narration added at the playhead");
            }
        }
    });
    hint(ui, "Natural voices: build MattCut with the natural-voices feature.");
}

// ---- text -------------------------------------------------------------------------------------

struct TextPreset {
    name: &'static str,
    text: &'static str,
    size: f64,
    props: fn() -> Value,
}

const TEXT_PRESETS: &[TextPreset] = &[
    TextPreset { name: "Default text", text: "Default text", size: 90.0, props: || json!({"fill_color": "#ffffff"}) },
    TextPreset { name: "Big title", text: "BIG TITLE", size: 160.0, props: || json!({"fill_color": "#ffffff", "font_style": "Bold", "shadow": true}) },
    TextPreset {
        name: "Outline",
        text: "Outline",
        size: 120.0,
        props: || json!({"fill_color": "#ffffff", "font_style": "Bold", "stroke": true, "stroke_width": 8, "stroke_color": "#000000"}),
    },
    TextPreset {
        name: "Yellow pop",
        text: "WOW!",
        size: 140.0,
        props: || json!({"fill_color": "#ffd60a", "font_style": "Bold", "stroke": true, "stroke_width": 10, "stroke_color": "#111111"}),
    },
    TextPreset { name: "Subtitle box", text: "Subtitle text", size: 64.0, props: || json!({"fill_color": "#ffffff", "background": true}) },
    TextPreset {
        name: "Lower third",
        text: "Name Surname\nTitle",
        size: 70.0,
        props: || json!({"fill_color": "#ffffff", "font_style": "SemiBold", "background": true, "align": "left"}),
    },
    TextPreset { name: "Neon", text: "NEON", size: 150.0, props: || json!({"fill_color": "#f0abfc", "font_style": "Bold", "shadow": true}) },
    TextPreset { name: "Quiet caption", text: "a quiet line", size: 56.0, props: || json!({"fill_color": "#e5e5e5", "opacity": 85}) },
];

fn text_tab(app: &mut MattCut, ui: &mut egui::Ui) {
    heading(ui, "Add text");
    hint(ui, "Click a style to add it at the playhead on a free track. Edit the words in the panel on the right.");
    ui.add_space(6.0);
    let mut chosen = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        for (i, preset) in TEXT_PRESETS.iter().enumerate() {
            let (r, resp) = ui.allocate_exact_size(vec2(108.0, 72.0), Sense::click());
            let p = ui.painter();
            p.rect_filled(r, CornerRadius::same(7), Color32::from_rgb(0x10, 0x10, 0x12));
            if resp.hovered() {
                p.rect_stroke(r, CornerRadius::same(7), Stroke::new(1.5, theme::ACCENT), egui::StrokeKind::Inside);
            }
            let props = (preset.props)();
            let color = props.get("fill_color").and_then(Value::as_str).and_then(|h| Color32::from_hex(h).ok()).unwrap_or(Color32::WHITE);
            let label: String = preset.text.lines().next().unwrap_or(preset.text).chars().take(10).collect();
            if props.get("stroke").is_some() {
                for d in [vec2(-1.0, 0.0), vec2(1.0, 0.0), vec2(0.0, -1.0), vec2(0.0, 1.0)] {
                    p.text(r.center() - vec2(0.0, 6.0) + d, egui::Align2::CENTER_CENTER, &label, FontId::proportional(16.0), Color32::BLACK);
                }
            }
            p.text(r.center() - vec2(0.0, 6.0), egui::Align2::CENTER_CENTER, &label, FontId::proportional(16.0), color);
            p.text(r.center_bottom() - vec2(0.0, 9.0), egui::Align2::CENTER_CENTER, preset.name, FontId::proportional(10.0), theme::TEXT_DIM);
            if resp.clicked() {
                chosen = Some(i);
            }
        }
    });
    if let Some(i) = chosen
        && let Some(preset) = TEXT_PRESETS.get(i)
    {
        add_text(app, preset);
    }
}

/// The first video track above the main one that is free for `secs` from the playhead.
fn free_overlay_track(app: &MattCut, len: i64) -> usize {
    let t = app.playhead();
    let Some(v) = &app.view else { return 1 };
    (1..v.video.len().max(2))
        .find(|i| v.video.get(*i).is_none_or(|tr| !tr.clips.iter().any(|c| c.start < t + len && t < c.end)))
        .unwrap_or(v.video.len().max(2) - 1)
}

fn add_text(app: &mut MattCut, preset: &TextPreset) {
    let len = crate::model::ticks(5.0);
    let track = free_overlay_track(app, len);
    let t = app.playhead();
    let Some(r) = app.run("graphics.newText", json!({"text": preset.text, "size": preset.size, "seconds": 5.0, "track": track, "time": t, "newClip": true}))
    else {
        return;
    };
    let clip = r.get("clip").and_then(Value::as_u64);
    let mut params = json!({"props": (preset.props)()});
    if let Some(c) = clip {
        params["clip"] = json!(c);
    }
    let _ = app.session.execute("graphics.set", params);
    if let Some(c) = clip {
        let _ = app.session.execute("timeline.select", json!({"clips": [c]}));
    }
}

// ---- stickers ---------------------------------------------------------------------------------

fn stickers_tab(app: &mut MattCut, ui: &mut egui::Ui) {
    heading(ui, "Stickers & overlays");
    hint(ui, "Add a PNG (with transparency), GIF or WebP as an overlay above your video at the playhead.");
    ui.add_space(6.0);
    if ui.button("+ Add sticker from a file…").clicked()
        && let Some(path) = rfd::FileDialog::new().add_filter("Images", &["png", "gif", "webp", "jpg", "jpeg"]).pick_file()
    {
        let before: Vec<u64> = app.media.iter().map(|m| m.id).collect();
        if app.run("file.import", json!({"paths": [path.to_string_lossy()]})).is_some() {
            app.media = crate::model::media_items(&mut app.session);
            if let Some(new) = app.media.iter().find(|m| !before.contains(&m.id)).cloned() {
                let track = free_overlay_track(app, crate::model::ticks(5.0));
                editor::place_media(app, new.id, false, Some(format!("V{}", track + 1)), None, false);
            }
        }
    }
    ui.add_space(10.0);
    let stills: Vec<MediaItem> = app.media.iter().filter(|m| m.kind == "Still").cloned().collect();
    if !stills.is_empty() {
        hint(ui, "Images in this project (drag onto a track above the main one):");
        media_grid(app, ui, stills);
    }
    ui.add_space(10.0);
    hint(ui, "A shared team sticker pack is on the roadmap (see ROADMAP.md).");
}

// ---- effects / transitions / filters ----------------------------------------------------------

fn load_effects(app: &mut MattCut, kind: &str) -> Vec<Entry> {
    app.session
        .execute("effects.list", json!({"kind": kind}))
        .ok()
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter(|e| !matches!(e.get("folder").and_then(Value::as_str), Some("Obsolete")))
        .filter(|e| !matches!(e.get("id").and_then(Value::as_str), Some("motion" | "opacity" | "time_remap")))
        .filter_map(|e| {
            Some(Entry {
                id: e.get("id")?.as_str()?.into(),
                name: e.get("name")?.as_str()?.into(),
                folder: e.get("folder").and_then(Value::as_str).unwrap_or("Other").into(),
                description: String::new(),
            })
        })
        .collect()
}

fn entry_grid(ui: &mut egui::Ui, entries: &[Entry], search: &str, icon: Icon) -> Option<Entry> {
    let mut clicked = None;
    let mut folders: Vec<&str> = entries.iter().filter(|e| matches(search, e)).map(|e| e.folder.as_str()).collect();
    folders.dedup();
    let mut seen = Vec::new();
    for f in folders {
        if seen.contains(&f) {
            continue;
        }
        seen.push(f);
        ui.add_space(4.0);
        ui.add_space(4.0);
        ui.label(RichText::new(f).size(11.5).color(theme::TEXT_FAINT));
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for e in entries.iter().filter(|e| e.folder == f && matches(search, e)) {
                let (r, resp) = ui.allocate_exact_size(vec2(108.0, 70.0), Sense::click());
                let p = ui.painter();
                let thumb = egui::Rect::from_min_size(r.min, vec2(r.width(), 46.0));
                p.rect_filled(thumb, CornerRadius::same(7), if resp.hovered() { theme::ACCENT_SOFT } else { theme::PANEL_2 });
                if resp.hovered() {
                    p.rect_stroke(thumb, CornerRadius::same(7), Stroke::new(1.5, theme::ACCENT), egui::StrokeKind::Inside);
                }
                icons::draw(
                    p,
                    egui::Rect::from_center_size(thumb.center(), vec2(20.0, 20.0)),
                    icon,
                    if resp.hovered() { theme::ACCENT_HOVER } else { theme::TEXT_FAINT },
                );
                let name: String = if e.name.chars().count() > 17 { format!("{}…", e.name.chars().take(16).collect::<String>()) } else { e.name.clone() };
                p.text(egui::pos2(r.min.x + 2.0, thumb.max.y + 6.0), egui::Align2::LEFT_TOP, name, FontId::proportional(11.0), theme::TEXT_DIM);
                let resp = if e.description.is_empty() { resp } else { resp.on_hover_text(&e.description) };
                if resp.clicked() {
                    clicked = Some(e.clone());
                }
            }
        });
    }
    clicked
}

fn needs_selection(app: &MattCut, ui: &mut egui::Ui) -> bool {
    let targets = app.target_clips();
    if targets.is_empty() {
        hint(ui, "Select a clip on the timeline (or put the playhead over the main track) first.");
        true
    } else {
        hint(ui, &format!("Applies to {} clip{}.", targets.len(), if targets.len() == 1 { "" } else { "s" }));
        false
    }
}

fn effects_tab(app: &mut MattCut, ui: &mut egui::Ui) {
    heading(ui, "Effects");
    if app.lib.effects.is_none() {
        app.lib.effects = Some(load_effects(app, "Video"));
    }
    needs_selection(app, ui);
    search(ui, &mut app.lib.search);
    let entries = app.lib.effects.clone().unwrap_or_default();
    if let Some(e) = entry_grid(ui, &entries, &app.lib.search, Icon::Effects) {
        let clips = app.target_clips();
        if !clips.is_empty() && app.run("effects.apply", json!({"clips": clips, "effect": e.id})).is_some() {
            app.toast(format!("{} applied — adjust it on the right", e.name));
        }
    }
}

fn transitions_tab(app: &mut MattCut, ui: &mut egui::Ui) {
    heading(ui, "Transitions");
    if app.lib.transitions.is_none() {
        app.lib.transitions = Some(load_effects(app, "VideoTransition"));
    }
    hint(ui, "Select a clip: the transition goes on the cut at its end. Put the playhead on a cut to use that cut.");
    search(ui, &mut app.lib.search);
    let entries = app.lib.transitions.clone().unwrap_or_default();
    if let Some(e) = entry_grid(ui, &entries, &app.lib.search, Icon::Transitions) {
        let Some(view) = app.view.clone() else { return };
        let t = app.playhead();
        let sel = app.selection();
        // the clip whose end is at (or nearest before) the playhead on the main track, else the selection
        let clip = sel.first().copied().or_else(|| view.main_track().and_then(|tr| tr.clips.iter().min_by_key(|c| (c.end - t).abs()).map(|c| c.id)));
        match clip {
            Some(c) => {
                if app.run("sequence.applyVideoTransition", json!({"clip": c, "effect": e.id, "edge": "out"})).is_some() {
                    app.toast(format!("{} added", e.name));
                }
            }
            None => app.toast("Add clips to the timeline first"),
        }
    }
}

fn filters_tab(app: &mut MattCut, ui: &mut egui::Ui) {
    heading(ui, "Filters");
    if app.lib.filters.is_none() {
        let v = app.session.execute("lumetri.presets", json!({})).unwrap_or(Value::Null);
        app.lib.filters = Some(
            v.get("presets")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
                .iter()
                .filter_map(|p| {
                    let name = p.get("name")?.as_str()?.to_string();
                    Some(Entry {
                        id: name.clone(),
                        name,
                        folder: p.get("folder").and_then(Value::as_str).unwrap_or("Looks").into(),
                        description: p.get("description").and_then(Value::as_str).unwrap_or_default().into(),
                    })
                })
                .collect(),
        );
    }
    needs_selection(app, ui);
    search(ui, &mut app.lib.search);
    let entries = app.lib.filters.clone().unwrap_or_default();
    if let Some(e) = entry_grid(ui, &entries, &app.lib.search, Icon::Filters) {
        let clips = app.target_clips();
        if !clips.is_empty() && app.run("lumetri.applyPreset", json!({"name": e.id, "clips": clips})).is_some() {
            app.toast(format!("Filter \"{}\" applied — fine-tune it under Adjust on the right", e.name));
        }
    }
}

// ---- captions ---------------------------------------------------------------------------------

fn captions_tab(app: &mut MattCut, ui: &mut egui::Ui) {
    heading(ui, "Auto captions");
    hint(ui, "Transcribes the speech in your edit and adds timed captions.");
    let auto = cfg!(feature = "auto-captions");
    if ui.add_enabled(auto, theme::primary("Generate captions")).clicked() {
        let items: Vec<u64> =
            app.view.as_ref().map(|v| v.video.iter().chain(&v.audio).flat_map(|t| t.clips.iter().map(|c| c.item)).collect()).unwrap_or_default();
        let mut items = items;
        items.sort_unstable();
        items.dedup();
        if app.run("transcript.generate", json!({"items": items, "language": "auto"})).is_some()
            && app.run("transcript.createCaptions", json!({"lines": 1})).is_some()
        {
            app.toast("Captions added");
        }
    }
    if !auto {
        hint(ui, "Build with --features auto-captions to turn this on (Whisper runs on this computer; nothing is uploaded).");
    }
    ui.add_space(12.0);
    heading(ui, "Add a caption");
    ui.add(egui::TextEdit::singleline(&mut app.lib.caption_text).hint_text("Caption text").desired_width(f32::INFINITY));
    ui.horizontal(|ui| {
        if ui.button("Add at playhead (3 s)").clicked() && !app.lib.caption_text.trim().is_empty() {
            let has_track = app.session.execute("captions.list", json!({})).is_ok();
            if !has_track {
                let _ = app.session.execute("captions.newTrack", json!({"format": "Subtitle", "name": "Captions"}));
            }
            let t = app.playhead();
            let text = app.lib.caption_text.clone();
            if app.run("captions.add", json!({"text": text, "time": t, "durationSeconds": 3.0})).is_some() {
                app.lib.caption_text.clear();
                app.lib.captions = None;
            }
        }
        if ui.button("Import SRT / VTT…").clicked()
            && let Some(p) = rfd::FileDialog::new().add_filter("Captions", &["srt", "vtt"]).pick_file()
        {
            app.run("captions.import", json!({"path": p.to_string_lossy()}));
            app.lib.captions = None;
        }
        if ui.button("Export SRT…").clicked()
            && let Some(p) = rfd::FileDialog::new().add_filter("SubRip", &["srt"]).set_file_name("captions.srt").save_file()
        {
            if app.run("captions.export", json!({"path": p.to_string_lossy(), "format": "srt"})).is_some() {
                app.toast("Captions exported");
            }
        }
    });
    ui.add_space(10.0);
    // the caption list, editable
    let rev = app.session.revision;
    if app.lib.captions.as_ref().is_none_or(|(r, _)| *r != rev) {
        let list = app.session.execute("captions.list", json!({})).ok();
        let rows = list
            .as_ref()
            .and_then(|v| v.get("captions").or(Some(v)).and_then(Value::as_array).cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|c| {
                Some((
                    c.get("id")?.as_u64()?,
                    c.get("text").and_then(Value::as_str).unwrap_or_default().to_string(),
                    c.get("start").and_then(Value::as_i64).unwrap_or(0),
                ))
            })
            .collect();
        app.lib.captions = Some((rev, rows));
    }
    let mut edits = Vec::new();
    let fps = app.view.as_ref().map_or(30.0, |v| v.fps);
    if let Some((_, rows)) = app.lib.captions.as_mut() {
        if !rows.is_empty() {
            heading(ui, "Captions");
        }
        for (id, text, start) in rows.iter_mut() {
            ui.horizontal(|ui| {
                if ui.small_button(crate::model::timecode(*start, fps)).clicked() {
                    edits.push((*id, None, Some(*start)));
                }
                let r = ui.add(egui::TextEdit::singleline(text).desired_width(f32::INFINITY));
                if r.lost_focus() {
                    edits.push((*id, Some(text.clone()), None));
                }
            });
        }
    }
    for (id, text, seek) in edits {
        if let Some(t) = text {
            app.run("captions.setText", json!({"caption": id, "text": t}));
        }
        if let Some(s) = seek {
            app.set_playhead(s);
        }
    }
}
