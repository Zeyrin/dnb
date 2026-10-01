//! The game's link to the audio engine. The engine runs on the sound card's
//! thread; this owns the output, the main-thread handle and the clock estimator.

use bevy::prelude::*;
use wu_audio::output::{DeviceOutput, NullOutput, OutputInfo, OutputOptions, prepare};
use wu_audio::{ClockEstimator, Command, EngineHandle, EngineParts, LiveHit, LiveSender, Program, Report, engine};
use wu_instruments::Pad;
use wu_time::TempoMap;

/// Sample rate of the null output, when there is no sound card.
const NULL_RATE: u32 = 48_000;
const NULL_BUFFER: u32 = 256;

#[derive(Debug)]
pub struct AudioPlugin {
    pub options: OutputOptions,
    /// Skip the sound card and run the engine silently.
    pub silent: bool,
}

impl Plugin for AudioPlugin {
    fn build(&self, app: &mut App) {
        app.insert_non_send(AudioLink::open(&self.options, self.silent))
            .add_message::<EngineReport>()
            .add_systems(PreUpdate, poll_engine);
    }
}

/// A report from the audio thread, forwarded to systems as a message.
#[derive(Message, Clone, Copy, Debug)]
pub struct EngineReport(pub Report);

#[derive(Debug)]
enum Output {
    Device(DeviceOutput),
    Null(NullOutput),
}

#[derive(Debug)]
pub struct AudioLink {
    pub handle: EngineHandle,
    pub estimator: ClockEstimator,
    /// Why the game fell back to the silent output, if it did.
    pub fallback: Option<String>,
    /// The loaded program's tempo, for turning frames into ticks.
    pub tempo: TempoMap,
    /// Live play from the main thread (keyboard, auditions).
    live_main: LiveSender,
    /// Live play from the input thread, until the input plugin takes it.
    live_input: Option<LiveSender>,
    /// Held so the sound keeps playing; dropping it closes the output.
    output: Output,
}

impl AudioLink {
    fn open(options: &OutputOptions, silent: bool) -> AudioLink {
        let failure = if silent {
            "started with --silent".to_owned()
        } else {
            match prepare(options) {
                Ok(prepared) => {
                    let parts = engine(prepared.sample_rate());
                    let EngineParts { engine, .. } = parts;
                    let rest = (parts.handle, parts.live, parts.live_main);
                    match prepared.start(engine) {
                        Ok(output) => return AudioLink::new(rest, Output::Device(output), None),
                        Err(error) => error.to_string(),
                    }
                }
                Err(error) => error.to_string(),
            }
        };
        warn!("no sound: {failure}");
        let parts = engine(NULL_RATE);
        let output = NullOutput::start(parts.engine, NULL_BUFFER);
        AudioLink::new(
            (parts.handle, parts.live, parts.live_main),
            Output::Null(output),
            Some(failure),
        )
    }

    fn new(
        (handle, live_input, live_main): (EngineHandle, LiveSender, LiveSender),
        output: Output,
        fallback: Option<String>,
    ) -> Self {
        AudioLink {
            handle,
            estimator: ClockEstimator::new(120),
            fallback,
            tempo: TempoMap::constant(120.0),
            live_main,
            live_input: Some(live_input),
            output,
        }
    }

    pub fn info(&self) -> &OutputInfo {
        match &self.output {
            Output::Device(device) => &device.info,
            Output::Null(null) => &null.info,
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.info().sample_rate
    }

    pub fn send(&mut self, command: Command) {
        if let Err(command) = self.handle.send(command) {
            warn!("audio command dropped, queue full: {command:?}");
        }
    }

    /// Replaces what the engine plays; the transport stops at tick 0. Returns
    /// the generation the clock reports once the engine has taken it in.
    pub fn load(&mut self, program: Program) -> u64 {
        self.tempo = program.tempo.clone();
        self.send(Command::Load(Box::new(program)));
        self.handle.loads_sent()
    }

    /// Whether the clock reflects the program of `generation`, playing.
    pub fn is_live(&self, generation: u64) -> bool {
        self.estimator
            .last()
            .is_some_and(|s| s.generation == generation && s.playing)
    }

    /// The sender the input thread plays pads through. Only handed out once.
    pub fn take_input_sender(&mut self) -> Option<LiveSender> {
        self.live_input.take()
    }

    /// Plays a pad from the main thread.
    pub fn hit(&mut self, pad: Pad, at_ns: u64) {
        if !self.live_main.hit(LiveHit {
            pad,
            velocity: 1.0,
            at_ns,
        }) {
            warn!("live hit dropped: queue full");
        }
    }

    /// The device frame reaching the speaker right now.
    pub fn device_frame_now(&self) -> Option<f64> {
        self.estimator.device_frame_at(wu_time::mono::now_ns())
    }

    /// The song position (fractional tick) reaching the speaker right now.
    pub fn tick_now(&self) -> Option<f64> {
        let frame = self.estimator.transport_frame_at(wu_time::mono::now_ns())?;
        Some(self.tempo.tick_at_frame(frame, self.sample_rate()))
    }

    pub fn playing(&self) -> bool {
        self.estimator.last().is_some_and(|s| s.playing)
    }
}

fn poll_engine(mut link: NonSendMut<AudioLink>, mut reports: MessageWriter<EngineReport>) {
    let link = &mut *link;
    let snapshot = link.handle.clock();
    link.estimator.observe(snapshot);
    link.handle.poll(|report| {
        reports.write(EngineReport(report));
    });
}
