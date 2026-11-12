//! The timeline: overlay tracks above the magnetic main track, audio below. Click to select, drag
//! to move, drag an edge to trim, drag the ruler to scrub, drop media from the library.

use egui::{Align, Align2, Color32, CornerRadius, FontId, Layout, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, pos2, vec2};
use serde_json::{Value, json};

use super::icons::{self, Icon};
use super::{MattCut, MediaDrag, editor, theme};
use crate::model::{ClipView, Kind, SeqView, secs, short_duration, ticks};

const HEADER_W: f32 = 104.0;
const RULER_H: f32 = 26.0;
const ROW_OVERLAY: f32 = 42.0;
const ROW_MAIN: f32 = 64.0;
const ROW_AUDIO: f32 = 40.0;
const EDGE_GRAB: f32 = 7.0;
const SNAP_PX: f32 = 9.0;

pub struct TimelineState {
    /// Pixels per second.
    pub pps: f32,
    /// Seconds at the left edge of the tracks.
    pub scroll: f32,
    pub fit_next: bool,
    drag: Option<Drag>,
}

impl Default for TimelineState {
    fn default() -> Self {
        Self { pps: 60.0, scroll: 0.0, fit_next: true, drag: None }
    }
}

impl TimelineState {
    pub fn zoom(&mut self, factor: f32) {
        self.pps = (self.pps * factor).clamp(2.0, 2400.0);
    }
}

#[derive(Clone, Debug)]
enum Drag {
    Scrub,
    Move { clip: ClipView, from_row: usize, grab: i64 },
    Trim { clip: ClipView, start_edge: bool },
}

/// One visible track lane.
#[derive(Clone, Debug)]
struct Row {
    kind: Kind,
    /// Track index within its kind (0 = V1 / A1).
    index: usize,
    track_id: Option<u64>,
    rect: Rect,
}

impl Row {
    fn is_main(&self) -> bool {
        self.kind == Kind::Video && self.index == 0
    }
    fn label(&self) -> String {
        match (self.kind, self.index) {
            (Kind::Video, 0) => "Main".into(),
            (Kind::Video, i) => format!("V{}", i + 1),
            (Kind::Audio, i) => format!("A{}", i + 1),
        }
    }
    fn track_name(&self) -> String {
        match self.kind {
            Kind::Video => format!("V{}", self.index + 1),
            Kind::Audio => format!("A{}", self.index + 1),
        }
    }
}

enum Hit {
    Ruler,
    Clip { clip: ClipView, row: usize, edge: Option<bool> },
    Empty { row: usize },
    Nothing,
}

