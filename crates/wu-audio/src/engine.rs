//! The engine on the audio thread, and the handles other threads hold.

use std::sync::Arc;

use rtrb::{Consumer, Producer, RingBuffer};
use wu_dsp::Sample;
use wu_instruments::Pad;
use wu_time::Tick;

use crate::clock::{ClockSnapshot, SharedClock};
use crate::mixer::Mixer;
use crate::program::{EventKind, LoopRange, Program};
use crate::voice::{VoicePool, VoiceRequest};
use crate::{MAX_BLOCK, VOICES};

const COMMAND_SLOTS: usize = 256;
const LIVE_SLOTS: usize = 256;
const REPORT_SLOTS: usize = 8192;
const GARBAGE_SLOTS: usize = 4096;
/// Samples waiting for room in the garbage queue. Reserved up front, never grown.
const STASH_SLOTS: usize = 512;

/// Main thread → audio thread.
#[derive(Debug)]
pub enum Command {
    /// Replaces the program. The transport stops and parks at tick 0.
    Load(Box<Program>),
    Play,
    Stop,
    Seek(Tick),
    /// Repeats `start..end`; `None` plays straight through.
    SetLoop(Option<(Tick, Tick)>),
    /// The listener's volume, after the limiter: 1 plays the master as mastered.
    SetVolume(f32),
    SetLiveMode(LiveMode),
    /// Fades every voice out at once.
    Panic,
}

/// How a live pad hit is placed in the output.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LiveMode {
    /// As early as possible: lowest latency, but it varies by up to one buffer.
    #[default]
    Asap,
    /// Exactly one buffer (plus the device's latency) after the press: a little
    /// later, but always the same.
    Stable,
}

/// Input thread → audio thread: a pad pressed at `at_ns` on the shared clock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiveHit {
    pub pad: Pad,
    pub velocity: f32,
    pub at_ns: u64,
}

/// A rail pressed: a note of the program's tone, held until the rail is let go
/// or the transport reaches `until_frame` (the charted end), whichever is first.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiveNote {
    /// Which rail: one note at a time on each.
    pub rail: u8,
    pub key: u8,
    pub velocity: f32,
    pub at_ns: u64,
    pub until_frame: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Live {
    Hit(LiveHit),
    NoteOn(LiveNote),
    NoteOff { rail: u8, at_ns: u64 },
}

impl Live {
    fn at_ns(&self) -> u64 {
        match *self {
            Live::Hit(hit) => hit.at_ns,
            Live::NoteOn(note) => note.at_ns,
            Live::NoteOff { at_ns, .. } => at_ns,
        }
    }
}

/// The longest a live note sounds without a charted end: a held trigger.
const LIVE_NOTE_MAX_S: f64 = 16.0;

/// What started a voice.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VoiceSource {
    Sequence { tick: Tick, transport_frame: i64 },
    Live { at_ns: u64 },
}

/// A voice that started, and the device frame it starts on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoiceStart {
    pub pad: Pad,
    pub velocity: f32,
    pub source: VoiceSource,
    pub device_frame: u64,
}

/// Audio thread → main thread.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Report {
    VoiceStarted(VoiceStart),
    /// A note of the program's tone started.
    NoteStarted {
        key: u8,
        tick: Tick,
        device_frame: u64,
    },
    /// A program built for another sample rate was refused.
    ProgramRejected {
        expected_rate: u32,
        got_rate: u32,
    },
}

/// Things the audio thread is done with. They are dropped on the main thread,
/// so freeing memory never happens inside a callback.
#[derive(Debug)]
pub enum Garbage {
    Program(Box<Program>),
    Sample(Arc<Sample>),
}

/// When the buffer being rendered will be heard.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BufferTiming {
    /// When the buffer's first frame reaches the speaker, on the shared clock.
    pub playback_ns: u64,
    /// The device's callback-to-speaker latency estimate.
    pub output_latency_ns: u64,
}

