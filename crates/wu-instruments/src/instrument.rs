//! The built-in instruments, by name: synth patches played in real time, and
//! baked samples pitched by playback rate (the sub, the FX one-shots).

use std::sync::Arc;

use wu_dsp::{Adsr, Sample, db_to_gain};

use crate::bus::{Bus, Sends};
use crate::fx;
use crate::synth::{Filter, FilterMode, Fm, Formant, Lfo, LfoShape, MAX_CHORD, Patch, Phaser, Sweep, Vowel, Wave};
use crate::tone::Tone;

#[derive(Clone, Debug)]
pub enum Instrument {
    /// A baked sample, pitched by playback rate.
    Sampled(Tone),
    /// A synth voice per note.
    Synth(Patch),
}

/// Every built-in instrument, in the order a menu lists them.
pub const INSTRUMENTS: [&str; 20] = [
    "sub",
    "reese",
    "wobble",
    "rave-stab",
    "organ-stab",
    "hoover",
    "atmos-pad",
    "supersaw-pad",
    "fm-rhodes",
    "pluck",
    "vocal-ah",
    "vocal-oh",
    "vocal-yeah",
    "dub-siren",
    "air-horn",
    "riser",
    "downlifter",
    "impact",
    "spinback",
    "crowd",
];

impl Instrument {
    /// A built-in instrument (see `INSTRUMENTS`), baked for `sample_rate` if it
    /// is sampled.
    pub fn named(name: &str, sample_rate: u32) -> Option<Instrument> {
        let one_shot = |name: &str, samples: Vec<f32>, root_key: u8, gain: f32, pan: f32, sends: Sends| {
            Instrument::Sampled(Tone {
                name: name.to_owned(),
                sample: Arc::new(Sample::mono(samples, sample_rate)),
                root_key,
                sustain: None,
                gain,
                pan,
                bus: Bus::Fx,
                sends,
                tune: 1.0,
            })
        };
        let patch = match name {
            "sub" => return Some(Instrument::Sampled(Tone::sub(sample_rate))),
            // The horn's chord sits on G♯4; the others play as recorded on C4.
            "air-horn" => {
                return Some(one_shot(
                    "Air Horn",
                    fx::air_horn(sample_rate),
                    68,
                    0.45,
                    0.2,
                    Sends {
                        reverb: 0.15,
                        delay: 0.4,
                    },
                ));
            }
            "spinback" => {
                return Some(one_shot(
                    "Spinback",
                    fx::spinback(sample_rate, 0x5917),
                    60,
                    0.7,
                    0.0,
                    Sends {
                        reverb: 0.2,
                        delay: 0.0,
                    },
                ));
            }
            "crowd" => {
                return Some(one_shot(
                    "Crowd",
                    fx::crowd(sample_rate, 0xC40D),
                    60,
                    0.35,
                    -0.1,
                    Sends {
                        reverb: 0.3,
                        delay: 0.0,
                    },
                ));
            }
            "reese" => REESE,
            "wobble" => WOBBLE,
            "rave-stab" => RAVE_STAB,
            "organ-stab" => ORGAN_STAB,
            "hoover" => HOOVER,
            "atmos-pad" => ATMOS_PAD,
            "supersaw-pad" => SUPERSAW_PAD,
            "fm-rhodes" => FM_RHODES,
            "pluck" => PLUCK,
            "vocal-ah" => VOCAL_AH,
            "vocal-oh" => VOCAL_OH,
            "vocal-yeah" => VOCAL_YEAH,
            "dub-siren" => DUB_SIREN,
            "riser" => RISER,
            "downlifter" => DOWNLIFTER,
            "impact" => IMPACT,
            _ => return None,
        };
        Some(Instrument::Synth(patch))
    }

    pub fn name(&self) -> &str {
        match self {
            Instrument::Sampled(tone) => &tone.name,
            Instrument::Synth(patch) => patch.name,
        }
    }

    pub fn bus(&self) -> Bus {
        match self {
            Instrument::Sampled(tone) => tone.bus,
            Instrument::Synth(patch) => patch.bus,
        }
    }

    pub fn sends(&self) -> Sends {
        match self {
            Instrument::Sampled(tone) => tone.sends,
            Instrument::Synth(patch) => patch.sends,
        }
    }

    /// The semitones each key plays (chord memory), the root first.
    pub fn chord(&self) -> &[i8] {
        match self {
            Instrument::Sampled(_) => &[0],
            Instrument::Synth(patch) => patch.chord(),
        }
    }

