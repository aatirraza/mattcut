//! The editor: top bar, library (left), player (centre), inspector (right), timeline (bottom),
//! and the keyboard shortcuts.

use egui::{Align, Color32, CornerRadius, Key, KeyboardShortcut, Layout, Modifiers, RichText, Stroke, pos2, vec2};
use serde_json::json;

use super::{MattCut, home, icons, inspector, library, theme, timeline};
use crate::model::timecode;

/// Shortcuts, as shown in Help ▸ Keyboard Shortcuts. `⌘` is Ctrl on Windows.
pub const SHORTCUTS: &[(&str, &str)] = &[
    ("Space", "Play / pause"),
    ("⌘B", "Split at playhead"),
    ("Q", "Delete left of playhead (trim start)"),
    ("W", "Delete right of playhead (trim end)"),
    ("Delete / Backspace", "Delete selected clips"),
    ("⌘Z", "Undo"),
    ("⇧⌘Z / ⌘Y", "Redo"),
    ("⌘C / ⌘X / ⌘V", "Copy / cut / paste"),
    ("⌘D", "Duplicate"),
    ("⌘A", "Select all"),
    ("Esc", "Deselect"),
    ("← / →", "Previous / next frame"),
    ("⇧← / ⇧→", "Back / forward 1 second"),
    ("↑ / ↓", "Previous / next edit point"),
    ("Home / End", "Go to start / end"),
    ("⌘= / ⌘-", "Zoom timeline in / out"),
    ("⇧Z", "Fit timeline to window"),
    ("M", "Add marker"),
    ("N", "Main-track magnet on / off"),
    ("S", "Snapping on / off"),
    ("⌘I", "Import media"),
    ("⌘E", "Export"),
    ("⌘S", "Save"),
    ("⇧⌘S", "Save a copy"),
    ("⌘N", "New project"),
    ("⌘/", "Show keyboard shortcuts"),
];

const GUTTER: i8 = 5;

pub fn show(app: &mut MattCut, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    keyboard(app, &ctx);
    top_bar(app, ui);
    let gutter = |m: egui::Margin| egui::Frame::new().fill(theme::BG).inner_margin(m);
    let tl = egui::Panel::bottom("timeline")
        .resizable(true)
        .show_separator_line(false)
        .default_size(app.cfg.timeline_height)
        .size_range(170.0..=720.0)
        .frame(gutter(egui::Margin { left: GUTTER, right: GUTTER, top: 0, bottom: GUTTER }))
        .show(ui, |ui| {
            theme::panel_frame().show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                timeline::show(app, ui);
            });
        });
    let h = tl.response.rect.height();
    if (h - app.cfg.timeline_height).abs() > 4.0 && !ctx.input(|i| i.pointer.any_down()) {
        app.cfg.timeline_height = h;
        app.cfg.save();
    }
    egui::Panel::left("library")
        .resizable(true)
        .show_separator_line(false)
        .default_size(400.0)
        .size_range(300.0..=640.0)
        .frame(gutter(egui::Margin { left: GUTTER, right: 0, top: 0, bottom: GUTTER }))
        .show(ui, |ui| {
            theme::panel_frame().show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                library::show(app, ui);
            });
        });
    egui::Panel::right("inspector")
        .resizable(true)
        .show_separator_line(false)
        .default_size(320.0)
        .size_range(270.0..=520.0)
        .frame(gutter(egui::Margin { left: 0, right: GUTTER, top: 0, bottom: GUTTER }))
        .show(ui, |ui| {
            theme::panel_frame().show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                inspector::show(app, ui);
            });
        });
    egui::CentralPanel::default().frame(gutter(egui::Margin { left: GUTTER, right: GUTTER, top: 0, bottom: GUTTER })).show(ui, |ui| {
        theme::panel_frame().show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            player(app, ui);
        });
    });
}

/// The logo (or a placeholder) at `size`.
pub fn logo(app: &MattCut, ui: &mut egui::Ui, size: f32) {
    let (r, _) = ui.allocate_exact_size(vec2(size, size), egui::Sense::hover());
    match &app.logo {
        Some(t) => {
            egui::Image::new((t.id(), vec2(size, size))).corner_radius(CornerRadius::same((size * 0.24) as u8)).paint_at(ui, r);
        }
        None => {
            ui.painter().rect_filled(r, CornerRadius::same((size * 0.24) as u8), theme::ACCENT);
        }
    }
}

