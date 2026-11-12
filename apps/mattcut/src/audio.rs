//! Playback: the program mix through the default audio output (cpal). The device's played-sample
//! count is the playback clock, so picture follows sound. Without an output device playback
//! runs silently on the wall clock.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use filmcraft_audio_dsp::channels::{Layout, Mixdown};
use filmcraft_engine::Session;
use filmcraft_engine::previews::MixOutput;
use filmcraft_project::ItemId;
use filmcraft_time::{TICKS_PER_SECOND, Tick};

pub struct Player {
    stream: Option<cpal::Stream>,
    played: Arc<AtomicU64>,
    rate: u32,
    start: Tick,
    wall: Instant,
    pub playing: bool,
    pub note: Option<String>,
}

impl Default for Player {
    fn default() -> Self {
        Self { stream: None, played: Arc::new(AtomicU64::new(0)), rate: 48_000, start: Tick::ZERO, wall: Instant::now(), playing: false, note: None }
    }
}

impl Player {
    pub fn play(&mut self, session: &Session, seq: ItemId, from: Tick) {
        self.stop();
        self.start = from;
        self.wall = Instant::now();
        self.played = Arc::new(AtomicU64::new(0));
        self.playing = true;
        match open_stream(session, seq, from, Arc::clone(&self.played)) {
            Ok((stream, rate)) => {
                self.stream = Some(stream);
                self.rate = rate.max(1);
                self.note = None;
            }
            Err(e) => {
                log::warn!("audio output: {e}");
                self.note = Some(format!("No sound: {e}"));
            }
        }
    }

    pub fn stop(&mut self) {
        self.stream = None;
        self.playing = false;
    }

    /// Where playback is now.
    pub fn position(&self) -> Tick {
        let secs = if self.stream.is_some() { self.played.load(Ordering::Relaxed) as f64 / f64::from(self.rate) } else { self.wall.elapsed().as_secs_f64() };
        Tick(self.start.0.saturating_add((secs * TICKS_PER_SECOND as f64) as i64))
    }
}

fn open_stream(session: &Session, seq: ItemId, from: Tick, played: Arc<AtomicU64>) -> Result<(cpal::Stream, u32), String> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or("no audio output device")?;
    let supported = device.default_output_config().map_err(|e| e.to_string())?;
    if supported.sample_format() != cpal::SampleFormat::F32 {
        return Err(format!("unsupported output format {:?}", supported.sample_format()));
    }
    let config: cpal::StreamConfig = supported.config();
    let rate = config.sample_rate.0;
    let ch = usize::from(config.channels).max(1);

    let project = session.project.clone();
    let provider = session.media.provider(project.clone(), session.services.clone());
    let previews = session.previews.clone();
    let seq_rate = project.sequence(seq).map(|s| s.settings.sample_rate).unwrap_or(48_000).max(1);
    let mut resampler = (seq_rate != rate).then(|| filmcraft_audio_dsp::resample::StreamResampler::new(seq_rate, rate));
    let mut cursor: i64 = (from.0 as f64 / TICKS_PER_SECOND as f64 * f64::from(rate)).round() as i64;
    let out = || MixOutput { layout: Layout::Stereo, mixdown: Mixdown::default() };

    let stream = device
        .build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let n = data.len() / ch;
                let mixed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match resampler.as_mut() {
                    None => previews.mix_layout(&project, seq, cursor, n, &provider, out()).channels,
                    Some(r) => r.process(cursor, n, |s0, m| previews.mix_layout(&project, seq, s0, m, &provider, out()).channels),
                }));
                match mixed {
                    Ok(channels) => {
                        for (i, frame) in data.chunks_mut(ch).enumerate() {
                            for (c, x) in frame.iter_mut().enumerate() {
                                *x = if c < 2 { channels.get(c).or_else(|| channels.first()).and_then(|v| v.get(i)).copied().unwrap_or(0.0) } else { 0.0 };
                            }
                        }
                    }
                    Err(_) => data.fill(0.0),
                }
                cursor = cursor.saturating_add(n as i64);
                played.fetch_add(n as u64, Ordering::Relaxed);
            },
            |e| log::warn!("audio stream: {e}"),
            None,
        )
        .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, rate))
}
