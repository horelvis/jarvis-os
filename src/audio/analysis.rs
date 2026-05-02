//! PCM → (RMS, FFT bands) analysis for the orb's audio reactivity.
//!
//! Lives in IronClaw core (not in any specific TTS backend) so every
//! `TtsBackend` impl — current voice-daemon IPC bridge or future
//! in-process engines like Piper / Kokoro — feeds raw PCM into the same
//! pipeline and the same numeric definition of "audio level" reaches
//! the UI orb.
//!
//! Pipeline:
//! - Convert `i16 → f32` in `[-1, 1]`.
//! - RMS over the whole buffer (cheap, always meaningful).
//! - Hann-window the first up-to-`FFT_SIZE` samples, zero-pad if the
//!   chunk is shorter, run `rustfft`, group magnitudes into 5
//!   log-spaced bands.
//! - Outputs are clamped to `[0, 1]` so the UI maps directly to
//!   alpha/scale without renormalising.

use rustfft::{FftPlanner, num_complex::Complex32};
use std::sync::{Arc, OnceLock};

const SAMPLE_RATE_HZ: f32 = 16_000.0;
const FFT_SIZE: usize = 1024;
pub const NUM_BANDS: usize = 5;

// 6 edges = 5 contiguous bands. Log-spaced over the speech-relevant
// range; below 80 Hz is mostly room rumble, above 4 kHz the orb's
// human-eye temporal resolution stops giving useful detail.
const BAND_EDGES_HZ: [f32; NUM_BANDS + 1] = [80.0, 175.0, 383.0, 837.0, 1832.0, 4000.0];

// `i16::MAX` rounds to 32_767, but we divide by 32_768 (2^15) so that
// the most-negative `i16::MIN` also lands inside `[-1, 1]`.
const I16_SCALE: f32 = 32_768.0;

/// Returns `(rms, bands)` with all values in `[0, 1]`.
///
/// `bands[i]` covers `BAND_EDGES_HZ[i] .. BAND_EDGES_HZ[i + 1]`.
/// Empty input yields all zeros (silence).
pub fn analyze_pcm(samples: &[i16]) -> (f32, [f32; NUM_BANDS]) {
    if samples.is_empty() {
        return (0.0, [0.0; NUM_BANDS]);
    }

    let mut sum_sq = 0.0f32;
    for &s in samples {
        let f = s as f32 / I16_SCALE;
        sum_sq += f * f;
    }
    let rms = (sum_sq / samples.len() as f32).sqrt().clamp(0.0, 1.0);

    let take = samples.len().min(FFT_SIZE);
    let mut buf: Vec<Complex32> = (0..FFT_SIZE)
        .map(|i| {
            if i < take {
                let f = samples[i] as f32 / I16_SCALE;
                Complex32::new(f * hann(i, take), 0.0)
            } else {
                Complex32::new(0.0, 0.0)
            }
        })
        .collect();

    fft_plan().process(&mut buf);

    let bin_hz = SAMPLE_RATE_HZ / FFT_SIZE as f32;
    let mut bands = [0.0f32; NUM_BANDS];
    let mut counts = [0u32; NUM_BANDS];

    // Real-valued input → conjugate-symmetric output; only the
    // positive-frequency half carries unique energy. The factor-of-2
    // on the magnitude compensates for the discarded mirror.
    for (k, c) in buf.iter().take(FFT_SIZE / 2).enumerate() {
        let hz = k as f32 * bin_hz;
        if hz < BAND_EDGES_HZ[0] || hz >= BAND_EDGES_HZ[NUM_BANDS] {
            continue;
        }
        let mag = (c.norm_sqr().sqrt() * 2.0) / FFT_SIZE as f32;
        for b in 0..NUM_BANDS {
            if hz >= BAND_EDGES_HZ[b] && hz < BAND_EDGES_HZ[b + 1] {
                bands[b] += mag;
                counts[b] += 1;
                break;
            }
        }
    }
    for b in 0..NUM_BANDS {
        if counts[b] > 0 {
            bands[b] = (bands[b] / counts[b] as f32).clamp(0.0, 1.0);
        }
    }

    (rms, bands)
}