pub fn show(app: &mut MattCut, ui: &mut egui::Ui) {
    toolbar(app, ui);
    let Some(view) = app.view.clone() else { return };
    let full = ui.available_rect_before_wrap();
    let tracks_left = full.min.x + HEADER_W;
    let tracks_w = (full.max.x - tracks_left).max(10.0);
    if app.tl.fit_next {
        app.tl.fit_next = false;
        let dur = secs(view.duration).max(10.0) as f32;
        app.tl.pps = ((tracks_w - 60.0) / dur).clamp(2.0, 2400.0);
        app.tl.scroll = 0.0;
    }
    let dragging = app.tl.drag.is_some() || egui::DragAndDrop::has_any_payload(ui.ctx());
    let rows = layout_rows(&view, full, dragging);
    let resp = ui.interact(full, ui.scope_id().with("tl-canvas"), Sense::click_and_drag());
    let painter = ui.painter_at(full);
    let (scroll0, pps0) = (app.tl.scroll, app.tl.pps);
    let x_of = move |t: i64| tracks_left + (secs(t) as f32 - scroll0) * pps0;
    let t_of = move |x: f32| ticks(f64::from((x - tracks_left) / pps0 + scroll0)).max(0);

    // background
    for r in &rows {
        if r.is_main() {
            painter.rect_filled(r.rect.expand2(vec2(0.0, 2.0)), CornerRadius::ZERO, Color32::from_rgb(0x1c, 0x1c, 0x21));
        }
    }
    ruler(&painter, full, tracks_left, scroll0, pps0, view.fps);

    // clips
    let selection = view.selection.clone();
    let dragging_clip = match &app.tl.drag {
        Some(Drag::Move { clip, .. } | Drag::Trim { clip, .. }) => Some(clip.id),
        _ => None,
    };
    let clip_painter = painter.with_clip_rect(Rect::from_min_max(pos2(tracks_left, full.min.y + RULER_H), full.max));
    for row in &rows {
        for c in clips_of(&view, row) {
            let r = clip_rect(row, c, &x_of);
            if r.max.x < tracks_left || r.min.x > full.max.x {
                continue;
            }
            let faded = dragging_clip == Some(c.id) && matches!(app.tl.drag, Some(Drag::Move { .. }));
            let kind = app.media.iter().find(|m| m.id == c.item).map(|m| m.kind.clone());
            // filmstrip: one frame per tile across the visible part of the clip
            let mut tiles = Vec::new();
            if row.kind == Kind::Video && !matches!(kind.as_deref(), Some("Graphic" | "Synthetic")) && r.height() > 20.0 {
                let th = r.height() - 2.0;
                let tw = (th * 16.0 / 9.0).max(16.0);
                let mut x = r.min.x.max(tracks_left - tw);
                let mut guard = 0;
                while x < r.max.x.min(full.max.x) && guard < 200 {
                    guard += 1;
                    let t = t_of(x.max(r.min.x));
                    let src = c.source_in + ((t - c.start) as f64 * c.speed.max(0.01)) as i64;
                    if let Some(tex) = app.thumbs.get(&app.session, c.item, src) {
                        tiles.push((Rect::from_min_size(pos2(x, r.min.y + 1.0), vec2(tw, th)), tex.id()));
                    }
                    x += tw;
                }
            }
            paint_clip(&clip_painter, r, c, row, selection.contains(&c.id), faded, kind.as_deref(), &tiles);
        }
    }

    // drop from the library
    let pointer = ui.ctx().pointer_hover_pos();
    if let Some(payload) = egui::DragAndDrop::payload::<MediaDrag>(ui.ctx())
        && let Some(p) = pointer.filter(|p| full.contains(*p))
    {
        let target = drop_target(&rows, p, payload.audio);
        if let Some(row) = target.and_then(|i| rows.get(i)) {
            let t = snap(&view, t_of(p.x), app, &x_of, None);
            let ghost =
                Rect::from_min_size(pos2(x_of(t), row.rect.min.y + 3.0), vec2((secs(payload.duration) as f32 * app.tl.pps).max(6.0), row.rect.height() - 6.0));
            clip_painter.rect_filled(ghost, CornerRadius::same(5), Color32::from_rgba_unmultiplied(139, 92, 246, 90));
            clip_painter.rect_stroke(ghost, CornerRadius::same(5), Stroke::new(1.5, theme::ACCENT), StrokeKind::Inside);
        }
        if ui.input(|i| i.pointer.any_released()) {
            let payload = (*payload).clone();
            egui::DragAndDrop::clear_payload(ui.ctx());
            if let Some(row) = target.and_then(|i| rows.get(i)) {
                let t = snap(&view, t_of(p.x), app, &x_of, None);
                let track = row.track_name();
                editor::place_media(app, payload.item, payload.audio, Some(track), Some(t), false);
            }
            return;
        }
    }

    // gestures
    let press = ui.input(|i| i.pointer.press_origin());
    if resp.drag_started()
        && let Some(origin) = press
    {
        app.tl.drag = match hit(&view, &rows, origin, full, &x_of) {
            Hit::Ruler | Hit::Empty { .. } => {
                if origin.y < full.min.y + RULER_H {
                    app.stop_playback();
                    Some(Drag::Scrub)
                } else {
                    None
                }
            }
            Hit::Clip { clip, edge: Some(start_edge), .. } => Some(Drag::Trim { clip, start_edge }),
            Hit::Clip { clip, row, edge: None } => {
                if !view.selection.contains(&clip.id) {
                    let _ = app.session.execute("timeline.select", json!({"clips": [clip.id]}));
                }
                let grab = t_of(origin.x) - clip.start;
                Some(Drag::Move { clip, from_row: row, grab })
            }
            Hit::Nothing => None,
        };
    }
    let cur = resp.interact_pointer_pos().or(pointer);
    match (app.tl.drag.clone(), cur) {
        (Some(Drag::Scrub), Some(p)) => {
            let t = snap(&view, t_of(p.x), app, &x_of, None);
            app.set_playhead(t);
        }
        (Some(Drag::Move { clip, from_row, grab }), Some(p)) => {
            let row_i =
                row_at(&rows, p).filter(|i| rows.get(*i).is_some_and(|r| r.kind == rows.get(from_row).map_or(Kind::Video, |f| f.kind))).unwrap_or(from_row);
            let start = snap(&view, t_of(p.x) - grab, app, &x_of, Some(clip.duration())).max(0);
            if let Some(row) = rows.get(row_i) {
                let ghost = Rect::from_min_size(
                    pos2(x_of(start), row.rect.min.y + 3.0),
                    vec2((secs(clip.duration()) as f32 * app.tl.pps).max(4.0), row.rect.height() - 6.0),
                );
                clip_painter.rect_filled(ghost, CornerRadius::same(5), Color32::from_rgba_unmultiplied(139, 92, 246, 110));
                clip_painter.rect_stroke(ghost, CornerRadius::same(5), Stroke::new(2.0, Color32::WHITE), StrokeKind::Inside);
                if row.is_main() && app.cfg.magnet {
                    let x = x_of(start);
                    clip_painter.line_segment([pos2(x, row.rect.min.y), pos2(x, row.rect.max.y)], Stroke::new(2.0, theme::ACCENT));
                }
            }
            if resp.drag_stopped() {
                app.tl.drag = None;
                if let Some(row) = rows.get(row_i) {
                    finish_move(app, &view, &clip, &rows[from_row.min(rows.len().saturating_sub(1))].clone(), row, start);
                }
            }
        }
        (Some(Drag::Trim { clip, start_edge }), Some(p)) => {
            let edge_t = snap(&view, t_of(p.x), app, &x_of, None);
            let (a, b) =
                if start_edge { (edge_t.min(clip.end - view.frame_ticks()), clip.end) } else { (clip.start, edge_t.max(clip.start + view.frame_ticks())) };
            if let Some(row) = rows.iter().find(|r| clips_of(&view, r).iter().any(|c| c.id == clip.id)) {
                let g = Rect::from_min_max(pos2(x_of(a), row.rect.min.y + 3.0), pos2(x_of(b), row.rect.max.y - 3.0));
                clip_painter.rect_stroke(g, CornerRadius::same(5), Stroke::new(2.0, theme::ACCENT), StrokeKind::Inside);
            }
            if resp.drag_stopped() {
                app.tl.drag = None;
                let delta = if start_edge { a - clip.start } else { b - clip.end };
                if delta != 0 {
                    let ripple = app.cfg.magnet && view.main_track().is_some_and(|t| t.clips.iter().any(|c| c.id == clip.id));
                    app.run("timeline.trim", json!({"clip": clip.id, "edge": if start_edge { "in" } else { "out" }, "mode": if ripple { "ripple" } else { "regular" }, "delta": delta}));
                }
            }
        }
        _ => {}
    }
    if resp.drag_stopped() {
        app.tl.drag = None;
    }
    if resp.clicked()
        && let Some(p) = resp.interact_pointer_pos()
    {
        let add = ui.input(|i| i.modifiers.shift || i.modifiers.command);
        match hit(&view, &rows, p, full, &x_of) {
            Hit::Ruler => {
                app.stop_playback();
                app.set_playhead(t_of(p.x));
            }
            Hit::Clip { clip, .. } => {
                app.run("timeline.select", json!({"clips": [clip.id], "toggle": add}));
            }
            Hit::Empty { .. } => {
                app.stop_playback();
                app.set_playhead(t_of(p.x));
                if !add {
                    app.run("edit.deselectAll", json!({}));
                }
            }
            Hit::Nothing => {}
        }
    }
    if resp.double_clicked()
        && let Some(p) = resp.interact_pointer_pos()
        && let Hit::Clip { clip, .. } = hit(&view, &rows, p, full, &x_of)
    {
        app.set_playhead(clip.start);
    }

    // cursor feedback
    if let Some(p) = pointer.filter(|p| full.contains(*p))
        && app.tl.drag.is_none()
        && let Hit::Clip { edge: Some(_), .. } = hit(&view, &rows, p, full, &x_of)
    {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }

    // wheel: scroll, ⌘/Ctrl + wheel: zoom around the pointer
    if resp.hovered() {
        let (scroll, zoom_mod, zoom) = ui.input(|i| (i.smooth_scroll_delta, i.modifiers.command, i.zoom_delta()));
        if (zoom - 1.0).abs() > 1e-3 || (zoom_mod && scroll.y.abs() > 0.0) {
            let factor = if (zoom - 1.0).abs() > 1e-3 { zoom } else { (1.0 + scroll.y * 0.004).clamp(0.5, 2.0) };
            if let Some(p) = pointer {
                let at = (p.x - tracks_left) / app.tl.pps + app.tl.scroll;
                app.tl.zoom(factor);
                app.tl.scroll = (at - (p.x - tracks_left) / app.tl.pps).max(0.0);
            }
        } else if scroll.x.abs() > 0.0 || scroll.y.abs() > 0.0 {
            let d = if scroll.x.abs() > scroll.y.abs() { scroll.x } else { scroll.y };
            app.tl.scroll = (app.tl.scroll - d / app.tl.pps).max(0.0);
        }
    }

    // keep the playhead in view while playing
    let tl = &app.tl;
    let px = tracks_left + (secs(app.playhead()) as f32 - tl.scroll) * tl.pps;
    if app.player.playing && (px > full.max.x - 30.0 || px < tracks_left) {
        app.tl.scroll = (secs(app.playhead()) as f32 - tracks_w * 0.1 / app.tl.pps).max(0.0);
    }

    // headers + playhead on top
    let painter = ui.painter_at(full);
    let head_rect = Rect::from_min_max(full.min, pos2(tracks_left, full.max.y));
    painter.rect_filled(head_rect, CornerRadius { nw: theme::R_PANEL, ne: 0, sw: theme::R_PANEL, se: 0 }, theme::PANEL);
    painter.line_segment([pos2(tracks_left - 1.0, full.min.y), pos2(tracks_left - 1.0, full.max.y)], Stroke::new(1.0, theme::BORDER));
    let mut toggles: Vec<(u64, &str, bool)> = Vec::new();
    for r in &rows {
        let h = Rect::from_min_max(pos2(full.min.x, r.rect.min.y), pos2(tracks_left - 1.0, r.rect.max.y));
        let track = match r.kind {
            Kind::Video => view.video.get(r.index),
            Kind::Audio => view.audio.get(r.index),
        };
        let small = h.height() < 30.0;
        if r.is_main() && !small {
            // cover tile, as the first frame of the edit
            let cover = Rect::from_min_size(pos2(h.min.x + 8.0, h.min.y + 4.0), vec2(42.0, h.height() - 8.0));
            painter.rect_filled(cover, CornerRadius::same(5), theme::PANEL_3);
            if let Some(first) = view.main_track().and_then(|t| t.clips.first())
                && let Some(tex) = app.thumbs.get(&app.session, first.item, first.source_in)
            {
                egui::Image::new((tex.id(), cover.size())).corner_radius(CornerRadius::same(5)).paint_at(ui, cover);
                painter.rect_filled(
                    Rect::from_min_max(pos2(cover.min.x, cover.max.y - 14.0), cover.max),
                    CornerRadius { nw: 0, ne: 0, sw: 5, se: 5 },
                    Color32::from_black_alpha(150),
                );
            }
            painter.text(pos2(cover.center().x, cover.max.y - 7.0), Align2::CENTER_CENTER, "Cover", FontId::proportional(9.5), Color32::WHITE);
        } else {
            painter.text(h.left_center() + vec2(10.0, 0.0), Align2::LEFT_CENTER, r.label(), FontId::proportional(11.0), theme::TEXT_FAINT);
        }
        if let Some(t) = track {
            let icon_sz = if small { 16.0 } else { 20.0 };
            let mut x = h.max.x - 6.0 - icon_sz;
            let (vis_icon, vis_on, vis_key) = match r.kind {
                Kind::Video => (Icon::Eye, t.enabled, "enabled"),
                Kind::Audio => (Icon::Speaker, !t.muted, "muted"),
            };
            for (icon, on, key, tip) in [
                (vis_icon, vis_on, vis_key, if r.kind == Kind::Video { "Show / hide track" } else { "Mute / unmute track" }),
                (Icon::Lock, !t.locked, "locked", "Lock / unlock track"),
            ] {
                let ir = Rect::from_center_size(pos2(x + icon_sz / 2.0, h.center().y), vec2(icon_sz, icon_sz));
                let resp = ui.interact(ir, ui.scope_id().with(("trk", t.id, key)), Sense::click()).on_hover_text(tip);
                let c = if !on {
                    theme::ACCENT_HOVER
                } else if resp.hovered() {
                    theme::TEXT
                } else {
                    theme::TEXT_FAINT
                };
                icons::draw(&painter, ir.shrink(3.0), icon, c);
                if !on {
                    painter.line_segment([ir.left_bottom() + vec2(3.0, -3.0), ir.right_top() + vec2(-3.0, 3.0)], Stroke::new(1.5, theme::ACCENT_HOVER));
                }
                if resp.clicked() {
                    toggles.push((
                        t.id,
                        key,
                        match key {
                            "enabled" => !t.enabled,
                            "muted" => !t.muted,
                            _ => !t.locked,
                        },
                    ));
                }
                x -= icon_sz + 2.0;
            }
        }
    }
    for (id, key, value) in toggles {
        app.run("timeline.setTrack", json!({"track": id, key: value}));
    }
    let x = tracks_left + (secs(app.playhead()) as f32 - app.tl.scroll) * app.tl.pps;
    if x >= tracks_left && x <= full.max.x {
        painter.line_segment([pos2(x, full.min.y + 10.0), pos2(x, full.max.y - 2.0)], Stroke::new(1.5, theme::PLAYHEAD));
        let handle = Rect::from_center_size(pos2(x, full.min.y + 10.0), vec2(11.0, 15.0));
        painter.rect_filled(handle, CornerRadius::same(3), theme::PLAYHEAD);
    }
    if view.duration == 0 && rows.len() > 1 {
        if let Some(main) = rows.iter().find(|r| r.is_main()) {
            painter.text(main.rect.center(), Align2::CENTER_CENTER, "Drag media here to start", FontId::proportional(13.0), theme::TEXT_DIM);
        }
    }
}