/// Creates an engine and the handles that talk to it.
pub fn engine(sample_rate: u32) -> EngineParts {
    let (commands_tx, commands_rx) = RingBuffer::new(COMMAND_SLOTS);
    let (live_tx, live_rx) = RingBuffer::new(LIVE_SLOTS);
    let (live_main_tx, live_main_rx) = RingBuffer::new(LIVE_SLOTS);
    let (reports_tx, reports_rx) = RingBuffer::new(REPORT_SLOTS);
    let (garbage_tx, garbage_rx) = RingBuffer::new(GARBAGE_SLOTS);
    let clock = Arc::new(SharedClock::default());
    clock.publish(&ClockSnapshot {
        sample_rate,
        ..ClockSnapshot::default()
    });
    let mixer = Mixer::new(sample_rate);
    let mixer_latency_ns = (mixer.latency() as f64 * 1e9 / f64::from(sample_rate)).round() as u64;
    EngineParts {
        engine: Engine {
            sample_rate,
            commands: commands_rx,
            live: [live_rx, live_main_rx],
            reports: reports_tx,
            garbage: garbage_tx,
            stash: Vec::with_capacity(STASH_SLOTS),
            freed_on_audio_thread: 0,
            clock: Arc::clone(&clock),
            program: None,
            playing: false,
            frame: 0,
            cursor: 0,
            epoch: 0,
            generation: 0,
            device_frame: 0,
            voices: VoicePool::new(VOICES, sample_rate),
            mixer,
            mixer_latency_ns,
            live_mode: LiveMode::default(),
        },
        handle: EngineHandle {
            sample_rate,
            loads_sent: 0,
            commands: commands_tx,
            reports: reports_rx,
            garbage: garbage_rx,
            clock,
        },
        live: LiveSender { events: live_tx },
        live_main: LiveSender { events: live_main_tx },
    }
}

#[derive(Debug)]
pub struct EngineParts {
    /// Goes to the audio thread (an output backend).
    pub engine: Engine,
    /// Stays on the main thread.
    pub handle: EngineHandle,
    /// Goes to the input thread.
    pub live: LiveSender,
    /// Stays on the main thread: keyboard play and auditions.
    pub live_main: LiveSender,
}

#[derive(Debug)]
pub struct EngineHandle {
    sample_rate: u32,
    loads_sent: u64,
    commands: Producer<Command>,
    reports: Consumer<Report>,
    garbage: Consumer<Garbage>,
    clock: Arc<SharedClock>,
}

impl EngineHandle {
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Queues a command. A full queue hands it back.
    pub fn send(&mut self, command: Command) -> Result<(), Command> {
        let is_load = matches!(command, Command::Load(_));
        self.commands
            .push(command)
            .map_err(|rtrb::PushError::Full(command)| command)?;
        if is_load {
            self.loads_sent += 1;
        }
        Ok(())
    }

    /// The generation the engine reaches once every load sent so far has
    /// been taken in (see `ClockSnapshot::generation`).
    pub fn loads_sent(&self) -> u64 {
        self.loads_sent
    }

    /// Drains the reports and drops whatever the audio thread has finished with.
    /// Call it every frame.
    pub fn poll(&mut self, mut on_report: impl FnMut(Report)) {
        while let Ok(report) = self.reports.pop() {
            on_report(report);
        }
        while let Ok(garbage) = self.garbage.pop() {
            drop(garbage);
        }
    }

    pub fn clock(&self) -> ClockSnapshot {
        self.clock.read()
    }

    pub fn shared_clock(&self) -> Arc<SharedClock> {
        Arc::clone(&self.clock)
    }
}

/// The input thread's direct line to the sound.
#[derive(Debug)]
pub struct LiveSender {
    events: Producer<Live>,
}

impl LiveSender {
    /// Each returns `false` if the queue was full and the event was dropped.
    pub fn hit(&mut self, hit: LiveHit) -> bool {
        self.events.push(Live::Hit(hit)).is_ok()
    }

