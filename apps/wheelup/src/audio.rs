//! The game's link to the audio engine. The engine runs on the sound card's
//! thread; this owns the output, the main-thread handle and the clock estimator.

use bevy::prelude::*;
use wu_audio::output::{DeviceOutput, NullOutput, OutputInfo, OutputOptions, prepare};
use wu_audio::{ClockEstimator, Command, EngineHandle, EngineParts, LiveSender, Report, engine};
use wu_content::demo::{DEMO_BARS, DEMO_BPM, demo_program};
use wu_time::TempoMap;

/// Sample rate of the null output, when there is no sound card.
const NULL_RATE: u32 = 48_000;
const NULL_BUFFER: u32 = 256;

#[derive(Debug)]
pub struct AudioPlugin {
    pub options: OutputOptions,
    /// Skip the sound card and run the engine silently.
    pub silent: bool,
    pub autoplay: bool,
}

impl Plugin for AudioPlugin {
    fn build(&self, app: &mut App) {
        let mut link = AudioLink::open(&self.options, self.silent);
        let program = demo_program(link.info().sample_rate, DEMO_BPM, DEMO_BARS, true);
        link.tempo = program.tempo.clone();
        link.send(Command::Load(Box::new(program)));
        if self.autoplay {
            link.send(Command::Play);
        }
        app.insert_non_send(link)
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
    pub live: LiveSender,
    pub estimator: ClockEstimator,
    /// Why the game fell back to the silent output, if it did.
    pub fallback: Option<String>,
    pub tempo: TempoMap,
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
                    let EngineParts { engine, handle, live } = engine(prepared.sample_rate());
                    match prepared.start(engine) {
                        Ok(output) => {
                            return AudioLink::new(handle, live, Output::Device(output), None);
                        }
                        Err(error) => error.to_string(),
                    }
                }
                Err(error) => error.to_string(),
            }
        };
        warn!("no sound: {failure}");
        let EngineParts { engine, handle, live } = engine(NULL_RATE);
        let output = NullOutput::start(engine, NULL_BUFFER);
        AudioLink::new(handle, live, Output::Null(output), Some(failure))
    }

    fn new(handle: EngineHandle, live: LiveSender, output: Output, fallback: Option<String>) -> Self {
        AudioLink {
            handle,
            live,
            estimator: ClockEstimator::new(120),
            fallback,
            tempo: TempoMap::constant(DEMO_BPM),
            output,
        }
    }

    pub fn info(&self) -> &OutputInfo {
        match &self.output {
            Output::Device(device) => &device.info,
            Output::Null(null) => &null.info,
        }
    }

    pub fn send(&mut self, command: Command) {
        if let Err(command) = self.handle.send(command) {
            warn!("audio command dropped, queue full: {command:?}");
        }
    }

    /// The device frame reaching the speaker right now.
    pub fn device_frame_now(&self) -> Option<f64> {
        self.estimator.device_frame_at(wu_time::mono::now_ns())
    }

    /// The song position (fractional tick) reaching the speaker right now.
    pub fn tick_now(&self) -> Option<f64> {
        let frame = self.estimator.transport_frame_at(wu_time::mono::now_ns())?;
        Some(self.tempo.tick_at_frame(frame, self.info().sample_rate))
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