fn menu_label(ui: &mut egui::Ui, text: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.menu_button(RichText::new(text).color(theme::TEXT_DIM).size(13.0), |ui| {
        ui.set_min_width(220.0);
        add(ui);
    });
}

fn menu_item(ui: &mut egui::Ui, label: &str, keys: &str) -> bool {
    let w = ui.available_width().max(200.0);
    let (r, resp) = ui.allocate_exact_size(vec2(w, 26.0), egui::Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(r, CornerRadius::same(5), theme::HOVER);
    }
    ui.painter().text(r.left_center() + vec2(10.0, 0.0), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(13.0), theme::TEXT);
    ui.painter().text(r.right_center() - vec2(10.0, 0.0), egui::Align2::RIGHT_CENTER, keys, egui::FontId::proportional(12.0), theme::TEXT_FAINT);
    if resp.clicked() {
        ui.close();
    }
    resp.clicked()
}

fn top_bar(app: &mut MattCut, ui: &mut egui::Ui) {
    let bar =
        egui::Panel::top("editor-top").exact_size(48.0).frame(egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin::symmetric(12, 8))).show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                logo(app, ui, 26.0);
                ui.add_space(2.0);
                ui.label(RichText::new("MattCut").size(14.5).color(theme::TEXT).strong());
                ui.add_space(10.0);
                menu_label(ui, "File", |ui| {
                    if menu_item(ui, "New project…", "⌘N") {
                        app.close_project();
                        app.new_project = Some(home::NewProject { name: String::new(), ratio: 0, res: 1080, fps: 30.0, office: true });
                    }
                    if menu_item(ui, "Import media…", "⌘I") {
                        app.import_dialog();
                    }
                    ui.separator();
                    if menu_item(ui, "Save", "⌘S") {
                        app.save();
                    }
                    if menu_item(ui, "Save a copy…", "⇧⌘S") {
                        app.save_copy = Some(format!("{} (copy)", app.doc.as_ref().map_or("Project", |d| d.name.as_str())));
                    }
                    if menu_item(ui, "Export…", "⌘E") {
                        app.export = Some(super::export::ExportDialog::new(app));
                    }
                    ui.separator();
                    if menu_item(ui, "Back to projects", "") {
                        app.close_project();
                    }
                });
                menu_label(ui, "Edit", |ui| {
                    if menu_item(ui, "Undo", "⌘Z") {
                        app.run("edit.undo", json!({}));
                    }
                    if menu_item(ui, "Redo", "⇧⌘Z") {
                        app.run("edit.redo", json!({}));
                    }
                    ui.separator();
                    if menu_item(ui, "Split", "⌘B") {
                        split(app);
                    }
                    if menu_item(ui, "Delete", "Del") {
                        delete_selection(app);
                    }
                    if menu_item(ui, "Duplicate", "⌘D") {
                        app.run("edit.duplicate", json!({}));
                    }
                    if menu_item(ui, "Select all", "⌘A") {
                        app.run("edit.selectAll", json!({}));
                    }
                });
                menu_label(ui, "Help", |ui| {
                    if menu_item(ui, "Keyboard shortcuts", "⌘/") {
                        app.show_shortcuts = true;
                    }
                    if menu_item(ui, "Connect Claude (MCP)", "") {
                        app.toast("In Terminal: claude mcp add mattcut -- <path to mattcut-mcp>   (see docs/mcp.md)");
                    }
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if icons::pill(ui, icons::Icon::Export, "Export", theme::ACCENT, Color32::WHITE).on_hover_text("⌘E").clicked() {
                        app.export = Some(super::export::ExportDialog::new(app));
                    }
                    ui.add_space(6.0);
                    home::update_button(app, ui);
                    if !crate::config::STANDALONE {
                        home::account_menu(app, ui);
                        let (dot, tip) = home::status_tip(app);
                        theme::status_dot(ui, dot).on_hover_text(tip);
                    }
                    if !app.online.is_empty() {
                        let names: Vec<String> =
                            app.online.iter().map(|u| u.editing.as_ref().map_or_else(|| u.user.clone(), |p| format!("{} — {p}", u.user))).collect();
                        ui.label(RichText::new(format!("{} online", app.online.len())).size(12.0).color(theme::TEXT_FAINT)).on_hover_text(names.join("\n"));
                    }
                });
            });
        });
    // project name centred on the window
    if let Some(d) = &app.doc {
        let r = bar.response.rect;
        let p = ui.painter();
        let name = p.layout_no_wrap(d.name.clone(), egui::FontId::proportional(13.5), theme::TEXT);
        let status = if d.read_only {
            "Read-only"
        } else if app.session.is_dirty() {
            "Edited"
        } else if d.office {
            "Saved to team"
        } else {
            "Saved"
        };
        let sub = p.layout_no_wrap(status.to_string(), egui::FontId::proportional(11.0), theme::TEXT_FAINT);
        let c = r.center();
        p.galley(pos2(c.x - name.size().x / 2.0, c.y - 13.0), name, theme::TEXT);
        p.galley(pos2(c.x - sub.size().x / 2.0, c.y + 3.0), sub, theme::TEXT_FAINT);
    }
}

