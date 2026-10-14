//! Desktop audio and MIDI host for the LoomDSP engine.
//!
//! This crate connects an [`Engine`] to a real audio device (through `cpal`)
//! and MIDI input (through `midir`). It is the only place in the desktop app
//! that knows about audio APIs, so it can be replaced per platform later
//! without touching the engine (NFR-PORT-02, OQ-11).
//!
//! Phase 0 limits:
//! - Output only (no audio input yet).
//! - 32-bit float devices only. This covers CoreAudio, WASAPI shared mode,
//!   and PipeWire/PulseAudio defaults.
//! - MIDI ports are chosen at start. Hot-plugging comes in P1
//!   (FR-MIDI-08).

#![warn(missing_docs)]

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use loom_core::{Engine, MidiEvent, MidiSender};
use loom_rt::ScopedFlushDenormals;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Errors from the desktop host.
#[derive(Debug, thiserror::Error)]
pub enum HostError {
    /// No audio output device matched.
    #[error("no audio output device found{}", .0.as_deref().map(|n| format!(" matching `{n}`")).unwrap_or_default())]
    NoOutputDevice(Option<String>),
    /// The device has no 32-bit float configuration.
    #[error("audio device `{0}` does not support 32-bit float output, which Phase 0 requires")]
    UnsupportedFormat(String),
    /// The requested sample rate is not supported.
    #[error("audio device `{device}` does not support {rate} Hz")]
    UnsupportedSampleRate {
        /// Device name.
        device: String,
        /// Requested rate.
        rate: u32,
    },
    /// An error from the audio backend.
    #[error("audio error: {0}")]
    Audio(String),
    /// No MIDI input port matched.
    #[error("no MIDI input port found{}", .0.as_deref().map(|n| format!(" matching `{n}`")).unwrap_or_default())]
    NoMidiPort(Option<String>),
    /// An error from the MIDI backend.
    #[error("MIDI error: {0}")]
    Midi(String),
}

fn audio_error(error: impl std::fmt::Display) -> HostError {
    HostError::Audio(error.to_string())
}

/// How to choose and configure the audio output.
#[derive(Debug, Clone, Default)]
pub struct AudioOptions {
    /// Case-insensitive part of the device name. `None` uses the default device.
    pub device: Option<String>,
    /// Requested sample rate. `None` uses the device default.
    pub sample_rate: Option<u32>,
    /// Requested buffer size in frames. `None` uses the device default.
    pub buffer_size: Option<u32>,
}

/// Names of all audio output devices.
pub fn output_device_names() -> Result<Vec<String>, HostError> {
    let host = cpal::default_host();
    let devices = host.output_devices().map_err(audio_error)?;
    Ok(devices.map(|d| device_name(&d)).collect())
}

/// Names of all MIDI input ports.
pub fn midi_input_names() -> Result<Vec<String>, HostError> {
    let input = midir::MidiInput::new("LoomDSP").map_err(|e| HostError::Midi(e.to_string()))?;
    Ok(input
        .ports()
        .iter()
        .filter_map(|port| input.port_name(port).ok())
        .collect())
}

fn device_name(device: &cpal::Device) -> String {
    device
        .description()
        .map(|d| d.name().to_string())
        .unwrap_or_else(|_| "<unnamed device>".to_string())
}

/// A chosen audio output, not yet running.
///
/// Open the device first, so you know the sample rate to create the engine
/// with. Then call [`AudioOutput::start`].
pub struct AudioOutput {
    device: cpal::Device,
    config: cpal::StreamConfig,
    name: String,
}

