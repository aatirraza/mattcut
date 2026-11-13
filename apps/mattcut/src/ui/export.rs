//! Export: name, folder, resolution, frame rate, format and quality, with progress.

use std::path::PathBuf;

use egui::{RichText, vec2};
use serde_json::{Value, json};

use super::{MattCut, theme};

const FORMATS: [(&str, &str, &str); 6] = [
    ("MP4 (H.264)", "h264", "mp4"),
    ("MP4 (HEVC / H.265)", "hevc", "mp4"),
    ("MOV (ProRes)", "prores", "mov"),
    ("GIF", "gif", "gif"),
    ("Audio only (AAC)", "aac", "m4a"),
    ("Audio only (WAV)", "wav", "wav"),
];

pub struct ExportDialog {
    name: String,
    dir: PathBuf,
    /// Short side; 0 = the sequence's own size.
    res: u32,
    fps: f64,
    format: usize,
    /// Kbit/s for H.264/HEVC; 0 = automatic.
    quality: usize,
    job: Option<u64>,
    done: Option<Result<PathBuf, String>>,
    /// Export only this span of the timeline (ticks), e.g. the selected clips.
    range: Option<(i64, i64)>,
}

const QUALITY: [(&str, f64); 3] = [("Recommended", 1.0), ("Higher", 1.8), ("Smaller file", 0.55)];

impl ExportDialog {
    pub fn new(app: &MattCut) -> Self {
        let name = app.doc.as_ref().map_or_else(|| "MattCut export".into(), |d| d.name.clone());
        let fps = app.view.as_ref().map_or(30.0, |v| (v.fps * 1000.0).round() / 1000.0);
        Self { name, dir: app.cfg.export_dir.clone(), res: 0, fps, format: 0, quality: 0, job: None, done: None, range: None }
    }

    /// Export just `start..end` of the timeline ("Export selected clips").
    pub fn for_range(app: &MattCut, start: i64, end: i64) -> Self {
        let mut d = Self::new(app);
        d.name = format!("{} (clip)", d.name);
        d.range = Some((start, end));
        d
    }
}

fn size_for(app: &MattCut, res: u32) -> (u32, u32) {
    let (w, h) = app.view.as_ref().map_or((1920, 1080), |v| (v.width, v.height));
    if res == 0 {
        return (w, h);
    }
    let short = w.min(h).max(1);
    let k = f64::from(res) / f64::from(short);
    let even = |x: f64| ((x / 2.0).round() as u32).max(1) * 2;
    (even(f64::from(w) * k), even(f64::from(h) * k))
}