fn player(app: &mut MattCut, ui: &mut egui::Ui) {
    let full = ui.max_rect();
    let header_h = 34.0;
    let controls_h = 46.0;
    // header
    let p = ui.painter_at(full);
    p.text(full.left_top() + vec2(14.0, header_h / 2.0), egui::Align2::LEFT_CENTER, "Player", egui::FontId::proportional(12.5), theme::TEXT_DIM);
    p.line_segment([pos2(full.min.x, full.min.y + header_h), pos2(full.max.x, full.min.y + header_h)], Stroke::new(1.0, theme::BORDER));
    let Some(view) = app.view.clone() else {
        p.text(full.center(), egui::Align2::CENTER_CENTER, "No sequence", egui::FontId::proportional(13.0), theme::TEXT_DIM);
        return;
    };
    let area = egui::Rect::from_min_max(full.min + vec2(16.0, header_h + 14.0), pos2(full.max.x - 16.0, full.max.y - controls_h - 6.0));
    let aspect = view.width as f32 / view.height.max(1) as f32;
    let mut size = vec2(area.width(), area.width() / aspect);
    if size.y > area.height() {
        size = vec2(area.height() * aspect, area.height());
    }
    let frame_rect = egui::Rect::from_center_size(area.center(), size.max(vec2(1.0, 1.0)));

    // the frame at the playhead, at a scale that fits the screen
    let ppp = ui.ctx().pixels_per_point();
    let mut scale = (size.x * ppp / view.width.max(1) as f32).clamp(0.05, 1.0);
    if app.player.playing {
        scale = scale.min(0.5);
    }
    let scale = ((scale * 8.0).ceil() / 8.0).clamp(0.125, 1.0);
    app.preview.request(&app.session, app.session.playhead(), scale);
    if let Some(f) = app.preview.take() {
        match app.tex.as_mut() {
            Some(t) => t.set(f.image, egui::TextureOptions::LINEAR),
            None => app.tex = Some(ui.ctx().load_texture("player", f.image, egui::TextureOptions::LINEAR)),
        }
    }
    p.rect_filled(frame_rect, CornerRadius::ZERO, Color32::BLACK);
    if let Some(t) = &app.tex {
        p.image(t.id(), frame_rect, egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    }
    if view.duration == 0 {
        p.text(
            frame_rect.center(),
            egui::Align2::CENTER_CENTER,
            "Import media, then drag it to the timeline",
            egui::FontId::proportional(13.0),
            theme::TEXT_DIM,
        );
    }
    let resp = ui.interact(frame_rect, ui.scope_id().with("player-frame"), egui::Sense::click());
    if resp.clicked() {
        app.toggle_play();
    }

    // controls
    let bar = egui::Rect::from_min_max(pos2(full.min.x + 14.0, full.max.y - controls_h), pos2(full.max.x - 14.0, full.max.y - 6.0));
    p.line_segment([pos2(full.min.x, bar.min.y - 2.0), pos2(full.max.x, bar.min.y - 2.0)], Stroke::new(1.0, theme::BORDER));
    let tc = format!("{}", timecode(app.playhead(), view.fps));
    let total = format!("  /  {}", timecode(view.duration, view.fps));
    let g1 = p.layout_no_wrap(tc, egui::FontId::monospace(12.5), theme::ACCENT_HOVER);
    let w1 = g1.size().x;
    p.galley(pos2(bar.min.x, bar.center().y - g1.size().y / 2.0), g1, theme::ACCENT_HOVER);
    p.text(pos2(bar.min.x + w1, bar.center().y), egui::Align2::LEFT_CENTER, total, egui::FontId::monospace(12.5), theme::TEXT_FAINT);
    let mid = bar.center();
    let mut child =
        ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_center_size(mid, vec2(132.0, 36.0))).layout(Layout::left_to_right(Align::Center)));
    if icons::button_sized(&mut child, icons::Icon::Prev, "Previous edit point (↑)", false, 32.0).clicked() {
        jump_edit(app, false);
    }
    let (pr, presp) = child.allocate_exact_size(vec2(40.0, 36.0), egui::Sense::click());
    child.painter().circle_filled(pr.center(), 17.0, if presp.hovered() { theme::HOVER } else { theme::PANEL_3 });
    icons::draw(
        child.painter(),
        egui::Rect::from_center_size(pr.center() + vec2(if app.player.playing { 0.0 } else { 1.5 }, 0.0), vec2(16.0, 16.0)),
        if app.player.playing { icons::Icon::Pause } else { icons::Icon::Play },
        Color32::WHITE,
    );
    if presp.on_hover_text("Play / pause (Space)").clicked() {
        app.toggle_play();
    }
    if icons::button_sized(&mut child, icons::Icon::Next, "Next edit point (↓)", false, 32.0).clicked() {
        jump_edit(app, true);
    }
    let mut right = ui.new_child(
        egui::UiBuilder::new().max_rect(egui::Rect::from_min_max(pos2(bar.max.x - 220.0, bar.min.y), bar.max)).layout(Layout::right_to_left(Align::Center)),
    );
    let current = home::RATIOS.iter().position(|(_, _, rw, rh)| (view.width * rh).abs_diff(view.height * rw) <= view.height.max(view.width) / 50);
    let label = current.and_then(|i| home::RATIOS.get(i)).map_or_else(|| format!("{}×{}", view.width, view.height), |r| r.0.to_string());
    egui::ComboBox::new("ratio", "").selected_text(RichText::new(format!("Ratio  {label}")).size(12.5)).width(110.0).show_ui(&mut right, |ui| {
        for (i, (name, hint, _, _)) in home::RATIOS.iter().enumerate() {
            if ui.selectable_label(current == Some(i), format!("{name}    {hint}")).clicked() {
                let short = view.width.min(view.height);
                let (w, h) = home::ratio_size(i, short);
                app.run("sequence.settings", json!({"width": w, "height": h}));
            }
        }
    });
}