fn toolbar(app: &mut MattCut, ui: &mut egui::Ui) {
    let r = ui.max_rect();
    egui::Frame::new().inner_margin(egui::Margin { left: 10, right: 10, top: 6, bottom: 4 }).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let can_undo = app.session.history.can_undo();
            let can_redo = app.session.history.can_redo();
            if ui.add_enabled_ui(can_undo, |ui| icons::button(ui, Icon::Undo, "Undo (⌘Z)", false)).inner.clicked() {
                app.run("edit.undo", json!({}));
            }
            if ui.add_enabled_ui(can_redo, |ui| icons::button(ui, Icon::Redo, "Redo (⇧⌘Z)", false)).inner.clicked() {
                app.run("edit.redo", json!({}));
            }
            ui.add_space(6.0);
            let (sr, _) = ui.allocate_exact_size(vec2(1.0, 18.0), Sense::hover());
            ui.painter().rect_filled(sr, CornerRadius::ZERO, theme::BORDER);
            ui.add_space(6.0);
            if icons::button(ui, Icon::Split, "Split at the playhead (⌘B)", false).clicked() {
                editor::split(app);
            }
            if icons::button(ui, Icon::TrimLeft, "Delete left of the playhead (Q)", false).clicked() {
                editor::trim_at_playhead(app, true);
            }
            if icons::button(ui, Icon::TrimRight, "Delete right of the playhead (W)", false).clicked() {
                editor::trim_at_playhead(app, false);
            }
            if icons::button(ui, Icon::Delete, "Delete (Del)", false).clicked() {
                editor::delete_selection(app);
            }
            if icons::button(ui, Icon::Duplicate, "Duplicate (⌘D)", false).clicked() {
                app.run("edit.duplicate", json!({}));
            }
            if icons::button(ui, Icon::Marker, "Add marker (M)", false).clicked() {
                app.run("markers.add", json!({"target": "program"}));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if icons::button(ui, Icon::Fit, "Fit the edit to the window (⇧Z)", false).clicked() {
                    app.tl.fit_next = true;
                }
                // zoom slider
                let mut z = app.tl.pps.log2();
                let resp = ui.add(egui::Slider::new(&mut z, 1.0..=11.2).show_value(false));
                if resp.changed() {
                    app.tl.pps = z.exp2();
                }
                if icons::button(ui, Icon::ZoomOut, "Zoom out (⌘-)", false).clicked() {
                    app.tl.zoom(0.8);
                }
                ui.add_space(6.0);
                let (sr, _) = ui.allocate_exact_size(vec2(1.0, 18.0), Sense::hover());
                ui.painter().rect_filled(sr, CornerRadius::ZERO, theme::BORDER);
                ui.add_space(6.0);
                if icons::button(ui, Icon::Snap, "Snapping (S)", app.session.state.snapping).clicked() {
                    app.run("sequence.snap", json!({}));
                }
                if icons::button(ui, Icon::Magnet, "Main-track magnet: main-track clips stay together with no gaps (N)", app.cfg.magnet).clicked() {
                    app.cfg.magnet = !app.cfg.magnet;
                    app.cfg.save();
                }
                if let Some(v) = &app.view
                    && !v.selection.is_empty()
                {
                    ui.add_space(8.0);
                    ui.label(RichText::new(format!("{} selected", v.selection.len())).size(11.5).color(theme::TEXT_FAINT));
                }
            });
        });
    });
    let y = ui.cursor().min.y;
    ui.painter().line_segment([pos2(r.min.x, y), pos2(r.max.x, y)], Stroke::new(1.0, theme::BORDER));
}