fn hann(i: usize, n: usize) -> f32 {
    use std::f32::consts::PI;
    if n <= 1 {
        return 1.0;
    }
    0.5 - 0.5 * (2.0 * PI * i as f32 / (n - 1) as f32).cos()
}

// FFT planning allocates twiddle factors and chooses an algorithm; for
// a fixed size used at TTS-chunk rate (tens of times per second) we
// build it once and share the `Arc` across calls.
fn fft_plan() -> Arc<dyn rustfft::Fft<f32>> {
    static PLAN: OnceLock<Arc<dyn rustfft::Fft<f32>>> = OnceLock::new();
    PLAN.get_or_init(|| FftPlanner::<f32>::new().plan_fft_forward(FFT_SIZE))
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_pcm_yields_zero_rms_and_zero_bands() {
        let (rms, bands) = analyze_pcm(&[]);
        assert_eq!(rms, 0.0);
        assert_eq!(bands, [0.0; NUM_BANDS]);
    }

    #[test]
    fn silence_yields_near_zero_rms() {
        let pcm = vec![0i16; 1024];
        let (rms, bands) = analyze_pcm(&pcm);
        assert!(rms < 1e-6, "expected ~0 rms, got {rms}");
        for (i, &b) in bands.iter().enumerate() {
            assert!(b < 1e-6, "band {i} = {b}");
        }
    }

    #[test]
    fn full_scale_dc_yields_near_unit_rms() {
        // i16::MAX / 32768 ≈ 0.99997 — close to but not exactly 1.
        let pcm = vec![i16::MAX; 1024];
        let (rms, _) = analyze_pcm(&pcm);
        assert!(rms > 0.99, "rms = {rms}");
        assert!(rms <= 1.0, "rms exceeded clamp: {rms}");
    }

    #[test]
    fn pure_tone_lights_up_expected_band() {
        // 500 Hz sine — falls inside band 2 (383-837 Hz).
        let n = 1024;
        let freq_hz = 500.0;
        let pcm: Vec<i16> = (0..n)
            .map(|i| {
                let t = i as f32 / SAMPLE_RATE_HZ;
                let amp = 0.5 * (2.0 * std::f32::consts::PI * freq_hz * t).sin();
                (amp * 32767.0) as i16
            })
            .collect();
        let (_, bands) = analyze_pcm(&pcm);
        let (max_idx, _) = bands
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .expect("non-empty bands");
        assert_eq!(
            max_idx, 2,
            "expected band 2 dominant at 500 Hz, got {max_idx}, bands={bands:?}"
        );
    }

    #[test]
    fn output_stays_within_unit_interval() {
        // Pseudo-random full-scale i16s; the clamp + normalisation must
        // keep every value inside [0, 1].
        let pcm: Vec<i16> = (0..1024)
            .map(|i| (((i * 7919) % 65_535) as i32 - 32_768) as i16)
            .collect();
        let (rms, bands) = analyze_pcm(&pcm);
        assert!((0.0..=1.0).contains(&rms), "rms out of range: {rms}");
        for (i, &b) in bands.iter().enumerate() {
            assert!((0.0..=1.0).contains(&b), "band {i} out of range: {b}");
        }
    }

    #[test]
    fn short_chunk_zero_pads_without_panic() {
        // 80 samples (= 5 ms at 16 kHz). FFT must zero-pad up to 1024
        // and still produce sane RMS over the original 80 samples.
        let pcm = vec![10_000i16; 80];
        let (rms, _) = analyze_pcm(&pcm);
        // 10_000 / 32_768 ≈ 0.305 → rms over a constant ≈ same value.
        assert!(rms > 0.29 && rms < 0.32, "rms = {rms}");
    }
}
