//! What the UI draws, read from the engine's `sequence.inspect` / `project.inspect` (rebuilt when
//! the project revision changes).

use filmcraft_engine::Session;
use filmcraft_time::TICKS_PER_SECOND;
use serde_json::{Value, json};

pub const TPS: f64 = TICKS_PER_SECOND as f64;

pub fn secs(ticks: i64) -> f64 {
    ticks as f64 / TPS
}

pub fn ticks(secs: f64) -> i64 {
    (secs * TPS).round() as i64
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Video,
    Audio,
}

#[derive(Clone, Debug)]
pub struct ClipView {
    pub id: u64,
    pub item: u64,
    pub name: String,
    pub start: i64,
    pub end: i64,
    pub source_in: i64,
    pub speed: f64,
    pub reverse: bool,
    pub link: Option<u64>,
    /// Off ("Deactivate clip"): kept on the timeline but not seen or heard.
    pub enabled: bool,
    pub effects: Vec<Value>,
    pub is_graphic: bool,
}

impl ClipView {
    pub fn duration(&self) -> i64 {
        self.end - self.start
    }

    /// The value of `effect.param` as the engine prints it (`Float(100.0)`, `Vec2(Vec2 { x: 1.0, y: 2.0 })`).
    fn raw_param(&self, effect: &str, param: &str) -> Option<String> {
        let e = self.effects.iter().find(|e| e.get("effect").and_then(Value::as_str) == Some(effect))?;
        e.get("params")?.get(param)?.get("value")?.as_str().map(str::to_string)
    }

    pub fn float_param(&self, effect: &str, param: &str) -> Option<f64> {
        let raw = self.raw_param(effect, param)?;
        let inner = raw.strip_prefix("Float(")?.strip_suffix(')')?;
        inner.parse().ok()
    }

    pub fn vec2_param(&self, effect: &str, param: &str) -> Option<[f64; 2]> {
        let raw = self.raw_param(effect, param)?;
        let x = raw.split("x: ").nth(1)?.split(',').next()?.trim().parse().ok()?;
        let y = raw.split("y: ").nth(1)?.split(' ').next()?.trim().parse().ok()?;
        Some([x, y])
    }

    pub fn has_effect(&self, effect: &str) -> bool {
        self.effects.iter().any(|e| e.get("effect").and_then(Value::as_str) == Some(effect))
    }
}

#[derive(Clone, Debug)]
pub struct TrackView {
    pub id: u64,
    pub name: String,
    pub kind: Kind,
    pub index: usize,
    pub muted: bool,
    pub enabled: bool,
    pub locked: bool,
    pub clips: Vec<ClipView>,
}

#[derive(Clone, Debug, Default)]
pub struct SeqView {
    pub id: u64,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration: i64,
    pub video: Vec<TrackView>,
    pub audio: Vec<TrackView>,
    pub selection: Vec<u64>,
}