    pub fn note_on(&mut self, note: LiveNote) -> bool {
        self.events.push(Live::NoteOn(note)).is_ok()
    }

    pub fn note_off(&mut self, rail: u8, at_ns: u64) -> bool {
        self.events.push(Live::NoteOff { rail, at_ns }).is_ok()
    }
}

#[derive(Debug)]
pub struct Engine {
    sample_rate: u32,
    commands: Consumer<Command>,
    /// Live play from the input thread and from the main thread.
    live: [Consumer<Live>; 2],
    reports: Producer<Report>,
    garbage: Producer<Garbage>,
    stash: Vec<Arc<Sample>>,
    freed_on_audio_thread: u64,
    clock: Arc<SharedClock>,
    program: Option<Box<Program>>,
    playing: bool,
    /// Transport position: the song frame the next rendered frame plays.
    frame: i64,
    /// Next event to fire.
    cursor: usize,
    epoch: u64,
    /// Programs loaded so far.
    generation: u64,
    device_frame: u64,
    voices: VoicePool,
    mixer: Mixer,
    /// The master's look-ahead, as time.
    mixer_latency_ns: u64,
    live_mode: LiveMode,
}

impl Engine {
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Times the engine had to free memory itself because the main thread
    /// stopped collecting garbage. Should stay 0.
    pub fn freed_on_audio_thread(&self) -> u64 {
        self.freed_on_audio_thread
    }

    /// Frames the master chain delays the sound by: the limiter's look-ahead.
    /// The clock already counts it as output latency.
    pub fn latency_frames(&self) -> usize {
        self.mixer.latency()
    }

    /// Renders `out.len() / 2` frames of interleaved stereo.
    pub fn process(&mut self, out: &mut [f32], timing: BufferTiming) {
        // Everything leaves the limiter a little late: to the clock and to live
        // scheduling, that is part of the output latency.
        let timing = BufferTiming {
            playback_ns: timing.playback_ns + self.mixer_latency_ns,
            output_latency_ns: timing.output_latency_ns + self.mixer_latency_ns,
        };
        self.handle_commands();
        self.publish_clock(timing);
        let total = out.len() / 2;
        self.schedule_live_hits(timing, total);

        let mut done = 0;
        while done < total {
            let len = (total - done).min(MAX_BLOCK);
            self.mixer.clear(len);
            if self.playing {
                self.sequence(len);
            }
            let Engine {
                voices,
                mixer,
                garbage,
                stash,
                freed_on_audio_thread,
                ..
            } = self;
            voices.render(&mut mixer.buses, len, &mut |sample| {
                release(garbage, stash, freed_on_audio_thread, sample)
            });
            self.mixer
                .process(&mut out[done * 2..(done + len) * 2], len, self.device_frame);
            self.device_frame += len as u64;
            done += len;
        }
        self.flush_stash();
    }

    fn handle_commands(&mut self) {
        // Bounded, so a flood of commands can't stall a callback.
        for _ in 0..COMMAND_SLOTS {
            let Ok(command) = self.commands.pop() else { break };
            match command {
                Command::Load(program) => self.load(program),
                Command::Play => {
                    self.playing = true;
                    self.epoch += 1;
                }
                Command::Stop => {
                    self.playing = false;
                    self.epoch += 1;
                }
                Command::Seek(tick) => {
                    if let Some(program) = &self.program {
                        self.frame = program.tempo.frame_at(tick, self.sample_rate);
                        self.cursor = program.first_event_at(self.frame);
                        self.epoch += 1;
                    }
                }
                Command::SetLoop(range) => {
                    if let Some(program) = self.program.as_deref_mut() {
                        let range =
                            range.and_then(|(start, end)| LoopRange::new(start, end, &program.tempo, self.sample_rate));
                        program.set_loop_range(range);
                        self.epoch += 1;
                    }
                }
                Command::SetVolume(volume) => self.mixer.set_volume(volume),
                Command::SetLiveMode(mode) => self.live_mode = mode,
                Command::Panic => self.voices.fade_all(),
            }
        }
    }