    /// Louder or softer by `db`.
    pub fn with_level_db(mut self, db: f32) -> Instrument {
        let gain = db_to_gain(db);
        match &mut self {
            Instrument::Sampled(tone) => tone.gain *= gain,
            Instrument::Synth(patch) => patch.gain *= gain,
        }
        self
    }

    pub fn with_pan(mut self, pan: f32) -> Instrument {
        match &mut self {
            Instrument::Sampled(tone) => tone.pan = pan,
            Instrument::Synth(patch) => patch.pan = pan,
        }
        self
    }

    pub fn with_sends(mut self, sends: Sends) -> Instrument {
        match &mut self {
            Instrument::Sampled(tone) => tone.sends = sends,
            Instrument::Synth(patch) => patch.sends = sends,
        }
        self
    }

    pub fn with_bus(mut self, bus: Bus) -> Instrument {
        match &mut self {
            Instrument::Sampled(tone) => tone.bus = bus,
            Instrument::Synth(patch) => patch.bus = bus,
        }
        self
    }

    /// Tempo-synced modulation set for `bpm`.
    pub fn at_tempo(self, bpm: f64) -> Instrument {
        match self {
            Instrument::Synth(patch) => Instrument::Synth(patch.at_tempo(bpm)),
            sampled => sampled,
        }
    }
}

/// Chord memory: `notes` played on every key.
const fn chord(notes: &[i8]) -> ([i8; MAX_CHORD], u8) {
    let mut chord = [0; MAX_CHORD];
    let mut i = 0;
    while i < notes.len() && i < MAX_CHORD {
        chord[i] = notes[i];
        i += 1;
    }
    (chord, i as u8)
}

const MINOR_SEVENTH: ([i8; MAX_CHORD], u8) = chord(&[0, 3, 7, 10]);
const FIFTH_AND_OCTAVE: ([i8; MAX_CHORD], u8) = chord(&[0, 7, 12]);

/// Two to seven detuned saws churning against each other, low-passed, the
/// filter breathing once a bar and a phaser turning it over: the jungle and
/// darkside mid-bass. High-passed, so it sits over the sub, not on it.
pub const REESE: Patch = Patch {
    name: "Reese",
    wave: Wave::Saw,
    unison: 4,
    detune_cents: 16.0,
    spread: 0.3,
    filter: Filter {
        mode: FilterMode::Low,
        cutoff_hz: 900.0,
        q: 1.1,
        steep: true,
        key_track: 0.0,
        env_octaves: 0.8,
        env: Adsr::new(0.002, 0.25, 0.3, 0.2),
        velocity_octaves: 0.5,
    },
    high_pass_hz: 110.0,
    lfo: Lfo {
        shape: LfoShape::Sine,
        rate_hz: 0.7,
        beats: 4.0,
        cutoff: 0.6,
        ..Lfo::OFF
    },
    phaser: Some(Phaser {
        rate_hz: 0.17,
        depth: 0.6,
        mix: 0.6,
    }),
    amp: Adsr::new(0.004, 0.3, 0.85, 0.08),
    velocity: 0.3,
    drive: 1.5,
    gain: 0.5,
    bus: Bus::Bass,
    ..Patch::BASIC
};

/// A resonant low-pass swung by an LFO in eighth notes, with its own sub.
pub const WOBBLE: Patch = Patch {
    name: "Wobble",
    wave: Wave::Saw,
    unison: 2,
    detune_cents: 8.0,
    spread: 0.2,
    sub: 0.5,
    filter: Filter {
        q: 4.0,
        ..Filter::low(400.0, 4.0)
    },
    lfo: Lfo {
        shape: LfoShape::Sine,
        rate_hz: 5.6,
        beats: 0.5,
        cutoff: 2.0,
        ..Lfo::OFF
    },
    amp: Adsr::new(0.005, 0.2, 0.9, 0.1),
    velocity: 0.3,
    drive: 2.0,
    gain: 0.4,
    bus: Bus::Bass,
    ..Patch::BASIC
};

