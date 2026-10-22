//! Input level for the waveform, and energy-based silence auto-stop.

/// RMS of a block, mapped to 0.0 – 1.0 on a -60 dBFS … 0 dBFS scale.
pub fn level(block: &[f32]) -> f32 {
    if block.is_empty() {
        return 0.0;
    }
    let rms = (block.iter().map(|s| s * s).sum::<f32>() / block.len() as f32).sqrt();
    let db = 20.0 * rms.max(1e-6).log10();
    ((db + 60.0) / 60.0).clamp(0.0, 1.0)
}

/// Level above which a block counts as speech (≈ -36 dBFS).
const SPEECH_LEVEL: f32 = 0.4;

/// Fires once `timeout_ms` of quiet follows some speech. Silence before the
/// user starts talking never triggers it.
#[derive(Debug)]
pub struct SilenceDetector {
    timeout_ms: u64,
    heard_speech: bool,
    quiet_ms: u64,
}

impl SilenceDetector {
    pub fn new(timeout_ms: u64) -> Self {
        Self {
            timeout_ms,
            heard_speech: false,
            quiet_ms: 0,
        }
    }

    /// Feeds one block of `block_ms` at `level`. Returns true when the
    /// timeout is reached.
    pub fn feed(&mut self, level: f32, block_ms: u64) -> bool {
        if level >= SPEECH_LEVEL {
            self.heard_speech = true;
            self.quiet_ms = 0;
            return false;
        }
        if !self.heard_speech {
            return false;
        }
        self.quiet_ms += block_ms;
        self.quiet_ms >= self.timeout_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_scale() {
        assert_eq!(level(&[]), 0.0);
        assert_eq!(level(&[0.0; 100]), 0.0);
        assert!((level(&[1.0; 100]) - 1.0).abs() < 1e-3);
        let mid = level(&[0.03; 100]); // ≈ -30 dBFS
        assert!((mid - 0.5).abs() < 0.02, "{mid}");
    }

    #[test]
    fn silence_only_counts_after_speech() {
        let mut d = SilenceDetector::new(1_000);
        for _ in 0..100 {
            assert!(!d.feed(0.0, 100), "leading silence must not stop");
        }
        assert!(!d.feed(0.8, 100));
        for _ in 0..9 {
            assert!(!d.feed(0.1, 100));
        }
        assert!(d.feed(0.1, 100));
    }

    #[test]
    fn speech_resets_the_timer() {
        let mut d = SilenceDetector::new(500);
        d.feed(0.9, 100);
        for _ in 0..4 {
            assert!(!d.feed(0.0, 100));
        }
        d.feed(0.9, 100);
        for _ in 0..4 {
            assert!(!d.feed(0.0, 100));
        }
        assert!(d.feed(0.0, 100));
    }
}
