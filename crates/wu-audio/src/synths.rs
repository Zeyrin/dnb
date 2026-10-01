//! Synth voices: a fixed pool of them, allocated once, with stealing, release
//! at a frame inside a block, and rails letting go of what they play.

use wu_instruments::{Bus, Patch, SynthVoice, VoiceOut};

use crate::mixer::{BUS_COUNT, SendBuffers};
use crate::voice::CHOKE_FADE;

/// A synth voice's frames-to-go counters, and what started it.
#[derive(Debug)]
struct Slot {
    voice: SynthVoice,
    /// Frames into the current block before it starts sounding.
    delay: u32,
    /// Block offset at which the note is let go, if scheduled.
    release_at: Option<u32>,
    /// Block offset at which a quick fade-out starts, if scheduled.
    fade_at: Option<u32>,
    rail: Option<u8>,
    /// Device frame it starts on; stealing takes the oldest first.
    starts_at: u64,
}

impl Slot {
    fn new(sample_rate: u32) -> Slot {
        Slot {
            voice: SynthVoice::new(sample_rate),
            delay: 0,
            release_at: None,
            fade_at: None,
            rail: None,
            starts_at: 0,
        }
    }

    fn fade(&mut self, at: u32) {
        if self.fade_at.is_none_or(|current| at < current) {
            self.fade_at = Some(at);
        }
    }

    fn release(&mut self, at: u32) {
        if self.release_at.is_none_or(|current| at < current) {
            self.release_at = Some(at);
        }
    }

    /// Renders the next `len` frames, letting go and fading on their frames.
    fn render(&mut self, out: &mut VoiceOut<'_>, len: usize) {
        let mut pos = (self.delay as usize).min(len);
        self.delay -= pos as u32;
        while self.voice.is_active() {
            // Whatever is due by now happens before the next frame renders.
            if self.release_at.is_some_and(|at| at as usize <= pos) {
                self.voice.release();
                self.release_at = None;
            }
            if self.fade_at.is_some_and(|at| at as usize <= pos) {
                self.voice.fade_out(CHOKE_FADE);
                self.fade_at = None;
            }
            if pos >= len {
                break;
            }
            let next = [self.release_at, self.fade_at]
                .into_iter()
                .flatten()
                .map(|at| at as usize)
                .filter(|&at| at > pos)
                .fold(len, usize::min);
            self.voice.render(out, pos, next);
            pos = next;
        }
        let elapsed = len as u32;
        self.release_at = self.release_at.map(|at| at.saturating_sub(elapsed));
        self.fade_at = self.fade_at.map(|at| at.saturating_sub(elapsed));
    }
}

#[derive(Debug)]
pub(crate) struct SynthPool {
    slots: Vec<Slot>,
    /// Stolen voices fade out here, so the slot they left is free at once.
    fading: Vec<Slot>,
    /// Seeds each note's unison phases: the same events, the same sound.
    notes_started: u64,
}

/// How a synth note should play.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SynthRequest<'a> {
    pub patch: &'a Patch,
    pub key: u8,
    pub velocity: f32,
    /// Frames after the start at which it is let go, if known.
    pub gate: Option<u32>,
    pub rail: Option<u8>,
    pub delay: u32,
    pub starts_at: u64,
}

impl SynthPool {
    pub fn new(voices: usize, sample_rate: u32) -> SynthPool {
        SynthPool {
            slots: (0..voices).map(|_| Slot::new(sample_rate)).collect(),
            fading: (0..(voices / 4).max(1)).map(|_| Slot::new(sample_rate)).collect(),
            notes_started: 0,
        }
    }

    #[cfg(test)]
    pub fn active(&self) -> usize {
        self.slots
            .iter()
            .chain(&self.fading)
            .filter(|s| s.voice.is_active())
            .count()
    }

    /// Starts a note: a voice for each note of the patch's chord, stealing the
    /// oldest voices if every one is busy.
    pub fn start(&mut self, request: &SynthRequest<'_>) {
        for &interval in request.patch.chord() {
            let Some(key) = request.key.checked_add_signed(interval) else {
                continue;
            };
            let slot = match self.slots.iter().position(|s| !s.voice.is_active()) {
                Some(free) => free,
                None => self.steal(),
            };
            self.notes_started += 1;
            let slot = &mut self.slots[slot];
            slot.voice.start(
                request.patch,
                f32::from(key),
                request.velocity,
                request.gate,
                self.notes_started,
            );
            slot.delay = request.delay;
            slot.release_at = request.gate.map(|gate| request.delay.saturating_add(gate));
            slot.fade_at = None;
            slot.rail = request.rail;
            slot.starts_at = request.starts_at;
        }
    }

    /// Lets go of the note a rail is playing, from block offset `at`.
    pub fn release_rail(&mut self, rail: u8, at: u32) {
        for slot in self
            .slots
            .iter_mut()
            .filter(|s| s.voice.is_active() && s.rail == Some(rail))
        {
            slot.release(at);
            slot.rail = None;
        }
    }

    /// Cuts everything but the FX bus from block offset `at`, like the sample voices.
    pub fn cut_music(&mut self, at: u32) {
        for slot in self
            .slots
            .iter_mut()
            .filter(|s| s.voice.is_active() && s.voice.bus() != Bus::Fx)
        {
            slot.fade(at);
        }
    }