/// The rave stab: a minor seventh on every key (chord memory), bright for an
/// instant and gone.
pub const RAVE_STAB: Patch = Patch {
    name: "Rave Stab",
    wave: Wave::Saw,
    unison: 3,
    detune_cents: 12.0,
    spread: 0.6,
    chord: MINOR_SEVENTH.0,
    chord_len: MINOR_SEVENTH.1,
    filter: Filter {
        mode: FilterMode::Low,
        cutoff_hz: 700.0,
        q: 1.6,
        steep: false,
        key_track: 0.5,
        env_octaves: 3.8,
        env: Adsr::new(0.001, 0.2, 0.15, 0.15),
        velocity_octaves: 1.0,
    },
    amp: Adsr::new(0.002, 0.45, 0.0, 0.15),
    drive: 1.2,
    gain: 0.2,
    sends: Sends {
        reverb: 0.25,
        delay: 0.3,
    },
    ..Patch::BASIC
};

/// A square organ with a sub, a fifth and an octave on every key.
pub const ORGAN_STAB: Patch = Patch {
    name: "Organ Stab",
    wave: Wave::Square,
    unison: 2,
    detune_cents: 5.0,
    spread: 0.4,
    sub: 0.5,
    chord: FIFTH_AND_OCTAVE.0,
    chord_len: FIFTH_AND_OCTAVE.1,
    filter: Filter {
        env_octaves: 1.0,
        env: Adsr::new(0.001, 0.15, 0.0, 0.1),
        ..Filter::low(2_400.0, 0.9)
    },
    amp: Adsr::new(0.001, 0.25, 0.0, 0.08),
    gain: 0.2,
    sends: Sends {
        reverb: 0.2,
        delay: 0.25,
    },
    ..Patch::BASIC
};

/// The hoover: detuned pulses whose widths swim, scooping up a fifth into
/// every note.
pub const HOOVER: Patch = Patch {
    name: "Hoover",
    wave: Wave::Pulse(0.5),
    unison: 5,
    detune_cents: 28.0,
    spread: 0.8,
    sub: 0.35,
    pitch_env: -7.0,
    pitch_env_s: 0.12,
    lfo: Lfo {
        shape: LfoShape::Triangle,
        rate_hz: 5.0,
        pitch: 0.15,
        width: 0.35,
        ..Lfo::OFF
    },
    filter: Filter {
        env_octaves: 0.6,
        env: Adsr::new(0.005, 0.4, 0.5, 0.3),
        ..Filter::low(3_200.0, 0.8)
    },
    amp: Adsr::new(0.01, 0.3, 0.85, 0.35),
    drive: 1.6,
    gain: 0.25,
    sends: Sends {
        reverb: 0.2,
        delay: 0.2,
    },
    ..Patch::BASIC
};

/// Slow strings of detuned saws and a breath of air, drifting in and out of focus.
pub const ATMOS_PAD: Patch = Patch {
    name: "Atmos Pad",
    wave: Wave::Saw,
    unison: 6,
    detune_cents: 14.0,
    spread: 0.9,
    noise: 0.015,
    filter: Filter {
        mode: FilterMode::Low,
        cutoff_hz: 1_500.0,
        q: 0.7,
        steep: false,
        key_track: 0.3,
        env_octaves: 0.5,
        env: Adsr::new(1.5, 2.0, 0.6, 2.0),
        velocity_octaves: 0.3,
    },
    lfo: Lfo {
        shape: LfoShape::Sine,
        rate_hz: 0.12,
        cutoff: 0.5,
        ..Lfo::OFF
    },
    amp: Adsr::new(1.4, 1.5, 0.85, 2.2),
    gain: 0.16,
    sends: Sends {
        reverb: 0.55,
        delay: 0.1,
    },
    ..Patch::BASIC
};

/// Seven saws spread wide: the big euphoric pad.
pub const SUPERSAW_PAD: Patch = Patch {
    name: "Supersaw Pad",
    wave: Wave::Saw,
    unison: 7,
    detune_cents: 30.0,
    spread: 1.0,
    filter: Filter::low(3_000.0, 0.7),
    amp: Adsr::new(0.6, 1.0, 0.9, 1.5),
    gain: 0.14,
    sends: Sends {
        reverb: 0.45,
        delay: 0.0,
    },
    ..Patch::BASIC
};