fn layout_rows(view: &SeqView, full: Rect, dragging: bool) -> Vec<Row> {
    let used = |tracks: &[crate::model::TrackView]| tracks.iter().rposition(|t| !t.clips.is_empty()).map_or(0, |i| i + 1);
    // the tracks in use, plus an empty lane above / below while something is being dragged
    let extra = usize::from(dragging);
    let v_count = (used(&view.video) + extra).max(1).min(view.video.len().max(1));
    let a_count = (used(&view.audio) + extra).max(1).min(view.audio.len().max(1));
    // shrink the lanes to fit the panel (main track keeps the most)
    let avail = (full.height() - RULER_H - 14.0).max(60.0);
    let want = (v_count - 1) as f32 * ROW_OVERLAY + ROW_MAIN + a_count as f32 * ROW_AUDIO + 10.0;
    let k = (avail / want).clamp(0.4, 1.0);
    let (ho, hm, ha) = (ROW_OVERLAY * k, (ROW_MAIN * k).max(34.0), ROW_AUDIO * k);
    let mut rows = Vec::new();
    let mut y = full.min.y + RULER_H + 4.0;
    let x0 = full.min.x + HEADER_W;
    for i in (1..v_count).rev() {
        rows.push(Row {
            kind: Kind::Video,
            index: i,
            track_id: view.video.get(i).map(|t| t.id),
            rect: Rect::from_min_size(pos2(x0, y), vec2(full.max.x - x0, ho)),
        });
        y += ho;
    }
    rows.push(Row {
        kind: Kind::Video,
        index: 0,
        track_id: view.video.first().map(|t| t.id),
        rect: Rect::from_min_size(pos2(x0, y + 3.0), vec2(full.max.x - x0, hm)),
    });
    y += hm + 8.0;
    for i in 0..a_count {
        rows.push(Row {
            kind: Kind::Audio,
            index: i,
            track_id: view.audio.get(i).map(|t| t.id),
            rect: Rect::from_min_size(pos2(x0, y), vec2(full.max.x - x0, ha)),
        });
        y += ha;
    }
    rows
}

