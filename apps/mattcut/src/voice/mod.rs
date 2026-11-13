//! Isolate voice: split a clip's sound into the voice and everything else, on this computer.
//!
//! The model is Meta's Hybrid Transformer Demucs, fine-tuned vocals network (`htdemucs_ft`, model
//! 4 of the bag, <https://github.com/facebookresearch/demucs>, MIT), exported to ONNX with its
//! STFT/iSTFT moved out of the graph (following sevagh/demucs.onnx, MIT) and run with tract
//! (pure Rust). It is downloaded once, on first use, and checked against its SHA-256.
//!
//! A separation works on a whole source file and is cached per file
//! (`<data dir>/Voice/<key>/voice.wav` and `no-voice.wav`), so every clip of that file can switch
//! between the original, voice only and voice removed instantly afterwards.

pub mod dsp;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use filmcraft_speech::models::ModelFile;
use tract_onnx::prelude::*;

/// The model file (hosted on MattCut's GitHub releases; the source checkpoint is Meta's
/// `04573f0d-f3cf25b2.th`).
pub const MODEL: ModelFile = ModelFile {
    name: "htdemucs_ft_vocals.onnx",
    url: "https://github.com/aatirraza/mattcut/releases/download/voice-model-v1/htdemucs_ft_vocals.onnx",
    sha256: "17932f5510fc840cec1e4f9573ef8c19d68bc366786a04edabbe2acdab03e554",
    size: 174_264_777,
};
pub const MODEL_CREDIT: &str = "Hybrid Transformer Demucs by Meta AI (MIT licence)";
pub const MODEL_SOURCE: &str = "https://github.com/facebookresearch/demucs";

/// What the network was trained on: 44.1 kHz stereo, 7.8 s segments.
pub const RATE: u32 = 44_100;
const SEGMENT: usize = 343_980;
/// Index of "vocals" in the network's sources (drums, bass, other, vocals).
const VOCALS: usize = 3;
const SOURCES: usize = 4;
/// Demucs's default overlap between segments.
const OVERLAP: f64 = 0.25;

type Plan = Arc<TypedRunnableModel>;

pub fn models_dir() -> PathBuf {
    mattcut_office::app_data_dir().unwrap_or_else(std::env::temp_dir).join("models").join("htdemucs-ft-vocals")
}

pub fn model_installed() -> bool {
    let p = models_dir().join(MODEL.name);
    std::fs::metadata(&p).is_ok_and(|m| m.len() == MODEL.size)
}

/// Download the model (`progress(done, total)`; return false to cancel).
pub fn download_model(progress: &mut dyn FnMut(u64, u64) -> bool) -> Result<(), String> {
    let mut cb = |done: u64, total: u64, _: &str| progress(done, total);
    filmcraft_speech::models::download_files(&models_dir(), &[MODEL], &mut cb).map_err(|e| e.to_string())
}

/// The loaded network, shared by every separation (loading and optimising takes a few seconds).
fn plan() -> Result<Plan, String> {
    static PLAN: OnceLock<Mutex<Option<Plan>>> = OnceLock::new();
    let cell = PLAN.get_or_init(|| Mutex::new(None));
    let mut guard = cell.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(p) = guard.as_ref() {
        return Ok(p.clone());
    }
    let le = dsp::frames(SEGMENT);
    let p = tract_onnx::onnx()
        .model_for_path(models_dir().join(MODEL.name))
        .and_then(|m| m.with_input_fact(0, f32::fact([1, 2, SEGMENT]).into()))
        .and_then(|m| m.with_input_fact(1, f32::fact([1, 4, dsp::BINS, le]).into()))
        .and_then(|m| m.into_optimized())
        .and_then(|m| m.into_runnable())
        .map_err(|e| format!("couldn't load the voice model: {e}"))?;
    let p: Plan = p;
    *guard = Some(p.clone());
    Ok(p)
}

/// Progress of a separation, shared with the UI.
#[derive(Default)]
pub struct Progress {
    pub done: AtomicU32,
    pub total: AtomicU32,
    pub cancel: AtomicBool,
}

impl Progress {
    pub fn fraction(&self) -> f32 {
        let t = self.total.load(Ordering::Relaxed);
        if t == 0 { 0.0 } else { self.done.load(Ordering::Relaxed) as f32 / t as f32 }
    }
}

/// The two results of a separation.
#[derive(Clone, Debug)]
pub struct Stems {
    pub voice: PathBuf,
    pub no_voice: PathBuf,
}

