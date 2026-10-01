//! Audio held in memory.

/// Interleaved frames with one or two channels.
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    data: Box<[f32]>,
    channels: u16,
    sample_rate: u32,
}

impl Sample {
    pub fn mono(data: Vec<f32>, sample_rate: u32) -> Sample {
        Sample {
            data: data.into_boxed_slice(),
            channels: 1,
            sample_rate,
        }
    }

    /// Interleaved left/right frames. A trailing half frame is dropped.
    pub fn stereo(mut interleaved: Vec<f32>, sample_rate: u32) -> Sample {
        interleaved.truncate(interleaved.len() / 2 * 2);
        Sample {
            data: interleaved.into_boxed_slice(),
            channels: 2,
            sample_rate,
        }
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn frames(&self) -> usize {
        self.data.len() / usize::from(self.channels)
    }

    pub fn duration_seconds(&self) -> f64 {
        self.frames() as f64 / f64::from(self.sample_rate)
    }

    pub fn data(&self) -> &[f32] {
        &self.data
    }

    /// Frame `i` as left/right; a mono sample feeds both sides. Out of range is silence.
    pub fn frame(&self, i: usize) -> (f32, f32) {
        if self.channels == 1 {
            let x = self.data.get(i).copied().unwrap_or(0.0);
            (x, x)
        } else {
            let l = self.data.get(2 * i).copied().unwrap_or(0.0);
            let r = self.data.get(2 * i + 1).copied().unwrap_or(0.0);
            (l, r)
        }
    }

    pub fn peak(&self) -> f32 {
        self.data.iter().fold(0.0f32, |m, x| m.max(x.abs()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_feeds_both_sides_and_stereo_splits() {
        let mono = Sample::mono(vec![0.5, -0.25], 48_000);
        assert_eq!(mono.frames(), 2);
        assert_eq!(mono.frame(1), (-0.25, -0.25));
        assert_eq!(mono.frame(9), (0.0, 0.0));

        let stereo = Sample::stereo(vec![0.1, 0.2, 0.3, 0.4, 0.5], 44_100);
        assert_eq!(stereo.frames(), 2);
        assert_eq!(stereo.frame(1), (0.3, 0.4));
        assert_eq!(stereo.peak(), 0.4);
    }
}
