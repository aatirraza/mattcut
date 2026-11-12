//! MattCut — the office video editor.
//!
//! ```text
//! mattcut                       start (signs in to the office server on this network)
//! mattcut --server ADDR         use this office server (e.g. 192.168.1.20:7700)
//! mattcut --control PORT        first port to try for the local MCP control channel (default 9877)
//! ```

mod audio;
mod config;
mod control;
mod model;
mod office_link;
mod preview;
mod thumbs;
mod ui;

/// Exports composite on the GPU where they can (FilmCraft's off-screen compositor), falling back
/// to the CPU renderer.
struct GpuFrameRenderer(filmcraft_gpu::ExportRenderer);

impl filmcraft_export::FrameRenderer for GpuFrameRenderer {
    fn render(
        &mut self,
        project: &filmcraft_project::Project,
        seq: filmcraft_project::ItemId,
        t: filmcraft_time::Tick,
        opts: filmcraft_render::RenderOptions,
        sources: &dyn filmcraft_render::SourceProvider,
    ) -> Option<filmcraft_render::Image> {
        self.0.render(project, seq, t, opts, sources)
    }
}

fn app_icon() -> egui::IconData {
    // the Mattrics logo, with rounded corners like a native app icon
    match ui::theme::logo_rgba() {
        Some((mut rgba, w, h)) => {
            let r = (w.min(h) as f32) * 0.22;
            for y in 0..h {
                for x in 0..w {
                    let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                    let dx = (r - fx).max(fx - (w as f32 - r)).max(0.0);
                    let dy = (r - fy).max(fy - (h as f32 - r)).max(0.0);
                    let d = (dx * dx + dy * dy).sqrt() - r;
                    if d > 0.0
                        && let Some(a) = rgba.get_mut(((y * w + x) * 4 + 3) as usize)
                    {
                        *a = ((1.0 - d.min(1.0)) * f32::from(*a)) as u8;
                    }
                }
            }
            egui::IconData { rgba, width: w, height: h }
        }
        None => egui::IconData::default(),
    }
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opt = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("mattcut [--server ADDR] [--control PORT]");
        return Ok(());
    }
    let control = opt("--control").and_then(|p| p.parse().ok());
    let server = opt("--server");

    // OS hardware decoders (VideoToolbox / Media Foundation) in front of the built-in ones
    let hw = filmcraft_platform::register();
    eprintln!("mattcut: hardware decoding {hw:?}");
    filmcraft_export::register_frame_renderer(|| {
        filmcraft_gpu::ExportRenderer::new().map(|r| Box::new(GpuFrameRenderer(r)) as Box<dyn filmcraft_export::FrameRenderer>)
    });

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("MattCut")
            .with_app_id("mattcut")
            .with_inner_size([1480.0, 920.0])
            .with_min_inner_size([1024.0, 640.0])
            .with_drag_and_drop(true)
            .with_icon(app_icon()),
        ..Default::default()
    };
    eframe::run_native("MattCut", options, Box::new(move |cc| Ok(Box::new(ui::MattCut::new(cc, control, server)))))
}