    fn load(&mut self, program: Box<Program>) {
        if program.sample_rate != self.sample_rate {
            let report = Report::ProgramRejected {
                expected_rate: self.sample_rate,
                got_rate: program.sample_rate,
            };
            let _ = self.reports.push(report);
            self.throw_away(Garbage::Program(program));
            return;
        }
        self.mixer.apply(&program.mix);
        if let Some(old) = self.program.replace(program) {
            self.throw_away(Garbage::Program(old));
        }
        self.playing = false;
        self.frame = 0;
        self.cursor = 0;
        self.epoch += 1;
        self.generation += 1;
    }

    fn throw_away(&mut self, garbage: Garbage) {
        if let Err(rtrb::PushError::Full(garbage)) = self.garbage.push(garbage) {
            // The main thread stopped collecting: nothing left but to free it here.
            self.freed_on_audio_thread += 1;
            drop(garbage);
        }
    }

    fn flush_stash(&mut self) {
        while let Some(sample) = self.stash.pop() {
            if let Err(rtrb::PushError::Full(Garbage::Sample(sample))) = self.garbage.push(Garbage::Sample(sample)) {
                self.stash.push(sample);
                break;
            }
        }
    }

    fn publish_clock(&self, timing: BufferTiming) {
        let (loop_start, loop_end) = self
            .program
            .as_ref()
            .and_then(|p| p.loop_range())
            .map_or((0, 0), |range| (range.start_frame, range.end_frame));
        self.clock.publish(&ClockSnapshot {
            device_frame: self.device_frame,
            transport_frame: self.frame,
            playback_ns: timing.playback_ns,
            output_latency_ns: timing.output_latency_ns,
            sample_rate: self.sample_rate,
            playing: self.playing,
            epoch: self.epoch,
            generation: self.generation,
            loop_start,
            loop_end,
        });
    }

    fn schedule_live_hits(&mut self, timing: BufferTiming, buffer_frames: usize) {
        let Some(program) = self.program.as_deref() else {
            // Nothing to play them on; drop them.
            for queue in &mut self.live {
                while queue.pop().is_ok() {}
            }
            return;
        };
        let ns_per_frame = 1e9 / f64::from(self.sample_rate);
        for queue in 0..self.live.len() {
            for _ in 0..LIVE_SLOTS {
                let Ok(event) = self.live[queue].pop() else { break };
                let delay = match self.live_mode {
                    LiveMode::Asap => 0,
                    LiveMode::Stable => {
                        let buffer_ns = buffer_frames as f64 * ns_per_frame;
                        let target = event.at_ns() as f64 + buffer_ns + timing.output_latency_ns as f64;
                        ((target - timing.playback_ns as f64) / ns_per_frame)
                            .round()
                            .clamp(0.0, 4.0 * buffer_frames as f64) as u32
                    }
                };
                let starts_at = self.device_frame + u64::from(delay);
                let Engine {
                    voices,
                    mixer,
                    garbage,
                    stash,
                    freed_on_audio_thread,
                    reports,
                    ..
                } = self;
                let mut release_sample = |sample| release(garbage, stash, freed_on_audio_thread, sample);
                match event {
                    Live::Hit(hit) => {
                        let request =
                            VoiceRequest::pad(hit.pad, program.kit.pad(hit.pad), hit.velocity, delay, starts_at);
                        voices.start(&request, &mut release_sample);
                        if request.sidechain {
                            mixer.duck_at(starts_at);
                        }
                        let _ = reports.push(Report::VoiceStarted(VoiceStart {
                            pad: hit.pad,
                            velocity: hit.velocity,
                            source: VoiceSource::Live { at_ns: hit.at_ns },
                            device_frame: starts_at,
                        }));
                    }
                    Live::NoteOn(note) => {
                        let Some(tone) = program.tone.as_ref() else { continue };
                        let longest = LIVE_NOTE_MAX_S * f64::from(self.sample_rate);
                        // Sounds until the charted end, counted from where the transport
                        // will be when the note starts.
                        let gate = match note.until_frame {
                            Some(until) if self.playing => (until - (self.frame + i64::from(delay))) as f64,
                            _ => longest,
                        };
                        let gate = gate.clamp(1.0, longest) as u32;
                        // One note per rail: a new press ends the last one.
                        voices.release_rail(note.rail, delay);
                        let mut request = VoiceRequest::note(tone, note.key, note.velocity, gate, delay, starts_at);
                        request.rail = Some(note.rail);
                        voices.start(&request, &mut release_sample);
                    }
                    Live::NoteOff { rail, .. } => voices.release_rail(rail, delay),
                }
            }
        }
    }