impl SeqView {
    pub fn load(session: &mut Session) -> Option<Self> {
        let v = session.execute("sequence.inspect", json!({})).ok()?;
        let settings = v.get("settings").cloned().unwrap_or(Value::Null);
        let num = |o: &Value, k: &str| o.get(k).and_then(Value::as_f64);
        let fps = settings.get("frame_rate").and_then(|r| Some(num(r, "num")? / num(r, "den")?)).unwrap_or(30.0);
        let tracks = |key: &str, kind: Kind| -> Vec<TrackView> {
            v.get(key)
                .and_then(Value::as_array)
                .map(|ts| {
                    ts.iter()
                        .enumerate()
                        .map(|(index, t)| TrackView {
                            id: t.get("id").and_then(Value::as_u64).unwrap_or(0),
                            name: t.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
                            kind,
                            index,
                            muted: t.get("muted").and_then(Value::as_bool).unwrap_or(false),
                            enabled: t.get("enabled").and_then(Value::as_bool).unwrap_or(true),
                            locked: t.get("locked").and_then(Value::as_bool).unwrap_or(false),
                            clips: t
                                .get("items")
                                .and_then(Value::as_array)
                                .map(|items| {
                                    items
                                        .iter()
                                        .filter_map(|c| {
                                            Some(ClipView {
                                                id: c.get("clip")?.as_u64()?,
                                                item: c.get("item").and_then(Value::as_u64).unwrap_or(0),
                                                name: c.get("name").and_then(Value::as_str).unwrap_or("Clip").to_string(),
                                                start: c.get("start")?.as_i64()?,
                                                end: c.get("end")?.as_i64()?,
                                                source_in: c.get("sourceIn").and_then(Value::as_i64).unwrap_or(0),
                                                speed: c.get("speed").and_then(Value::as_f64).unwrap_or(1.0),
                                                reverse: c.get("reverse").and_then(Value::as_bool).unwrap_or(false),
                                                link: c.get("link").and_then(Value::as_u64),
                                                enabled: c.get("enabled").and_then(Value::as_bool).unwrap_or(true),
                                                effects: c.get("effects").and_then(Value::as_array).cloned().unwrap_or_default(),
                                                is_graphic: c.get("graphic").is_some()
                                                    || c.get("name").and_then(Value::as_str).is_some_and(|n| n.ends_with(" Text") || n == "Graphic"),
                                            })
                                        })
                                        .collect()
                                })
                                .unwrap_or_default(),
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        Some(SeqView {
            id: v.get("id").and_then(Value::as_u64).unwrap_or(0),
            name: v.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
            width: settings.get("width").and_then(Value::as_u64).unwrap_or(1920) as u32,
            height: settings.get("height").and_then(Value::as_u64).unwrap_or(1080) as u32,
            fps: if fps > 0.0 { fps } else { 30.0 },
            duration: v.get("duration").and_then(Value::as_i64).unwrap_or(0),
            video: tracks("video", Kind::Video),
            audio: tracks("audio", Kind::Audio),
            selection: v.get("selection").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default(),
        })
    }

    pub fn clip(&self, id: u64) -> Option<(&TrackView, &ClipView)> {
        self.video.iter().chain(&self.audio).find_map(|t| t.clips.iter().find(|c| c.id == id).map(|c| (t, c)))
    }

    pub fn main_track(&self) -> Option<&TrackView> {
        self.video.first()
    }

    /// End of the last clip on the main track.
    pub fn main_end(&self) -> i64 {
        self.main_track().and_then(|t| t.clips.iter().map(|c| c.end).max()).unwrap_or(0)
    }

    /// All clip edges (snapping, edit-point navigation).
    pub fn edges(&self) -> Vec<i64> {
        let mut e: Vec<i64> = self.video.iter().chain(&self.audio).flat_map(|t| t.clips.iter().flat_map(|c| [c.start, c.end])).collect();
        e.push(0);
        e.sort_unstable();
        e.dedup();
        e
    }

    /// Clips under time `t`, top video track first.
    pub fn clips_at(&self, t: i64) -> Vec<&ClipView> {
        self.video.iter().rev().chain(&self.audio).flat_map(|tr| tr.clips.iter().filter(move |c| c.start <= t && t < c.end)).collect()
    }

    pub fn frame_ticks(&self) -> i64 {
        (TPS / self.fps.max(1.0)).round() as i64
    }
}

#[derive(Clone, Debug)]
pub struct MediaItem {
    pub id: u64,
    pub name: String,
    pub kind: String,
    pub duration: i64,
    pub bin: String,
}

impl MediaItem {
    pub fn is_audio(&self) -> bool {
        self.kind == "Audio"
    }
    pub fn is_sequence(&self) -> bool {
        self.kind == "Sequence"
    }
    pub fn icon(&self) -> &'static str {
        match self.kind.as_str() {
            "Audio" => "♪",
            "Still" | "Image" => "🖼",
            "Sequence" => "🎞",
            "Synthetic" | "Graphic" => "✦",
            _ => "▶",
        }
    }
}

pub fn media_items(session: &mut Session) -> Vec<MediaItem> {
    let Ok(v) = session.execute("project.inspect", json!({})) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    fn walk(node: &Value, bin: &str, out: &mut Vec<MediaItem>) {
        let Some(children) = node.get("children").and_then(Value::as_array) else {
            return;
        };
        for c in children {
            if c.get("children").is_some() {
                let name = c.get("name").and_then(Value::as_str).unwrap_or_default();
                walk(c, name, out);
            } else if let Some(id) = c.get("item").and_then(Value::as_u64) {
                out.push(MediaItem {
                    id,
                    name: c.get("name").and_then(Value::as_str).unwrap_or("Item").to_string(),
                    kind: c.get("type").and_then(Value::as_str).unwrap_or_default().to_string(),
                    duration: c.get("duration").and_then(Value::as_i64).unwrap_or(0),
                    bin: bin.to_string(),
                });
            }
        }
    }
    if let Some(root) = v.get("root") {
        walk(root, "", &mut out);
    }
    out
}

/// `00:01:23:12` at `fps`.
pub fn timecode(ticks: i64, fps: f64) -> String {
    let t = secs(ticks.max(0));
    let whole = t.floor() as i64;
    let frames = ((t - whole as f64) * fps).floor() as i64;
    format!("{:02}:{:02}:{:02}:{:02}", whole / 3600, (whole / 60) % 60, whole % 60, frames)
}

/// `1:05` / `12.4s` for clip labels.
pub fn short_duration(ticks: i64) -> String {
    let s = secs(ticks);
    if s < 60.0 { format!("{s:.1}s") } else { format!("{}:{:02}", (s / 60.0) as i64, (s % 60.0) as i64) }
}