/// An electric piano by FM: a mellowing body and a bell-like tine, with tremolo.
pub const FM_RHODES: Patch = Patch {
    name: "FM Rhodes",
    fm: Some(Fm {
        ratio: 1.0,
        index: 1.8,
        index_sustain: 0.35,
        index_decay_s: 1.4,
        tine_ratio: 14.0,
        tine_index: 1.4,
        tine_level: 0.28,
        tine_decay_s: 0.3,
    }),
    filter: Filter::low(9_000.0, 0.707),
    lfo: Lfo {
        shape: LfoShape::Sine,
        rate_hz: 4.8,
        amp: 0.18,
        ..Lfo::OFF
    },
    amp: Adsr::new(0.002, 3.0, 0.0, 0.5),
    velocity: 0.7,
    gain: 0.3,
    sends: Sends {
        reverb: 0.3,
        delay: 0.0,
    },
    ..Patch::BASIC
};

/// A plucked saw: the filter snaps shut, the dub delay carries it on.
pub const PLUCK: Patch = Patch {
    name: "Pluck",
    wave: Wave::Saw,
    unison: 2,
    detune_cents: 7.0,
    spread: 0.5,
    filter: Filter {
        mode: FilterMode::Low,
        cutoff_hz: 380.0,
        q: 2.2,
        steep: false,
        key_track: 0.5,
        env_octaves: 4.2,
        env: Adsr::new(0.0005, 0.14, 0.0, 0.08),
        velocity_octaves: 1.0,
    },
    amp: Adsr::new(0.001, 0.5, 0.0, 0.1),
    gain: 0.35,
    sends: Sends {
        reverb: 0.2,
        delay: 0.45,
    },
    ..Patch::BASIC
};

/// A synthesised voice singing along `path` (no recorded vocals anywhere).
const fn vocal(name: &'static str, path: [Vowel; 3], glide_s: f32) -> Patch {
    Patch {
        name,
        wave: Wave::Saw,
        noise: 0.05,
        formant: Some(Formant { path, glide_s }),
        pitch_env: -1.0,
        pitch_env_s: 0.08,
        filter: Filter::low(6_000.0, 0.707),
        lfo: Lfo {
            shape: LfoShape::Sine,
            rate_hz: 5.4,
            pitch: 0.3,
            fade_s: 0.35,
            ..Lfo::OFF
        },
        amp: Adsr::new(0.05, 0.3, 0.8, 0.25),
        gain: 1.6,
        sends: Sends {
            reverb: 0.35,
            delay: 0.25,
        },
        ..Patch::BASIC
    }
}

pub const VOCAL_AH: Patch = vocal("Vocal Ah", [Vowel::A; 3], 0.0);
pub const VOCAL_OH: Patch = vocal("Vocal Oh", [Vowel::O; 3], 0.0);
pub const VOCAL_YEAH: Patch = vocal("Vocal Yeah", [Vowel::I, Vowel::E, Vowel::A], 0.3);

/// The dub siren: a square whose pitch an LFO swings, rising as it's held,
/// thrown into the delay.
pub const DUB_SIREN: Patch = Patch {
    name: "Dub Siren",
    wave: Wave::Square,
    lfo: Lfo {
        shape: LfoShape::Triangle,
        rate_hz: 4.5,
        pitch: 5.0,
        ..Lfo::OFF
    },
    sweep: Some(Sweep {
        semitones: 7.0,
        octaves: 0.0,
        level_from: 1.0,
    }),
    filter: Filter::low(2_800.0, 1.2),
    amp: Adsr::new(0.01, 0.1, 1.0, 0.25),
    gain: 0.18,
    bus: Bus::Fx,
    sends: Sends {
        reverb: 0.25,
        delay: 0.6,
    },
    ..Patch::BASIC
};

/// Noise and saws climbing for as long as the note lasts: the build to a drop.
pub const RISER: Patch = Patch {
    name: "Riser",
    wave: Wave::Saw,
    unison: 3,
    detune_cents: 25.0,
    spread: 0.7,
    noise: 0.7,
    sweep: Some(Sweep {
        semitones: 12.0,
        octaves: 5.2,
        level_from: 0.08,
    }),
    filter: Filter::low(250.0, 1.4),
    amp: Adsr::new(0.05, 0.1, 1.0, 0.08),
    gain: 0.22,
    bus: Bus::Fx,
    sends: Sends {
        reverb: 0.4,
        delay: 0.0,
    },
    ..Patch::BASIC
};

