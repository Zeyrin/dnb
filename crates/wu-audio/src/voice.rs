//! Sample voices: a fixed pool, allocated once, with stealing and choke groups.

use std::f32::consts::FRAC_PI_4;
use std::sync::Arc;

use wu_dsp::Sample;
use wu_instruments::{Bus, Pad, PadSound, Tone};

use crate::mixer::BUS_COUNT;

/// Frames a choked voice takes to fade out (8 ms at 48 kHz).
pub(crate) const CHOKE_FADE: u32 = 384;
/// Frames a stolen voice takes to fade out (2 ms at 48 kHz).
const STEAL_FADE: u32 = 96;
/// How long a released note takes to die away, in seconds.
const RELEASE_SECONDS: f64 = 0.015;

#[derive(Debug, Default)]
struct Voice {
    sample: Option<Arc<Sample>>,
    pad: Option<Pad>,
    /// Index of the bus it plays into.
    bus: usize,
    /// The rail playing it, for a live note.
    rail: Option<u8>,
    pos: f64,
    step: f64,
    /// A stretch of the sample that repeats while the note is held.
    sustain: Option<(f64, f64)>,
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
            let (l, r) = interpolate(sample, self.pos, self.sustain);
            mix[2 * i] += l * self.gain_l * gain;
            mix[2 * i + 1] += r * self.gain_r * gain;
            self.pos += self.step;
            if let Some((loop_start, loop_end)) = self.sustain
                && self.pos >= loop_end
            {
                self.pos -= loop_end - loop_start;
            }
        }
        if let Some(fade_at) = self.fade_at.as_mut() {
            *fade_at = fade_at.saturating_sub(len as u32);
        }
        alive
    }
}

/// Linear interpolation between neighbouring frames; inside a sustain loop the
/// frame after the loop end is the loop start.
fn interpolate(sample: &Sample, pos: f64, sustain: Option<(f64, f64)>) -> (f32, f32) {
    let i = pos as usize;
    let frac = (pos - i as f64) as f32;
    let (l0, r0) = sample.frame(i);
    if frac == 0.0 {
        return (l0, r0);
    }
    let next = match sustain {
        Some((loop_start, loop_end)) if (i + 1) as f64 >= loop_end => loop_start as usize,
        _ => i + 1,
    };
    let (l1, r1) = sample.frame(next);
    (l0 + (l1 - l0) * frac, r0 + (r1 - r0) * frac)
}

/// How a sound should play, worked out before a voice is claimed.
#[derive(Debug)]
pub(crate) struct VoiceRequest<'a> {
    pub pad: Option<Pad>,
    pub sample: &'a Arc<Sample>,
    pub gain: f32,
    pub pan: f32,
    pub choke: Option<u8>,
    pub velocity: f32,
    /// Playback rate relative to the sample's own pitch.
    pub rate: f64,
    pub sustain: Option<(usize, usize)>,
    /// Frames after the start at which the note is released.
    pub gate: Option<u32>,
    pub bus: Bus,
    /// It ducks the bass bus.
    pub sidechain: bool,
    /// The rail playing it, for a live note: letting go of the rail releases it.
    pub rail: Option<u8>,
    pub delay: u32,
    pub starts_at: u64,
}

