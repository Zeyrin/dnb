//! A recording played under the program, in step with the transport: an
//! imported tune is its own backing track. The transport says where in the
//! song it is; the recording is read from the same place, at the same speed
//! (slower and lower at a practice tempo, as on a turntable). A seek, a loop or
//! a WHEEL UP! moves the reading with the transport, faded in so it never
//! clicks. A miss in Classic audio muffles the recording until the next hit,
//! the way a DJ rolls the highs off.

use std::sync::Arc;

use wu_dsp::Svf;
use wu_time::{PPQ, TempoMap};

/// A recording to play under a program.
#[derive(Clone, Debug)]
pub struct Backing {
    /// Interleaved stereo at the program's sample rate.
    audio: Arc<[f32]>,
    /// The recording's frame on the song's first bar line (tick 0).
    zero_frame: f64,
    /// Its frames per tick, at the tune's own tempo.
    frames_per_tick: f64,
    /// How loud it plays.
    gain: f32,
}

impl Backing {
    /// `audio` (interleaved stereo at `sample_rate`, the program's) has its
    /// first bar line `first_bar_s` in, and plays at `bpm`; it plays at `gain`.
    pub fn new(audio: Arc<[f32]>, sample_rate: u32, first_bar_s: f64, bpm: f64, gain: f32) -> Backing {
        let frames_per_beat = 60.0 / bpm * f64::from(sample_rate);
        Backing {
            audio,
            zero_frame: first_bar_s * f64::from(sample_rate),
            frames_per_tick: frames_per_beat / PPQ as f64,
            gain,
        }
    }

    pub fn frames(&self) -> usize {
        self.audio.len() / 2
    }

    fn frame(&self, i: i64) -> [f32; 2] {
        usize::try_from(i)
            .ok()
            .and_then(|i| self.audio.get(2 * i..2 * i + 2))
            .map_or([0.0; 2], |f| [f[0], f[1]])
    }

    /// The recording at a position between frames: exact on a frame, and
    /// between them a cubic through the four frames around.
    fn at(&self, position: f64) -> [f32; 2] {
        let whole = position.floor();
        let t = (position - whole) as f32;
        let i = whole as i64;
        if t < 1e-6 {
            return self.frame(i);
        }
        let [a, b, c, d] = [i - 1, i, i + 1, i + 2].map(|j| self.frame(j));
        std::array::from_fn(|ch| hermite(a[ch], b[ch], c[ch], d[ch], t))
    }
}

/// The Catmull-Rom cubic between `b` and `c`, `t` of the way.
fn hermite(a: f32, b: f32, c: f32, d: f32, t: f32) -> f32 {
    let c1 = 0.5 * (c - a);
    let c2 = a - 2.5 * b + 2.0 * c - 0.5 * d;
    let c3 = 0.5 * (d - a) + 1.5 * (b - c);
    ((c3 * t + c2) * t + c1) * t + b
}

/// How long the recording takes to come in after it jumps.
const FADE_S: f32 = 0.003;
/// Muffled, the recording is cut above this…
const MUFFLED_HZ: f32 = 420.0;
/// …and this much quieter.
const MUFFLED_GAIN: f32 = 0.7;
/// Open, the filter is out of the way from here up.
const OPEN_HZ: f32 = 18_000.0;
/// How fast it muffles, and opens up again (time constants).
const MUFFLE_S: f32 = 0.03;
const OPEN_S: f32 = 0.12;
/// The filter is retuned every this many frames while it moves.
const CONTROL_FRAMES: usize = 16;

/// The engine's side of a backing: where it is, and how muffled.
#[derive(Debug)]
pub(crate) struct BackingPlayer {
    sample_rate: u32,
    /// The reading position the next frame continues from, had nothing moved.
    next: Option<f64>,
    /// Frames of the fade in still to go.
    fading: u32,
    fade_frames: u32,
    muffled: bool,
    /// The filter's cut-off now, and its state per channel.
    cutoff: f32,
    filters: [Svf; 2],
    /// Gain now, gliding with the cut-off.
    level: f32,
}

