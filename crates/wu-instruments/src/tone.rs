//! Pitched instruments built from one baked sample: played back faster or
//! slower for each note, sustained through a loop, cut when the note ends.

use std::f32::consts::TAU;
use std::sync::Arc;

use wu_dsp::{Sample, soft_clip};

use crate::bus::Bus;

/// A pitched sound. `root_key` is the MIDI note the sample sounds at
/// (scientific pitch: 60 = C4); `sustain` is a range of frames that repeats
/// seamlessly for as long as a note is held.
#[derive(Clone, Debug)]
pub struct Tone {
    pub name: String,
    pub sample: Arc<Sample>,
    pub root_key: u8,
    pub sustain: Option<(usize, usize)>,
    pub gain: f32,
    pub pan: f32,
    pub bus: Bus,
}

impl Tone {
    /// Playback rate for `key`, relative to the root.
    pub fn rate(&self, key: u8) -> f64 {
        2f64.powf((f64::from(key) - f64::from(self.root_key)) / 12.0)
    }

    /// A sub bass: a sine with a little second harmonic and warmth, so it still
    /// reads on small speakers. Baked at A1 (55 Hz) over a whole number of
    /// cycles, so the sustain loop has no seam.
    pub fn sub(sample_rate: u32) -> Tone {
        const ROOT_HZ: f32 = 55.0;
        // 11 cycles of 55 Hz is a whole number of frames at every common rate
        // (48 000 → 9 600, 44 100 → 8 820, 96 000 → 19 200).
        const LOOP_CYCLES: f32 = 11.0;
        let sr = sample_rate as f32;
        let loop_len = (sr * LOOP_CYCLES / ROOT_HZ).round() as usize;
        let attack = (0.004 * sr) as usize;
        // The attack is a whole number of loops long too, so the loop starts on a cycle boundary.
        let loop_start = loop_len * attack.div_ceil(loop_len).max(1);
        let total = loop_start + loop_len;
        let drive = 1.6;
        let samples: Vec<f32> = (0..total)
            .map(|i| {
                let phase = ROOT_HZ * i as f32 / sr;
                let wave = (TAU * phase).sin() + 0.15 * (2.0 * TAU * phase).sin();
                let ramp = (i as f32 / attack.max(1) as f32).min(1.0);
                ramp * soft_clip(drive * wave) / soft_clip(drive)
            })
            .collect();
        Tone {
            name: "Sub".to_owned(),
            sample: Arc::new(Sample::mono(samples, sample_rate)),
            root_key: 33,
            sustain: Some((loop_start, total)),
            // Under the drums: a sustained sub at full level would push the master
            // into its limiter for the whole drop.
            gain: 0.55,
            pan: 0.0,
            bus: Bus::Bass,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sub_loops_without_a_seam() {
        for rate in [44_100, 48_000, 96_000] {
            let sub = Tone::sub(rate);
            let (start, end) = sub.sustain.expect("sustains");
            let data = sub.sample.data();
            assert_eq!(end, data.len());
            // The frame after the loop end is the loop start: the step across the
            // seam must look like any other step of the wave.
            let seam = (data[start] - data[end - 1]).abs();
            let typical = (data[start + 1] - data[start])
                .abs()
                .max((data[end - 1] - data[end - 2]).abs());
            assert!(seam <= typical * 1.5 + 1e-4, "rate {rate}: seam {seam} vs {typical}");
        }
    }

    #[test]
    fn rates_follow_equal_temperament() {
        let sub = Tone::sub(48_000);
        assert!((sub.rate(33) - 1.0).abs() < 1e-12);
        assert!((sub.rate(45) - 2.0).abs() < 1e-12, "an octave up");
        assert!((sub.rate(29) - 2f64.powf(-4.0 / 12.0)).abs() < 1e-12, "F1");
    }
}
