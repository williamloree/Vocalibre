use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};

use super::AudioError;

pub const TARGET_RATE: u32 = 16_000;

/// Resamples a complete mono clip to 16 kHz.
pub fn to_16k(samples: &[f32], rate: u32) -> Result<Vec<f32>, AudioError> {
    if rate == TARGET_RATE || samples.is_empty() {
        return Ok(samples.to_vec());
    }
    let resample = |e: &dyn std::fmt::Display| AudioError::Resample(e.to_string());
    let mut fft = Fft::<f32>::new(
        rate as usize,
        TARGET_RATE as usize,
        1024,
        1,
        FixedSync::Input,
    )
    .map_err(|e| resample(&e))?;
    let input = InterleavedSlice::new(samples, 1, samples.len()).map_err(|e| resample(&e))?;
    let out = fft
        .process_all(&input, samples.len(), None)
        .map_err(|e| resample(&e))?;
    Ok(out.take_data())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, rate: u32, seconds: f32) -> Vec<f32> {
        let n = (rate as f32 * seconds) as usize;
        (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin() * 0.5)
            .collect()
    }

    /// Counts upward zero crossings → frequency estimate.
    fn freq(samples: &[f32], rate: u32) -> f32 {
        let crossings = samples
            .windows(2)
            .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
            .count();
        crossings as f32 * rate as f32 / samples.len() as f32
    }

    #[test]
    fn resamples_common_rates_preserving_length_and_pitch() {
        for rate in [44_100, 48_000, 96_000, 22_050, 8_000] {
            let input = sine(440.0, rate, 2.0);
            let out = to_16k(&input, rate).unwrap();
            let expected = 2 * TARGET_RATE as usize;
            assert!(
                out.len().abs_diff(expected) < 50,
                "{rate}: {} samples",
                out.len()
            );
            let f = freq(&out, TARGET_RATE);
            assert!((f - 440.0).abs() < 5.0, "{rate}: {f} Hz");
        }
    }

    #[test]
    fn passthrough_at_16k_and_empty() {
        assert_eq!(to_16k(&[0.1, 0.2], 16_000).unwrap(), vec![0.1, 0.2]);
        assert!(to_16k(&[], 48_000).unwrap().is_empty());
    }
}
