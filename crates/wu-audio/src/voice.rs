//! Sample voices: a fixed pool, allocated once, with stealing and choke groups.

use std::f32::consts::FRAC_PI_4;
use std::sync::Arc;

use wu_dsp::Sample;
use wu_instruments::{Pad, PadSound};

/// Frames a choked voice takes to fade out (8 ms at 48 kHz).
pub(crate) const CHOKE_FADE: u32 = 384;
/// Frames a stolen voice takes to fade out (2 ms at 48 kHz).
const STEAL_FADE: u32 = 96;

#[derive(Debug, Default)]
struct Voice {
    sample: Option<Arc<Sample>>,
    pad: Option<Pad>,
    pos: f64,
    step: f64,
    gain_l: f32,
    gain_r: f32,
    /// Frames into the current block before the voice starts sounding.
    delay: u32,
    choke: Option<u8>,
    /// Block offset where a fade-out begins, if one is scheduled.
    fade_at: Option<u32>,
    fade_len: u32,
    fade_pos: u32,
    /// Device frame the voice starts on; stealing takes the oldest first.
    starts_at: u64,
}

impl Voice {
    fn fade(&mut self, at: u32, len: u32) {
        if self.fade_at.is_none_or(|current| at < current) {
            self.fade_at = Some(at);
            self.fade_len = len.max(1);
            self.fade_pos = 0;
        }
    }

    /// Mixes the next `len` frames into `mix` (interleaved stereo).
    /// Returns `false` once the voice has finished.
    fn render(&mut self, mix: &mut [f32], len: usize) -> bool {
        let Some(sample) = self.sample.as_deref() else {
            return false;
        };
        let start = (self.delay as usize).min(len);
        self.delay -= start as u32;
        let frames = sample.frames() as f64;
        let mut alive = true;
        for i in start..len {
            let mut gain = 1.0;
            if let Some(fade_at) = self.fade_at
                && i as u32 >= fade_at
            {
                if self.fade_pos >= self.fade_len {
                    alive = false;
                    break;
                }
                gain = 1.0 - self.fade_pos as f32 / self.fade_len as f32;
                self.fade_pos += 1;
            }
            if self.pos >= frames {
                alive = false;
                break;
            }
            let (l, r) = interpolate(sample, self.pos);
            mix[2 * i] += l * self.gain_l * gain;
            mix[2 * i + 1] += r * self.gain_r * gain;
            self.pos += self.step;
        }
        if let Some(fade_at) = self.fade_at.as_mut() {
            *fade_at = fade_at.saturating_sub(len as u32);
        }
        alive
    }
}

fn interpolate(sample: &Sample, pos: f64) -> (f32, f32) {
    let i = pos as usize;
    let frac = (pos - i as f64) as f32;
    let (l0, r0) = sample.frame(i);
    if frac == 0.0 {
        return (l0, r0);
    }
    let (l1, r1) = sample.frame(i + 1);
    (l0 + (l1 - l0) * frac, r0 + (r1 - r0) * frac)
}

/// How a hit should sound, worked out before a voice is claimed.
#[derive(Debug)]
pub(crate) struct VoiceRequest<'a> {
    pub pad: Pad,
    pub sound: &'a PadSound,
    pub velocity: f32,
    pub delay: u32,
    pub starts_at: u64,
}

#[derive(Debug)]
pub(crate) struct VoicePool {
    voices: Vec<Voice>,
    /// Stolen voices finish their short fade here, so the slot they left is free at once.
    fading: Vec<Voice>,
    sample_rate: u32,
}

impl VoicePool {
    pub fn new(voices: usize, sample_rate: u32) -> VoicePool {
        VoicePool {
            voices: (0..voices).map(|_| Voice::default()).collect(),
            fading: (0..(voices / 4).max(1)).map(|_| Voice::default()).collect(),
            sample_rate,
        }
    }

    #[cfg(test)]
    pub fn active(&self) -> usize {
        self.voices
            .iter()
            .chain(&self.fading)
            .filter(|v| v.sample.is_some())
            .count()
    }

    /// Starts a voice, cutting off its choke group first and stealing the oldest
    /// voice if every slot is busy. `release` receives any sample a voice lets go of.
    pub fn start(&mut self, request: &VoiceRequest<'_>, release: &mut impl FnMut(Arc<Sample>)) {
        if let Some(group) = request.sound.choke {
            self.choke(group, request.delay);
        }
        let slot = match self.voices.iter().position(|v| v.sample.is_none()) {
            Some(free) => free,
            None => self.steal(release),
        };
        let sound = request.sound;
        // Equal-power pan; full velocity plays the pad at its kit gain.
        let angle = (sound.pan.clamp(-1.0, 1.0) + 1.0) * FRAC_PI_4;
        let gain = sound.gain * request.velocity.clamp(0.0, 1.0);
        self.voices[slot] = Voice {
            sample: Some(Arc::clone(&sound.sample)),
            pad: Some(request.pad),
            pos: 0.0,
            step: f64::from(sound.sample.sample_rate()) / f64::from(self.sample_rate),
            gain_l: gain * angle.cos(),
            gain_r: gain * angle.sin(),
            delay: request.delay,
            choke: sound.choke,
            fade_at: None,
            fade_len: 0,
            fade_pos: 0,
            starts_at: request.starts_at,
        };
    }

