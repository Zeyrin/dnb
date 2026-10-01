//! Where the engine's sound goes: a real device through cpal, or a "null"
//! output that keeps the engine and its clock running at real-time pace
//! without a sound card (CI, headless machines, broken drivers).

#[cfg(feature = "device")]
mod device;
mod null;

#[cfg(feature = "device")]
pub use device::{DeviceOutput, OutputOptions, PreparedOutput, list_outputs, prepare};
pub use null::NullOutput;

/// What an output turned out to be.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputInfo {
    pub host: String,
    pub device: String,
    pub sample_rate: u32,
    pub channels: u16,
    /// The buffer size asked for, if one was; otherwise the driver decides.
    pub buffer_frames: Option<u32>,
    pub sample_format: String,
    /// Bluetooth adds 100 ms or more: too much for live play.
    pub bluetooth: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("no audio output device")]
    NoDevice,
    #[error("no output device matches \"{0}\"")]
    DeviceNotFound(String),
    #[error("unsupported sample format {0}")]
    UnsupportedFormat(String),
    #[error("audio backend: {0}")]
    Backend(String),
}

/// Runs the engine's `process` with the allocation checker armed, when the
/// `rt-check` feature is on.
#[inline]
pub(crate) fn rt_checked<T>(f: impl FnOnce() -> T) -> T {
    #[cfg(feature = "rt-check")]
    {
        assert_no_alloc::assert_no_alloc(f)
    }
    #[cfg(not(feature = "rt-check"))]
    {
        f()
    }
}
