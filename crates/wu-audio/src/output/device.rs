//! A real sound card, through cpal.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, InterfaceType, SampleFormat, SizedSample, SupportedBufferSize};

use super::{AudioError, OutputInfo, rt_checked};
use crate::engine::{BufferTiming, Engine};

/// Frames rendered per engine call inside one device callback; longer device
/// buffers are filled in several calls.
const SCRATCH_FRAMES: usize = 4096;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OutputOptions {
    /// Part of the device's name; the default output when `None`.
    pub device: Option<String>,
    /// Frames per callback. Smaller is lower latency; the driver may refuse.
    pub buffer_frames: Option<u32>,
}

/// The names of every output device on the default host.
pub fn list_outputs() -> Result<Vec<String>, AudioError> {
    let host = cpal::default_host();
    let devices = host.output_devices().map_err(|e| AudioError::Backend(e.to_string()))?;
    Ok(devices.map(|d| device_name(&d)).collect())
}

fn device_name(device: &cpal::Device) -> String {
    device
        .description()
        .map_or_else(|_| device.to_string(), |d| d.name().to_owned())
}

/// A device chosen and configured, not yet playing. Its sample rate decides
/// what the engine (and every baked kit) runs at.
pub struct PreparedOutput {
    device: cpal::Device,
    config: cpal::StreamConfig,
    format: SampleFormat,
    pub info: OutputInfo,
}

impl std::fmt::Debug for PreparedOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedOutput")
            .field("info", &self.info)
            .finish_non_exhaustive()
    }
}

pub fn prepare(options: &OutputOptions) -> Result<PreparedOutput, AudioError> {
    let host = cpal::default_host();
    let device = match &options.device {
        None => host.default_output_device().ok_or(AudioError::NoDevice)?,
        Some(wanted) => {
            let wanted_lower = wanted.to_lowercase();
            host.output_devices()
                .map_err(|e| AudioError::Backend(e.to_string()))?
                .find(|d| device_name(d).to_lowercase().contains(&wanted_lower))
                .ok_or_else(|| AudioError::DeviceNotFound(wanted.clone()))?
        }
    };
    let supported = device
        .default_output_config()
        .map_err(|e| AudioError::Backend(e.to_string()))?;
    let mut config = supported.config();
    let buffer_frames = options.buffer_frames.map(|wanted| match supported.buffer_size() {
        SupportedBufferSize::Range { min, max } => wanted.clamp(*min, *max),
        SupportedBufferSize::Unknown => wanted,
    });
    if let Some(frames) = buffer_frames {
        config.buffer_size = cpal::BufferSize::Fixed(frames);
    }
    let description = device.description().ok();
    let info = OutputInfo {
        host: host.id().name().to_owned(),
        device: device_name(&device),
        sample_rate: config.sample_rate,
        channels: config.channels,
        buffer_frames,
        sample_format: supported.sample_format().to_string(),
        bluetooth: description.is_some_and(|d| d.interface_type() == InterfaceType::Bluetooth),
    };
    Ok(PreparedOutput {
        device,
        config,
        format: supported.sample_format(),
        info,
    })
}

impl PreparedOutput {
    pub fn sample_rate(&self) -> u32 {
        self.info.sample_rate
    }

    /// Hands the engine to the device's callback and starts playing.
    pub fn start(self, engine: Engine) -> Result<DeviceOutput, AudioError> {
        let PreparedOutput {
            device,
            config,
            format,
            info,
        } = self;
        let stream = match format {
            SampleFormat::F32 => build::<f32>(&device, config, engine),
            SampleFormat::F64 => build::<f64>(&device, config, engine),
            SampleFormat::I16 => build::<i16>(&device, config, engine),
            SampleFormat::U16 => build::<u16>(&device, config, engine),
            SampleFormat::I32 => build::<i32>(&device, config, engine),
            SampleFormat::U32 => build::<u32>(&device, config, engine),
            SampleFormat::I8 => build::<i8>(&device, config, engine),
            SampleFormat::U8 => build::<u8>(&device, config, engine),
            other => return Err(AudioError::UnsupportedFormat(other.to_string())),
        }
        .map_err(|e| AudioError::Backend(e.to_string()))?;
        stream.play().map_err(|e| AudioError::Backend(e.to_string()))?;
        Ok(DeviceOutput { _stream: stream, info })
    }
}

/// A playing device. Dropping it stops the sound.
pub struct DeviceOutput {
    _stream: cpal::Stream,
    pub info: OutputInfo,
}

impl std::fmt::Debug for DeviceOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceOutput")
            .field("info", &self.info)
            .finish_non_exhaustive()
    }
}

fn build<T>(device: &cpal::Device, config: cpal::StreamConfig, mut engine: Engine) -> Result<cpal::Stream, cpal::Error>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels).max(1);
    let ns_per_frame = 1e9 / f64::from(config.sample_rate);
    let mut scratch = vec![0.0f32; SCRATCH_FRAMES * 2];
    device.build_output_stream::<T, _, _>(
        config,
        move |data: &mut [T], info: &cpal::OutputCallbackInfo| {
            let stamp = info.timestamp();
            let latency_ns = stamp.playback.duration_since(stamp.callback).as_nanos() as u64;
            let playback_ns = wu_time::mono::now_ns() + latency_ns;
            let frames = data.len() / channels;
            let mut done = 0;
            while done < frames {
                let n = (frames - done).min(SCRATCH_FRAMES);
                let timing = BufferTiming {
                    playback_ns: playback_ns + (done as f64 * ns_per_frame) as u64,
                    output_latency_ns: latency_ns,
                };
                let stereo = &mut scratch[..n * 2];
                rt_checked(|| engine.process(stereo, timing));
                let out = &mut data[done * channels..(done + n) * channels];
                for (frame, lr) in out.chunks_exact_mut(channels).zip(stereo.as_chunks::<2>().0) {
                    if channels == 1 {
                        frame[0] = T::from_sample(0.5 * (lr[0] + lr[1]));
                    } else {
                        frame[0] = T::from_sample(lr[0]);
                        frame[1] = T::from_sample(lr[1]);
                        for extra in &mut frame[2..] {
                            *extra = T::from_sample(0.0f32);
                        }
                    }
                }
                done += n;
            }
        },
        |error| tracing::warn!("audio stream error: {error}"),
        None,
    )
}
