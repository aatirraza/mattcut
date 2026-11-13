//! The timeline's right-click menu: on a clip (copy / paste / attributes, extract audio, split
//! scenes, compound clip, group, deactivate, replace, show the file, export) or on empty space
//! (paste, select all).

use egui::{Button, RichText};
use serde_json::json;

use super::{MattCut, editor, export, theme};
use crate::model::{ClipView, Kind};

/// What the menu was opened on.
#[derive(Clone, Debug)]
pub enum Target {
    Clip { clip: ClipView, kind: Kind },
    Empty,
}

/// "⌘C" on macOS, "Ctrl+C" elsewhere (`⌘` / `⇧` / `⌥` in `mac` are spelled out).
fn sc(mac: &str) -> String {
    if cfg!(target_os = "macos") {
        mac.to_string()
    } else {
        let mut parts = Vec::new();
        let mut key = String::new();
        for ch in mac.chars() {
            match ch {
                '⌘' => parts.push("Ctrl"),
                '⇧' => parts.push("Shift"),
                '⌥' => parts.push("Alt"),
                c => key.push(c),
            }
        }
        if key == "⌫" {
            key = "Del".into();
        }
        parts.push(&key);
        parts.join("+")
    }
}

fn item(ui: &mut egui::Ui, enabled: bool, label: &str, shortcut: &str) -> bool {
    let mut b = Button::new(label);
    if !shortcut.is_empty() {
        b = b.shortcut_text(sc(shortcut));
    }
    let clicked = ui.add_enabled(enabled, b).clicked();
    if clicked {
        ui.close();
    }
    clicked
}

pub fn show(app: &mut MattCut, ui: &mut egui::Ui, target: &Target) {
    ui.set_min_width(250.0);
    match target {
        Target::Empty => {
            if item(ui, app.session.is_enabled("edit.paste"), "Paste", "⌘V") {
                app.run("edit.paste", json!({}));
            }
            if item(ui, true, "Select all", "⌘A") {
                app.run("edit.selectAll", json!({}));
            }
        }
        Target::Clip { clip, kind } => clip_items(app, ui, clip, *kind),
    }
}

fn clip_items(app: &mut MattCut, ui: &mut egui::Ui, clip: &ClipView, kind: Kind) {
    let sel = app.selection();
    let multi = sel.len() > 1;
    let partner = linked_partner(app, clip);
    let t = app.playhead();

    // ---- clipboard -----------------------------------------------------------------------------
    if item(ui, true, "Copy", "⌘C") {
        app.run("edit.copy", json!({}));
    }
    if item(ui, true, "Cut", "⌘X") {
        app.run("edit.cut", json!({}));
    }
    if item(ui, app.session.is_enabled("edit.paste"), "Paste", "⌘V") {
        app.run("edit.paste", json!({}));
    }
    if item(ui, true, "Duplicate", "⌘D") {
        app.run("edit.duplicate", json!({}));
    }
    if item(ui, true, "Delete", "⌫") {
        editor::delete_selection(app);
    }
    ui.separator();

    // ---- attributes ----------------------------------------------------------------------------
    if item(ui, true, "Copy attributes", "⌥⌘C") {
        copy_attributes(app);
    }
    if item(ui, app.session.is_enabled("edit.pasteAttributes"), "Paste attributes", "⌥⌘V") {
        paste_attributes(app);
    }
    if item(ui, true, "Reset attributes", "")
        && let Some(r) = app.run("edit.removeAttributes", json!({"clips": sel}))
    {
        let n = r.get("clips").and_then(serde_json::Value::as_u64).unwrap_or(0);
        app.toast(if n == 0 { "Nothing to reset: the clip has no changes".to_string() } else { format!("Reset {n} clip{}", if n == 1 { "" } else { "s" }) });
    }
    ui.separator();

    // ---- cutting -------------------------------------------------------------------------------
    let inside = t > clip.start && t < clip.end;
    let split = item(ui, inside, "Split at playhead", "⌘B");
    if split {
        editor::split(app);
    }
    let is_video = kind == Kind::Video && !clip.is_graphic;
    if item(ui, is_video, "Split scenes", "") {
        split_scenes(app, clip);
    }
    if item(ui, is_video && inside, "Freeze frame", "") && app.run("clip.insertFrameHoldSegment", json!({"clip": clip.id, "time": t, "seconds": 3.0})).is_some()
    {
        app.toast("Added a 3-second freeze frame at the playhead (drag its edge to change the length)");
    }
    ui.separator();

    // ---- audio ---------------------------------------------------------------------------------
    match (kind, &partner) {
        (Kind::Video, Some(audio)) => {
            if item(ui, true, "Extract audio", "⌥⌘X") {
                extract_audio(app, clip, audio);
            }
        }
        (Kind::Video, None) => {
            ui.add_enabled(false, Button::new("Extract audio")).on_disabled_hover_text(if clip.is_graphic {
                "Text and graphics have no sound"
            } else {
                "This clip has no sound, or its audio was already extracted"
            });
        }
        (Kind::Audio, Some(video)) => {
            if item(ui, true, "Detach from video", "⌥⌘X") {
                extract_audio(app, video, clip);
            }
        }
        (Kind::Audio, None) => {}
    }
    super::voice::menu(app, ui, clip, kind, partner.as_ref());
    ui.separator();

    // ---- structure -----------------------------------------------------------------------------
    if item(ui, true, "Create compound clip", "⌥G") {
        compound(app);
    }
    if item(ui, multi, "Group", "⌘G") {
        app.run("clip.group", json!({}));
    }
    if item(ui, true, "Ungroup", "⇧⌘G") {
        app.run("clip.ungroup", json!({}));
    }
    let label = if clip.enabled { "Deactivate clip" } else { "Activate clip" };
    if item(ui, true, label, "V") {
        toggle_enabled(app);
    }
    ui.separator();

    // ---- media ---------------------------------------------------------------------------------
    let candidates: Vec<(u64, String)> =
        app.media.iter().filter(|m| m.id != clip.item && !m.is_sequence() && m.is_audio() == (kind == Kind::Audio)).map(|m| (m.id, m.name.clone())).collect();
    ui.add_enabled_ui(!clip.is_graphic, |ui| {
        ui.menu_button("Replace clip", |ui| {
            ui.set_min_width(220.0);
            ui.label(RichText::new("Keeps the clip's place, length and effects").small().color(theme::TEXT_DIM));
            for (id, name) in &candidates {
                if ui.button(name).clicked() {
                    replace(app, clip, *id);
                    ui.close();
                }
            }
            if !candidates.is_empty() {
                ui.separator();
            }
            if ui.button("From a file…").clicked() {
                ui.close();
                replace_from_file(app, clip);
            }
        });
    });
    let reveal = if cfg!(target_os = "macos") { "Show in Finder" } else { "Show in File Explorer" };
    if item(ui, !clip.is_graphic, reveal, "") {
        app.run("file.reveal", json!({"items": [clip.item]}));
    }
    if item(ui, true, if multi { "Export selected clips…" } else { "Export this clip…" }, "") {
        export_selection(app);
    }
}

