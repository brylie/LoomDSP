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

use crate::{AudioOptions, HostError, Recorder, audio_error, choose_config, device_name, find_device};
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
    /// Errors the input stream reported, such as the device going away.
    /// The file may then end early or have a gap.
    pub stream_errors: u64,
}

/// An input device being recorded to a WAV file. Stop it with
/// [`Capture::stop`]; dropping it also stops and closes the file.
pub struct Capture {
    // Declared before the sink, so it drops first: the callback has
    // stopped pushing when the writer drains the last frames.
    stream: cpal::Stream,
    sink: Sink,
    name: String,
    sample_rate: u32,
}

/// The part of a capture that needs no device: the file, and the counts
/// the stream's callbacks update.
struct Sink {
    recorder: Recorder,
    frames: Arc<AtomicU64>,
    errors: Arc<AtomicU64>,
}

impl Sink {
    /// Creates the WAV file at `sample_rate` and starts its writer.
    fn start(path: &Path, sample_rate: u32) -> Result<Self, HostError> {
        Ok(Self {
            recorder: Recorder::start(path, sample_rate)?,
            frames: Arc::new(AtomicU64::new(0)),
            errors: Arc::new(AtomicU64::new(0)),
        })
    }

    /// The body of the input callback, for frames of `channels` channels.
    /// Real-time safe: it only pushes into the ring and counts.
    fn input(&self, channels: usize) -> impl FnMut(&[f32]) + Send + 'static {
        let mut tap = self.recorder.tap();
        let frames = self.frames.clone();
        move |data| {
            tap.push_interleaved(data, channels);
            frames.fetch_add((data.len() / channels) as u64, Ordering::Relaxed);
        }
    }

    /// The body of the error callback. Printing there could block the
    /// audio thread on some backends, so it only counts.
    fn error(&self) -> impl FnMut() + Send + 'static {
        let errors = self.errors.clone();
        move || {
            errors.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Writes what is left and closes the file. Call it after the input
    /// has stopped. Fails if a write failed.
    fn finish(self) -> Result<CaptureReport, HostError> {
        let report = CaptureReport {
            frames: self.frames.load(Ordering::Relaxed),
            dropped: self.recorder.dropped(),
            stream_errors: self.errors.load(Ordering::Relaxed),
        };
        match self.recorder.close() {
            Some(e) => Err(HostError::Record(e)),
            None => Ok(report),
        }
    }
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
        let config = choose_config(
            &name,
            default,
            || device.supported_input_configs().map_err(audio_error),
            options,
            HostError::UnsupportedInputFormat,
        )?;
        let sample_rate = config.sample_rate;
        let channels = usize::from(config.channels.max(1));

        let sink = Sink::start(path, sample_rate)?;
        let mut input = sink.input(channels);
        let mut error = sink.error();
        let stream = device
            .build_input_stream::<f32, _, _>(
                config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| input(data),
                move |_: cpal::Error| error(),
                None,
            )
            .map_err(audio_error)?;
        stream.play().map_err(audio_error)?;
        Ok(Self {
            stream,
            sink,
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
        self.sink.frames.load(Ordering::Relaxed)
    }

    /// Stops the stream, writes what is left, and closes the file. Fails
    /// if a write failed; what was written before stays readable.
    pub fn stop(self) -> Result<CaptureReport, HostError> {
        let Self { stream, sink, .. } = self;
        // The stream's callback holds the tap: dropping the stream ends it.
        drop(stream);
        sink.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::TestDir;

    /// The sink writes what a fake input delivers, counts its frames and
    /// errors, and reports them when it finishes.
    #[test]
    fn the_sink_writes_and_counts_what_the_input_delivers() {
        let dir = TestDir::new("capture-sink");
        let path = dir.0.join("capture.wav");
        let sink = Sink::start(&path, 48_000).unwrap();
        let mut input = sink.input(2);
        let mut error = sink.error();
        input(&[0.1, 0.2, 0.3, 0.4]);
        input(&[0.5, 0.6]);
        error();
        drop(input);
        let report = sink.finish().unwrap();
        assert_eq!(
            report,
            CaptureReport {
                frames: 3,
                dropped: 0,
                stream_errors: 1
            }
        );

        let mut reader = hound::WavReader::open(&path).unwrap();
        assert_eq!(reader.spec().sample_rate, 48_000);
        let samples: Vec<f32> = reader.samples::<f32>().map(Result::unwrap).collect();
        assert_eq!(samples, [0.1, 0.2, 0.3, 0.4, 0.5, 0.6]);
    }

    /// Frames that do not fit the ring are reported as dropped, and still
    /// counted as delivered.
    #[test]
    fn the_sink_reports_dropped_frames() {
        let dir = TestDir::new("capture-sink-full");
        // 10 Hz: the ring holds 2 seconds, 20 frames.
        let sink = Sink::start(&dir.0.join("capture.wav"), 10).unwrap();
        let mut input = sink.input(1);
        input(&[0.0; 100]);
        drop(input);
        let report = sink.finish().unwrap();
        assert_eq!(report.frames, 100);
        assert!(report.dropped > 0, "{report:?}");
    }

    /// A file that cannot be created is an error before any device opens.
    #[test]
    fn an_unwritable_file_is_an_error() {
        let dir = TestDir::new("capture-sink-bad");
        let path = dir.0.join("no such folder").join("capture.wav");
        assert!(matches!(Sink::start(&path, 48_000), Err(HostError::Record(_))));
    }
}