fn clips_of<'a>(view: &'a SeqView, row: &Row) -> &'a [ClipView] {
    let tracks = match row.kind {
        Kind::Video => &view.video,
        Kind::Audio => &view.audio,
    };
    tracks.get(row.index).map_or(&[], |t| t.clips.as_slice())
}

fn clip_rect(row: &Row, c: &ClipView, x_of: &impl Fn(i64) -> f32) -> Rect {
    Rect::from_min_max(pos2(x_of(c.start), row.rect.min.y + 3.0), pos2(x_of(c.end).max(x_of(c.start) + 2.0), row.rect.max.y - 3.0))
}

fn paint_clip(p: &egui::Painter, r: Rect, c: &ClipView, row: &Row, selected: bool, faded: bool, media_kind: Option<&str>, tiles: &[(Rect, egui::TextureId)]) {
    let base = match (row.kind, media_kind) {
        (Kind::Audio, _) => theme::CLIP_AUDIO,
        (_, Some("Still" | "Image")) => theme::CLIP_STILL,
        (_, Some("Graphic" | "Synthetic")) | (_, None) => theme::CLIP_TEXT,
        _ if row.is_main() => theme::CLIP_MAIN,
        _ => theme::CLIP_VIDEO,
    };
    let fill = if faded { base.gamma_multiply(0.35) } else { base };
    p.rect_filled(r, CornerRadius::same(5), fill);
    if !tiles.is_empty() {
        let strip = p.with_clip_rect(r.shrink(1.0).intersect(p.clip_rect()));
        let tint = if faded { Color32::from_white_alpha(70) } else { Color32::WHITE };
        for (tr, tex) in tiles {
            strip.image(*tex, *tr, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), tint);
        }
        // a soft shade at the top for the label
        p.rect_filled(Rect::from_min_max(r.min, pos2(r.max.x, r.min.y + 17.0)), CornerRadius { nw: 5, ne: 5, sw: 0, se: 0 }, Color32::from_black_alpha(110));
    }
    if row.kind == Kind::Audio {
        // a stylised level line
        let mid = r.center().y;
        let mut x = r.min.x + 2.0;
        let mut k = (c.id as f32) * 1.7;
        while x < r.max.x - 2.0 {
            k += 0.9;
            let h = ((k.sin() * 0.5 + 0.5) * 0.6 + 0.15) * (r.height() * 0.4);
            p.line_segment([pos2(x, mid - h), pos2(x, mid + h)], Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 70)));
            x += 3.0;
        }
    }
    let text_r = r.shrink2(vec2(6.0, 3.0));
    if text_r.width() > 18.0 && r.height() >= 18.0 {
        let mut label = c.name.clone();
        if (c.speed - 1.0).abs() > 1e-3 {
            label = format!("{:.2}x  {label}", c.speed);
        }
        if c.reverse {
            label = format!("◀ {label}");
        }
        let galley = p.layout_no_wrap(label, FontId::proportional(11.5), Color32::WHITE);
        let clip_p = p.with_clip_rect(text_r.intersect(p.clip_rect()));
        clip_p.galley(text_r.min, galley, Color32::WHITE);
        if text_r.width() > 70.0 {
            clip_p.text(
                pos2(text_r.max.x, text_r.min.y),
                Align2::RIGHT_TOP,
                short_duration(c.duration()),
                FontId::proportional(10.5),
                Color32::from_rgba_unmultiplied(255, 255, 255, 170),
            );
        }
    }
    let effects: Vec<&str> = c
        .effects
        .iter()
        .filter_map(|e| e.get("effect").and_then(Value::as_str))
        .filter(|e| !matches!(*e, "motion" | "opacity" | "time_remap" | "volume" | "channel_volume" | "panner"))
        .collect();
    if !effects.is_empty() && r.width() > 30.0 {
        p.text(
            pos2(r.min.x + 6.0, r.max.y - 4.0),
            Align2::LEFT_BOTTOM,
            format!("★{}", effects.len()),
            FontId::proportional(10.0),
            Color32::from_rgb(0xff, 0xe0, 0x8a),
        );
    }
    if selected {
        p.rect_stroke(r, CornerRadius::same(5), Stroke::new(2.0, Color32::WHITE), StrokeKind::Inside);
    } else {
        p.rect_stroke(r, CornerRadius::same(5), Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 0, 0, 90)), StrokeKind::Inside);
    }
}