/// Returns false when the dialog should close.
pub fn show(app: &mut MattCut, ctx: &egui::Context, d: &mut ExportDialog) -> bool {
    let mut keep = true;
    // job progress
    let job = d.job.and_then(|id| app.session.jobs.iter().find(|j| j.id == id)).map(filmcraft_engine::Job::to_json);
    if let Some(j) = &job
        && j.get("finished").and_then(Value::as_bool) == Some(true)
        && d.done.is_none()
    {
        let err = j.get("result").and_then(|r| r.get("error")).and_then(Value::as_str).map(str::to_string);
        let path = d.dir.join(format!("{}.{}", d.name, FORMATS.get(d.format).map_or("mp4", |f| f.2)));
        d.done = Some(match err {
            Some(e) => Err(e),
            None => Ok(path),
        });
    }
    egui::Modal::new(egui::Id::unique("export")).show(ctx, |ui| {
        ui.set_width(460.0);
        ui.heading(if d.range.is_some() { "Export selected clips" } else { "Export" });
        if let Some((a, b)) = d.range {
            ui.label(
                RichText::new(format!("{} to {} ({})", crate::model::short_duration(a), crate::model::short_duration(b), crate::model::short_duration(b - a)))
                    .small()
                    .color(theme::TEXT_DIM),
            );
        }
        ui.add_space(8.0);
        if let Some(done) = &d.done {
            match done {
                Ok(path) => {
                    ui.label(RichText::new("✔ Export finished").color(theme::OK).size(16.0));
                    ui.label(RichText::new(path.display().to_string()).small().color(theme::TEXT_DIM));
                    ui.horizontal(|ui| {
                        if ui.button(if cfg!(target_os = "macos") { "Show in Finder" } else { "Show in folder" }).clicked() {
                            reveal(path);
                        }
                        if ui.add(theme::primary("Done")).clicked() {
                            keep = false;
                        }
                    });
                }
                Err(e) => {
                    ui.colored_label(theme::ERR, format!("Export failed: {e}"));
                    if ui.button("Close").clicked() {
                        keep = false;
                    }
                }
            }
            return;
        }
        if let Some(j) = &job {
            let frac = j.get("progress").and_then(Value::as_f64).unwrap_or(0.0) as f32;
            let eta = j.get("etaSeconds").and_then(Value::as_f64).map(|s| format!(" — about {}s left", s.round() as i64)).unwrap_or_default();
            ui.label(format!("Exporting \"{}\"{eta}", d.name));
            ui.add(egui::ProgressBar::new(frac).show_percentage().desired_width(f32::INFINITY));
            if ui.button("Cancel").clicked()
                && let Some(id) = d.job
            {
                let _ = app.session.execute("jobs.cancel", json!({"job": id}));
                keep = false;
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
            return;
        }
        egui::Grid::new("export-grid").num_columns(2).spacing(vec2(12.0, 8.0)).show(ui, |ui| {
            ui.label("Name");
            ui.add(egui::TextEdit::singleline(&mut d.name).desired_width(300.0));
            ui.end_row();
            ui.label("Save to");
            ui.horizontal(|ui| {
                let shown: String = d.dir.display().to_string();
                ui.label(RichText::new(shown).small());
                if ui.small_button("Change…").clicked()
                    && let Some(p) = rfd::FileDialog::new().set_directory(&d.dir).pick_folder()
                {
                    d.dir = p;
                }
            });
            ui.end_row();
            ui.label("Format");
            egui::ComboBox::new("ex-format", "").selected_text(FORMATS.get(d.format).map_or("", |f| f.0)).show_ui(ui, |ui| {
                for (i, f) in FORMATS.iter().enumerate() {
                    ui.selectable_value(&mut d.format, i, f.0);
                }
            });
            ui.end_row();
            let audio_only = d.format >= 4;
            if !audio_only {
                ui.label("Resolution");
                let (w, h) = size_for(app, d.res);
                egui::ComboBox::new("ex-res", "")
                    .selected_text(if d.res == 0 { format!("Project ({w}×{h})") } else { format!("{}p ({w}×{h})", d.res) })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut d.res, 0, "Project size");
                        for r in [480, 720, 1080, 1440, 2160] {
                            ui.selectable_value(&mut d.res, r, format!("{r}p"));
                        }
                    });
                ui.end_row();
                ui.label("Frame rate");
                egui::ComboBox::new("ex-fps", "").selected_text(format!("{}", d.fps)).show_ui(ui, |ui| {
                    for f in [23.976, 24.0, 25.0, 29.97, 30.0, 50.0, 60.0] {
                        ui.selectable_value(&mut d.fps, f, format!("{f}"));
                    }
                });
                ui.end_row();
                if d.format <= 1 {
                    ui.label("Quality");
                    egui::ComboBox::new("ex-q", "").selected_text(QUALITY.get(d.quality).map_or("", |q| q.0)).show_ui(ui, |ui| {
                        for (i, q) in QUALITY.iter().enumerate() {
                            ui.selectable_value(&mut d.quality, i, q.0);
                        }
                    });
                    ui.end_row();
                }
            }
        });
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let ok_name = !d.name.trim().is_empty() && !d.name.contains(['/', '\\', ':']);
            if ui.add_enabled(ok_name, theme::primary("Export")).clicked() {
                start(app, d);
            }
            if ui.button("Cancel").clicked() {
                keep = false;
            }
        });
    });
    keep
}

fn start(app: &mut MattCut, d: &mut ExportDialog) {
    let Some((_, fmt, ext)) = FORMATS.get(d.format).copied() else {
        return;
    };
    if let Err(e) = std::fs::create_dir_all(&d.dir) {
        app.error(format!("{}: {e}", d.dir.display()));
        return;
    }
    let path = d.dir.join(format!("{}.{ext}", d.name.trim()));
    let mut params = json!({"path": path.to_string_lossy(), "format": fmt, "wait": false});
    if let Some((a, b)) = d.range {
        params["range"] = json!("custom");
        params["startSeconds"] = json!(crate::model::secs(a));
        params["endSeconds"] = json!(crate::model::secs(b));
    }
    if d.format < 4 {
        let (w, h) = size_for(app, d.res);
        params["width"] = json!(w);
        params["height"] = json!(h);
        params["fps"] = json!(d.fps);
        if d.format <= 1 {
            // about 0.1 bit per pixel per frame for H.264, less for HEVC
            let bpp = if d.format == 0 { 0.1 } else { 0.06 };
            let k = QUALITY.get(d.quality).map_or(1.0, |q| q.1);
            let kbps = (f64::from(w) * f64::from(h) * d.fps * bpp * k / 1000.0).round().clamp(500.0, 200_000.0);
            params["bitrateKbps"] = json!(kbps as u64);
        }
    }
    if let Some(r) = app.run("file.exportMedia", params) {
        match r.get("job").and_then(Value::as_u64) {
            Some(job) => d.job = Some(job),
            None => d.done = Some(Ok(path)),
        }
    }
}

fn reveal(path: &std::path::Path) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(path.parent().unwrap_or(path)).spawn();
}