    /// Fires every event in the next `len` frames, wrapping at the loop end.
    fn sequence(&mut self, len: usize) {
        let Some(program) = self.program.as_deref() else { return };
        let events = program.events();
        let mut pos = 0usize;
        while pos < len {
            let seg_start = self.frame;
            let mut seg_end = seg_start + (len - pos) as i64;
            let mut wrap = None;
            if let Some(range) = program.loop_range()
                && seg_start < range.end_frame
                && seg_end >= range.end_frame
            {
                seg_end = range.end_frame;
                wrap = Some(range);
            }
            while let Some(event) = events.get(self.cursor) {
                if event.frame >= seg_end {
                    break;
                }
                if event.frame >= seg_start {
                    let offset = pos + (event.frame - seg_start) as usize;
                    let starts_at = self.device_frame + offset as u64;
                    let (request, report) = match event.kind {
                        EventKind::Pad { pad, velocity } => (
                            VoiceRequest::pad(pad, program.kit.pad(pad), velocity, offset as u32, starts_at),
                            Report::VoiceStarted(VoiceStart {
                                pad,
                                velocity,
                                source: VoiceSource::Sequence {
                                    tick: event.tick,
                                    transport_frame: event.frame,
                                },
                                device_frame: starts_at,
                            }),
                        ),
                        EventKind::Note { key, velocity, frames } => {
                            let Some(tone) = program.tone.as_ref() else {
                                self.cursor += 1;
                                continue;
                            };
                            (
                                VoiceRequest::note(tone, key, velocity, frames, offset as u32, starts_at),
                                Report::NoteStarted {
                                    key,
                                    tick: event.tick,
                                    device_frame: starts_at,
                                },
                            )
                        }
                    };
                    let Engine {
                        voices,
                        mixer,
                        garbage,
                        stash,
                        freed_on_audio_thread,
                        reports,
                        ..
                    } = self;
                    voices.start(&request, &mut |sample| {
                        release(garbage, stash, freed_on_audio_thread, sample)
                    });
                    if request.sidechain {
                        mixer.duck_at(starts_at);
                    }
                    let _ = reports.push(report);
                }
                self.cursor += 1;
            }
            pos += (seg_end - seg_start) as usize;
            self.frame = seg_end;
            if let Some(range) = wrap {
                self.frame = range.start_frame;
                self.cursor = program.first_event_at(range.start_frame);
                self.epoch += 1;
            }
        }
    }
}

/// Hands a finished voice's sample to the main thread for freeing.
fn release(
    garbage: &mut Producer<Garbage>,
    stash: &mut Vec<Arc<Sample>>,
    freed_on_audio_thread: &mut u64,
    sample: Arc<Sample>,
) {
    if let Err(rtrb::PushError::Full(Garbage::Sample(sample))) = garbage.push(Garbage::Sample(sample)) {
        if stash.len() < stash.capacity() {
            stash.push(sample);
        } else {
            *freed_on_audio_thread += 1;
            drop(sample);
        }
    }
}
