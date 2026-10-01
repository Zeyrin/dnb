//! The engine on the audio thread, and the handles other threads hold.

use std::sync::Arc;

use rtrb::{Consumer, Producer, RingBuffer};
use wu_dsp::{Sample, Smoothed, soft_clip};
use wu_instruments::Pad;
use wu_time::Tick;

use crate::clock::{ClockSnapshot, SharedClock};
use crate::program::{LoopRange, Program};
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
    SetMasterGain(f32),
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
            device_frame: 0,
            voices: VoicePool::new(VOICES, sample_rate),
            mix: vec![0.0; MAX_BLOCK * 2],
            master: Smoothed::new(1.0, 0.01, sample_rate),
            live_mode: LiveMode::default(),
        },
        handle: EngineHandle {
            sample_rate,
            commands: commands_tx,
            reports: reports_rx,
            garbage: garbage_rx,
            clock,
        },
        live: LiveSender { hits: live_tx },
        live_main: LiveSender { hits: live_main_tx },
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
        self.commands
            .push(command)
            .map_err(|rtrb::PushError::Full(command)| command)
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
    hits: Producer<LiveHit>,
}

impl LiveSender {
    /// Returns `false` if the queue was full and the hit was dropped.
    pub fn hit(&mut self, hit: LiveHit) -> bool {
        self.hits.push(hit).is_ok()
    }
}

#[derive(Debug)]
pub struct Engine {
    sample_rate: u32,
    commands: Consumer<Command>,
    /// Live hits from the input thread and from the main thread.
    live: [Consumer<LiveHit>; 2],
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
    device_frame: u64,
    voices: VoicePool,
    mix: Vec<f32>,
    master: Smoothed,
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

    /// Renders `out.len() / 2` frames of interleaved stereo.
    pub fn process(&mut self, out: &mut [f32], timing: BufferTiming) {
        self.handle_commands();
        self.publish_clock(timing);
        let total = out.len() / 2;
        self.schedule_live_hits(timing, total);

        let mut done = 0;
        while done < total {
            let len = (total - done).min(MAX_BLOCK);
            self.mix[..len * 2].fill(0.0);
            if self.playing {
                self.sequence(len);
            }
            let Engine {
                voices,
                mix,
                garbage,
                stash,
                freed_on_audio_thread,
                ..
            } = self;
            voices.render(mix, len, &mut |sample| {
                release(garbage, stash, freed_on_audio_thread, sample)
            });
            let (out_frames, _) = out[done * 2..(done + len) * 2].as_chunks_mut::<2>();
            let (mix_frames, _) = self.mix.as_chunks::<2>();
            for (frame_out, frame_in) in out_frames.iter_mut().zip(mix_frames) {
                let gain = self.master.step();
                frame_out[0] = soft_clip(frame_in[0] * gain);
                frame_out[1] = soft_clip(frame_in[1] * gain);
            }
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
                Command::SetMasterGain(gain) => self.master.set_target(gain.clamp(0.0, 4.0)),
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
        if let Some(old) = self.program.replace(program) {
            self.throw_away(Garbage::Program(old));
        }
        self.playing = false;
        self.frame = 0;
        self.cursor = 0;
        self.epoch += 1;
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
                let Ok(hit) = self.live[queue].pop() else { break };
                let delay = match self.live_mode {
                    LiveMode::Asap => 0,
                    LiveMode::Stable => {
                        let buffer_ns = buffer_frames as f64 * ns_per_frame;
                        let target = hit.at_ns as f64 + buffer_ns + timing.output_latency_ns as f64;
                        ((target - timing.playback_ns as f64) / ns_per_frame)
                            .round()
                            .clamp(0.0, 4.0 * buffer_frames as f64) as u32
                    }
                };
                let request = VoiceRequest {
                    pad: hit.pad,
                    sound: program.kit.pad(hit.pad),
                    velocity: hit.velocity,
                    delay,
                    starts_at: self.device_frame + u64::from(delay),
                };
                let Engine {
                    voices,
                    garbage,
                    stash,
                    freed_on_audio_thread,
                    reports,
                    ..
                } = self;
                voices.start(&request, &mut |sample| {
                    release(garbage, stash, freed_on_audio_thread, sample)
                });
                let _ = reports.push(Report::VoiceStarted(VoiceStart {
                    pad: hit.pad,
                    velocity: hit.velocity,
                    source: VoiceSource::Live { at_ns: hit.at_ns },
                    device_frame: request.starts_at,
                }));
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
                    let request = VoiceRequest {
                        pad: event.pad,
                        sound: program.kit.pad(event.pad),
                        velocity: event.velocity,
                        delay: offset as u32,
                        starts_at: self.device_frame + offset as u64,
                    };
                    let Engine {
                        voices,
                        garbage,
                        stash,
                        freed_on_audio_thread,
                        reports,
                        ..
                    } = self;
                    voices.start(&request, &mut |sample| {
                        release(garbage, stash, freed_on_audio_thread, sample)
                    });
                    let _ = reports.push(Report::VoiceStarted(VoiceStart {
                        pad: event.pad,
                        velocity: event.velocity,
                        source: VoiceSource::Sequence {
                            tick: event.tick,
                            transport_frame: event.frame,
                        },
                        device_frame: request.starts_at,
                    }));
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
