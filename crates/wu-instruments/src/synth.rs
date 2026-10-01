//! Real-time synth voices, for the melodic instruments. A [`Patch`] describes
//! a sound (oscillators, filter, envelopes, modulation); a [`SynthVoice`] plays
//! one note of it, sample by sample on the audio thread, without allocating.

use std::f32::consts::{FRAC_PI_4, PI, TAU};

use wu_dsp::{Adsr, Envelope, Rng, Svf, midi_to_hz, polyblep_saw, polyblep_square, soft_clip};

use crate::bus::{Bus, Sends};

/// The most oscillators a voice stacks in unison.
pub const MAX_UNISON: usize = 7;
/// The most notes one key plays on a chord-memory patch.
pub const MAX_CHORD: usize = 4;
/// Modulation (filter envelope and LFO, sweeps, formants) is worked out every
/// this many samples: smooth enough for the ear, far cheaper than every sample.
const CONTROL_BLOCK: u32 = 16;
/// The note the filter's key tracking pivots on: C4.
const KEY_TRACK_PIVOT: f32 = 60.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Wave {
    Saw,
    Square,
    /// A pulse this wide (0–1); the LFO can move it (pulse-width modulation).
    Pulse(f32),
    Triangle,
    Sine,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterMode {
    Low,
    Band,
    High,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Filter {
    pub mode: FilterMode,
    pub cutoff_hz: f32,
    pub q: f32,
    /// Two filters in a row: 24 dB an octave instead of 12.
    pub steep: bool,
    /// How far the cutoff follows the note: 1 moves it an octave per octave.
    pub key_track: f32,
    /// How far the filter envelope opens it, in octaves.
    pub env_octaves: f32,
    pub env: Adsr,
    /// How many octaves darker a soft note is than a full one.
    pub velocity_octaves: f32,
}

impl Filter {
    /// A low-pass at `cutoff_hz` with nothing moving it.
    pub const fn low(cutoff_hz: f32, q: f32) -> Filter {
        Filter {
            mode: FilterMode::Low,
            cutoff_hz,
            q,
            steep: false,
            key_track: 0.0,
            env_octaves: 0.0,
            env: Adsr::new(0.0, 0.0, 1.0, 0.0),
            velocity_octaves: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LfoShape {
    Sine,
    Triangle,
    Square,
    SawDown,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lfo {
    pub shape: LfoShape,
    pub rate_hz: f32,
    /// If not zero, a cycle lasts this many beats: `Patch::at_tempo` sets the
    /// rate from the song's tempo.
    pub beats: f32,
    /// Depths: semitones of pitch, octaves of cutoff, share of level
    /// (tremolo), pulse width.
    pub pitch: f32,
    pub cutoff: f32,
    pub amp: f32,
    pub width: f32,
    /// Fades in over this long once the note starts (a delayed vibrato).
    pub fade_s: f32,
}

impl Lfo {
    pub const OFF: Lfo = Lfo {
        shape: LfoShape::Sine,
        rate_hz: 1.0,
        beats: 0.0,
        pitch: 0.0,
        cutoff: 0.0,
        amp: 0.0,
        width: 0.0,
        fade_s: 0.0,
    };

    /// -1 to 1 at `phase` (0–1).
    fn value(&self, phase: f32) -> f32 {
        match self.shape {
            LfoShape::Sine => (TAU * phase).sin(),
            LfoShape::Triangle => 1.0 - 4.0 * (phase - 0.5).abs(),
            LfoShape::Square => {
                if phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            LfoShape::SawDown => 1.0 - 2.0 * phase,
        }
    }
}

/// Frequency modulation: a sine carrier whose phase a sine at `ratio` times its
/// pitch pushes about, by an index falling from `index` to `index_sustain`
/// (bright attack, mellow body); and a second pair, bell-like, that dies away
/// fast (an electric piano's tine).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fm {
    pub ratio: f32,
    pub index: f32,
    pub index_sustain: f32,
    pub index_decay_s: f32,
    pub tine_ratio: f32,
    pub tine_index: f32,
    pub tine_level: f32,
    pub tine_decay_s: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vowel {
    A,
    E,
    I,
    O,
    U,
}

impl Vowel {
    /// The first three formants: centre (Hz), bandwidth (Hz), level. Adult
    /// averages from the classic vowel measurements.
    fn formants(self) -> [(f32, f32, f32); 3] {
        match self {
            Vowel::A => [(730.0, 90.0, 1.0), (1090.0, 110.0, 0.5), (2440.0, 140.0, 0.25)],
            Vowel::E => [(530.0, 80.0, 1.0), (1840.0, 110.0, 0.45), (2480.0, 140.0, 0.3)],
            Vowel::I => [(270.0, 60.0, 1.0), (2290.0, 120.0, 0.35), (3010.0, 160.0, 0.3)],
            Vowel::O => [(570.0, 80.0, 1.0), (840.0, 100.0, 0.55), (2410.0, 140.0, 0.15)],
            Vowel::U => [(300.0, 60.0, 1.0), (870.0, 100.0, 0.3), (2240.0, 140.0, 0.1)],
        }
    }
}

/// A sung vowel: the oscillators sung through three formant filters, gliding
/// along `path` over `glide_s` ("yeah" is I → E → A).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Formant {
    pub path: [Vowel; 3],
    pub glide_s: f32,
}

/// Movement over the note's whole length: a riser climbs to the drop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sweep {
    /// Pitch reached at the end, in semitones.
    pub semitones: f32,
    /// Cutoff reached at the end, in octaves.
    pub octaves: f32,
    /// The level at the start, rising to full by the end.
    pub level_from: f32,
}

/// Four all-pass stages swept by their own slow LFO: the Reese's churn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Phaser {
    pub rate_hz: f32,
    /// 0–1: how wide the sweep is.
    pub depth: f32,
    /// 0–1: how deep the notches are.
    pub mix: f32,
}

/// Everything a synth sound is. Plain data: copying a patch into a voice
/// costs nothing and allocates nothing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Patch {
    pub name: &'static str,
    pub wave: Wave,
    /// Oscillators stacked on each note (1–7).
    pub unison: u8,
    /// The outermost unison oscillators sit this many cents either side.
    pub detune_cents: f32,
    /// How far the unison oscillators spread across the stereo field (0–1).
    pub spread: f32,
    /// A sine an octave under the note.
    pub sub: f32,
    pub noise: f32,
    /// Replaces the oscillators.
    pub fm: Option<Fm>,
    pub formant: Option<Formant>,
    /// Chord memory: semitones each key plays, the first `chord_len` of them.
    pub chord: [i8; MAX_CHORD],
    pub chord_len: u8,
    pub transpose: i8,
    /// The note starts this many semitones off and glides onto pitch.
    pub pitch_env: f32,
    pub pitch_env_s: f32,
    pub sweep: Option<Sweep>,
    pub filter: Filter,
    /// A steep (24 dB an octave) high-pass after the filter, 0 for none: keeps
    /// a mid bass off the sub.
    pub high_pass_hz: f32,
    pub lfo: Lfo,
    pub phaser: Option<Phaser>,
    pub amp: Adsr,
    /// How much quieter a soft note is: 0 not at all, 1 in proportion.
    pub velocity: f32,
    /// Saturation before the filter: 0 for none.
    pub drive: f32,
    pub gain: f32,
    pub pan: f32,
    pub bus: Bus,
    pub sends: Sends,
}

impl Patch {
    /// The plainest patch: one saw, an open low-pass, an organ envelope.
    pub const BASIC: Patch = Patch {
        name: "Basic",
        wave: Wave::Saw,
        unison: 1,
        detune_cents: 0.0,
        spread: 0.0,
        sub: 0.0,
        noise: 0.0,
        fm: None,
        formant: None,
        chord: [0; MAX_CHORD],
        chord_len: 1,
        transpose: 0,
        pitch_env: 0.0,
        pitch_env_s: 0.1,
        sweep: None,
        filter: Filter::low(8_000.0, 0.707),
        high_pass_hz: 0.0,
        lfo: Lfo::OFF,
        phaser: None,
        amp: Adsr::new(0.005, 0.1, 1.0, 0.1),
        velocity: 0.5,
        drive: 0.0,
        gain: 0.3,
        pan: 0.0,
        bus: Bus::Music,
        sends: Sends::DRY,
    };

    /// The same patch with its tempo-synced LFO set for `bpm`.
    pub fn at_tempo(mut self, bpm: f64) -> Patch {
        if self.lfo.beats > 0.0 {
            self.lfo.rate_hz = (bpm / 60.0) as f32 / self.lfo.beats;
        }
        self
    }

    /// The semitones each key plays.
    pub fn chord(&self) -> &[i8] {
        &self.chord[..usize::from(self.chord_len.clamp(1, MAX_CHORD as u8))]
    }
}

/// Where a voice plays into: interleaved stereo, the block's length each.
#[derive(Debug)]
pub struct VoiceOut<'a> {
    pub bus: &'a mut [f32],
    pub reverb: &'a mut [f32],
    pub delay: &'a mut [f32],
}

/// One first-order all-pass per stage, per side.
#[derive(Clone, Copy, Debug, Default)]
struct AllPass {
    x1: f32,
    y1: f32,
}

impl AllPass {
    fn process(&mut self, x: f32, a: f32) -> f32 {
        let y = a * x + self.x1 - a * self.y1;
        self.x1 = x;
        self.y1 = y;
        y
    }
}

/// One note of a patch.
#[derive(Clone, Debug)]
pub struct SynthVoice {
    patch: Patch,
    sample_rate: f32,
    /// The note's own pitch (transposed, chord note included).
    key: f32,
    velocity: f32,
    /// Its length in samples, for sweeps; `None` until let go.
    length: Option<u32>,
    age: u32,
    rng: Rng,
    phases: [f32; MAX_UNISON],
    ratios: [f32; MAX_UNISON],
    pans: [(f32, f32); MAX_UNISON],
    unison_gain: f32,
    sub_phase: f32,
    /// Carrier and modulator phases of the body and tine pairs.
    fm_phases: [f32; 4],
    amp: Envelope,
    filter_env: Envelope,
    /// Left and right, then the second stage of each when steep.
    filters: [Svf; 4],
    /// Left and right, two stages each.
    high_pass: [Svf; 4],
    formants: [Svf; 3],
    formant_levels: [f32; 3],
    phaser: [[AllPass; 4]; 2],
    phaser_coefficient: f32,
    phaser_phase: f32,
    lfo_phase: f32,
    // Worked out each control block:
    control_left: u32,
    dt: f32,
    width: f32,
    fm_index: f32,
    tine_index: f32,
    tine_level: f32,
    level: f32,
    gain_l: f32,
    gain_r: f32,
    /// A quick fade to silence in progress: (samples left, samples in all).
    fade: Option<(u32, u32)>,
    active: bool,
}

impl SynthVoice {
    pub fn new(sample_rate: u32) -> SynthVoice {
        SynthVoice {
            patch: Patch::BASIC,
            sample_rate: sample_rate as f32,
            key: 60.0,
            velocity: 1.0,
            length: None,
            age: 0,
            rng: Rng::new(0),
            phases: [0.0; MAX_UNISON],
            ratios: [1.0; MAX_UNISON],
            pans: [(FRAC_PI_4.cos(), FRAC_PI_4.sin()); MAX_UNISON],
            unison_gain: 1.0,
            sub_phase: 0.0,
            fm_phases: [0.0; 4],
            amp: Envelope::default(),
            filter_env: Envelope::default(),
            filters: [(); 4].map(|_| Svf::new(1_000.0, 0.707, sample_rate)),
            high_pass: [(); 4].map(|_| Svf::new(100.0, 0.707, sample_rate)),
            formants: [(); 3].map(|_| Svf::new(1_000.0, 5.0, sample_rate)),
            formant_levels: [0.0; 3],
            phaser: [[AllPass::default(); 4]; 2],
            phaser_coefficient: 0.0,
            phaser_phase: 0.0,
            lfo_phase: 0.0,
            control_left: 0,
            dt: 0.0,
            width: 0.5,
            fm_index: 0.0,
            tine_index: 0.0,
            tine_level: 0.0,
            level: 0.0,
            gain_l: 0.0,
            gain_r: 0.0,
            fade: None,
            active: false,
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn bus(&self) -> Bus {
        self.patch.bus
    }

    /// Starts `key` (MIDI, before the patch's transposition) at `velocity`
    /// (0–1). `length` is how long it will be held, in samples, if known: a
    /// sweep spreads over it. `seed` scatters the unison oscillators' phases.
    pub fn start(&mut self, patch: &Patch, key: f32, velocity: f32, length: Option<u32>, seed: u64) {
        let sample_rate = self.sample_rate as u32;
        let mut rng = Rng::new(seed);
        let unison = usize::from(patch.unison.clamp(1, MAX_UNISON as u8));
        for i in 0..MAX_UNISON {
            // Spread evenly from one side (−1) to the other (+1).
            let at = if unison > 1 {
                2.0 * i as f32 / (unison - 1) as f32 - 1.0
            } else {
                0.0
            };
            self.ratios[i] = 2f32.powf(patch.detune_cents * at / 1200.0);
            let angle = (patch.spread.clamp(0.0, 1.0) * at + 1.0) * FRAC_PI_4;
            self.pans[i] = (angle.cos(), angle.sin());
            self.phases[i] = if unison > 1 { rng.next_f32() } else { 0.0 };
        }
        let angle = (patch.pan.clamp(-1.0, 1.0) + 1.0) * FRAC_PI_4;
        let velocity = velocity.clamp(0.0, 1.0);
        let level = patch.gain * (1.0 - patch.velocity.clamp(0.0, 1.0) * (1.0 - velocity));
        self.patch = *patch;
        self.key = key + f32::from(patch.transpose);
        self.velocity = velocity;
        self.length = length;
        self.age = 0;
        self.rng = rng;
        self.unison_gain = 1.0 / (unison as f32).sqrt();
        self.sub_phase = 0.0;
        self.fm_phases = [0.0; 4];
        self.amp = Envelope::new(&patch.amp, sample_rate);
        self.filter_env = Envelope::new(&patch.filter.env, sample_rate);
        self.phaser = [[AllPass::default(); 4]; 2];
        self.lfo_phase = 0.0;
        self.control_left = 0;
        self.gain_l = level * angle.cos();
        self.gain_r = level * angle.sin();
        self.fade = None;
        self.active = true;
        for filter in self
            .filters
            .iter_mut()
            .chain(&mut self.high_pass)
            .chain(&mut self.formants)
        {
            filter.reset();
        }
        self.amp.trigger();
        self.filter_env.trigger();
    }

    /// Lets go of the note: the envelopes release.
    pub fn release(&mut self) {
        self.amp.release();
        self.filter_env.release();
        if self.length.is_none() {
            self.length = Some(self.age.max(1));
        }
    }

    /// Fades to silence over `samples`, then stops: for a choke or a steal.
    pub fn fade_out(&mut self, samples: u32) {
        if self.fade.is_none() {
            let samples = samples.max(1);
            self.fade = Some((samples, samples));
        }
    }

    /// Works out the modulation for the next control block.
    fn control(&mut self) {
        let patch = &self.patch;
        let sr = self.sample_rate;
        let t = self.age as f32 / sr;
        let block = CONTROL_BLOCK as f32 / sr;
        self.lfo_phase = (self.lfo_phase + patch.lfo.rate_hz * block).fract();
        let fade_in = if patch.lfo.fade_s > 0.0 {
            (t / patch.lfo.fade_s).min(1.0)
        } else {
            1.0
        };
        let lfo = patch.lfo.value(self.lfo_phase) * fade_in;
        let progress = self
            .length
            .map_or(0.0, |l| (self.age as f32 / l.max(1) as f32).min(1.0));
        let sweep = patch.sweep.unwrap_or(Sweep {
            semitones: 0.0,
            octaves: 0.0,
            level_from: 1.0,
        });
        let glide = if patch.pitch_env_s > 0.0 {
            patch.pitch_env * (-3.0 * t / patch.pitch_env_s).exp()
        } else {
            0.0
        };
        let pitch = self.key + glide + lfo * patch.lfo.pitch + sweep.semitones * progress;
        self.dt = (midi_to_hz(pitch) / sr).min(0.45);

        let f = &patch.filter;
        let octaves = f.key_track * (self.key - KEY_TRACK_PIVOT) / 12.0
            + f.env_octaves * self.filter_env.level()
            + lfo * patch.lfo.cutoff
            + sweep.octaves * progress
            - f.velocity_octaves * (1.0 - self.velocity);
        let cutoff = f.cutoff_hz * 2f32.powf(octaves);
        let sample_rate = sr as u32;
        for filter in &mut self.filters {
            filter.set(cutoff, f.q, sample_rate);
        }
        if patch.high_pass_hz > 0.0 && self.age == 0 {
            for filter in &mut self.high_pass {
                filter.set(patch.high_pass_hz, 0.707, sample_rate);
            }
        }
        if let Some(formant) = patch.formant {
            // Along the path: first half from vowel 0 to 1, second from 1 to 2.
            let along = if formant.glide_s > 0.0 {
                2.0 * (t / formant.glide_s).min(1.0)
            } else {
                2.0
            };
            let (from, to, mix) = if along < 1.0 {
                (formant.path[0], formant.path[1], along)
            } else {
                (formant.path[1], formant.path[2], along - 1.0)
            };
            let (a, b) = (from.formants(), to.formants());
            for k in 0..3 {
                let centre = a[k].0 + (b[k].0 - a[k].0) * mix;
                let width = a[k].1 + (b[k].1 - a[k].1) * mix;
                let q = centre / width;
                self.formants[k].set(centre, q, sample_rate);
                // The band output peaks at Q: bring each formant back to unity.
                self.formant_levels[k] = (a[k].2 + (b[k].2 - a[k].2) * mix) / q;
            }
        }
        if let Some(fm) = patch.fm {
            let soft = 0.5 + 0.5 * self.velocity;
            self.fm_index = soft
                * (fm.index_sustain + (fm.index - fm.index_sustain) * (-3.0 * t / fm.index_decay_s.max(1e-3)).exp());
            let tine = (-3.0 * t / fm.tine_decay_s.max(1e-3)).exp();
            self.tine_index = soft * fm.tine_index * tine;
            self.tine_level = fm.tine_level * tine;
        }
        if let Wave::Pulse(width) = patch.wave {
            self.width = (width + lfo * patch.lfo.width).clamp(0.05, 0.95);
        }
        if let Some(phaser) = patch.phaser {
            self.phaser_phase = (self.phaser_phase + phaser.rate_hz * block).fract();
            let sweep = 0.5 + 0.5 * (TAU * self.phaser_phase).sin();
            let centre = 250.0 * 2f32.powf(4.0 * phaser.depth.clamp(0.0, 1.0) * sweep);
            let g = (PI * centre.min(0.45 * sr) / sr).tan();
            self.phaser_coefficient = (g - 1.0) / (g + 1.0);
        }
        let tremolo = 1.0 - patch.lfo.amp * (0.5 - 0.5 * lfo);
        let swell = sweep.level_from + (1.0 - sweep.level_from) * progress * progress;
        self.level = tremolo * swell;
    }

    /// One sample of the oscillators, before the filter: (left, right).
    fn oscillate(&mut self) -> (f32, f32) {
        let patch = &self.patch;
        let dt = self.dt;
        if let Some(fm) = patch.fm {
            let advance = |phase: &mut f32, step: f32| {
                let current = *phase;
                *phase = (*phase + step).fract();
                current
            };
            let modulator = (TAU * advance(&mut self.fm_phases[1], dt * fm.ratio)).sin();
            let body = (TAU * advance(&mut self.fm_phases[0], dt) + self.fm_index * modulator).sin();
            let tine_mod = (TAU * advance(&mut self.fm_phases[3], (dt * fm.tine_ratio).min(0.45))).sin();
            let tine = (TAU * advance(&mut self.fm_phases[2], dt) + self.tine_index * tine_mod).sin();
            let x = body + self.tine_level * tine;
            return (x, x);
        }
        let unison = usize::from(patch.unison.clamp(1, MAX_UNISON as u8));
        let (mut left, mut right) = (0.0, 0.0);
        for i in 0..unison {
            let step = (dt * self.ratios[i]).min(0.45);
            let t = self.phases[i];
            self.phases[i] = (t + step).fract();
            let x = match patch.wave {
                Wave::Saw => polyblep_saw(t, step),
                Wave::Square => polyblep_square(t, step),
                Wave::Pulse(_) => polyblep_saw(t, step) - polyblep_saw((t + 1.0 - self.width).fract(), step),
                Wave::Triangle => 1.0 - 4.0 * (t - 0.5).abs(),
                Wave::Sine => (TAU * t).sin(),
            };
            left += x * self.pans[i].0;
            right += x * self.pans[i].1;
        }
        // Equal-power pans put a centred oscillator at 0.707 a side: back to unity.
        let scale = self.unison_gain * std::f32::consts::SQRT_2;
        let (mut left, mut right) = (left * scale, right * scale);
        if patch.sub > 0.0 {
            let sub = patch.sub * (TAU * self.sub_phase).sin();
            self.sub_phase = (self.sub_phase + 0.5 * dt).fract();
            left += sub;
            right += sub;
        }
        if patch.noise > 0.0 {
            let noise = patch.noise * self.rng.noise();
            left += noise;
            right += noise;
        }
        (left, right)
    }

    /// Adds samples `from..to` of the block into `out`. Returns `false` once
    /// the note has died away.
    pub fn render(&mut self, out: &mut VoiceOut<'_>, from: usize, to: usize) -> bool {
        if !self.active {
            return false;
        }
        let sends = self.patch.sends;
        for i in from..to {
            if self.control_left == 0 {
                self.control();
                self.control_left = CONTROL_BLOCK;
            }
            self.control_left -= 1;
            let amp = self.amp.step();
            self.filter_env.step();
            let (mut left, mut right) = self.oscillate();
            let patch = &self.patch;
            if let Some(_formant) = patch.formant {
                let source = 0.5 * (left + right);
                let mut sung = 0.0;
                for (filter, level) in self.formants.iter_mut().zip(self.formant_levels) {
                    sung += level * filter.process(source).band;
                }
                left = sung;
                right = sung;
            }
            if patch.drive > 0.0 {
                left = soft_clip(left * patch.drive);
                right = soft_clip(right * patch.drive);
            }
            let pick = |out: wu_dsp::SvfOut| match patch.filter.mode {
                FilterMode::Low => out.low,
                FilterMode::Band => out.band,
                FilterMode::High => out.high,
            };
            left = pick(self.filters[0].process(left));
            right = pick(self.filters[1].process(right));
            if patch.filter.steep {
                left = pick(self.filters[2].process(left));
                right = pick(self.filters[3].process(right));
            }
            if patch.high_pass_hz > 0.0 {
                let [first_l, first_r, second_l, second_r] = &mut self.high_pass;
                left = second_l.process(first_l.process(left).high).high;
                right = second_r.process(first_r.process(right).high).high;
            }
            if let Some(phaser) = patch.phaser {
                let a = self.phaser_coefficient;
                let mix = phaser.mix.clamp(0.0, 1.0);
                for (side, x) in [&mut left, &mut right].into_iter().enumerate() {
                    let mut shifted = *x;
                    for stage in &mut self.phaser[side] {
                        shifted = stage.process(shifted, a);
                    }
                    *x = (*x + mix * shifted) / (1.0 + mix);
                }
            }
            let mut gain = amp * self.level;
            if let Some((left_in_fade, total)) = self.fade.as_mut() {
                gain *= *left_in_fade as f32 / *total as f32;
                *left_in_fade = left_in_fade.saturating_sub(1);
            }
            let (l, r) = (left * gain * self.gain_l, right * gain * self.gain_r);
            out.bus[2 * i] += l;
            out.bus[2 * i + 1] += r;
            if sends.reverb > 0.0 {
                out.reverb[2 * i] += l * sends.reverb;
                out.reverb[2 * i + 1] += r * sends.reverb;
            }
            if sends.delay > 0.0 {
                out.delay[2 * i] += l * sends.delay;
                out.delay[2 * i + 1] += r * sends.delay;
            }
            self.age = self.age.saturating_add(1);
            if self.amp.is_idle() || self.fade.is_some_and(|(left, _)| left == 0) {
                self.active = false;
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    /// Renders `patch` playing `key` for `held` samples, then until it stops
    /// (or `limit` samples): the left channel.
    fn play(patch: &Patch, key: f32, held: usize, limit: usize) -> Vec<f32> {
        let mut voice = SynthVoice::new(SR);
        voice.start(patch, key, 1.0, Some(held as u32), 7);
        let mut bus = vec![0.0; 2 * limit];
        let mut reverb = vec![0.0; 2 * limit];
        let mut delay = vec![0.0; 2 * limit];
        let mut out = VoiceOut {
            bus: &mut bus,
            reverb: &mut reverb,
            delay: &mut delay,
        };
        let alive = voice.render(&mut out, 0, held.min(limit));
        if alive && held < limit {
            voice.release();
            voice.render(&mut out, held, limit);
        }
        bus.iter().step_by(2).copied().collect()
    }

    /// The strongest frequency, by counting upward zero crossings.
    fn pitch_hz(samples: &[f32]) -> f32 {
        let crossings = samples.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        crossings as f32 * SR as f32 / samples.len() as f32
    }

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|x| x * x).sum::<f32>() / samples.len().max(1) as f32).sqrt()
    }

    #[test]
    fn a_sine_patch_plays_its_note() {
        let patch = Patch {
            wave: Wave::Sine,
            filter: Filter::low(20_000.0, 0.707),
            ..Patch::BASIC
        };
        let out = play(&patch, 69.0, 48_000, 48_000);
        let hz = pitch_hz(&out[4_800..]);
        assert!((hz - 440.0).abs() < 3.0, "{hz} Hz");
    }

    #[test]
    fn a_released_note_dies_away_and_stops() {
        let patch = Patch {
            amp: Adsr::new(0.001, 0.05, 0.8, 0.05),
            ..Patch::BASIC
        };
        let mut voice = SynthVoice::new(SR);
        voice.start(&patch, 48.0, 1.0, None, 1);
        let mut buffers = [vec![0.0; 2 * 9_600], vec![0.0; 2 * 9_600], vec![0.0; 2 * 9_600]];
        let [bus, reverb, delay] = &mut buffers;
        let mut out = VoiceOut { bus, reverb, delay };
        assert!(voice.render(&mut out, 0, 4_800));
        voice.release();
        assert!(!voice.render(&mut out, 4_800, 9_600), "silent within the release");
        assert!(!voice.is_active());
    }

    #[test]
    fn unison_and_spread_make_it_stereo_but_not_louder() {
        let wide = Patch {
            unison: 7,
            detune_cents: 20.0,
            spread: 1.0,
            ..Patch::BASIC
        };
        let mut voice = SynthVoice::new(SR);
        voice.start(&wide, 48.0, 1.0, None, 3);
        let mut bus = vec![0.0; 2 * 24_000];
        let (mut reverb, mut delay) = (vec![0.0; 2 * 24_000], vec![0.0; 2 * 24_000]);
        voice.render(
            &mut VoiceOut {
                bus: &mut bus,
                reverb: &mut reverb,
                delay: &mut delay,
            },
            0,
            24_000,
        );
        let left: Vec<f32> = bus.iter().step_by(2).copied().collect();
        let right: Vec<f32> = bus.iter().skip(1).step_by(2).copied().collect();
        let difference: Vec<f32> = left.iter().zip(&right).map(|(l, r)| l - r).collect();
        assert!(rms(&difference) > 0.1 * rms(&left), "the sides differ");
        let single = play(&Patch::BASIC, 48.0, 24_000, 24_000);
        let ratio = rms(&left) / rms(&single);
        assert!((0.5..2.0).contains(&ratio), "seven saws about as loud as one: {ratio}");
        assert!(reverb.iter().all(|&x| x == 0.0), "a dry patch sends nothing");
    }

    #[test]
    fn the_filter_envelope_brightens_the_attack() {
        let pluck = Patch {
            filter: Filter {
                cutoff_hz: 300.0,
                env_octaves: 5.0,
                env: Adsr::new(0.0005, 0.1, 0.0, 0.05),
                ..Filter::low(300.0, 0.707)
            },
            ..Patch::BASIC
        };
        let out = play(&pluck, 45.0, 24_000, 24_000);
        // Brightness: the share of energy in sample-to-sample changes.
        let bright = |s: &[f32]| rms(&s.windows(2).map(|w| w[1] - w[0]).collect::<Vec<_>>()) / rms(s);
        // The first 15 ms, while the envelope holds the filter open.
        let (attack, body) = (bright(&out[48..720]), bright(&out[16_000..20_000]));
        assert!(attack > 2.0 * body, "{attack} vs {body}");
    }

    #[test]
    fn a_sweep_climbs_over_the_note() {
        let riser = Patch {
            wave: Wave::Sine,
            sweep: Some(Sweep {
                semitones: 12.0,
                octaves: 0.0,
                level_from: 0.2,
            }),
            filter: Filter::low(20_000.0, 0.707),
            ..Patch::BASIC
        };
        let out = play(&riser, 57.0, 48_000, 48_000);
        let (start, end) = (pitch_hz(&out[0..9_600]), pitch_hz(&out[38_400..48_000]));
        assert!(end > 1.5 * start, "about an octave up by the end: {start} → {end}");
        assert!(rms(&out[38_400..48_000]) > 2.0 * rms(&out[0..9_600]), "and louder");
    }

    #[test]
    fn fm_and_formant_patches_make_sound() {
        let rhodes = Patch {
            fm: Some(Fm {
                ratio: 1.0,
                index: 2.0,
                index_sustain: 0.3,
                index_decay_s: 1.0,
                tine_ratio: 14.0,
                tine_index: 1.5,
                tine_level: 0.3,
                tine_decay_s: 0.2,
            }),
            ..Patch::BASIC
        };
        let vocal = Patch {
            formant: Some(Formant {
                path: [Vowel::I, Vowel::E, Vowel::A],
                glide_s: 0.2,
            }),
            ..Patch::BASIC
        };
        for patch in [rhodes, vocal] {
            let out = play(&patch, 57.0, 24_000, 30_000);
            assert!(out.iter().all(|x| x.is_finite()));
            assert!(rms(&out[2_400..24_000]) > 0.01, "{}", patch.name);
        }
    }

    #[test]
    fn a_tempo_synced_lfo_follows_the_song() {
        let wobble = Patch {
            lfo: Lfo { beats: 0.5, ..Lfo::OFF },
            ..Patch::BASIC
        };
        assert!(
            (wobble.at_tempo(120.0).lfo.rate_hz - 4.0).abs() < 1e-6,
            "eighths at 120"
        );
    }
}
