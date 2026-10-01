//! The mix: every voice plays into a bus; the buses are levelled and summed,
//! the bass ducking under each kick; the sum is mastered through a look-ahead
//! limiter; the listener's volume comes last.

use wu_dsp::{Limiter, Smoothed, db_to_gain};
use wu_instruments::Bus;

use crate::MAX_BLOCK;

pub const BUS_COUNT: usize = Bus::ALL.len();

/// The ceiling the limiter holds the master under, in dB true peak: under the
/// −1 dBTP that loudness standards ask for, with room for a converter's rounding.
pub const CEILING_DB: f32 = -1.2;
const LOOKAHEAD_S: f32 = 0.0015;
const LIMITER_RELEASE_S: f32 = 0.1;
/// How fast the bass ducks when a kick lands: quick, but not a click.
const DUCK_ATTACK_S: f32 = 0.003;
/// Kicks waiting to duck the bass (live hits can be scheduled a few buffers ahead).
const PENDING_DUCKS: usize = 32;
/// Level and volume changes glide over this long.
const GAIN_GLIDE_S: f32 = 0.01;

/// How a program wants to be mixed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MixSettings {
    /// Each bus's level in dB, in `Bus::ALL` order.
    pub bus_db: [f32; BUS_COUNT],
    /// How far the bass bus dips under each kick, in dB; 0 for not at all.
    pub duck_db: f32,
    /// How long the bass takes to come back after a kick, in milliseconds.
    pub duck_release_ms: f32,
    /// Gain into the limiter: how hard the song is mastered.
    pub master_db: f32,
}

impl Default for MixSettings {
    fn default() -> MixSettings {
        MixSettings {
            bus_db: [0.0; BUS_COUNT],
            duck_db: 0.0,
            duck_release_ms: 120.0,
            master_db: 0.0,
        }
    }
}

/// Dips the bass under each kick: a sidechain driven by the sequencer, so it
/// lands on the kick's exact frame instead of reacting a little after it.
#[derive(Debug)]
struct Ducker {
    sample_rate: u32,
    /// 0 (off) to 1 (silence at the bottom of the dip).
    depth: f32,
    attack_step: f32,
    release: f32,
    level: f32,
    attacking: bool,
    /// Device frames of kicks still to come.
    pending: [u64; PENDING_DUCKS],
    count: usize,
}

impl Ducker {
    fn new(sample_rate: u32) -> Ducker {
        let mut ducker = Ducker {
            sample_rate,
            depth: 0.0,
            attack_step: 1.0 / (DUCK_ATTACK_S * sample_rate as f32).max(1.0),
            release: 0.0,
            level: 0.0,
            attacking: false,
            pending: [0; PENDING_DUCKS],
            count: 0,
        };
        ducker.set(0.0, MixSettings::default().duck_release_ms);
        ducker
    }

    fn set(&mut self, duck_db: f32, release_ms: f32) {
        self.depth = 1.0 - db_to_gain(duck_db.min(0.0));
        // Down to 5 % of the dip (three time constants) after `release_ms`.
        let time_constant = release_ms.max(1.0) / 1000.0 / 3.0;
        self.release = (-1.0 / (time_constant * self.sample_rate as f32)).exp();
    }

    fn duck_at(&mut self, frame: u64) {
        if self.count < PENDING_DUCKS {
            self.pending[self.count] = frame;
            self.count += 1;
        }
    }

    /// The bass bus's gain on device frame `frame`.
    fn gain(&mut self, frame: u64) -> f32 {
        let mut i = 0;
        while i < self.count {
            if self.pending[i] <= frame {
                self.attacking = true;
                self.count -= 1;
                self.pending[i] = self.pending[self.count];
            } else {
                i += 1;
            }
        }
        if self.attacking {
            self.level += self.attack_step;
            if self.level >= 1.0 {
                self.level = 1.0;
                self.attacking = false;
            }
        } else {
            self.level *= self.release;
        }
        1.0 - self.depth * self.level
    }
}

#[derive(Debug)]
pub(crate) struct Mixer {
    /// Interleaved stereo, `MAX_BLOCK` frames each, in `Bus::ALL` order.
    pub buses: [Vec<f32>; BUS_COUNT],
    levels: [Smoothed; BUS_COUNT],
    master: Smoothed,
    volume: Smoothed,
    duck: Ducker,
    limiter: Limiter,
}

impl Mixer {
    pub fn new(sample_rate: u32) -> Mixer {
        let glide = |value| Smoothed::new(value, GAIN_GLIDE_S, sample_rate);
        Mixer {
            buses: std::array::from_fn(|_| vec![0.0; MAX_BLOCK * 2]),
            levels: [glide(1.0); BUS_COUNT],
            master: glide(1.0),
            volume: glide(1.0),
            duck: Ducker::new(sample_rate),
            limiter: Limiter::new(sample_rate, CEILING_DB, LOOKAHEAD_S, LIMITER_RELEASE_S),
        }
    }

    /// Frames the master chain delays the sound by (the limiter's look-ahead).
    pub fn latency(&self) -> usize {
        self.limiter.latency()
    }

    pub fn apply(&mut self, settings: &MixSettings) {
        for (level, db) in self.levels.iter_mut().zip(settings.bus_db) {
            level.set_target(db_to_gain(db));
        }
        self.master.set_target(db_to_gain(settings.master_db));
        self.duck.set(settings.duck_db, settings.duck_release_ms);
    }

