//! The spectrogram half of HTDemucs, outside the network (the exported ONNX graph takes the
//! complex-as-channels spectrogram as its second input and returns one as its first output).
//!
//! These reproduce, sample for sample, what Demucs does in PyTorch:
//! `standalone_spec` (reflect padding + `torch.stft(n_fft=4096, hop=1024, hann, normalized=True,
//! center=True, pad_mode="reflect")`, last bin dropped, frames `2..2+le`) and `standalone_ispec`
//! (`torch.istft` with the same settings on the re-padded spectrogram, then trimmed).

use std::sync::Arc;

use realfft::num_complex::Complex32;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};

pub const NFFT: usize = 4096;
pub const HOP: usize = NFFT / 4;
/// Frequency bins the network sees (`NFFT / 2`, the Nyquist bin is dropped).
pub const BINS: usize = NFFT / 2;

pub struct Stft {
    fwd: Arc<dyn RealToComplex<f32>>,
    inv: Arc<dyn ComplexToReal<f32>>,
    /// Periodic Hann window (`torch.hann_window(4096)`).
    window: Vec<f32>,
}

impl Default for Stft {
    fn default() -> Self {
        let mut planner = RealFftPlanner::<f32>::new();
        let window = (0..NFFT).map(|n| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * n as f64 / NFFT as f64).cos() as f32).collect();
        Self { fwd: planner.plan_fft_forward(NFFT), inv: planner.plan_fft_inverse(NFFT), window }
    }
}

/// `x` reflect-padded (PyTorch `F.pad(mode="reflect")`: the edge sample is not repeated).
fn reflect(x: &[f32], left: usize, right: usize) -> Vec<f32> {
    let n = x.len() as isize;
    (-(left as isize)..n + right as isize)
        .map(|j| {
            let k = if j < 0 {
                -j
            } else if j >= n {
                2 * (n - 1) - j
            } else {
                j
            };
            x[k.clamp(0, n - 1) as usize]
        })
        .collect()
}

/// Frames the network works with for a segment of `len` samples.
pub fn frames(len: usize) -> usize {
    len.div_ceil(HOP)
}

impl Stft {
    /// The network's spectrogram input for one segment: `[channel*2 + (re|im)][bin][frame]`,
    /// i.e. shape `(2 * channels, BINS, frames(len))`, flattened.
    pub fn spec(&self, channels: &[Vec<f32>]) -> Vec<f32> {
        let len = channels[0].len();
        let le = frames(len);
        let pad = HOP / 2 * 3;
        let mut out = vec![0.0f32; channels.len() * 2 * BINS * le];
        let mut buf = self.fwd.make_input_vec();
        let mut spectrum = self.fwd.make_output_vec();
        let norm = 1.0 / (NFFT as f32).sqrt();
        for (c, x) in channels.iter().enumerate() {
            // Demucs's own padding, then torch.stft's centre padding
            let x1 = reflect(x, pad, pad + le * HOP - len);
            let x2 = reflect(&x1, NFFT / 2, NFFT / 2);
            for t in 0..le {
                let start = (t + 2) * HOP; // frames 2..2+le of the full STFT
                for (n, b) in buf.iter_mut().enumerate() {
                    *b = x2[start + n] * self.window[n];
                }
                self.fwd.process(&mut buf, &mut spectrum).expect("fft sizes match");
                for (f, z) in spectrum.iter().take(BINS).enumerate() {
                    out[((c * 2) * BINS + f) * le + t] = z.re * norm;
                    out[((c * 2 + 1) * BINS + f) * le + t] = z.im * norm;
                }
            }
        }
        out
    }

    /// The waveform of one source from the network's spectrogram output (same layout as
    /// [`spec`](Self::spec)), `len` samples per channel.
    pub fn ispec(&self, spec: &[f32], channels: usize, len: usize) -> Vec<Vec<f32>> {
        let le = frames(len);
        let n_frames = le + 4; // two empty frames each side (Demucs re-pads before istft)
        let ola_len = NFFT + HOP * (n_frames - 1);
        let norm = 1.0 / (NFFT as f32).sqrt();
        // window envelope: the same for every channel
        let mut env = vec![0.0f32; ola_len];
        for j in 0..n_frames {
            for (n, w) in self.window.iter().enumerate() {
                env[j * HOP + n] += w * w;
            }
        }
        let offset = NFFT / 2 + HOP / 2 * 3; // istft centre trim, then Demucs's pad
        let mut spectrum = self.inv.make_input_vec();
        let mut buf = self.inv.make_output_vec();
        (0..channels)
            .map(|c| {
                let mut ola = vec![0.0f32; ola_len];
                for t in 0..le {
                    for (f, z) in spectrum.iter_mut().enumerate() {
                        *z = if f < BINS {
                            Complex32::new(spec[((c * 2) * BINS + f) * le + t], spec[((c * 2 + 1) * BINS + f) * le + t])
                        } else {
                            Complex32::new(0.0, 0.0) // the dropped Nyquist bin
                        };
                    }
                    spectrum[0].im = 0.0;
                    self.inv.process(&mut spectrum, &mut buf).expect("fft sizes match");
                    let start = (t + 2) * HOP;
                    for (n, v) in buf.iter().enumerate() {
                        ola[start + n] += v * norm * self.window[n];
                    }
                }
                (0..len).map(|i| ola[offset + i] / env[offset + i].max(1e-11)).collect()
            })
            .collect()
    }
}
