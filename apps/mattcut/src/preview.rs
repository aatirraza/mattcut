//! Player frames, rendered on a worker thread so the UI never waits for decoding. Only the newest
//! request is rendered; older ones are dropped.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};

use filmcraft_engine::{MediaPool, Services, Session};
use filmcraft_project::{ItemId, Project};
use filmcraft_time::Tick;

struct Req {
    project: Arc<Project>,
    media: Arc<MediaPool>,
    services: Arc<dyn Services>,
    seq: ItemId,
    t: Tick,
    scale: f32,
    key: Key,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Key {
    pub revision: u64,
    pub epoch: u64,
    pub t: i64,
    pub scale_milli: u32,
}

pub struct Frame {
    pub key: Key,
    pub image: egui::ColorImage,
}

pub struct Preview {
    tx: Sender<Req>,
    out: Arc<Mutex<Option<Frame>>>,
    requested: Option<Key>,
}

impl Preview {
    pub fn start(ctx: egui::Context) -> Self {
        let (tx, rx) = channel::<Req>();
        let out = Arc::new(Mutex::new(None));
        let out2 = Arc::clone(&out);
        let spawned = std::thread::Builder::new().name("preview".into()).spawn(move || worker(rx, out2, ctx));
        if let Err(e) = spawned {
            log::error!("preview thread: {e}");
        }
        Self { tx, out, requested: None }
    }

    /// Ask for the frame at `t` unless it is the one already asked for.
    pub fn request(&mut self, session: &Session, t: Tick, scale: f32) {
        let Some(seq) = session.state.active_sequence else {
            return;
        };
        let key = Key { revision: session.revision, epoch: session.document_epoch, t: t.0, scale_milli: (scale * 1000.0) as u32 };
        if self.requested == Some(key) {
            return;
        }
        self.requested = Some(key);
        let _ = self.tx.send(Req { project: session.project.clone(), media: session.media.clone(), services: session.services.clone(), seq, t, scale, key });
    }

    /// Force the next request through (after a project is replaced).
    pub fn invalidate(&mut self) {
        self.requested = None;
    }

    /// A newly rendered frame, if one finished since the last call.
    pub fn take(&self) -> Option<Frame> {
        self.out.lock().unwrap_or_else(PoisonError::into_inner).take()
    }
}

fn worker(rx: Receiver<Req>, out: Arc<Mutex<Option<Frame>>>, ctx: egui::Context) {
    while let Ok(mut req) = rx.recv() {
        // skip to the newest request
        while let Ok(newer) = rx.try_recv() {
            req = newer;
        }
        let rendered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let provider = req.media.provider(req.project.clone(), req.services.clone());
            let opts = filmcraft_render::RenderOptions { scale: req.scale, captions: true, ..Default::default() };
            filmcraft_render::render_sequence(&req.project, req.seq, req.t, opts, &provider)
        }));
        let Ok(img) = rendered else {
            eprintln!("mattcut: preview render panicked at {:?}", req.t);
            continue;
        };
        if img.w == 0 || img.h == 0 {
            eprintln!("mattcut: preview render returned an empty frame (scale {})", req.scale);
            continue;
        }
        if std::env::var_os("MATTCUT_DEBUG").is_some() {
            eprintln!("mattcut: preview {}x{} at {:?}", img.w, img.h, req.t);
        }
        let rgba = img.over_black_rgba8();
        let image = egui::ColorImage::from_rgba_unmultiplied([img.w, img.h], &rgba);
        *out.lock().unwrap_or_else(PoisonError::into_inner) = Some(Frame { key: req.key, image });
        ctx.request_repaint();
    }
}

/// Render the frame at the playhead synchronously, as PNG bytes no larger than `max_side`.
pub fn snapshot_png(session: &Session, max_side: u32) -> Result<Vec<u8>, String> {
    let seq = session.active_sequence().ok_or("no sequence open")?;
    let (w, h) = (seq.settings.width.max(1), seq.settings.height.max(1));
    let scale = (max_side as f32 / w.max(h) as f32).min(1.0);
    let img = session.try_render_program_at(scale, session.playhead()).map_err(|e| e.to_string())?;
    encode_png(img.w as u32, img.h as u32, img.over_black_rgba8())
}

pub fn encode_png(w: u32, h: u32, rgba: Vec<u8>) -> Result<Vec<u8>, String> {
    let img = image::RgbaImage::from_raw(w, h, rgba).ok_or("bad image size")?;
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}