    /// The listener's volume, after the limiter (0–1, or a little more).
    pub fn set_volume(&mut self, volume: f32) {
        self.volume.set_target(volume.clamp(0.0, 4.0));
    }

    /// Ducks the bass from device frame `frame`.
    pub fn duck_at(&mut self, frame: u64) {
        self.duck.duck_at(frame);
    }

    pub fn clear(&mut self, len: usize) {
        for bus in &mut self.buses {
            bus[..len * 2].fill(0.0);
        }
    }

    /// Mixes the buses' first `len` frames into `out` (interleaved stereo).
    /// `frame` is the device frame of the block's first frame.
    pub fn process(&mut self, out: &mut [f32], len: usize, frame: u64) {
        let (out_frames, _) = out[..len * 2].as_chunks_mut::<2>();
        for (i, slot) in out_frames.iter_mut().enumerate() {
            let duck = self.duck.gain(frame + i as u64);
            let (mut left, mut right) = (0.0, 0.0);
            for (bus, (samples, level)) in Bus::ALL.iter().zip(self.buses.iter().zip(&mut self.levels)) {
                let mut gain = level.step();
                if *bus == Bus::Bass {
                    gain *= duck;
                }
                left += samples[2 * i] * gain;
                right += samples[2 * i + 1] * gain;
            }
            let master = self.master.step();
            let (left, right) = self.limiter.process(left * master, right * master);
            let volume = self.volume.step();
            // The limiter keeps the master under the ceiling; this only guards a
            // volume turned up past 1.
            slot[0] = (left * volume).clamp(-1.0, 1.0);
            slot[1] = (right * volume).clamp(-1.0, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    /// A steady tone on the bass bus, a kick at `kick_at`; the bass's output level per frame.
    fn bass_level_around_a_kick(settings: &MixSettings, kick_at: u64, frames: usize) -> Vec<f32> {
        let mut mixer = Mixer::new(SR);
        mixer.apply(settings);
        mixer.duck_at(kick_at);
        let mut levels = Vec::with_capacity(frames);
        let mut frame = 0u64;
        while levels.len() < frames + mixer.latency() {
            let len = MAX_BLOCK;
            mixer.clear(len);
            mixer.buses[Bus::Bass.index()][..len * 2].fill(0.25);
            let mut out = vec![0.0; len * 2];
            mixer.process(&mut out, len, frame);
            levels.extend(out.iter().step_by(2));
            frame += len as u64;
        }
        levels.drain(..mixer.latency());
        levels.truncate(frames);
        levels
    }

    #[test]
    fn the_bass_dips_on_the_kick_frame_and_comes_back() {
        let settings = MixSettings {
            duck_db: -12.0,
            duck_release_ms: 120.0,
            ..MixSettings::default()
        };
        let kick = 4_800;
        let levels = bass_level_around_a_kick(&settings, kick, 24_000);
        let at = |ms: f32| levels[kick as usize + (ms * 48.0) as usize];
        assert!(
            (levels[kick as usize - 1] - 0.25).abs() < 1e-6,
            "untouched before the kick"
        );
        assert!(at(1.0) < 0.25 && at(1.0) > at(3.0), "dipping, not jumping");
        assert!(
            (at(3.5) - 0.25 * db_to_gain(-12.0)).abs() < 0.01,
            "12 dB down after the attack"
        );
        assert!(at(130.0) > 0.23, "back after the release");
    }

    #[test]
    fn no_ducking_unless_asked() {
        let levels = bass_level_around_a_kick(&MixSettings::default(), 100, 2_000);
        assert!(levels.iter().all(|&x| x == 0.25));
    }

    #[test]
    fn buses_are_levelled_then_summed() {
        let mut mixer = Mixer::new(SR);
        mixer.apply(&MixSettings {
            bus_db: [0.0, -6.0, -120.0, 0.0],
            ..MixSettings::default()
        });
        let mut out = vec![0.0; MAX_BLOCK * 2];
        // Long enough for the levels to glide into place.
        for block in 0..20u64 {
            mixer.clear(MAX_BLOCK);
            for bus in [Bus::Drums, Bus::Bass, Bus::Music] {
                mixer.buses[bus.index()].fill(0.1);
            }
            mixer.process(&mut out, MAX_BLOCK, block * MAX_BLOCK as u64);
        }
        let expected = 0.1 + 0.1 * db_to_gain(-6.0) + 0.1 * db_to_gain(-120.0);
        assert!((out[out.len() - 2] - expected).abs() < 1e-4, "{}", out[out.len() - 2]);
    }

    #[test]
    fn a_hot_master_is_held_at_the_ceiling() {
        let mut mixer = Mixer::new(SR);
        mixer.apply(&MixSettings {
            master_db: 12.0,
            ..MixSettings::default()
        });
        let ceiling = db_to_gain(CEILING_DB);
        let mut peak = 0.0f32;
        for block in 0..40u64 {
            mixer.clear(MAX_BLOCK);
            mixer.buses[Bus::Drums.index()].fill(0.5);
            let mut out = vec![0.0; MAX_BLOCK * 2];
            mixer.process(&mut out, MAX_BLOCK, block * MAX_BLOCK as u64);
            peak = out.iter().fold(peak, |p, x| p.max(x.abs()));
        }
        assert!(peak <= ceiling * 1.000_01, "{peak}");
        assert!(peak > ceiling * 0.99, "a +12 dB master drives it to the ceiling");
    }
}