fn ruler(p: &egui::Painter, full: Rect, left: f32, scroll: f32, pps: f32, fps: f64) {
    let r = Rect::from_min_max(pos2(left, full.min.y), pos2(full.max.x, full.min.y + RULER_H));
    let steps = [0.1f32, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0];
    let major = steps.iter().copied().find(|s| s * pps >= 90.0).unwrap_or(600.0);
    let minor = major / 5.0;
    let start = (scroll / minor).floor() * minor;
    let mut t = start;
    let end = scroll + (full.max.x - left) / pps;
    while t <= end {
        let x = left + (t - scroll) * pps;
        let is_major = ((t / major).round() * major - t).abs() < minor * 0.1;
        let h = if is_major { 10.0 } else { 5.0 };
        p.line_segment([pos2(x, r.max.y - h), pos2(x, r.max.y)], Stroke::new(1.0, theme::TEXT_DIM));
        if is_major && t >= 0.0 {
            let label = if major < 1.0 {
                let f = ((t - t.floor()) * fps as f32).round() as i64;
                format!("{:02}:{:02}f", (t as i64) % 60, f)
            } else {
                format!("{:02}:{:02}", (t as i64) / 60, (t as i64) % 60)
            };
            p.text(pos2(x + 3.0, r.min.y + 3.0), Align2::LEFT_TOP, label, FontId::proportional(10.0), theme::TEXT_DIM);
        }
        t += minor;
    }
    p.line_segment([pos2(full.min.x, r.max.y), pos2(full.max.x, r.max.y)], Stroke::new(1.0, theme::BORDER));
}