pub fn shortcuts_window(app: &mut MattCut, ctx: &egui::Context) {
    let mut open = true;
    egui::Window::new("Keyboard shortcuts").open(&mut open).collapsible(false).resizable(false).default_width(380.0).show(ctx, |ui| {
        egui::Grid::new("keys").striped(true).num_columns(2).show(ui, |ui| {
            for (k, what) in SHORTCUTS {
                ui.label(RichText::new(*k).monospace().color(theme::ACCENT));
                ui.label(*what);
                ui.end_row();
            }
        });
        ui.label(RichText::new("On Windows, ⌘ is Ctrl and ⇧ is Shift.").small().color(theme::TEXT_DIM));
    });
    app.show_shortcuts = open;
}

// ---- editing actions ---------------------------------------------------------------------------

/// Split (⌘B): the selected clips under the playhead, else every clip under it on the main track.
pub fn split(app: &mut MattCut) {
    let Some(view) = app.view.clone() else { return };
    let t = app.playhead();
    let sel = view.selection.clone();
    let targets: Vec<u64> = view.clips_at(t).into_iter().filter(|c| c.start < t && (sel.is_empty() || sel.contains(&c.id))).map(|c| c.id).collect();
    let targets = if targets.is_empty() && !sel.is_empty() { view.clips_at(t).into_iter().filter(|c| c.start < t).map(|c| c.id).collect() } else { targets };
    if targets.is_empty() {
        app.toast("Move the playhead over a clip to split it");
        return;
    }
    let main_first: Vec<u64> = targets.into_iter().collect();
    let _ = app.session.edit_group("Split", |s| {
        for id in &main_first {
            s.execute("timeline.razor", json!({"time": t, "clip": id}))?;
        }
        Ok(())
    });
}