impl AudioOutput {
    /// Chooses a device and a 32-bit float configuration.
    pub fn open(options: &AudioOptions) -> Result<Self, HostError> {
        let host = cpal::default_host();
        let device = match &options.device {
            None => host.default_output_device(),
            Some(wanted) => {
                let wanted = wanted.to_lowercase();
                host.output_devices()
                    .map_err(audio_error)?
                    .find(|d| device_name(d).to_lowercase().contains(&wanted))
            }
        }
        .ok_or_else(|| HostError::NoOutputDevice(options.device.clone()))?;
        let name = device_name(&device);

        let default = device.default_output_config().map_err(audio_error)?;
        let rate = options.sample_rate.unwrap_or(default.sample_rate());
        let supported = if default.sample_format() == cpal::SampleFormat::F32 && rate == default.sample_rate() {
            default
        } else {
            let mut ranges = device.supported_output_configs().map_err(audio_error)?;
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
                        HostError::UnsupportedFormat(name.clone())
                    }
                })?;
            range.with_sample_rate(rate)
        };
        let mut config = supported.config();
        if let Some(frames) = options.buffer_size {
            config.buffer_size = cpal::BufferSize::Fixed(frames);
        }
        Ok(Self { device, config, name })
    }

    /// Device name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Sample rate in Hz. Create the engine with this rate.
    pub fn sample_rate(&self) -> u32 {
        self.config.sample_rate
    }

    /// Number of output channels.
    pub fn channels(&self) -> u16 {
        self.config.channels
    }

    /// Requested buffer size, if fixed.
    pub fn buffer_size(&self) -> Option<u32> {
        match self.config.buffer_size {
            cpal::BufferSize::Fixed(frames) => Some(frames),
            cpal::BufferSize::Default => None,
        }
    }

    /// Starts audio. The engine moves to the audio thread.
    pub fn start(self, mut engine: Engine) -> Result<RunningAudio, HostError> {
        let stats = Arc::new(CallbackStats::default());
        let channels = usize::from(self.config.channels.max(1));
        let sample_rate = f64::from(self.config.sample_rate);

        // Pre-allocate scratch buffers before the stream starts. The audio
        // callback must not allocate (NFR-RT-01).
        const SCRATCH: usize = 8192;
        let mut left = vec![0.0f32; SCRATCH];
        let mut right = vec![0.0f32; SCRATCH];

        let callback_stats = stats.clone();
        let data_callback = move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            let _ftz = ScopedFlushDenormals::new();
            let started = Instant::now();
            let frames = data.len() / channels;

            let mut done = 0;
            while done < frames {
                let n = (frames - done).min(SCRATCH);
                engine.process(&mut left[..n], &mut right[..n]);
                let out = &mut data[done * channels..(done + n) * channels];
                for (frame, (l, r)) in out.chunks_exact_mut(channels).zip(left.iter().zip(right.iter())) {
                    if channels == 1 {
                        frame[0] = 0.5 * (l + r);
                    } else {
                        frame[0] = *l;
                        frame[1] = *r;
                        frame[2..].fill(0.0);
                    }
                }
                done += n;
            }

            let budget = frames as f64 / sample_rate;
            let load = started.elapsed().as_secs_f64() / budget.max(1e-9);
            callback_stats.record(frames, load);
        };

        let error_stats = stats.clone();
        let error_callback = move |_error: cpal::Error| {
            // Printing here could block the audio thread on some backends.
            // Count it; the control thread reports it.
            error_stats.stream_errors.fetch_add(1, Ordering::Relaxed);
        };

        let stream = self
            .device
            .build_output_stream::<f32, _, _>(self.config, data_callback, error_callback, None)
            .map_err(audio_error)?;
        stream.play().map_err(audio_error)?;
        Ok(RunningAudio {
            _stream: stream,
            stats,
            name: self.name,
            sample_rate: self.config.sample_rate,
        })
    }
}

/// Audio that is playing. Audio stops when this is dropped.
pub struct RunningAudio {
    _stream: cpal::Stream,
    stats: Arc<CallbackStats>,
    name: String,
    sample_rate: u32,
}

impl RunningAudio {
    /// Callback timing counters.
    pub fn stats(&self) -> &Arc<CallbackStats> {
        &self.stats
    }

    /// Device name.
    pub fn device_name(&self) -> &str {
        &self.name
    }

    /// Sample rate in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}

/// Audio callback timing, updated by the audio thread.
///
/// "Load" is the time spent in the callback divided by the time the buffer
/// represents. Above 100% the callback missed its deadline (an overrun),
/// which is very likely an audible dropout.
#[derive(Debug, Default)]
pub struct CallbackStats {
    callbacks: AtomicU64,
    frames: AtomicU64,
    overruns: AtomicU64,
    stream_errors: AtomicU64,
    /// Highest load seen, in parts per thousand.
    max_load_permille: AtomicU64,
    /// Load of the latest callback, in parts per thousand.
    last_load_permille: AtomicU64,
    smallest_buffer: AtomicU64,
    largest_buffer: AtomicU64,
}

/// A copy of [`CallbackStats`] at one moment.
#[derive(Debug, Clone, Copy, Default)]
pub struct StatsSnapshot {
    /// Callbacks so far.
    pub callbacks: u64,
    /// Frames rendered so far.
    pub frames: u64,
    /// Callbacks that took longer than their buffer duration.
    pub overruns: u64,
    /// Errors reported by the audio backend (for example xruns).
    pub stream_errors: u64,
    /// Highest load seen (1.0 = 100% of the time budget).
    pub max_load: f64,
    /// Load of the latest callback.
    pub last_load: f64,
    /// Smallest buffer the device asked for, in frames.
    pub smallest_buffer: u64,
    /// Largest buffer the device asked for, in frames.
    pub largest_buffer: u64,
}

impl CallbackStats {
    fn record(&self, frames: usize, load: f64) {
        let permille = (load * 1000.0) as u64;
        self.callbacks.fetch_add(1, Ordering::Relaxed);
        self.frames.fetch_add(frames as u64, Ordering::Relaxed);
        self.last_load_permille.store(permille, Ordering::Relaxed);
        self.max_load_permille.fetch_max(permille, Ordering::Relaxed);
        if load > 1.0 {
            self.overruns.fetch_add(1, Ordering::Relaxed);
        }
        let frames = frames as u64;
        self.largest_buffer.fetch_max(frames, Ordering::Relaxed);
        let smallest = self.smallest_buffer.load(Ordering::Relaxed);
        if smallest == 0 || frames < smallest {
            self.smallest_buffer.store(frames, Ordering::Relaxed);
        }
    }

