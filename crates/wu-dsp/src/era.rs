//! "Sampler Era": the sound of the 8- and 12-bit samplers and trackers early
//! jungle was made on. Band-limit, hold at a lower rate, quantise, then smooth
//! the steps the way a reconstruction filter would.

use crate::filter::Svf;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SamplerEra {
    /// Quantisation depth, 4–24.
    pub bits: u32,
    /// The rate the sound is held at, in Hz.
    pub rate_hz: f32,
}

impl SamplerEra {
    /// 8-bit, ~28 kHz: an Amiga-style tracker.
    pub const TRACKER: SamplerEra = SamplerEra {
        bits: 8,
        rate_hz: 28_000.0,
    };
    /// 12-bit, ~32 kHz: a late-80s rack sampler.
    pub const RACK_SAMPLER: SamplerEra = SamplerEra {
        bits: 12,
        rate_hz: 32_000.0,
    };
    /// 12-bit, 26.04 kHz: a sampling drum machine.
    pub const DRUM_MACHINE: SamplerEra = SamplerEra {
        bits: 12,
        rate_hz: 26_040.0,
    };

    pub fn apply(&self, input: &[f32], sample_rate: u32) -> Vec<f32> {
        let sr = sample_rate as f32;
        let rate = self.rate_hz.clamp(1000.0, sr);
        let mut anti_alias = Svf::new(0.45 * rate, 0.6, sample_rate);
        let mut reconstruct = Svf::new(0.45 * rate, 0.6, sample_rate);
        let levels = (1u64 << (self.bits.clamp(4, 24) - 1)) as f32;
        let step = rate / sr;
        let mut acc = 1.0f32;
        let mut held = 0.0f32;
        input
            .iter()
            .map(|&x| {
                let band_limited = anti_alias.process(x).low;
                acc += step;
                if acc >= 1.0 {
                    acc -= 1.0;
                    held = (band_limited * levels).round() / levels;
                }
                reconstruct.process(held).low
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_level_and_length() {
        let sr = 48_000;
        let input: Vec<f32> = (0..sr)
            .map(|i| (2.0 * std::f32::consts::PI * 200.0 * i as f32 / sr as f32).sin() * 0.5)
            .collect();
        let out = SamplerEra::DRUM_MACHINE.apply(&input, sr);
        assert_eq!(out.len(), input.len());
        let peak = out[sr as usize / 2..].iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!((peak - 0.5).abs() < 0.05, "peak {peak}");
    }

    #[test]
    fn eight_bits_quantise_audibly() {
        let input = vec![0.001f32; 4800];
        let out = SamplerEra::TRACKER.apply(&input, 48_000);
        // 0.001 is below half an 8-bit step: the tracker rounds it to silence.
        assert!(out[4000..].iter().all(|x| x.abs() < 1e-6));
    }
}
