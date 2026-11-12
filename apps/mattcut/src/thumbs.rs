//! Thumbnails for the media library and the timeline filmstrips, decoded on a worker thread and
//! kept as textures (one per item and whole second).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use filmcraft_engine::{MediaPool, Services, Session};
use filmcraft_project::{ItemId, Project};
use filmcraft_time::{TICKS_PER_SECOND, Tick};

const THUMB_H: u32 = 96;
const MAX_TEXTURES: usize = 600;

type Key = (u64, i64);

struct Req {
    key: Key,
    project: Arc<Project>,
    media: Arc<MediaPool>,
    services: Arc<dyn Services>,
}

pub struct Thumbs {
    tx: Sender<Req>,
    done: Receiver<(Key, Option<egui::ColorImage>)>,
    cache: HashMap<Key, Option<egui::TextureHandle>>,
    pending: HashSet<Key>,
    epoch: u64,
}

impl Thumbs {
    pub fn start(ctx: egui::Context) -> Self {
        let (tx, rx) = channel::<Req>();
        let (dtx, done) = channel();
        let spawned = std::thread::Builder::new().name("thumbs".into()).spawn(move || {
            while let Ok(req) = rx.recv() {
                let img = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| decode(&req))).ok().flatten();
                if dtx.send((req.key, img)).is_err() {
                    return;
                }
                ctx.request_repaint();
            }
        });
        if let Err(e) = spawned {
            log::warn!("thumbnail thread: {e}");
        }
        Self { tx, done, cache: HashMap::new(), pending: HashSet::new(), epoch: u64::MAX }
    }

    /// Collect finished thumbnails; forget everything when another project is opened.
    pub fn poll(&mut self, ctx: &egui::Context, session: &Session) {
        if self.epoch != session.document_epoch {
            self.epoch = session.document_epoch;
            self.cache.clear();
            self.pending.clear();
        }
        for (key, img) in self.done.try_iter() {
            self.pending.remove(&key);
            if self.cache.len() > MAX_TEXTURES {
                self.cache.clear();
            }
            let tex = img.map(|i| ctx.load_texture(format!("thumb-{}-{}", key.0, key.1), i, egui::TextureOptions::LINEAR));
            self.cache.insert(key, tex);
        }
    }

    /// The frame of `item` at media time `t` (rounded to the second), asking for it if needed.
    pub fn get(&mut self, session: &Session, item: u64, t: i64) -> Option<egui::TextureHandle> {
        let key = (item, t.max(0) / TICKS_PER_SECOND);
        if let Some(t) = self.cache.get(&key) {
            return t.clone();
        }
        if self.pending.insert(key) {
            let _ = self.tx.send(Req { key, project: session.project.clone(), media: session.media.clone(), services: session.services.clone() });
        }
        None
    }
}

fn decode(req: &Req) -> Option<egui::ColorImage> {
    let source = req.media.source_for(&req.project, ItemId(req.key.0), &*req.services)?;
    let info = source.info();
    let (w, h) = info.video.as_ref().map(|v| (v.width.max(1), v.height.max(1)))?;
    let scale = (THUMB_H as f32 / h as f32).min(1.0);
    let frame = source.video_frame(filmcraft_media::FrameRequest { time: Tick(req.key.1 * TICKS_PER_SECOND + TICKS_PER_SECOND / 10), scale }).ok()?;
    let rgba = frame.to_rgba8();
    let img = image::RgbaImage::from_raw(frame.width, frame.height, rgba)?;
    let th = THUMB_H.min(frame.height.max(1));
    let tw = ((u64::from(w) * u64::from(th)) / u64::from(h)).clamp(1, 1024) as u32;
    let small = image::imageops::resize(&img, tw, th, image::imageops::FilterType::Triangle);
    Some(egui::ColorImage::from_rgba_unmultiplied([tw as usize, th as usize], small.as_raw()))
}