    pub fn fade_all(&mut self) {
        for slot in self.slots.iter_mut().filter(|s| s.voice.is_active()) {
            slot.fade(0);
        }
    }

    /// Mixes every voice's next `len` frames into its bus and sends.
    pub fn render(&mut self, buses: &mut [Vec<f32>; BUS_COUNT], sends: &mut SendBuffers, len: usize) {
        for slot in self.slots.iter_mut().chain(self.fading.iter_mut()) {
            if !slot.voice.is_active() {
                continue;
            }
            let mut out = VoiceOut {
                bus: &mut buses[slot.voice.bus().index()],
                reverb: &mut sends.reverb,
                delay: &mut sends.delay,
            };
            slot.render(&mut out, len);
        }
    }

    /// Moves the oldest voice to fade out among the stolen; returns its slot.
    fn steal(&mut self) -> usize {
        let oldest = self
            .slots
            .iter()
            .enumerate()
            .min_by_key(|(_, s)| s.starts_at)
            .map_or(0, |(i, _)| i);
        let parking = match self.fading.iter().position(|s| !s.voice.is_active()) {
            Some(free) => free,
            None => self
                .fading
                .iter()
                .enumerate()
                .min_by_key(|(_, s)| s.starts_at)
                .map_or(0, |(i, _)| i),
        };
        std::mem::swap(&mut self.slots[oldest], &mut self.fading[parking]);
        let stolen = &mut self.fading[parking];
        stolen.rail = None;
        stolen.fade(0);
        oldest
    }
}

#[cfg(test)]
mod tests {
    use wu_dsp::Adsr;
    use wu_instruments::synth::Filter;

    use super::*;
    use crate::MAX_BLOCK;

    const SR: u32 = 48_000;

    fn patch() -> Patch {
        Patch {
            amp: Adsr::new(0.001, 0.05, 1.0, 0.01),
            filter: Filter::low(20_000.0, 0.707),
            ..Patch::BASIC
        }
    }

    /// Renders blocks until `frames` have passed: the left channel of every bus summed.
    fn run(pool: &mut SynthPool, frames: usize) -> Vec<f32> {
        let mut left = Vec::new();
        while left.len() < frames {
            let len = MAX_BLOCK.min(frames - left.len());
            let mut buses = std::array::from_fn(|_| vec![0.0; MAX_BLOCK * 2]);
            let mut sends = SendBuffers::new(MAX_BLOCK);
            pool.render(&mut buses, &mut sends, len);
            left.extend((0..len).map(|i| buses.iter().map(|b| b[2 * i]).sum::<f32>()));
        }
        left
    }

    fn request(patch: &Patch, gate: Option<u32>, delay: u32, starts_at: u64) -> SynthRequest<'_> {
        SynthRequest {
            patch,
            key: 57,
            velocity: 1.0,
            gate,
            rail: None,
            delay,
            starts_at,
        }
    }

    #[test]
    fn a_note_starts_on_its_frame_and_stops_after_its_gate() {
        let patch = patch();
        let mut pool = SynthPool::new(4, SR);
        pool.start(&request(&patch, Some(1_000), 300, 0));
        let out = run(&mut pool, 3_000);
        assert!(out[..300].iter().all(|&x| x == 0.0), "silent before its frame");
        assert!(out[300..1_300].iter().any(|&x| x.abs() > 0.05), "sounding while held");
        // A 10 ms release: gone well before 3 000 frames.
        assert!(out[2_500..].iter().all(|&x| x.abs() < 1e-6));
        assert_eq!(pool.active(), 0);
    }

    #[test]
    fn chords_take_a_voice_a_note_and_stealing_keeps_the_newest() {
        let stab = Patch {
            chord: [0, 3, 7, 10],
            chord_len: 4,
            ..patch()
        };
        // Eight slots, and room for two stolen voices to fade out.
        let mut pool = SynthPool::new(8, SR);
        pool.start(&request(&stab, None, 0, 0));
        assert_eq!(pool.active(), 4);
        pool.start(&request(&stab, None, 0, 100));
        assert_eq!(pool.active(), 8, "two chords fill the pool");
        pool.start(&request(&stab, None, 0, 200));
        // The first chord was stolen: two of it fade out, two had no room to.
        assert_eq!(pool.slots.iter().filter(|s| s.starts_at == 200).count(), 4);
        assert_eq!(pool.slots.iter().filter(|s| s.starts_at == 0).count(), 0);
        assert_eq!(pool.active(), 10);
        run(&mut pool, 2 * CHOKE_FADE as usize);
        assert_eq!(pool.active(), 8);
    }

    #[test]
    fn letting_go_of_a_rail_releases_its_note_only() {
        let patch = patch();
        let mut pool = SynthPool::new(4, SR);
        pool.start(&SynthRequest {
            rail: Some(0),
            ..request(&patch, None, 0, 0)
        });
        pool.start(&SynthRequest {
            rail: Some(1),
            ..request(&patch, None, 0, 1)
        });
        run(&mut pool, 1_000);
        pool.release_rail(0, 10);
        run(&mut pool, 4_800);
        assert_eq!(pool.active(), 1, "the other rail still holds its note");
    }
}