fn row_at(rows: &[Row], p: Pos2) -> Option<usize> {
    rows.iter().position(|r| p.y >= r.rect.min.y - 3.0 && p.y < r.rect.max.y + 3.0)
}

fn drop_target(rows: &[Row], p: Pos2, audio: bool) -> Option<usize> {
    let want = if audio { Kind::Audio } else { Kind::Video };
    match row_at(rows, p) {
        Some(i) if rows.get(i).is_some_and(|r| r.kind == want) => Some(i),
        _ => rows.iter().position(|r| r.kind == want && (audio || r.is_main())),
    }
}

fn hit(view: &SeqView, rows: &[Row], p: Pos2, full: Rect, x_of: &impl Fn(i64) -> f32) -> Hit {
    if !full.contains(p) || p.x < full.min.x + HEADER_W {
        return Hit::Nothing;
    }
    if p.y < full.min.y + RULER_H {
        return Hit::Ruler;
    }
    let Some(ri) = row_at(rows, p) else {
        return Hit::Nothing;
    };
    let Some(row) = rows.get(ri) else {
        return Hit::Nothing;
    };
    for c in clips_of(view, row) {
        let r = clip_rect(row, c, x_of);
        if r.expand2(vec2(EDGE_GRAB * 0.5, 0.0)).contains(p) {
            let edge = if r.width() > EDGE_GRAB * 3.0 {
                if (p.x - r.min.x).abs() <= EDGE_GRAB {
                    Some(true)
                } else if (r.max.x - p.x).abs() <= EDGE_GRAB {
                    Some(false)
                } else {
                    None
                }
            } else {
                None
            };
            return Hit::Clip { clip: c.clone(), row: ri, edge };
        }
    }
    Hit::Empty { row: ri }
}