/// Where the separation of `source` is (or will be) kept.
pub fn cache_dir(source: &Path) -> PathBuf {
    let meta = std::fs::metadata(source).ok();
    let size = meta.as_ref().map_or(0, std::fs::Metadata::len);
    let mtime = meta.and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    // FNV-1a of path, size and modification time: a changed file gets a fresh separation
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in source.to_string_lossy().bytes().chain(size.to_le_bytes()).chain(mtime.to_le_bytes()) {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    mattcut_office::app_data_dir().unwrap_or_else(std::env::temp_dir).join("Voice").join(format!("{h:016x}"))
}

/// The cached separation of `source`, if there is one.
pub fn cached(source: &Path) -> Option<Stems> {
    let dir = cache_dir(source);
    let s = Stems { voice: dir.join("voice.wav"), no_voice: dir.join("no-voice.wav") };
    (s.voice.is_file() && s.no_voice.is_file()).then_some(s)
}

/// The original file a separated file was made from (written next to the results).
pub fn original_of(stem: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(stem.parent()?.join("source.txt")).ok()?;
    Some(PathBuf::from(text.trim()))
}

/// Separate `source` (any file MattCut can open) into voice and everything else.
pub fn separate(source: &Path, progress: &Progress) -> Result<Stems, String> {
    if let Some(s) = cached(source) {
        return Ok(s);
    }
    let mix = read_stereo(source)?;
    let len = mix[0].len();
    if len == 0 {
        return Err("the clip has no sound".into());
    }
    // Demucs normalises the whole track by its mono mean / standard deviation first
    let mono: Vec<f64> = (0..len).map(|i| f64::from(mix[0][i] + mix[1][i]) / 2.0).collect();
    let mean = mono.iter().sum::<f64>() / len as f64;
    let var = mono.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (len.max(2) - 1) as f64;
    let std = var.sqrt();
    let voice = if std < 1e-9 {
        vec![vec![0.0f32; len]; 2] // silence: no voice
    } else {
        let norm: Vec<Vec<f32>> = mix.iter().map(|c| c.iter().map(|v| ((f64::from(*v) - mean) / std) as f32).collect()).collect();
        let v = apply(&norm, progress)?;
        v.into_iter().map(|c| c.into_iter().map(|x| (f64::from(x) * std + mean) as f32).collect()).collect()
    };
    let no_voice: Vec<Vec<f32>> = (0..2).map(|c| (0..len).map(|i| mix[c][i] - voice[c][i]).collect()).collect();

    let dir = cache_dir(source);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let stems = Stems { voice: dir.join("voice.wav"), no_voice: dir.join("no-voice.wav") };
    for (path, data) in [(&stems.voice, &voice), (&stems.no_voice, &no_voice)] {
        let inter: Vec<f32> = (0..len).flat_map(|i| [data[0][i], data[1][i]]).collect();
        let part = path.with_extension("wav.part");
        std::fs::write(&part, filmcraft_media::wav::write_wav16(&inter, 2, RATE)).map_err(|e| format!("{}: {e}", part.display()))?;
        std::fs::rename(&part, path).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    let _ = std::fs::write(dir.join("source.txt"), source.to_string_lossy().as_bytes());
    Ok(stems)
}

/// The file's sound as two channels at 44.1 kHz (mono is doubled, extra channels dropped).
fn read_stereo(path: &Path) -> Result<Vec<Vec<f32>>, String> {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let src = filmcraft_codecs::open_bytes(&name, bytes.into()).map_err(|e| format!("{name}: {e}"))?;
    if src.info().audio().is_none() {
        return Err(format!("{name} has no sound"));
    }
    let total = src.info().duration.to_units_floor(i64::from(RATE)).max(0) as usize;
    let mut out = vec![Vec::with_capacity(total), Vec::with_capacity(total)];
    let block = RATE as usize * 30;
    let mut at = 0;
    while at < total {
        let n = block.min(total - at);
        let buf = src.audio(at as i64, n, RATE).map_err(|e| format!("{name}: {e}"))?;
        let l = buf.channels.first().cloned().unwrap_or_else(|| vec![0.0; n]);
        let r = buf.channels.get(1).cloned().unwrap_or_else(|| l.clone());
        out[0].extend(l.iter().copied().chain(std::iter::repeat(0.0)).take(n));
        out[1].extend(r.iter().copied().chain(std::iter::repeat(0.0)).take(n));
        at += n;
    }
    Ok(out)
}

/// Demucs `apply_model(split=True, overlap=0.25)` for the vocals source: overlapping 7.8 s
/// segments, each padded with its neighbouring audio, cross-faded with triangular weights.
fn apply(mix: &[Vec<f32>], progress: &Progress) -> Result<Vec<Vec<f32>>, String> {
    let len = mix[0].len();
    let stride = ((1.0 - OVERLAP) * SEGMENT as f64) as usize;
    let offsets: Vec<usize> = (0..len).step_by(stride).collect();
    progress.total.store(offsets.len() as u32, Ordering::Relaxed);
    progress.done.store(0, Ordering::Relaxed);
    let half = SEGMENT / 2;
    let weight: Vec<f32> = (1..=half).chain((1..=SEGMENT - half).rev()).map(|w| w as f32 / half.max(SEGMENT - half) as f32).collect();

    let plan = plan()?;
    let out = Mutex::new((vec![vec![0.0f32; len]; 2], vec![0.0f32; len]));
    let next = AtomicU32::new(0);
    let error: Mutex<Option<String>> = Mutex::new(None);
    // each running segment holds ~0.6 GB: run a few at once, not one per core
    let cores = std::thread::available_parallelism().map_or(2, std::num::NonZero::get);
    let workers = (cores / 2).clamp(1, 3).min(offsets.len());
    std::thread::scope(|sc| {
        for _ in 0..workers {
            sc.spawn(|| {
                let stft = dsp::Stft::default();
                loop {
                    if progress.cancel.load(Ordering::Relaxed) || error.lock().is_ok_and(|e| e.is_some()) {
                        return;
                    }
                    let i = next.fetch_add(1, Ordering::Relaxed) as usize;
                    let Some(&offset) = offsets.get(i) else { return };
                    match segment(&plan, &stft, mix, offset) {
                        Ok(v) => {
                            let mut g = out.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                            let (acc, sum) = &mut *g;
                            for (j, w) in weight.iter().take(v[0].len()).enumerate() {
                                acc[0][offset + j] += w * v[0][j];
                                acc[1][offset + j] += w * v[1][j];
                                sum[offset + j] += w;
                            }
                            progress.done.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(e) => {
                            if let Ok(mut slot) = error.lock() {
                                slot.get_or_insert(e);
                            }
                            return;
                        }
                    }
                }
            });
        }
    });
    if let Some(e) = error.into_inner().ok().flatten() {
        return Err(e);
    }
    if progress.cancel.load(Ordering::Relaxed) {
        return Err("cancelled".into());
    }
    let (mut acc, sum) = out.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner);
    for c in &mut acc {
        for (v, s) in c.iter_mut().zip(&sum) {
            *v /= s.max(1e-9);
        }
    }
    Ok(acc)
}

/// The vocals of `mix[offset .. offset + SEGMENT]` (shorter at the end), as Demucs computes them.
fn segment(plan: &Plan, stft: &dsp::Stft, mix: &[Vec<f32>], offset: usize) -> Result<Vec<Vec<f32>>, String> {
    let len = mix[0].len();
    let chunk = SEGMENT.min(len - offset);
    // TensorChunk.padded: centre the chunk in a full segment, filling with real neighbouring audio
    let delta = SEGMENT - chunk;
    let start = offset as isize - (delta / 2) as isize;
    let seg: Vec<Vec<f32>> = mix
        .iter()
        .map(|c| (0..SEGMENT as isize).map(|k| start + k).map(|j| if j >= 0 && (j as usize) < len { c[j as usize] } else { 0.0 }).collect())
        .collect();
    let v = run_segment(plan, stft, &seg)?;
    let trim = delta / 2;
    Ok(v.into_iter().map(|c| c[trim..trim + chunk].to_vec()).collect())
}

/// One full 7.8 s segment through STFT, the network and iSTFT: the vocals source, 2 channels.
fn run_segment(plan: &Plan, stft: &dsp::Stft, seg: &[Vec<f32>]) -> Result<Vec<Vec<f32>>, String> {
    let le = dsp::frames(SEGMENT);
    let spec = stft.spec(seg);
    let wave: Vec<f32> = seg.iter().flatten().copied().collect();
    let to_tensor = |shape: &[usize], v: Vec<f32>| -> Result<TValue, String> {
        Ok(tract_ndarray::ArrayD::from_shape_vec(shape, v).map_err(|e| e.to_string())?.into_tensor().into())
    };
    let out = plan.run(tvec!(to_tensor(&[1, 2, SEGMENT], wave)?, to_tensor(&[1, 4, dsp::BINS, le], spec)?)).map_err(|e| format!("voice model: {e}"))?;
    let (spec_out, wave_out) = (out[0].try_as_plain_ram().map_err(|e| e.to_string())?, out[1].try_as_plain_ram().map_err(|e| e.to_string())?);
    let spec_out = spec_out.as_slice::<f32>().map_err(|e| e.to_string())?;
    let wave_out = wave_out.as_slice::<f32>().map_err(|e| e.to_string())?;
    let per_source = 4 * dsp::BINS * le;
    debug_assert_eq!(spec_out.len(), SOURCES * per_source);
    let freq = stft.ispec(&spec_out[VOCALS * per_source..(VOCALS + 1) * per_source], 2, SEGMENT);
    Ok((0..2).map(|c| (0..SEGMENT).map(|i| freq[c][i] + wave_out[(VOCALS * 2 + c) * SEGMENT + i]).collect()).collect())
}

#[cfg(test)]
mod tests {
    //! Checked against PyTorch reference vectors written by the export script
    //! (`MATTCUT_VOICE_REF=<dir with mix.f32, spec.f32, out_x.f32, out_xt.f32, sources.f32>`).
    use super::*;

    fn read(dir: &str, name: &str) -> Vec<f32> {
        std::fs::read(format!("{dir}/{name}.f32")).unwrap().chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect()
    }

    fn max_diff(a: &[f32], b: &[f32]) -> f32 {
        assert_eq!(a.len(), b.len());
        a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max)
    }

    #[test]
    fn spectrogram_matches_pytorch() {
        let Ok(dir) = std::env::var("MATTCUT_VOICE_REF") else { return };
        let mix = read(&dir, "mix");
        let seg = vec![mix[..SEGMENT].to_vec(), mix[SEGMENT..].to_vec()];
        let stft = dsp::Stft::default();
        let spec = stft.spec(&seg);
        let want = read(&dir, "spec");
        let d = max_diff(&spec, &want);
        let peak = want.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        println!("spec: max diff {d:.2e} (peak {peak:.2})");
        assert!(d < 1e-3 * peak.max(1.0), "spec differs: {d}");

        // iSTFT of PyTorch's network output + its time branch = PyTorch's sources
        let le = dsp::frames(SEGMENT);
        let per_source = 4 * dsp::BINS * le;
        let out_x = read(&dir, "out_x");
        let out_xt = read(&dir, "out_xt");
        let sources = read(&dir, "sources");
        let freq = stft.ispec(&out_x[VOCALS * per_source..(VOCALS + 1) * per_source], 2, SEGMENT);
        let ours: Vec<f32> = (0..2).flat_map(|c| (0..SEGMENT).map(move |i| (c, i))).map(|(c, i)| freq[c][i] + out_xt[(VOCALS * 2 + c) * SEGMENT + i]).collect();
        let want = &sources[VOCALS * 2 * SEGMENT..(VOCALS + 1) * 2 * SEGMENT];
        let d = max_diff(&ours, want);
        println!("istft + time branch: max diff {d:.2e}");
        assert!(d < 1e-4, "istft differs: {d}");
    }

    #[test]
    fn whole_segment_matches_pytorch() {
        let Ok(dir) = std::env::var("MATTCUT_VOICE_REF") else { return };
        if !model_installed() {
            std::fs::create_dir_all(models_dir()).unwrap();
            std::fs::copy(format!("{dir}/{}", MODEL.name), models_dir().join(MODEL.name)).unwrap();
        }
        let mix = read(&dir, "mix");
        let seg = vec![mix[..SEGMENT].to_vec(), mix[SEGMENT..].to_vec()];
        let t = std::time::Instant::now();
        let v = run_segment(&plan().unwrap(), &dsp::Stft::default(), &seg).unwrap();
        println!("segment in {:.1}s", t.elapsed().as_secs_f32());
        let sources = read(&dir, "sources");
        let ours: Vec<f32> = v.concat();
        let d = max_diff(&ours, &sources[VOCALS * 2 * SEGMENT..(VOCALS + 1) * 2 * SEGMENT]);
        println!("whole segment vs PyTorch: max diff {d:.2e}");
        assert!(d < 5e-3, "segment differs: {d}");
    }
}