impl BackingPlayer {
    pub fn new(sample_rate: u32) -> BackingPlayer {
        BackingPlayer {
            sample_rate,
            next: None,
            fading: 0,
            fade_frames: (FADE_S * sample_rate as f32).max(1.0) as u32,
            muffled: false,
            cutoff: OPEN_HZ,
            filters: std::array::from_fn(|_| Svf::new(OPEN_HZ, 0.707, sample_rate)),
            level: 1.0,
        }
    }

    /// A new program: from the top, open.
    pub fn reset(&mut self) {
        *self = BackingPlayer::new(self.sample_rate);
    }

    pub fn set_muffled(&mut self, muffled: bool) {
        self.muffled = muffled;
    }

    /// Adds `frames` of the recording, from transport frame `from` on, into
    /// `bus` (interleaved stereo) from its frame `offset`.
    pub fn render(
        &mut self,
        backing: &Backing,
        tempo: &TempoMap,
        from: i64,
        frames: usize,
        bus: &mut [f32],
        offset: usize,
    ) {
        if frames == 0 {
            return;
        }
        let tick = tempo.tick_at_frame(from as f64, self.sample_rate);
        let ticks_per_frame = tempo.tick_at_frame((from + 1) as f64, self.sample_rate) - tick;
        let start = backing.zero_frame + tick * backing.frames_per_tick;
        let speed = ticks_per_frame * backing.frames_per_tick;
        // Somewhere else than the last block left off: fade in from there.
        if self.next.is_none_or(|next| (next - start).abs() > 0.5) {
            self.fading = self.fade_frames;
        }
        // On a frame and at speed, read the frames as they are (no rounding
        // error accumulates: each is placed from its own index).
        let exact = (speed - 1.0).abs() < 1e-9 && (start - start.round()).abs() < 1e-6;
        let target = if self.muffled { MUFFLED_HZ } else { OPEN_HZ };
        let (target_level, time) = if self.muffled {
            (MUFFLED_GAIN, MUFFLE_S)
        } else {
            (1.0, OPEN_S)
        };
        let glide = 1.0 - (-(CONTROL_FRAMES as f32) / (time * self.sample_rate as f32)).exp();
        let (out, _) = bus[offset * 2..(offset + frames) * 2].as_chunks_mut::<2>();
        for (i, slot) in out.iter_mut().enumerate() {
            if i % CONTROL_FRAMES == 0 && (self.cutoff != target || self.level != target_level) {
                // Glide in octaves, so it sounds even all the way down.
                let octaves = (target / self.cutoff).log2();
                self.cutoff = if octaves.abs() < 0.01 {
                    target
                } else {
                    self.cutoff * 2f32.powf(octaves * glide)
                };
                self.level += (target_level - self.level) * glide;
                if (self.level - target_level).abs() < 1e-3 {
                    self.level = target_level;
                }
                for filter in &mut self.filters {
                    filter.set(self.cutoff, 0.707, self.sample_rate);
                }
            }
            let mut frame = if exact {
                backing.frame(start.round() as i64 + i as i64)
            } else {
                backing.at(start + i as f64 * speed)
            };
            if self.cutoff < OPEN_HZ {
                for (x, filter) in frame.iter_mut().zip(&mut self.filters) {
                    *x = filter.process(*x).low;
                }
            } else {
                // Open: out of the way, but kept in step for when it closes.
                for (&x, filter) in frame.iter().zip(&mut self.filters) {
                    filter.process(x);
                }
            }
            let mut gain = backing.gain * self.level;
            if self.fading > 0 {
                gain *= 1.0 - self.fading as f32 / self.fade_frames as f32;
                self.fading -= 1;
            }
            slot[0] += frame[0] * gain;
            slot[1] += frame[1] * gain;
        }
        self.next = Some(start + frames as f64 * speed);
    }