fn on_main(app: &MattCut, clip: u64) -> bool {
    app.view.as_ref().and_then(|v| v.main_track()).is_some_and(|t| t.clips.iter().any(|c| c.id == clip))
}

pub fn delete_selection(app: &mut MattCut) {
    let sel = app.selection();
    if sel.is_empty() {
        return;
    }
    let ripple = app.cfg.magnet && sel.iter().any(|c| on_main(app, *c));
    app.run("timeline.delete", json!({"clips": sel, "ripple": ripple}));
}

/// Q / W: cut the clip under the playhead and drop the part before (`left`) or after it.
pub fn trim_at_playhead(app: &mut MattCut, left: bool) {
    let Some(view) = app.view.clone() else { return };
    let t = app.playhead();
    let sel = view.selection.clone();
    let Some(clip) = view.clips_at(t).into_iter().find(|c| sel.is_empty() || sel.contains(&c.id)).cloned() else {
        return;
    };
    let Some((track, _)) = view.clip(clip.id) else {
        return;
    };
    let track_id = track.id;
    let ripple = app.cfg.magnet && on_main(app, clip.id);
    let start = clip.start;
    let r = app.session.edit_group(if left { "Delete left" } else { "Delete right" }, |s| {
        if clip.start < t && t < clip.end {
            s.execute("timeline.razor", json!({"time": t, "clip": clip.id}))?;
        }
        let v = s.execute("sequence.inspect", json!({}))?;
        let target = v
            .get("video")
            .into_iter()
            .chain(v.get("audio"))
            .filter_map(|a| a.as_array())
            .flatten()
            .filter(|tr| tr.get("id").and_then(serde_json::Value::as_u64) == Some(track_id))
            .flat_map(|tr| tr.get("items").and_then(|i| i.as_array()).cloned().unwrap_or_default())
            .find(|c| {
                let (cs, ce) = (c.get("start").and_then(|x| x.as_i64()).unwrap_or(-1), c.get("end").and_then(|x| x.as_i64()).unwrap_or(-1));
                if left { ce == t && cs < t } else { cs == t && ce > t }
            })
            .and_then(|c| c.get("clip").and_then(serde_json::Value::as_u64));
        if let Some(id) = target {
            s.execute("timeline.delete", json!({"clips": [id], "ripple": ripple}))?;
        }
        Ok(())
    });
    if let Err(e) = r {
        app.error(e.to_string());
    } else if left && ripple {
        app.set_playhead(start);
    }
}

fn step(app: &mut MattCut, frames: i64) {
    let ft = app.view.as_ref().map_or(1, crate::model::SeqView::frame_ticks);
    app.stop_playback();
    app.set_playhead(app.playhead() + frames * ft);
}

pub fn jump_edit(app: &mut MattCut, forward: bool) {
    let Some(view) = &app.view else { return };
    let t = app.playhead();
    let mut edges = view.edges();
    edges.push(view.duration);
    let next = if forward { edges.iter().copied().find(|e| *e > t + 1) } else { edges.iter().rev().copied().find(|e| *e < t - 1) };
    if let Some(e) = next {
        app.stop_playback();
        app.set_playhead(e);
    }
}