    /// Reads all counters.
    pub fn snapshot(&self) -> StatsSnapshot {
        StatsSnapshot {
            callbacks: self.callbacks.load(Ordering::Relaxed),
            frames: self.frames.load(Ordering::Relaxed),
            overruns: self.overruns.load(Ordering::Relaxed),
            stream_errors: self.stream_errors.load(Ordering::Relaxed),
            max_load: self.max_load_permille.load(Ordering::Relaxed) as f64 / 1000.0,
            last_load: self.last_load_permille.load(Ordering::Relaxed) as f64 / 1000.0,
            smallest_buffer: self.smallest_buffer.load(Ordering::Relaxed),
            largest_buffer: self.largest_buffer.load(Ordering::Relaxed),
        }
    }
}

/// The engine's MIDI input, shared by every MIDI port and by the UI (for
/// example an on-screen "hold note" button).
///
/// The engine's MIDI queue has a single producer, so senders take turns
/// through a mutex. Only MIDI and UI threads lock it; the audio thread reads
/// the queue without locks, so real-time rules are kept (NFR-RT-01).
pub type SharedMidi = Arc<Mutex<MidiSender>>;

/// Wraps a [`MidiSender`] so several threads can send through it.
pub fn share_midi(sender: MidiSender) -> SharedMidi {
    Arc::new(Mutex::new(sender))
}

/// Sends one event through a shared sender. Returns false if the queue was
/// full.
pub fn send_midi(midi: &SharedMidi, event: MidiEvent) -> bool {
    // A panic on another sending thread poisons the lock. The sender is only
    // a queue producer with no state that a panic could leave half-changed,
    // so keep using it instead of silently dropping all later MIDI.
    midi.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .send(event)
}

/// An open MIDI input. MIDI stops when this is dropped.
pub struct MidiConnection {
    _connection: midir::MidiInputConnection<SharedMidi>,
    port_name: String,
}

impl MidiConnection {
    /// Name of the connected port.
    pub fn port_name(&self) -> &str {
        &self.port_name
    }
}

fn midi_input() -> Result<midir::MidiInput, HostError> {
    let mut input = midir::MidiInput::new("LoomDSP").map_err(|e| HostError::Midi(e.to_string()))?;
    // System messages such as clock are passed through; the engine ignores
    // what it does not use. Only SysEx is filtered out.
    input.ignore(midir::Ignore::Sysex);
    Ok(input)
}

fn open_port(
    input: midir::MidiInput,
    port: &midir::MidiInputPort,
    midi: SharedMidi,
) -> Result<MidiConnection, HostError> {
    let port_name = input.port_name(port).unwrap_or_else(|_| "<unnamed port>".into());
    let connection = input
        .connect(
            port,
            "loomdsp-in",
            |_timestamp, bytes, midi: &mut SharedMidi| {
                if bytes.first().is_some_and(|&status| status != 0xF0) && bytes.len() <= 3 {
                    // Frame 0: applied at the start of the next audio buffer
                    // (NFR-PERF-07). Sub-buffer timestamps come later.
                    send_midi(midi, MidiEvent::from_bytes(0, bytes));
                }
            },
            midi,
        )
        .map_err(|e| HostError::Midi(format!("{port_name}: {e}")))?;
    Ok(MidiConnection {
        _connection: connection,
        port_name,
    })
}

/// Connects one MIDI input port to the engine.
///
/// `port` is a case-insensitive part of the port name. `None` picks the first
/// port. Returns `Ok(None)` if `port` is `None` and no ports exist.
pub fn connect_midi(port: Option<&str>, midi: SharedMidi) -> Result<Option<MidiConnection>, HostError> {
    let input = midi_input()?;
    let ports = input.ports();
    let chosen = match port {
        None => ports.first().cloned(),
        Some(wanted) => {
            let wanted = wanted.to_lowercase();
            ports
                .iter()
                .find(|p| {
                    input
                        .port_name(p)
                        .map(|n| n.to_lowercase().contains(&wanted))
                        .unwrap_or(false)
                })
                .cloned()
        }
    };
    let Some(chosen) = chosen else {
        return match port {
            None => Ok(None),
            Some(p) => Err(HostError::NoMidiPort(Some(p.to_string()))),
        };
    };
    open_port(input, &chosen, midi).map(Some)
}

/// Connects every MIDI input port to the engine, so any keyboard or
/// controller plays without choosing it first. Ports that fail to open are
/// returned as errors next to the ones that worked.
pub fn connect_all_midi(midi: &SharedMidi) -> Result<(Vec<MidiConnection>, Vec<HostError>), HostError> {
    let ports = midi_input()?.ports();
    let mut connections = Vec::new();
    let mut errors = Vec::new();
    for port in &ports {
        // midir consumes the `MidiInput` on connect, so each port needs its own.
        match midi_input().and_then(|input| open_port(input, port, midi.clone())) {
            Ok(connection) => connections.push(connection),
            Err(error) => errors.push(error),
        }
    }
    Ok((connections, errors))
}