/// `t` pulled to the nearest clip edge or the playhead within a few pixels (when snapping is on).
/// With `len`, the end of a block of that length snaps too.
fn snap(view: &SeqView, t: i64, app: &MattCut, x_of: &impl Fn(i64) -> f32, len: Option<i64>) -> i64 {
    if !app.session.state.snapping {
        return t.max(0);
    }
    let mut targets = view.edges();
    targets.push(app.playhead());
    let mut best = (SNAP_PX, t);
    for e in targets {
        let d = (x_of(e) - x_of(t)).abs();
        if d < best.0 {
            best = (d, e);
        }
        if let Some(l) = len {
            let d2 = (x_of(e) - x_of(t + l)).abs();
            if d2 < best.0 {
                best = (d2, e - l);
            }
        }
    }
    best.1.max(0)
}

fn finish_move(app: &mut MattCut, view: &SeqView, clip: &ClipView, from: &Row, to: &Row, start: i64) {
    let Some(track_id) = to.track_id else { return };
    let magnet = app.cfg.magnet;
    let into_main = to.is_main() && magnet;
    let time = if into_main {
        // land on the nearest cut of the main track (without counting the clip itself)
        let edges: Vec<i64> =
            view.main_track().map(|t| t.clips.iter().filter(|c| c.id != clip.id).flat_map(|c| [c.start, c.end]).collect()).unwrap_or_default();
        edges.into_iter().chain([0]).min_by_key(|e| (e - start).abs()).unwrap_or(0)
    } else {
        start
    };
    if !into_main && time == clip.start && from.track_id == to.track_id {
        return;
    }
    let close_main = magnet && (from.is_main() || to.is_main());
    let main_id = view.main_track().map(|t| t.id);
    let id = clip.id;
    let r = app.session.edit_group("Move clip", |s| {
        s.execute("timeline.move", json!({"moves": [{"clip": id, "track": track_id, "time": time}], "insert": into_main}))?;
        if close_main && let Some(main) = main_id {
            close_gaps(s, main)?;
        }
        Ok(())
    });
    if let Err(e) = r {
        app.error(e.to_string());
    }
}

/// Main-track magnet: pull every clip on the main track left so there are no gaps.
fn close_gaps(s: &mut filmcraft_engine::Session, track: u64) -> filmcraft_engine::Result<()> {
    let v = s.execute("sequence.inspect", json!({}))?;
    let mut clips: Vec<(u64, i64, i64)> = v
        .get("video")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|t| t.get("id").and_then(Value::as_u64) == Some(track))
        .flat_map(|t| t.get("items").and_then(Value::as_array).cloned().unwrap_or_default())
        .filter_map(|c| Some((c.get("clip")?.as_u64()?, c.get("start")?.as_i64()?, c.get("end")?.as_i64()?)))
        .collect();
    clips.sort_by_key(|c| c.1);
    let mut at = 0;
    for (id, start, end) in clips {
        if start != at {
            s.execute("timeline.move", json!({"moves": [{"clip": id, "track": track, "time": at}], "insert": false}))?;
        }
        at += end - start;
    }
    Ok(())
}