fn keyboard(app: &mut MattCut, ctx: &egui::Context) {
    if ctx.egui_wants_keyboard_input() || !app.office.licensed() {
        return;
    }
    let cmd = Modifiers::COMMAND;
    let shift = Modifiers::SHIFT;
    let none = Modifiers::NONE;
    let pressed = |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(m, k)));
    if pressed(none, Key::Space) {
        app.toggle_play();
    }
    if pressed(cmd, Key::B) {
        split(app);
    }
    if pressed(none, Key::Q) {
        trim_at_playhead(app, true);
    }
    if pressed(none, Key::W) {
        trim_at_playhead(app, false);
    }
    if pressed(none, Key::Delete) || pressed(none, Key::Backspace) {
        delete_selection(app);
    }
    if pressed(cmd | shift, Key::Z) || pressed(cmd, Key::Y) {
        app.run("edit.redo", json!({}));
    }
    if pressed(cmd, Key::Z) {
        app.stop_playback();
        app.run("edit.undo", json!({}));
    }
    if pressed(cmd, Key::C) {
        app.run("edit.copy", json!({}));
    }
    if pressed(cmd, Key::X) {
        app.run("edit.cut", json!({}));
    }
    if pressed(cmd, Key::V) {
        app.run("edit.paste", json!({}));
    }
    if pressed(cmd, Key::D) {
        app.run("edit.duplicate", json!({}));
    }
    if pressed(cmd, Key::A) {
        app.run("edit.selectAll", json!({}));
    }
    if pressed(none, Key::Escape) {
        app.run("edit.deselectAll", json!({}));
    }
    let fps = app.view.as_ref().map_or(30.0, |v| v.fps).round() as i64;
    if pressed(shift, Key::ArrowLeft) {
        step(app, -fps.max(1));
    }
    if pressed(shift, Key::ArrowRight) {
        step(app, fps.max(1));
    }
    if pressed(none, Key::ArrowLeft) {
        step(app, -1);
    }
    if pressed(none, Key::ArrowRight) {
        step(app, 1);
    }
    if pressed(none, Key::ArrowUp) {
        jump_edit(app, false);
    }
    if pressed(none, Key::ArrowDown) {
        jump_edit(app, true);
    }
    if pressed(none, Key::Home) {
        app.stop_playback();
        app.set_playhead(0);
    }
    if pressed(none, Key::End) {
        app.stop_playback();
        let end = app.view.as_ref().map_or(0, |v| v.duration);
        app.set_playhead(end);
    }
    if pressed(cmd, Key::Equals) || pressed(cmd, Key::Plus) || pressed(cmd | shift, Key::Equals) {
        app.tl.zoom(1.25);
    }
    if pressed(cmd, Key::Minus) {
        app.tl.zoom(0.8);
    }
    if pressed(shift, Key::Z) {
        app.tl.fit_next = true;
    }
    if pressed(none, Key::M) {
        app.run("markers.add", json!({"target": "program"}));
    }
    if pressed(none, Key::N) {
        app.cfg.magnet = !app.cfg.magnet;
        app.cfg.save();
        app.toast(if app.cfg.magnet { "Main-track magnet on" } else { "Main-track magnet off" });
    }
    if pressed(none, Key::S) {
        app.run("sequence.snap", json!({}));
        app.toast(if app.session.state.snapping { "Snapping on" } else { "Snapping off" });
    }
    if pressed(cmd, Key::I) {
        app.import_dialog();
    }
    if pressed(cmd, Key::E) {
        app.export = Some(super::export::ExportDialog::new(app));
    }
    if pressed(cmd | shift, Key::S) {
        app.save_copy = Some(format!("{} (copy)", app.doc.as_ref().map_or("Project", |d| d.name.as_str())));
    }
    if pressed(cmd, Key::S) {
        app.save();
    }
    if pressed(cmd, Key::N) {
        app.close_project();
        app.new_project = Some(home::NewProject { name: String::new(), ratio: 0, res: 1080, fps: 30.0, office: true });
    }
    if pressed(cmd, Key::Slash) {
        app.show_shortcuts = !app.show_shortcuts;
    }
}

/// Place a library item: on the main track at the playhead (insert, magnetic) or on its own track.
pub fn place_media(app: &mut MattCut, item: u64, audio: bool, track: Option<String>, time: Option<i64>, insert: bool) {
    let t = time.unwrap_or_else(|| app.playhead());
    // the main track is gap-free: drop in at the nearest cut, not mid-clip
    let main_end = app.view.as_ref().map_or(0, crate::model::SeqView::main_end);
    let track = track.unwrap_or_else(|| if audio { "A1".into() } else { "V1".into() });
    let t = if track == "V1" && app.cfg.magnet {
        let view = app.view.as_ref();
        let edges: Vec<i64> = view.and_then(|v| v.main_track()).map(|tr| tr.clips.iter().flat_map(|c| [c.start, c.end]).collect()).unwrap_or_default();
        if t >= main_end { main_end } else { edges.into_iter().min_by_key(|e| (e - t).abs()).unwrap_or(0) }
    } else {
        t
    };
    let mut params = json!({"item": item, "track": track, "time": t, "insert": insert || (track == "V1" && app.cfg.magnet)});
    if audio {
        params["video"] = json!(false);
    }
    app.run("timeline.place", params);
}