/// The riser in reverse: falling and closing as it fades.
pub const DOWNLIFTER: Patch = Patch {
    name: "Downlifter",
    wave: Wave::Saw,
    unison: 2,
    detune_cents: 20.0,
    spread: 0.6,
    noise: 0.8,
    sweep: Some(Sweep {
        semitones: -12.0,
        octaves: -5.0,
        level_from: 1.0,
    }),
    filter: Filter::low(9_000.0, 1.2),
    amp: Adsr::new(0.002, 1.6, 0.0, 0.3),
    gain: 0.25,
    bus: Bus::Fx,
    sends: Sends {
        reverb: 0.5,
        delay: 0.0,
    },
    ..Patch::BASIC
};

/// A boom that drops two octaves onto its note, and a burst of noise: play it low.
pub const IMPACT: Patch = Patch {
    name: "Impact",
    wave: Wave::Sine,
    noise: 0.35,
    pitch_env: 24.0,
    pitch_env_s: 0.09,
    filter: Filter {
        env_octaves: 5.5,
        env: Adsr::new(0.0005, 0.12, 0.0, 0.2),
        ..Filter::low(220.0, 0.8)
    },
    amp: Adsr::new(0.0005, 2.2, 0.0, 0.6),
    drive: 1.8,
    gain: 0.6,
    bus: Bus::Fx,
    sends: Sends {
        reverb: 0.55,
        delay: 0.0,
    },
    ..Patch::BASIC
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::{SynthVoice, VoiceOut};

    const SR: u32 = 48_000;

    /// One note of `instrument` held for a second at a key that suits it,
    /// rendered for two: (RMS over the note, peak).
    fn level(instrument: &Instrument) -> (f32, f32) {
        let key = match instrument.bus() {
            Bus::Bass => 29.0,
            _ if instrument.name() == "Impact" => 33.0,
            _ => 57.0,
        };
        let frames = 2 * SR as usize;
        let mut bus = vec![0.0f32; 2 * frames];
        match instrument {
            Instrument::Sampled(tone) => {
                let rate = tone.rate(key as u8);
                for i in 0..frames {
                    let pos = i as f64 * rate;
                    if pos as usize >= tone.sample.frames() {
                        break;
                    }
                    let x = tone.sample.frame(pos as usize).0 * tone.gain;
                    bus[2 * i] = x;
                    bus[2 * i + 1] = x;
                }
            }
            Instrument::Synth(patch) => {
                let (mut reverb, mut delay) = (vec![0.0f32; 2 * frames], vec![0.0f32; 2 * frames]);
                for (n, &interval) in patch.chord().iter().enumerate() {
                    let mut voice = SynthVoice::new(SR);
                    voice.start(patch, key + f32::from(interval), 1.0, Some(SR), n as u64);
                    let mut out = VoiceOut {
                        bus: &mut bus,
                        reverb: &mut reverb,
                        delay: &mut delay,
                    };
                    voice.render(&mut out, 0, SR as usize);
                    voice.release();
                    voice.render(&mut out, SR as usize, frames);
                }
            }
        }
        let note = &bus[..2 * SR as usize];
        let rms = (note.iter().map(|x| x * x).sum::<f32>() / note.len() as f32).sqrt();
        let peak = bus.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        (rms, peak)
    }

    #[test]
    fn every_instrument_is_there_and_sits_at_a_sensible_level() {
        for name in INSTRUMENTS {
            let instrument = Instrument::named(name, SR).unwrap_or_else(|| panic!("{name} is missing"));
            let (rms, peak) = level(&instrument);
            let db = 20.0 * rms.max(1e-9).log10();
            assert!((-34.0..=-8.0).contains(&db), "{name}: {db:.1} dBFS RMS");
            assert!(peak < 1.0, "{name}: peak {peak}");
        }
        assert!(Instrument::named("kazoo", SR).is_none());
    }

    #[test]
    fn a_track_can_level_pan_and_send_an_instrument() {
        let reese = Instrument::named("reese", SR)
            .expect("built in")
            .with_level_db(-6.0)
            .with_pan(0.5)
            .with_sends(Sends {
                reverb: 0.1,
                delay: 0.0,
            });
        let Instrument::Synth(patch) = &reese else {
            panic!("the Reese is a synth")
        };
        assert!((patch.gain - REESE.gain * db_to_gain(-6.0)).abs() < 1e-6);
        assert_eq!((patch.pan, reese.sends().reverb, reese.bus()), (0.5, 0.1, Bus::Bass));
        assert_eq!(
            Instrument::named("rave-stab", SR).expect("built in").chord(),
            &[0, 3, 7, 10]
        );
    }
}
