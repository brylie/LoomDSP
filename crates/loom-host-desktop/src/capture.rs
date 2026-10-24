//! Capturing an input device to a WAV file, to check what really reached
//! a device (`docs/design/audio-qa.md`, Part 4).
//!
//! An app plays into a loopback device, and the capture records that
//! device's input side. Unlike `--record`, which saves what the engine
//! rendered, the file then shows what the device played: a stream cut
//! before its fade, or a device gone silent, is in it.
//!
//! The input callback only pushes into the same lock-free ring as a
//! recording ([`Recorder`]), and its writer thread does the file I/O. The
//! capture writes what the device delivered, with nothing resampled or
//! filled in.

use crate::{AudioOptions, HostError, Recorder, audio_error, device_name, find_device};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// What a finished capture wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureReport {
    /// Frames the device delivered.
    pub frames: u64,
    /// Frames that did not fit the ring and are missing from the file.
    pub dropped: u64,
}

/// An input device being recorded to a WAV file. Stop it with
/// [`Capture::stop`]; dropping it also stops and closes the file.
pub struct Capture {
    // Declared before the recorder, so it drops first: the callback has
    // stopped pushing when the writer drains the last frames.
    stream: cpal::Stream,
    recorder: Recorder,
    frames: Arc<AtomicU64>,
    name: String,
    sample_rate: u32,
}

impl Capture {
    /// Opens the input device that matches `options.device`, creates the
    /// WAV file (32-bit float, stereo, at the device's rate), and starts
    /// recording. The file holds the device's first two channels, or its
    /// one channel on both sides.
    pub fn start(options: &AudioOptions, path: &Path) -> Result<Self, HostError> {
        let host = cpal::default_host();
        let device = match &options.device {
            None => host.default_input_device(),
            Some(wanted) => find_device(host.input_devices().map_err(audio_error)?, wanted, options.exact),
        }
        .ok_or_else(|| HostError::NoInputDevice(options.device.clone()))?;
        let name = device_name(&device);

        let default = device.default_input_config().map_err(audio_error)?;
        let rate = options.sample_rate.unwrap_or(default.sample_rate());
        let supported = if default.sample_format() == cpal::SampleFormat::F32 && rate == default.sample_rate() {
            default
        } else {
            let mut ranges = device.supported_input_configs().map_err(audio_error)?;
            let range = ranges
                .find(|r| {
                    r.sample_format() == cpal::SampleFormat::F32
                        && r.min_sample_rate() <= rate
                        && rate <= r.max_sample_rate()
                })
                .ok_or_else(|| {
                    if options.sample_rate.is_some() {
                        HostError::UnsupportedSampleRate {
                            device: name.clone(),
                            rate,
                        }
                    } else {
                        HostError::UnsupportedInputFormat(name.clone())
                    }
                })?;
            range.with_sample_rate(rate)
        };
        let mut config = supported.config();
        if let Some(frames) = options.buffer_size {
            config.buffer_size = cpal::BufferSize::Fixed(frames);
        }
        let sample_rate = config.sample_rate;
        let channels = usize::from(config.channels.max(1));

        let recorder = Recorder::start(path, sample_rate)?;
        let mut tap = recorder.tap();
        let frames = Arc::new(AtomicU64::new(0));
        let counted = frames.clone();
        let data_callback = move |data: &[f32], _: &cpal::InputCallbackInfo| {
            tap.push_interleaved(data, channels);
            counted.fetch_add((data.len() / channels) as u64, Ordering::Relaxed);
        };
        // Printing here could block the audio thread on some backends, and
        // an input error does not drop frames by itself: ignore it.
        let stream = device
            .build_input_stream::<f32, _, _>(config, data_callback, |_| {}, None)
            .map_err(audio_error)?;
        stream.play().map_err(audio_error)?;
        Ok(Self {
            stream,
            recorder,
            frames,
            name,
            sample_rate,
        })
    }

    /// Device name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Sample rate of the device and the file, in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Frames delivered so far.
    pub fn frames(&self) -> u64 {
        self.frames.load(Ordering::Relaxed)
    }

    /// Stops the stream, writes what is left, and closes the file. Fails
    /// if a write failed; what was written before stays readable.
    pub fn stop(self) -> Result<CaptureReport, HostError> {
        let Self {
            stream,
            recorder,
            frames,
            ..
        } = self;
        // The stream's callback holds the tap: dropping the stream ends it.
        drop(stream);
        let report = CaptureReport {
            frames: frames.load(Ordering::Relaxed),
            dropped: recorder.dropped(),
        };
        match recorder.close() {
            Some(e) => Err(HostError::Record(e)),
            None => Ok(report),
        }
    }
}