/// The audio clip linked to a video clip, or the video clip linked to an audio clip.
fn linked_partner(app: &MattCut, clip: &ClipView) -> Option<ClipView> {
    let link = clip.link?;
    let view = app.view.as_ref()?;
    view.video.iter().chain(view.audio.iter()).flat_map(|t| t.clips.iter()).find(|c| c.id != clip.id && c.link == Some(link)).cloned()
}

/// Detach a video clip's sound so it can be moved, trimmed or deleted on its own.
pub fn extract_audio(app: &mut MattCut, video: &ClipView, audio: &ClipView) {
    if app.run("clip.link", json!({"clips": [video.id, audio.id]})).is_some() {
        app.run("timeline.select", json!({"clips": [audio.id], "linked": false}));
        app.toast("Audio extracted: it's now its own clip on the audio track");
    }
}

pub fn copy_attributes(app: &mut MattCut) {
    if app.run("edit.copy", json!({})).is_some() {
        app.toast("Attributes copied: select other clips and choose Paste attributes");
    }
}

pub fn paste_attributes(app: &mut MattCut) {
    let sel = app.selection();
    if sel.is_empty() {
        return;
    }
    if let Some(r) = app.run("edit.pasteAttributes", json!({"clips": sel})) {
        let n = r.get("clips").and_then(serde_json::Value::as_u64).unwrap_or(0);
        app.toast(if n == 0 {
            "Nothing to paste: the copied clip has no effects or changes".to_string()
        } else {
            format!("Attributes pasted onto {n} clip{}", if n == 1 { "" } else { "s" })
        });
    }
}

pub fn compound(app: &mut MattCut) {
    if app.selection().is_empty() {
        return;
    }
    if app.run("clip.nest", json!({})).is_some() {
        app.toast("Compound clip created");
    }
}

pub fn toggle_enabled(app: &mut MattCut) {
    let sel = app.selection();
    if !sel.is_empty() {
        app.run("clip.enable", json!({"clips": sel}));
    }
}

fn split_scenes(app: &mut MattCut, clip: &ClipView) {
    if app.run("clip.sceneEditDetection", json!({"clips": [clip.id], "applyCuts": true})).is_some() {
        app.toast("Finding scene changes… the clip is split at each one when it's done");
    }
}

fn replace(app: &mut MattCut, clip: &ClipView, item: u64) {
    if app.run("clip.replaceFromBin", json!({"clips": [clip.id], "item": item})).is_some() {
        app.toast("Clip replaced");
    }
}

fn replace_from_file(app: &mut MattCut, clip: &ClipView) {
    let Some(path) = rfd::FileDialog::new().set_title("Replace clip with…").pick_file() else { return };
    let before: Vec<u64> = app.media.iter().map(|m| m.id).collect();
    app.import_paths(vec![path]);
    app.refresh_views();
    let Some(new) = app.media.iter().find(|m| !before.contains(&m.id)).map(|m| m.id) else {
        app.error("That file couldn't be imported");
        return;
    };
    replace(app, clip, new);
}

/// Export the span the selected clips cover.
pub fn export_selection(app: &mut MattCut) {
    let Some(view) = app.view.clone() else { return };
    let sel = app.selection();
    let clips: Vec<&ClipView> = view.video.iter().chain(view.audio.iter()).flat_map(|t| t.clips.iter()).filter(|c| sel.contains(&c.id)).collect();
    let (Some(start), Some(end)) = (clips.iter().map(|c| c.start).min(), clips.iter().map(|c| c.end).max()) else { return };
    app.export = Some(export::ExportDialog::for_range(app, start, end));
}