    /// The transport stopped or cut: whatever plays next fades in.
    pub fn interrupt(&mut self) {
        self.next = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    /// A recording whose every frame says where it is: left = index, right = −index.
    fn ramp(frames: usize) -> Backing {
        let audio: Vec<f32> = (0..frames).flat_map(|i| [i as f32, -(i as f32)]).collect();
        Backing::new(audio.into(), SR, 0.5, 120.0, 1.0)
    }

    #[test]
    fn at_the_songs_tempo_the_recording_plays_frame_for_frame_from_its_first_bar() {
        let backing = ramp(96_000);
        let tempo = TempoMap::constant(120.0);
        let mut player = BackingPlayer::new(SR);
        // Past the fade: render twice, and look at the second block.
        let mut bus = vec![0.0f32; 2 * 256];
        player.render(&backing, &tempo, 0, 256, &mut bus, 0);
        bus.fill(0.0);
        player.render(&backing, &tempo, 256, 256, &mut bus, 0);
        // Transport frame 256 is 256 frames after the bar line at 0.5 s.
        assert_eq!(bus[0], 24_256.0);
        assert_eq!(bus[1], -24_256.0);
        assert_eq!(bus[2 * 255], 24_511.0);
    }

    #[test]
    fn before_its_start_and_after_its_end_there_is_silence() {
        let backing = ramp(1_000);
        let tempo = TempoMap::constant(120.0);
        let mut player = BackingPlayer::new(SR);
        let mut bus = vec![0.0f32; 2 * 64];
        // The count-in, a second before the bar line, half a second before the recording.
        player.render(&backing, &tempo, -48_000, 64, &mut bus, 0);
        assert!(bus.iter().all(|&x| x == 0.0));
        player.render(&backing, &tempo, 10_000, 64, &mut bus, 0);
        assert!(bus.iter().all(|&x| x == 0.0));
    }

    #[test]
    fn a_slower_tempo_reads_it_slower() {
        let backing = ramp(96_000);
        // At 60 BPM the transport takes two frames for each of the recording's.
        let tempo = TempoMap::constant(60.0);
        let mut player = BackingPlayer::new(SR);
        let mut bus = vec![0.0f32; 2 * 512];
        player.render(&backing, &tempo, 1_000, 512, &mut bus, 0);
        let read = |i: usize| bus[2 * i];
        // Each frame moves half a frame on, smoothly.
        assert!(
            (read(400) - read(399) - 0.5).abs() < 1e-3,
            "{} → {}",
            read(399),
            read(400)
        );
        assert!((read(400) - (24_000.0 + 500.0 + 200.0)).abs() < 0.01, "{}", read(400));
    }

    #[test]
    fn a_jump_fades_in_and_a_miss_muffles_it() {
        // A full-scale 5 kHz tone: muffling must take it far down.
        let audio: Vec<f32> = (0..96_000)
            .flat_map(|i| {
                let x = (std::f32::consts::TAU * 5_000.0 * i as f32 / SR as f32).sin();
                [x, x]
            })
            .collect();
        let backing = Backing::new(audio.into(), SR, 0.0, 120.0, 1.0);
        let tempo = TempoMap::constant(120.0);
        let mut player = BackingPlayer::new(SR);
        let mut bus = vec![0.0f32; 2 * 4_800];
        player.render(&backing, &tempo, 0, 4_800, &mut bus, 0);
        assert!(bus[0].abs() < 1e-6, "fades in from nothing");
        let peak = |bus: &[f32]| bus[bus.len() / 2..].iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(peak(&bus) > 0.99, "then plays as recorded");
        player.set_muffled(true);
        bus.fill(0.0);
        player.render(&backing, &tempo, 4_800, 4_800, &mut bus, 0);
        assert!(peak(&bus) < 0.05, "well down within 100 ms: {}", peak(&bus));
        bus.fill(0.0);
        player.render(&backing, &tempo, 9_600, 4_800, &mut bus, 0);
        assert!(peak(&bus) < 0.01, "and 40 dB down after: {}", peak(&bus));
        player.set_muffled(false);
        for start in [14_400, 19_200, 24_000, 28_800, 33_600, 38_400] {
            bus.fill(0.0);
            player.render(&backing, &tempo, start, 4_800, &mut bus, 0);
        }
        assert!(peak(&bus) > 0.99, "open again: {}", peak(&bus));
    }
}