    /// Fades every voice in `group` from block offset `at`.
    pub fn choke(&mut self, group: u8, at: u32) {
        for voice in self
            .voices
            .iter_mut()
            .filter(|v| v.sample.is_some() && v.choke == Some(group))
        {
            voice.fade(at, CHOKE_FADE);
        }
    }

    /// Fades everything out quickly: the panic button.
    pub fn fade_all(&mut self) {
        for voice in self.voices.iter_mut().filter(|v| v.sample.is_some()) {
            voice.fade(0, CHOKE_FADE);
        }
    }

    /// Mixes every voice's next `len` frames into `mix`.
    pub fn render(&mut self, mix: &mut [f32], len: usize, release: &mut impl FnMut(Arc<Sample>)) {
        for voice in self.voices.iter_mut().chain(self.fading.iter_mut()) {
            if voice.sample.is_some() && !voice.render(mix, len) {
                voice.pad = None;
                if let Some(sample) = voice.sample.take() {
                    release(sample);
                }
            }
        }
    }

    fn steal(&mut self, release: &mut impl FnMut(Arc<Sample>)) -> usize {
        let oldest = self
            .voices
            .iter()
            .enumerate()
            .min_by_key(|(_, v)| v.starts_at)
            .map_or(0, |(i, _)| i);
        let mut stolen = std::mem::take(&mut self.voices[oldest]);
        stolen.fade(0, STEAL_FADE);
        let parking = match self.fading.iter().position(|v| v.sample.is_none()) {
            Some(free) => free,
            None => {
                let oldest_fading = self
                    .fading
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, v)| v.starts_at)
                    .map_or(0, |(i, _)| i);
                if let Some(sample) = self.fading[oldest_fading].sample.take() {
                    release(sample);
                }
                oldest_fading
            }
        };
        self.fading[parking] = stolen;
        oldest
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sound(samples: Vec<f32>, choke: Option<u8>) -> PadSound {
        PadSound {
            name: "test".into(),
            sample: Arc::new(Sample::mono(samples, 48_000)),
            gain: 1.0,
            pan: 0.0,
            choke,
        }
    }

    fn request(sound: &PadSound, delay: u32, starts_at: u64) -> VoiceRequest<'_> {
        VoiceRequest {
            pad: Pad::P1,
            sound,
            velocity: 1.0,
            delay,
            starts_at,
        }
    }

    #[test]
    fn a_voice_starts_on_its_delay_and_plays_its_sample() {
        let mut pool = VoicePool::new(4, 48_000);
        let s = sound(vec![1.0, 0.5], None);
        let mut released = 0;
        pool.start(&request(&s, 3, 0), &mut |_| released += 1);
        let mut mix = vec![0.0; 16];
        pool.render(&mut mix, 8, &mut |_| released += 1);
        let centre = FRAC_PI_4.cos();
        assert_eq!(&mix[..6], &[0.0; 6]);
        assert!((mix[6] - centre).abs() < 1e-6 && (mix[8] - 0.5 * centre).abs() < 1e-6);
        assert_eq!(mix[10], 0.0);
        assert_eq!((released, pool.active()), (1, 0));
    }

    #[test]
    fn delays_carry_over_into_later_blocks() {
        let mut pool = VoicePool::new(4, 48_000);
        let s = sound(vec![1.0; 4], None);
        pool.start(&request(&s, 10, 0), &mut |_| {});
        let mut mix = vec![0.0; 16];
        pool.render(&mut mix, 8, &mut |_| {});
        assert!(mix.iter().all(|&x| x == 0.0));
        let mut mix = vec![0.0; 16];
        pool.render(&mut mix, 8, &mut |_| {});
        assert_eq!(
            mix[2..4].iter().filter(|&&x| x > 0.0).count(),
            0,
            "frame 1 is still silent"
        );
        assert!(mix[4] > 0.0, "starts at frame 10 = block 2, offset 2");
    }

    #[test]
    fn choke_groups_cut_each_other_off() {
        let mut pool = VoicePool::new(4, 48_000);
        let open = sound(vec![1.0; 48_000], Some(1));
        pool.start(&request(&open, 0, 0), &mut |_| {});
        let closed = sound(vec![0.0; 10], Some(1));
        pool.start(&request(&closed, 100, 100), &mut |_| {});
        let len = 100 + CHOKE_FADE as usize + 10;
        let mut mix = vec![0.0; len * 2];
        pool.render(&mut mix, len, &mut |_| {});
        assert!(mix[2 * 99] > 0.7, "full level until the choke");
        assert!(mix[2 * (100 + CHOKE_FADE as usize / 2)] < 0.5, "fading");
        assert_eq!(mix[2 * (len - 1)], 0.0, "gone after the fade");
    }

    #[test]
    fn stealing_keeps_the_newest_voices() {
        let mut pool = VoicePool::new(2, 48_000);
        let long = sound(vec![0.1; 48_000], None);
        let mut released = 0;
        for i in 0..3 {
            pool.start(&request(&long, 0, i), &mut |_| released += 1);
        }
        assert_eq!(pool.voices.iter().map(|v| v.starts_at).collect::<Vec<_>>(), vec![2, 1]);
        assert_eq!(pool.active(), 3, "the stolen voice is still fading");
        let mut mix = vec![0.0; 2 * 256];
        pool.render(&mut mix, 256, &mut |_| released += 1);
        assert_eq!((pool.active(), released), (2, 1));
    }
}