impl<'a> VoiceRequest<'a> {
    /// A one-shot drum hit.
    pub fn pad(pad: Pad, sound: &'a PadSound, velocity: f32, delay: u32, starts_at: u64) -> VoiceRequest<'a> {
        VoiceRequest {
            pad: Some(pad),
            sample: &sound.sample,
            gain: sound.gain,
            pan: sound.pan,
            choke: sound.choke,
            velocity,
            rate: 1.0,
            sustain: None,
            gate: None,
            bus: sound.bus,
            sidechain: sound.sidechain,
            rail: None,
            delay,
            starts_at,
        }
    }

    /// A held note: pitched by rate, sustained through its loop, released after `gate` frames.
    pub fn note(tone: &'a Tone, key: u8, velocity: f32, gate: u32, delay: u32, starts_at: u64) -> VoiceRequest<'a> {
        VoiceRequest {
            pad: None,
            sample: &tone.sample,
            gain: tone.gain,
            pan: tone.pan,
            choke: None,
            velocity,
            rate: tone.rate(key),
            sustain: tone.sustain,
            gate: Some(gate),
            bus: tone.bus,
            sidechain: false,
            rail: None,
            delay,
            starts_at,
        }
    }
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
        if let Some(group) = request.choke {
            self.choke(group, request.delay);
        }
        let slot = match self.voices.iter().position(|v| v.sample.is_none()) {
            Some(free) => free,
            None => self.steal(release),
        };
        // Equal-power pan; full velocity plays at the sound's own gain.
        let angle = (request.pan.clamp(-1.0, 1.0) + 1.0) * FRAC_PI_4;
        let gain = request.gain * request.velocity.clamp(0.0, 1.0);
        let mut voice = Voice {
            sample: Some(Arc::clone(request.sample)),
            pad: request.pad,
            bus: request.bus.index(),
            rail: request.rail,
            pos: 0.0,
            step: request.rate * f64::from(request.sample.sample_rate()) / f64::from(self.sample_rate),
            sustain: request.sustain.map(|(start, end)| (start as f64, end as f64)),
            gain_l: gain * angle.cos(),
            gain_r: gain * angle.sin(),
            delay: request.delay,
            choke: request.choke,
            fade_at: None,
            fade_len: 0,
            fade_pos: 0,
            starts_at: request.starts_at,
        };
        if let Some(gate) = request.gate {
            let release = (RELEASE_SECONDS * f64::from(self.sample_rate)) as u32;
            voice.fade(request.delay.saturating_add(gate), release);
        }
        self.voices[slot] = voice;
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

    /// Releases the note a rail is playing, from block offset `at`.
    pub fn release_rail(&mut self, rail: u8, at: u32) {
        let release = (RELEASE_SECONDS * f64::from(self.sample_rate)) as u32;
        for voice in self
            .voices
            .iter_mut()
            .filter(|v| v.sample.is_some() && v.rail == Some(rail))
        {
            voice.fade(at, release);
            voice.rail = None;
        }
    }

    /// Fades everything out quickly: the panic button.
    pub fn fade_all(&mut self) {
        for voice in self.voices.iter_mut().filter(|v| v.sample.is_some()) {
            voice.fade(0, CHOKE_FADE);
        }
    }

    /// Mixes every voice's next `len` frames into its bus.
    pub fn render(&mut self, buses: &mut [Vec<f32>; BUS_COUNT], len: usize, release: &mut impl FnMut(Arc<Sample>)) {
        for voice in self.voices.iter_mut().chain(self.fading.iter_mut()) {
            if voice.sample.is_some() && !voice.render(&mut buses[voice.bus], len) {
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
            bus: Bus::Drums,
            sidechain: false,
        }
    }

    /// Renders `len` frames and sums the buses.
    fn mixdown(pool: &mut VoicePool, len: usize, release: &mut impl FnMut(Arc<Sample>)) -> Vec<f32> {
        let mut buses = std::array::from_fn(|_| vec![0.0; len * 2]);
        pool.render(&mut buses, len, release);
        (0..len * 2).map(|i| buses.iter().map(|bus| bus[i]).sum()).collect()
    }

    fn request(sound: &PadSound, delay: u32, starts_at: u64) -> VoiceRequest<'_> {
        VoiceRequest::pad(Pad::P1, sound, 1.0, delay, starts_at)
    }

    #[test]
    fn a_held_note_loops_past_the_end_of_its_sample_then_releases() {
        let mut pool = VoicePool::new(4, 48_000);
        let tone = Tone {
            name: "square".into(),
            sample: Arc::new(Sample::mono(vec![0.0, 0.0, 1.0, -1.0], 48_000)),
            root_key: 60,
            sustain: Some((2, 4)),
            gain: 1.0,
            pan: 0.0,
            bus: Bus::Bass,
        };
        // Held for 1000 frames: far longer than the four-frame sample.
        pool.start(&VoiceRequest::note(&tone, 60, 1.0, 1000, 0, 0), &mut |_| {});
        let len = 1000 + 720 + 10;
        let mix = mixdown(&mut pool, len, &mut |_| {});
        let centre = FRAC_PI_4.cos();
        assert!((mix[2 * 998] - centre).abs() < 1e-6, "still looping at frame 998");
        assert!((mix[2 * 999] + centre).abs() < 1e-6);
        assert!(
            (mix[2 * 1360].abs() - 0.5 * centre).abs() < 0.01,
            "half way through the release"
        );
        assert_eq!(mix[2 * (len - 1)], 0.0, "silent after the release");
        assert_eq!(pool.active(), 0);
    }

    #[test]
    fn notes_play_at_their_pitch() {
        let mut pool = VoicePool::new(4, 48_000);
        let ramp: Vec<f32> = (0..100).map(|i| i as f32 / 100.0).collect();
        let tone = Tone {
            name: "ramp".into(),
            sample: Arc::new(Sample::mono(ramp, 48_000)),
            root_key: 60,
            sustain: None,
            gain: 1.0,
            pan: 0.0,
            bus: Bus::Bass,
        };
        // An octave up plays the sample twice as fast.
        pool.start(&VoiceRequest::note(&tone, 72, 1.0, 10_000, 0, 0), &mut |_| {});
        let mix = mixdown(&mut pool, 10, &mut |_| {});
        let centre = FRAC_PI_4.cos();
        assert!((mix[2 * 5] - 0.10 * centre).abs() < 1e-6);
    }

    #[test]
    fn a_voice_starts_on_its_delay_and_plays_its_sample() {
        let mut pool = VoicePool::new(4, 48_000);
        let s = sound(vec![1.0, 0.5], None);
        let mut released = 0;
        pool.start(&request(&s, 3, 0), &mut |_| released += 1);
        let mix = mixdown(&mut pool, 8, &mut |_| released += 1);
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
        let mix = mixdown(&mut pool, 8, &mut |_| {});
        assert!(mix.iter().all(|&x| x == 0.0));
        let mix = mixdown(&mut pool, 8, &mut |_| {});
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
        let mix = mixdown(&mut pool, len, &mut |_| {});
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
        mixdown(&mut pool, 256, &mut |_| released += 1);
        assert_eq!((pool.active(), released), (2, 1));
    }
}
