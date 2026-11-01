//! Desktop audio and MIDI host for the LoomDSP engine.
//!
//! This crate connects an [`Engine`] to a real audio device (through `cpal`)
//! and MIDI input (through `midir`). It is the only place in the desktop app
//! that knows about audio APIs, so it can be replaced per platform later
//! without touching the engine (NFR-PORT-02, OQ-11).
//!
//! Phase 0 limits:
//! - [`AudioInput`] feeds a separate input stream to the engine through a
//!   bounded ring. [`Capture`] records an input device to a file for checks.
//! - 32-bit float devices only. This covers CoreAudio, WASAPI shared mode,
//!   and PipeWire/PulseAudio defaults.
//! - MIDI ports are chosen at start, or opened and closed by name. Hot-plugging
//!   comes in M1 (FR-MIDI-08).
//!
//! For development and tests, [`AudioOutput::clock`] plays the engine from
//! a timer thread instead of a device, [`AudioOutput::stepped`] plays it only
//! when a test asks and keeps every sample on a [`Tape`], and
//! [`AudioOutput::record`] saves what the engine plays to a WAV file through
//! a [`Recorder`] that can outlive many streams. A [`VirtualController`]
//! plays MIDI into the app through a virtual port, on macOS and Linux.

#![warn(missing_docs)]

mod capture;
mod record;
mod stepped;
mod virtual_midi;

mod input;
pub use input::AudioInput;
pub use input::microphone_access_denied;

pub use capture::{Capture, CaptureReport};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use loom_core::{Engine, MidiEvent, MidiSender};
use loom_rt::ScopedFlushDenormals;
pub use record::Recorder;
use record::Tap;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
pub use stepped::Tape;
pub use virtual_midi::VirtualController;

/// Errors from the desktop host.
#[derive(Debug, thiserror::Error)]
pub enum HostError {
    /// No audio input device matched.
    #[error("no audio input device found{}", .0.as_deref().map(|n| format!(" matching `{n}`")).unwrap_or_default())]
    NoInputDevice(Option<String>),
    /// The input device has no 32-bit float configuration.
    #[error("audio device `{0}` does not support 32-bit float input")]
    UnsupportedInputFormat(String),
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
    /// The system cannot make virtual MIDI ports: Windows, or Linux
    /// without the ALSA sequencer (`/dev/snd/seq`).
    #[error("no MIDI service for virtual ports: {0}")]
    NoMidiService(String),
    /// The recording file could not be created or written.
    #[error("recording: {0}")]
    Record(String),
}

/// An error that prevented an audio stream from starting, returning the
/// [`AudioOutput`] and [`Engine`] that could not be started.
pub struct StartError {
    /// The output device that could not start.
    pub output: AudioOutput,
    /// The engine that was to be played on `output`.
    pub engine: Engine,
    /// The error that stopped the stream from starting.
    pub error: HostError,
}

impl fmt::Debug for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StartError")
            .field("output", &self.output.name())
            .field("error", &self.error)
            .finish()
    }
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.error.fmt(f)
    }
}

impl std::error::Error for StartError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

impl From<StartError> for HostError {
    fn from(e: StartError) -> Self {
        e.error
    }
}

impl HostError {
    /// True when the error means the hardware is not on this machine: no
    /// device or port matched, or the system has none at all. Tests that
    /// need hardware skip on these errors and fail on any other, so a
    /// broken device still fails its test.
    pub fn is_missing_hardware(&self) -> bool {
        // A device that is here but cannot do what a test asks, such as a
        // sample rate it does not offer, fails the test: the test should
        // ask for what the device offers.
        matches!(
            self,
            Self::NoOutputDevice(_) | Self::NoInputDevice(_) | Self::NoMidiPort(_) | Self::NoMidiService(_)
        )
    }
}

fn audio_error(error: impl std::fmt::Display) -> HostError {
    HostError::Audio(error.to_string())
}

/// How to choose and configure the audio output.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AudioOptions {
    /// Case-insensitive part of the device name. `None` uses the default device.
    pub device: Option<String>,
    /// Match [`AudioOptions::device`] to the whole name, exactly, instead of
    /// to a part of it. A choice saved from a list knows the full name.
    pub exact: bool,
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

/// Names of all audio input devices.
pub fn input_device_names() -> Result<Vec<String>, HostError> {
    let host = cpal::default_host();
    let devices = host.input_devices().map_err(audio_error)?;
    Ok(devices.map(|d| device_name(&d)).collect())
}

/// The system default audio input's name, when one exists.
pub fn default_input_device_name() -> Option<String> {
    cpal::default_host()
        .default_input_device()
        .map(|device| device_name(&device))
}

/// The first of `devices` whose name contains `wanted`, ignoring case, or
/// is exactly `wanted` if `exact`.
fn find_device(mut devices: impl Iterator<Item = cpal::Device>, wanted: &str, exact: bool) -> Option<cpal::Device> {
    if exact {
        return devices.find(|d| device_name(d) == wanted);
    }
    let wanted = wanted.to_lowercase();
    devices.find(|d| device_name(d).to_lowercase().contains(&wanted))
}

/// What an output device offers, for a settings window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputInfo {
    /// Full device name.
    pub name: String,
    /// The rate the device uses when none is asked for, in Hz.
    pub default_rate: u32,
    /// Sample rate ranges, in Hz, of the device's 32-bit float
    /// configurations, lowest and highest included.
    pub rates: Vec<(u32, u32)>,
    /// Buffer sizes in frames the device accepts, lowest and highest
    /// included, or `None` if it does not say.
    pub buffer: Option<(u32, u32)>,
}

/// The output devices, and the name of the system default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outputs {
    /// Name of the system default output, if there is one.
    pub default: Option<String>,
    /// Every output device that works with 32-bit float.
    pub devices: Vec<OutputInfo>,
}

/// Lists the output devices and what each offers. Asking can take a moment
/// on some systems, so call it when a list is needed, not every frame.
pub fn outputs() -> Result<Outputs, HostError> {
    let host = cpal::default_host();
    let default = host.default_output_device().map(|d| device_name(&d));
    let mut devices = Vec::new();
    for device in host.output_devices().map_err(audio_error)? {
        // A device that cannot say what it offers cannot be chosen.
        let (Ok(default_config), Ok(configs)) = (device.default_output_config(), device.supported_output_configs())
        else {
            continue;
        };
        let mut rates = Vec::new();
        let mut buffer: Option<(u32, u32)> = None;
        for range in configs.filter(|r| r.sample_format() == cpal::SampleFormat::F32) {
            rates.push((range.min_sample_rate(), range.max_sample_rate()));
            if let cpal::SupportedBufferSize::Range { min, max } = *range.buffer_size() {
                buffer = Some(buffer.map_or((min, max), |(lo, hi)| (lo.min(min), hi.max(max))));
            }
        }
        if rates.is_empty() {
            continue;
        }
        devices.push(OutputInfo {
            name: device_name(&device),
            default_rate: default_config.sample_rate(),
            rates,
            buffer,
        });
    }
    Ok(Outputs { default, devices })
}

/// One MIDI input as the app lists it (`docs/design/device-changes.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MidiInputInfo {
    /// The system's name, made unique in the list: a second input with the
    /// same name is `Pad (2)`, a third `Pad (3)`, by their order on the
    /// system.
    pub name: String,
    /// The system's identity for the port, as an opaque text. A port that
    /// comes back with a new identity is a new connection.
    pub id: String,
}

/// Names made unique by order: the first `Pad` stays `Pad`, the second is
/// `Pad (2)`, and so on. The system gives no better identity portably.
pub fn unique_names(names: &[String]) -> Vec<String> {
    let mut seen: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    names
        .iter()
        .map(|name| {
            let count = seen.entry(name.as_str()).or_insert(0);
            *count += 1;
            if *count == 1 {
                name.clone()
            } else {
                format!("{name} ({count})")
            }
        })
        .collect()
}

/// The inputs as listed: system names, in system order, made unique, each
/// with its port identity. `ids` has one entry per name.
pub fn input_infos(names: Vec<String>, ids: Vec<String>) -> Vec<MidiInputInfo> {
    unique_names(&names)
        .into_iter()
        .zip(ids)
        .map(|(name, id)| MidiInputInfo { name, id })
        .collect()
}

/// Where the input listed as `name` is among `names`, the system names in
/// system order. An error names the input when it is not there.
pub fn find_input(names: &[String], name: &str) -> Result<usize, HostError> {
    unique_names(names)
        .iter()
        .position(|n| n == name)
        .ok_or_else(|| HostError::NoMidiPort(Some(name.to_string())))
}

/// The system names of a client's ports, in order.
fn port_names(client: &midir::MidiInput, ports: &[midir::MidiInputPort]) -> Vec<String> {
    ports
        .iter()
        .map(|port| client.port_name(port).unwrap_or_else(|_| "<unnamed port>".into()))
        .collect()
}

/// The MIDI inputs now, with unique names and port identities.
pub fn midi_inputs() -> Result<Vec<MidiInputInfo>, HostError> {
    let client = midi_input()?;
    let ports = client.ports();
    Ok(input_infos(
        port_names(&client, &ports),
        ports.iter().map(|p| p.id()).collect(),
    ))
}

/// Connects the MIDI input listed as `name` by [`midi_inputs`], with its
/// events stamped as `input`, so the engine knows which notes it holds.
pub fn connect_midi_input(
    name: &str,
    midi: SharedMidi,
    input: loom_core::MidiInputId,
) -> Result<MidiConnection, HostError> {
    let client = midi_input()?;
    let ports = client.ports();
    let port = ports[find_input(&port_names(&client, &ports), name)?].clone();
    let mut connection = open_port_as(client, &port, midi, input)?;
    connection.port_name = name.to_string();
    Ok(connection)
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

/// Chooses a 32-bit float configuration for the device called `name`:
/// its `default`, if that is float at the rate asked for, or else the first
/// float range of `ranges` that holds the rate. `ranges` is only asked for
/// when needed, because asking can take a moment. With no float range at
/// the rate, the error is [`HostError::UnsupportedSampleRate`] if a rate
/// was asked for, and `unsupported` of the name if not. A buffer size in
/// `options` is fixed in the result.
fn choose_config<I>(
    name: &str,
    default: cpal::SupportedStreamConfig,
    ranges: impl FnOnce() -> Result<I, HostError>,
    options: &AudioOptions,
    unsupported: fn(String) -> HostError,
) -> Result<cpal::StreamConfig, HostError>
where
    I: IntoIterator<Item = cpal::SupportedStreamConfigRange>,
{
    let rate = options.sample_rate.unwrap_or(default.sample_rate());
    let supported = if default.sample_format() == cpal::SampleFormat::F32 && rate == default.sample_rate() {
        default
    } else {
        ranges()?
            .into_iter()
            .find(|r| {
                r.sample_format() == cpal::SampleFormat::F32
                    && r.min_sample_rate() <= rate
                    && rate <= r.max_sample_rate()
            })
            .ok_or_else(|| {
                if options.sample_rate.is_some() {
                    HostError::UnsupportedSampleRate {
                        device: name.to_string(),
                        rate,
                    }
                } else {
                    unsupported(name.to_string())
                }
            })?
            .with_sample_rate(rate)
    };
    let mut config = supported.config();
    if let Some(frames) = options.buffer_size {
        config.buffer_size = cpal::BufferSize::Fixed(frames);
    }
    Ok(config)
}

/// A chosen audio output, not yet running.
///
/// Open the device first, so you know the sample rate to create the engine
/// with. Then call [`AudioOutput::start`].
pub struct AudioOutput {
    kind: Kind,
    name: String,
    /// Where the stream sends what it plays, if it is recorded.
    record: Option<Tap>,
}

/// What plays the engine.
enum Kind {
    Device {
        device: cpal::Device,
        config: cpal::StreamConfig,
    },
    /// A timer thread, with no device.
    Clock { sample_rate: u32, block: u32 },
    /// Played only when a test steps the tape, with no device.
    Stepped { sample_rate: u32, block: u32, tape: Tape },
}

impl AudioOutput {
    /// Chooses a device and a 32-bit float configuration.
    pub fn open(options: &AudioOptions) -> Result<Self, HostError> {
        let host = cpal::default_host();
        let device = match &options.device {
            None => host.default_output_device(),
            Some(wanted) => find_device(host.output_devices().map_err(audio_error)?, wanted, options.exact),
        }
        .ok_or_else(|| HostError::NoOutputDevice(options.device.clone()))?;
        let name = device_name(&device);

        let default = device.default_output_config().map_err(audio_error)?;
        let config = choose_config(
            &name,
            default,
            || device.supported_output_configs().map_err(audio_error),
            options,
            HostError::UnsupportedFormat,
        )?;
        Ok(Self {
            kind: Kind::Device { device, config },
            name,
            record: None,
        })
    }

    /// An output with no device: a timer thread plays the engine at
    /// `sample_rate`, `block` frames at a time, and the sound goes nowhere.
    /// For machines without audio, such as CI, and for agents: live values,
    /// lights, and meters move as with a device.
    pub fn clock(sample_rate: u32, block: u32) -> Self {
        Self {
            kind: Kind::Clock {
                sample_rate: sample_rate.max(1),
                block: block.max(1),
            },
            name: "no device (clock)".to_string(),
            record: None,
        }
    }

    /// An output with no device that plays only when a test calls
    /// [`Tape::step`], `block` frames at a time, and keeps every sample on
    /// `tape`. Many streams can share one tape, one after another. Time is
    /// counted in frames, never on the clock, so a test gives the same tape
    /// on any machine (`docs/design/audio-qa.md`, Stepped test output).
    pub fn stepped(sample_rate: u32, block: u32, tape: &Tape) -> Self {
        Self {
            kind: Kind::Stepped {
                sample_rate: sample_rate.max(1),
                block: block.max(1),
                tape: tape.clone(),
            },
            name: "no device (stepped)".to_string(),
            record: None,
        }
    }

    /// Gives the output another name, for an output with no device that
    /// stands in for one in tests.
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Also sends what the engine plays to `recorder`, after what earlier
    /// streams sent it. Refused when the recorder's sample rate is not this
    /// output's: a WAV file has one rate.
    pub fn record(&mut self, recorder: &Recorder) -> Result<(), HostError> {
        let rate = self.sample_rate();
        if recorder.sample_rate() != rate {
            return Err(HostError::Record(format!(
                "the recording is at {} Hz and the output at {rate} Hz",
                recorder.sample_rate()
            )));
        }
        self.record = Some(recorder.tap());
        Ok(())
    }

    /// Device name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Sample rate in Hz. Create the engine with this rate.
    pub fn sample_rate(&self) -> u32 {
        match &self.kind {
            Kind::Device { config, .. } => config.sample_rate,
            Kind::Clock { sample_rate, .. } | Kind::Stepped { sample_rate, .. } => *sample_rate,
        }
    }

    /// Number of output channels.
    pub fn channels(&self) -> u16 {
        match &self.kind {
            Kind::Device { config, .. } => config.channels,
            Kind::Clock { .. } | Kind::Stepped { .. } => 2,
        }
    }

    /// Requested buffer size, if fixed.
    pub fn buffer_size(&self) -> Option<u32> {
        match &self.kind {
            Kind::Device { config, .. } => match config.buffer_size {
                cpal::BufferSize::Fixed(frames) => Some(frames),
                cpal::BufferSize::Default => None,
            },
            Kind::Clock { block, .. } | Kind::Stepped { block, .. } => Some(*block),
        }
    }

    /// Starts audio. The engine moves to the audio thread. On failure,
    /// [`StartError`] returns both the [`AudioOutput`] and the [`Engine`] so
    /// the session can keep the engine and retry.
    #[allow(clippy::result_large_err)]
    pub fn start(mut self, engine: Engine) -> Result<RunningAudio, StartError> {
        let stats = Arc::new(CallbackStats::default());
        let sample_rate = self.sample_rate();
        #[cfg(target_os = "macos")]
        let (engine, mut restart_fade) = {
            let mut engine = engine;
            let sender = engine.restart_fade_sender();
            (engine, sender)
        };
        let mut render = Render::new(
            engine,
            usize::from(self.channels().max(1)),
            sample_rate,
            stats.clone(),
            self.record.take(),
        );
        // A late block on the clock cannot be heard: it is not a dropout.
        render.count_overruns = matches!(self.kind, Kind::Device { .. });

        let player = match self.kind {
            Kind::Device { ref device, config } => {
                let render_box = Box::new(render);
                let render_ptr = Box::into_raw(render_box);
                let ptr = RenderPtr(render_ptr);
                let data_callback = move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    // SAFETY: render_ptr is kept alive until StreamHandle drops.
                    unsafe {
                        ptr.render(data);
                    }
                };
                let error_stats = stats.clone();
                let error_callback = move |error: cpal::Error| {
                    // Printing here could block the audio thread on some backends.
                    // Count it; the control thread reports it.
                    error_stats.stream_error(error.kind());
                };
                #[cfg(target_os = "macos")]
                let switched = settle_rate(device, &config);
                #[cfg(target_os = "macos")]
                stats.start_rate_switched.store(switched, Ordering::Relaxed);
                let stream = match device.build_output_stream::<f32, _, _>(config, data_callback, error_callback, None)
                {
                    Ok(s) => s,
                    Err(e) => {
                        // SAFETY: stream was not created, callback will never run.
                        let render = unsafe { *Box::from_raw(render_ptr) };
                        let (engine, tap) = render.into_parts();
                        self.record = tap;
                        return Err(StartError {
                            output: self,
                            engine,
                            error: audio_error(e),
                        });
                    }
                };
                if let Err(e) = stream.play() {
                    drop(stream);
                    // SAFETY: stream play failed, dropped stream; reclaiming Render box.
                    let render = unsafe { *Box::from_raw(render_ptr) };
                    let (engine, tap) = render.into_parts();
                    self.record = tap;
                    return Err(StartError {
                        output: self,
                        engine,
                        error: audio_error(e),
                    });
                }
                #[cfg(target_os = "macos")]
                if switched && let Err(e) = resume_after_switch(&stream, &stats, &mut restart_fade) {
                    drop(stream);
                    // SAFETY: stream dropped; reclaiming Render box.
                    let render = unsafe { *Box::from_raw(render_ptr) };
                    let (engine, tap) = render.into_parts();
                    self.record = tap;
                    return Err(StartError {
                        output: self,
                        engine,
                        error: e,
                    });
                }
                Player::Device(StreamHandle {
                    stream: Some(stream),
                    render: render_ptr,
                })
            }
            Kind::Clock { sample_rate, block } => match Clock::start(render, sample_rate, block) {
                Ok(clock) => Player::Clock(clock),
                Err((render, e)) => {
                    let (engine, tap) = render.into_parts();
                    self.record = tap;
                    return Err(StartError {
                        output: self,
                        engine,
                        error: e,
                    });
                }
            },
            Kind::Stepped { block, tape, .. } => Player::Stepped(tape.attach(render, block as usize)),
        };
        Ok(RunningAudio {
            player: Some(player),
            stats,
            name: self.name,
            sample_rate,
        })
    }
}

/// How long [`settle_rate`] waits for the device to report its new rate.
#[cfg(target_os = "macos")]
const RATE_SETTLE_LIMIT: Duration = Duration::from_secs(1);

/// How long [`resume_after_switch`] watches a new stream after a rate
/// switch for the late notice of that switch.
#[cfg(target_os = "macos")]
const RATE_NOTICE_WINDOW: Duration = Duration::from_millis(500);

/// Switches the device to the stream's sample rate before the stream is
/// built, and waits until the device reports it. Does nothing when the
/// device is at that rate already.
///
/// On macOS, building a stream at a new rate switches the device's rate,
/// and the stream also listens for rate changes, which it treats as the
/// device changing under it: it pauses. The notice of its own switch can
/// arrive after it has started listening, so the new stream paused at once
/// and the app went silent after a sample rate change. A short silent
/// stream makes the switch first, so the real stream starts on a device
/// that is already at its rate. Any error here is left for the real stream
/// to report. Returns true if the rate was switched.
///
/// Some devices report the new rate before the notice is sent, so the
/// notice can still pause the real stream: see [`resume_after_switch`].
/// `docs/learn/stopping-a-stream.md` tells the story.
#[cfg(target_os = "macos")]
fn settle_rate(device: &cpal::Device, config: &cpal::StreamConfig) -> bool {
    let current = || device.default_output_config().map(|c| c.sample_rate()).ok();
    if current() == Some(config.sample_rate) {
        return false;
    }
    let silence = |data: &mut [f32], _: &cpal::OutputCallbackInfo| data.fill(0.0);
    if let Ok(stream) = device.build_output_stream::<f32, _, _>(*config, silence, |_| {}, None) {
        drop(stream);
    }
    let started = Instant::now();
    while current() != Some(config.sample_rate) && started.elapsed() < RATE_SETTLE_LIMIT {
        std::thread::sleep(Duration::from_millis(5));
    }
    true
}

/// Queues the fade before the device can call back on a restarted stream.
#[cfg(any(target_os = "macos", test))]
fn restart_stream(
    restart_fade: &mut loom_core::RestartFadeSender,
    play: impl FnOnce() -> Result<(), HostError>,
) -> Result<(), HostError> {
    restart_fade.request();
    play()
}

/// Starts the stream again each time it reports an error in the moments
/// after a rate switch. A late notice of the switch pauses the stream (see
/// [`settle_rate`]); starting it again is right, because this stream made
/// the switch. A rate change from outside later on still pauses it.
/// Fails if the stream cannot be started again, so the caller does not
/// get a stream that stays silent.
#[cfg(target_os = "macos")]
fn resume_after_switch(
    stream: &cpal::Stream,
    stats: &CallbackStats,
    restart_fade: &mut loom_core::RestartFadeSender,
) -> Result<(), HostError> {
    let started = Instant::now();
    // From zero, not from the count now: the stats are this stream's own,
    // so an error already counted came after its first `play` and paused it.
    let mut errors = 0;
    while started.elapsed() < RATE_NOTICE_WINDOW {
        let now = stats.stream_errors.load(Ordering::Relaxed);
        if now != errors {
            errors = now;
            restart_stream(restart_fade, || stream.play().map_err(audio_error))?;
            stats.start_restarts.fetch_add(1, Ordering::Relaxed);
            stats.clear_dead();
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    stats.clear_dead();
    Ok(())
}

/// Renders the engine into interleaved output buffers, and records it if
/// asked. Runs on the audio thread: it must not allocate (NFR-RT-01).
pub(crate) struct Render {
    engine: Engine,
    left: Vec<f32>,
    right: Vec<f32>,
    channels: usize,
    sample_rate: f64,
    stats: Arc<CallbackStats>,
    tap: Option<Tap>,
    /// Count blocks that took longer than their duration as overruns.
    count_overruns: bool,
}

/// Frames the scratch buffers hold. Larger device buffers are rendered in
/// parts.
const SCRATCH: usize = 8192;

impl Render {
    fn new(engine: Engine, channels: usize, sample_rate: u32, stats: Arc<CallbackStats>, tap: Option<Tap>) -> Self {
        // Pre-allocate scratch buffers before audio starts.
        Self {
            engine,
            left: vec![0.0; SCRATCH],
            right: vec![0.0; SCRATCH],
            channels,
            sample_rate: f64::from(sample_rate),
            stats,
            tap,
            count_overruns: true,
        }
    }

    pub(crate) fn render(&mut self, data: &mut [f32]) {
        let _ftz = ScopedFlushDenormals::new();
        let started = Instant::now();
        let channels = self.channels;
        let frames = data.len() / channels;
        // A buffer that starts silent stays silent: the fade-out has ended.
        if self.engine.is_silent() {
            self.stats.silent_buffers.fetch_add(1, Ordering::Relaxed);
        }

        let mut done = 0;
        while done < frames {
            let n = (frames - done).min(SCRATCH);
            let (left, right) = (&mut self.left[..n], &mut self.right[..n]);
            self.engine.process(left, right);
            if let Some(tap) = &mut self.tap {
                tap.push(left, right);
            }
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

        let budget = frames as f64 / self.sample_rate;
        let load = started.elapsed().as_secs_f64() / budget.max(1e-9);
        self.stats.record(frames, load, self.count_overruns);
    }

    /// Takes the engine and recording tap back, for returning them when an
    /// audio output fails to start.
    pub(crate) fn into_parts(self) -> (Engine, Option<Tap>) {
        (self.engine, self.tap)
    }
}

/// Wrapper for passing raw pointer to the audio callback closure or clock thread.
struct RenderPtr(*mut Render);

// SAFETY: Rendering access is exclusive to the audio callback or clock thread, respectively,
// and stream teardown or thread joining completes before Render is reclaimed.
unsafe impl Send for RenderPtr {}

impl RenderPtr {
    /// Calls render on the underlying Render pointer.
    unsafe fn render(&self, data: &mut [f32]) {
        // SAFETY: caller guarantees the pointee is valid and exclusively accessed.
        unsafe {
            (*self.0).render(data);
        }
    }
}

/// Keeps a cpal stream and its render state alive until dropped.
struct StreamHandle {
    stream: Option<cpal::Stream>,
    render: *mut Render,
}

impl Drop for StreamHandle {
    fn drop(&mut self) {
        drop(self.stream.take());
        // SAFETY: the stream is dropped, stopping the audio callback, so no other thread accesses render.
        unsafe {
            drop(Box::from_raw(self.render));
        }
    }
}

/// What keeps audio playing. Never read: dropping it stops audio.
#[allow(dead_code)]
enum Player {
    Device(StreamHandle),
    Clock(Clock),
    Stepped(stepped::Attached),
}

/// A timer thread that plays the engine in real time, with no device.
struct Clock {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    render: *mut Render,
}

impl Clock {
    #[allow(clippy::result_large_err)]
    fn start(render: Render, sample_rate: u32, block: u32) -> Result<Self, (Render, HostError)> {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let mut buffer = vec![0.0f32; block as usize * 2];
        let period = Duration::from_secs_f64(f64::from(block) / f64::from(sample_rate));
        let render_box = Box::new(render);
        let render_ptr = Box::into_raw(render_box);
        let ptr = RenderPtr(render_ptr);
        let thread = match std::thread::Builder::new()
            .name("loom-audio-clock".to_string())
            .spawn(move || {
                let mut next = Instant::now();
                while !thread_stop.load(Ordering::Relaxed) {
                    // SAFETY: render_ptr stays valid until Clock drops.
                    unsafe {
                        ptr.render(&mut buffer);
                    }
                    next += period;
                    let now = Instant::now();
                    if next > now {
                        std::thread::sleep(next - now);
                    } else if now - next > Duration::from_secs(1) {
                        // The thread was held up (a debugger, a sleeping
                        // laptop): carry on from now instead of catching up.
                        next = now;
                    }
                }
            }) {
            Ok(thread) => thread,
            Err(e) => {
                // SAFETY: thread spawn failed, so the raw pointer was never used on another thread.
                let render = unsafe { *Box::from_raw(render_ptr) };
                return Err((render, audio_error(e)));
            }
        };
        Ok(Self {
            stop,
            thread: Some(thread),
            render: render_ptr,
        })
    }
}

impl Drop for Clock {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        // SAFETY: the clock thread has joined, so render is no longer accessed by any thread.
        unsafe {
            drop(Box::from_raw(self.render));
        }
    }
}

/// Buffers of silence the device must be handed after the engine reports
/// silence, before the stream stops (output safety, rule 14). The engine
/// renders ahead of what the device plays, so stopping at the report could
/// drop the end of the fade.
pub const SILENT_BUFFERS_BEFORE_STOP: u64 = 2;

/// The shortest limit [`RunningAudio::stop`] waits for the fade-out. With
/// large buffers it waits longer: see [`stop_limit`].
pub const STOP_LIMIT: Duration = Duration::from_millis(100);

/// Buffers a stop needs, on top of the fade itself: one while the fade-out
/// request waits for the next callback, one that renders the fade, and the
/// silent ones after it.
const BUFFERS_PER_STOP: u64 = 2 + SILENT_BUFFERS_BEFORE_STOP;

/// How long a stop may wait for the fade-out with buffers of `frames` at
/// `sample_rate`: `minimum`, or the fade plus four buffers
/// if that is longer (output safety, rule 14). A fixed limit cut streams
/// with large buffers before the fade reached the device, which clicked.
/// `docs/learn/stopping-a-stream.md` explains the count.
pub fn stop_limit(minimum: Duration, frames: u64, sample_rate: u32) -> Duration {
    let buffers = (BUFFERS_PER_STOP * frames) as f64 / f64::from(sample_rate.max(1));
    let needed = Duration::from_secs_f64(f64::from(loom_core::safety::FADE_SECONDS) + buffers);
    minimum.max(needed)
}

/// Audio that is playing. Audio stops when this is dropped. To stop
/// without a click, ask the engine to fade out
/// ([`loom_core::Controller::fade_out`]) and call [`RunningAudio::stop`].
pub struct RunningAudio {
    player: Option<Player>,
    stats: Arc<CallbackStats>,
    name: String,
    sample_rate: u32,
}

impl RunningAudio {
    /// Stops audio once the engine has faded out: after it reported silence
    /// and the device was handed [`SILENT_BUFFERS_BEFORE_STOP`] more
    /// buffers, or at the limit, whichever comes first. The limit is
    /// `limit`, or longer for large buffers ([`stop_limit`]). Returns true
    /// if it stopped on the silence. Ask the engine to fade out first.
    ///
    /// A stepped output plays the blocks itself, and counts the limit in
    /// frames at its sample rate instead of waiting on the clock.
    pub fn stop(self, limit: Duration) -> bool {
        let sample_rate = self.sample_rate;
        self.stop_within(|frames| stop_limit(limit, frames, sample_rate))
    }

    /// [`RunningAudio::stop`], with the limit for buffers of a number of
    /// frames given by `limit`.
    fn stop_within(mut self, limit: impl Fn(u64) -> Duration) -> bool {
        if let Some(Player::Stepped(attached)) = &self.player {
            let limit = limit(attached.block() as u64);
            let limit = (limit.as_secs_f64() * f64::from(self.sample_rate)).ceil() as usize;
            let stats = self.stats.clone();
            let silent = attached.play_until(limit, || {
                stats.silent_buffers.load(Ordering::Relaxed) >= SILENT_BUFFERS_BEFORE_STOP
            });
            self.player = None;
            return silent;
        }
        let started = Instant::now();
        let silent = loop {
            if self.stats.silent_buffers.load(Ordering::Relaxed) >= SILENT_BUFFERS_BEFORE_STOP {
                break true;
            }
            // The largest buffer so far: the device may not have asked for
            // one yet, or may ask for larger ones than at first.
            let frames = self.stats.largest_buffer.load(Ordering::Relaxed);
            if started.elapsed() >= limit(frames) {
                break false;
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        self.player = None;
        silent
    }

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

    /// True when the stream encountered an error that stopped it (e.g. device
    /// was disconnected).
    pub fn is_dead(&self) -> bool {
        self.stats.is_dead()
    }

    /// Marks the stream as dead (e.g. when tests simulate device loss).
    pub fn mark_dead(&self) {
        self.stats.mark_dead();
    }

    /// Stops audio immediately without waiting for a fade-out. Used when the
    /// device was lost or disconnected and no fade-out is possible (output
    /// safety, rule 14).
    pub fn stop_immediate(mut self) {
        self.player = None;
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
    /// Overload notices before the first callback.
    start_overloads: AtomicU64,
    /// Whether startup had to switch the device sample rate.
    start_rate_switched: AtomicBool,
    /// Restarts after a late rate-switch notice.
    start_restarts: AtomicU64,
    /// Highest load seen, in parts per thousand.
    max_load_permille: AtomicU64,
    /// Highest load since the watcher last took a reading.
    peak_since_read_permille: AtomicU64,
    /// Load of the latest callback, in parts per thousand.
    last_load_permille: AtomicU64,
    smallest_buffer: AtomicU64,
    largest_buffer: AtomicU64,
    /// Buffers rendered after the engine reported silence.
    silent_buffers: AtomicU64,
    /// True when the stream encountered a fatal error (e.g. device unplugged).
    stream_dead: AtomicBool,
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
    /// Errors reported by the audio backend (for example xruns), except
    /// the overload notices in `start_overloads`.
    pub stream_errors: u64,
    /// Overload notices (xruns) that arrived before the stream's first
    /// callback. No audio was rendered yet, so none was dropped: they are
    /// not dropouts. macOS sends one as a stream starts with a very small
    /// buffer, while the device switches to it.
    pub start_overloads: u64,
    /// Whether startup had to switch the device sample rate.
    pub start_rate_switched: bool,
    /// Restarts after a late rate-switch notice during stream startup.
    pub start_restarts: u64,
    /// Highest load seen (1.0 = 100% of the time budget).
    pub max_load: f64,
    /// Load of the latest callback.
    pub last_load: f64,
    /// Smallest buffer the device asked for, in frames.
    pub smallest_buffer: u64,
    /// Largest buffer the device asked for, in frames.
    pub largest_buffer: u64,
    /// Buffers handed to the device after the engine faded out to silence.
    pub silent_buffers: u64,
    /// True if the stream encountered an error that stopped it.
    pub is_dead: bool,
}

impl CallbackStats {
    /// Counts an error the backend reported. An overload notice before the
    /// first callback goes to `start_overloads`, not `stream_errors`: the
    /// stream had rendered nothing, so nothing was lost (panels.md, Audio
    /// load and dropouts). Every other error is a stream error, and non-xrun
    /// errors mark the stream as dead.
    fn stream_error(&self, kind: cpal::ErrorKind) {
        if kind == cpal::ErrorKind::Xrun && self.callbacks.load(Ordering::Relaxed) == 0 {
            self.start_overloads.fetch_add(1, Ordering::Relaxed);
        } else {
            self.stream_errors.fetch_add(1, Ordering::Relaxed);
            if kind != cpal::ErrorKind::Xrun && kind != cpal::ErrorKind::DeviceChanged {
                self.stream_dead.store(true, Ordering::Relaxed);
            }
        }
    }

    /// True if the stream encountered an error that stopped it.
    pub fn is_dead(&self) -> bool {
        self.stream_dead.load(Ordering::Relaxed)
    }

    /// Marks the stream as dead.
    pub fn mark_dead(&self) {
        self.stream_dead.store(true, Ordering::Relaxed);
    }

    /// Clears the dead flag (used when a stream successfully recovers,
    /// such as after a sample rate switch).
    pub fn clear_dead(&self) {
        self.stream_dead.store(false, Ordering::Relaxed);
    }

    fn record(&self, frames: usize, load: f64, count_overrun: bool) {
        let permille = (load * 1000.0) as u64;
        self.callbacks.fetch_add(1, Ordering::Relaxed);
        self.frames.fetch_add(frames as u64, Ordering::Relaxed);
        self.last_load_permille.store(permille, Ordering::Relaxed);
        self.max_load_permille.fetch_max(permille, Ordering::Relaxed);
        self.peak_since_read_permille.fetch_max(permille, Ordering::Relaxed);
        if count_overrun && load > 1.0 {
            self.overruns.fetch_add(1, Ordering::Relaxed);
        }
        let frames = frames as u64;
        self.largest_buffer.fetch_max(frames, Ordering::Relaxed);
        let smallest = self.smallest_buffer.load(Ordering::Relaxed);
        if smallest == 0 || frames < smallest {
            self.smallest_buffer.store(frames, Ordering::Relaxed);
        }
    }

    /// Takes the peak across every callback since the previous read.
    pub fn take_peak_since_read(&self) -> f64 {
        self.peak_since_read_permille.swap(0, Ordering::Relaxed) as f64 / 1000.0
    }

    /// Reads all counters.
    pub fn snapshot(&self) -> StatsSnapshot {
        StatsSnapshot {
            callbacks: self.callbacks.load(Ordering::Relaxed),
            frames: self.frames.load(Ordering::Relaxed),
            overruns: self.overruns.load(Ordering::Relaxed),
            stream_errors: self.stream_errors.load(Ordering::Relaxed),
            start_overloads: self.start_overloads.load(Ordering::Relaxed),
            start_rate_switched: self.start_rate_switched.load(Ordering::Relaxed),
            start_restarts: self.start_restarts.load(Ordering::Relaxed),
            max_load: self.max_load_permille.load(Ordering::Relaxed) as f64 / 1000.0,
            last_load: self.last_load_permille.load(Ordering::Relaxed) as f64 / 1000.0,
            smallest_buffer: self.smallest_buffer.load(Ordering::Relaxed),
            largest_buffer: self.largest_buffer.load(Ordering::Relaxed),
            silent_buffers: self.silent_buffers.load(Ordering::Relaxed),
            is_dead: self.stream_dead.load(Ordering::Relaxed),
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

/// Which notes are held, one bit per note on each of the 16 channels. A
/// MIDI input and a virtual controller each keep one, so each can release
/// exactly the notes it holds.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct HeldNotes([u128; 16]);

impl HeldNotes {
    /// Follows one MIDI message: a note on holds its note, and a note off
    /// (or a note on with velocity 0) lets it go. Other messages change
    /// nothing.
    pub fn track(&mut self, bytes: &[u8]) {
        if let [status, note, velocity] = *bytes
            && note < 128
        {
            let bit = 1u128 << note;
            let held = &mut self.0[usize::from(status & 0x0F)];
            match status & 0xF0 {
                0x90 if velocity > 0 => *held |= bit,
                0x80 | 0x90 => *held &= !bit,
                _ => {}
            }
        }
    }

    /// The held notes as `(channel, note)`, in order. The iterator owns a
    /// copy, so the caller can send, and change what is held, while it walks.
    pub fn notes(&self) -> impl Iterator<Item = (u8, u8)> + use<> {
        let held = self.0;
        (0..16u8).flat_map(move |channel| {
            (0..128u8)
                .filter(move |&note| held[usize::from(channel)] & (1u128 << note) != 0)
                .map(move |note| (channel, note))
        })
    }

    /// The held notes, as [`notes`](Self::notes) gives them. None is held
    /// after.
    pub fn take(&mut self) -> impl Iterator<Item = (u8, u8)> + use<> {
        let notes = self.notes();
        *self = Self::default();
        notes
    }

    /// True if any note is held.
    pub fn any(&self) -> bool {
        self.0.iter().any(|&h| h != 0)
    }
}

/// One MIDI input's path into the engine. It remembers which notes are held
/// on that input, so closing the input can release them.
pub struct MidiInputState {
    midi: SharedMidi,
    held: HeldNotes,
    /// Stamped on every event, so the engine can tell inputs apart.
    input: loom_core::MidiInputId,
}

impl MidiInputState {
    /// A path into `midi` with no notes held, for a source the engine does
    /// not track.
    pub fn new(midi: SharedMidi) -> Self {
        Self::for_input(midi, loom_core::MidiInputId(0))
    }

    /// A path into `midi` whose events are stamped as `input`. Open the
    /// input on the engine first ([`loom_core::Controller::open_midi_input`]).
    pub fn for_input(midi: SharedMidi, input: loom_core::MidiInputId) -> Self {
        Self {
            midi,
            held: HeldNotes::default(),
            input,
        }
    }

    /// Sends one message from the input to the engine, applied at the start
    /// of the next audio buffer. SysEx and longer messages are dropped.
    pub fn receive(&mut self, bytes: &[u8]) {
        let Some(&status) = bytes.first() else { return };
        if status == 0xF0 || bytes.len() > 3 {
            return;
        }
        self.held.track(bytes);
        // Frame 0: applied at the start of the next audio buffer
        // (NFR-PERF-07). Sub-buffer timestamps come later.
        send_midi(&self.midi, MidiEvent::from_bytes(0, bytes).from_input(self.input));
    }

    /// Sends a note off for every note held on this input.
    pub fn release_all(&mut self) {
        for (channel, note) in self.held.take() {
            send_midi(&self.midi, MidiEvent::note_off(0, channel, note).from_input(self.input));
        }
    }

    /// True if any note is held on this input.
    pub fn holds_notes(&self) -> bool {
        self.held.any()
    }
}

/// An open MIDI input. MIDI stops when this is dropped. [`close`] it to
/// also release the notes held on it.
///
/// [`close`]: MidiConnection::close
pub struct MidiConnection {
    connection: midir::MidiInputConnection<MidiInputState>,
    port_name: String,
    port_id: String,
}

impl MidiConnection {
    /// Name of the connected port.
    pub fn port_name(&self) -> &str {
        &self.port_name
    }

    /// The system's identity for the port, as [`MidiInputInfo::id`].
    pub fn port_id(&self) -> &str {
        &self.port_id
    }

    /// Closes the input, then releases the notes that were held on it, so
    /// none hangs.
    pub fn close(self) {
        let (_, mut state) = self.connection.close();
        state.release_all();
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
    open_port_as(input, port, midi, loom_core::MidiInputId(0))
}

fn open_port_as(
    input: midir::MidiInput,
    port: &midir::MidiInputPort,
    midi: SharedMidi,
    id: loom_core::MidiInputId,
) -> Result<MidiConnection, HostError> {
    let port_name = input.port_name(port).unwrap_or_else(|_| "<unnamed port>".into());
    let port_id = port.id();
    let connection = input
        .connect(
            port,
            "loomdsp-in",
            |_timestamp, bytes, state: &mut MidiInputState| state.receive(bytes),
            MidiInputState::for_input(midi, id),
        )
        .map_err(|e| HostError::Midi(format!("{port_name}: {e}")))?;
    Ok(MidiConnection {
        connection,
        port_name,
        port_id,
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

/// Connects the MIDI input port whose name is exactly `name`.
pub fn connect_midi_named(name: &str, midi: SharedMidi) -> Result<MidiConnection, HostError> {
    let input = midi_input()?;
    let port = input
        .ports()
        .into_iter()
        .find(|p| input.port_name(p).is_ok_and(|n| n == name))
        .ok_or_else(|| HostError::NoMidiPort(Some(name.to_string())))?;
    open_port(input, &port, midi)
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

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::{EngineConfig, EngineParts, OutputSafety, create_engine};

    /// Two inputs with the same name are told apart by order.
    #[test]
    fn same_names_are_numbered_by_order() {
        let names = ["Pad", "Keys", "Pad", "Pad"].map(String::from);
        assert_eq!(unique_names(&names), ["Pad", "Keys", "Pad (2)", "Pad (3)"]);
        assert_eq!(find_input(&names, "Pad (2)").unwrap(), 2);
        assert_eq!(find_input(&names, "Keys").unwrap(), 1);
        let missing = find_input(&names, "Drums").unwrap_err();
        assert!(missing.to_string().contains("Drums"), "{missing}");
        let ids = ["a", "b", "c", "d"].map(String::from).to_vec();
        let infos = input_infos(names.to_vec(), ids);
        assert_eq!(
            infos[2],
            MidiInputInfo {
                name: "Pad (2)".into(),
                id: "c".into()
            }
        );
    }

    /// An input's events carry its id, and so do the note offs it sends
    /// when it closes.
    #[test]
    fn an_input_stamps_its_events() {
        let parts = create_engine(loom_modules::registry(), EngineConfig::default());
        let EngineParts {
            mut engine,
            midi,
            mut controller,
            ..
        } = parts;
        let midi = share_midi(midi);
        let id = loom_core::MidiInputId(3);
        controller.open_midi_input(id).unwrap();
        let (mut l, mut r) = (vec![0.0; 64], vec![0.0; 64]);
        engine.process(&mut l, &mut r);
        let mut state = MidiInputState::for_input(midi, id);
        state.receive(&[0x90, 60, 100]);
        assert!(state.holds_notes());
        engine.process(&mut l, &mut r);
        assert_eq!(engine.held_notes(id), Some(1), "the note on carried the id");
        state.release_all();
        assert!(!state.holds_notes());
        engine.process(&mut l, &mut r);
        assert_eq!(engine.held_notes(id), Some(0), "the note off carried the id");
    }

    /// A configuration at `rate` in `format`, as a device reports it.
    fn reported(rate: u32, format: cpal::SampleFormat) -> cpal::SupportedStreamConfig {
        cpal::SupportedStreamConfig::new(2, rate, cpal::SupportedBufferSize::Unknown, format)
    }

    /// A range of rates in `format`, as a device reports it.
    fn range(min: u32, max: u32, format: cpal::SampleFormat) -> cpal::SupportedStreamConfigRange {
        cpal::SupportedStreamConfigRange::new(2, min, max, cpal::SupportedBufferSize::Unknown, format)
    }

    /// Options asking for `rate` and `buffer`, on any device.
    fn asking(rate: Option<u32>, buffer: Option<u32>) -> AudioOptions {
        AudioOptions {
            sample_rate: rate,
            buffer_size: buffer,
            ..AudioOptions::default()
        }
    }

    /// The device's float default at its own rate is used as it is, and
    /// the ranges are not asked for.
    #[test]
    fn the_float_default_is_used_without_asking_for_ranges() {
        let config = choose_config(
            "Dev",
            reported(48_000, cpal::SampleFormat::F32),
            || -> Result<Vec<_>, HostError> { panic!("ranges were asked for") },
            &asking(None, Some(256)),
            HostError::UnsupportedFormat,
        )
        .unwrap();
        assert_eq!(config.sample_rate, 48_000);
        assert_eq!(config.buffer_size, cpal::BufferSize::Fixed(256));
    }

    /// Another rate, or a default that is not float, takes the first float
    /// range that holds the rate.
    #[test]
    fn another_rate_or_format_takes_a_float_range() {
        let ranges = || {
            Ok(vec![
                range(44_100, 192_000, cpal::SampleFormat::I16),
                range(44_100, 96_000, cpal::SampleFormat::F32),
            ])
        };
        let options = asking(Some(96_000), None);
        let config = choose_config(
            "Dev",
            reported(48_000, cpal::SampleFormat::F32),
            ranges,
            &options,
            HostError::UnsupportedFormat,
        )
        .unwrap();
        assert_eq!(config.sample_rate, 96_000);
        assert_eq!(config.buffer_size, cpal::BufferSize::Default);
        let config = choose_config(
            "Dev",
            reported(48_000, cpal::SampleFormat::I16),
            ranges,
            &asking(None, None),
            HostError::UnsupportedFormat,
        )
        .unwrap();
        assert_eq!(config.sample_rate, 48_000);
    }

    /// With no float range at the rate, a rate asked for names the rate,
    /// and no rate asked for names the format, as the caller chose.
    #[test]
    fn no_float_range_is_an_error() {
        let ranges = || Ok(vec![range(44_100, 48_000, cpal::SampleFormat::F32)]);
        let error = choose_config(
            "Dev",
            reported(48_000, cpal::SampleFormat::F32),
            ranges,
            &asking(Some(96_000), None),
            HostError::UnsupportedFormat,
        )
        .unwrap_err();
        assert!(
            matches!(error, HostError::UnsupportedSampleRate { rate: 96_000, .. }),
            "{error}"
        );
        let error = choose_config(
            "Dev",
            reported(48_000, cpal::SampleFormat::I16),
            || Ok(vec![range(44_100, 48_000, cpal::SampleFormat::I16)]),
            &asking(None, None),
            HostError::UnsupportedInputFormat,
        )
        .unwrap_err();
        assert!(
            matches!(error, HostError::UnsupportedInputFormat(ref name) if name == "Dev"),
            "{error}"
        );
        let error = choose_config(
            "Dev",
            reported(48_000, cpal::SampleFormat::I16),
            || -> Result<Vec<_>, HostError> { Err(HostError::Audio("gone".into())) },
            &asking(None, None),
            HostError::UnsupportedFormat,
        )
        .unwrap_err();
        assert!(matches!(error, HostError::Audio(_)), "{error}");
    }

    /// A held offset into Audio Out with no safety stage, so the output
    /// stays at 0.5 until it fades: a thump if the stream stopped mid-way.
    fn held_offset() -> EngineParts {
        held_offset_at(48_000)
    }

    /// The same held offset at a chosen device rate.
    fn held_offset_at(rate: u32) -> EngineParts {
        let config = EngineConfig {
            output_safety: OutputSafety::Off,
            ..EngineConfig::with_sample_rate(rate as f32)
        };
        let mut parts = create_engine(loom_modules::registry(), config);
        let c = &mut parts.controller;
        let constant = c.add_module("loom.math.constant").unwrap();
        c.set_param_named(constant, "value", 0.5).unwrap();
        let out = c.add_module("loom.io.audio-out").unwrap();
        c.connect_named(constant, "out", out, "in").unwrap();
        c.commit().unwrap();
        parts
    }

    /// A device restart after discarding its first buffer must fade in again.
    #[test]
    fn a_restart_after_a_dropped_buffer_starts_silent() {
        for rate in [48_000, 96_000] {
            let mut parts = held_offset_at(rate);
            let mut restart = parts.engine.restart_fade_sender();
            let stats = Arc::new(CallbackStats::default());
            let mut render = Render::new(parts.engine, 2, rate, stats, None);
            let mut first = vec![0.0; 4096 * 2];
            render.render(&mut first);
            assert_eq!(first[0], 0.0);
            assert_eq!(first[first.len() - 2], 0.5);
            // Simulate a device discarding that buffer while it pauses, then
            // accepting the first callback after the host restarts it.
            let mut resumed = vec![0.0; 4096 * 2];
            restart_stream(&mut restart, || {
                render.render(&mut resumed);
                Ok(())
            })
            .unwrap();
            assert_eq!(resumed[0], 0.0, "the first resumed sample must fade in at {rate} Hz");
            let fade_frames = rate as usize / 100;
            for (frame, samples) in resumed.as_chunks::<2>().0.iter().enumerate() {
                let expected = 0.5 * (frame.min(fade_frames) as f32 / fade_frames as f32);
                assert_eq!(*samples, [expected, expected], "frame {frame} at {rate} Hz");
            }
        }
    }

    /// An overload notice before the first callback is not a stream error:
    /// nothing was rendered, so nothing dropped. After a callback it is, and
    /// other errors always are.
    #[test]
    fn an_overload_before_the_first_callback_is_not_a_stream_error() {
        let stats = CallbackStats::default();
        stats.stream_error(cpal::ErrorKind::Xrun);
        stats.stream_error(cpal::ErrorKind::StreamInvalidated);
        let s = stats.snapshot();
        assert_eq!((s.start_overloads, s.stream_errors), (1, 1));

        stats.record(32, 0.1, true);
        stats.stream_error(cpal::ErrorKind::Xrun);
        let s = stats.snapshot();
        assert_eq!((s.start_overloads, s.stream_errors), (1, 2));
    }

    /// A short high-load callback survives until the watcher takes its peak.
    #[test]
    fn callback_peak_covers_every_callback_since_read() {
        let stats = CallbackStats::default();
        for _ in 0..1_000 {
            stats.record(32, 0.2, false);
        }
        stats.record(32, 0.95, false);
        for _ in 0..1_000 {
            stats.record(32, 0.2, false);
        }
        assert_eq!(stats.take_peak_since_read(), 0.95);
        assert_eq!(stats.take_peak_since_read(), 0.0);
    }

    #[test]
    fn a_stream_stops_after_the_fade_and_two_silent_buffers() {
        let mut parts = held_offset();
        let audio = AudioOutput::clock(48_000, 64).start(parts.engine).unwrap();
        let stats = audio.stats().clone();
        std::thread::sleep(Duration::from_millis(30));
        parts.controller.fade_out().unwrap();
        let started = Instant::now();
        assert!(audio.stop(STOP_LIMIT), "stopped on the silence");
        assert!(started.elapsed() < STOP_LIMIT);
        assert!(stats.snapshot().silent_buffers >= SILENT_BUFFERS_BEFORE_STOP);
    }

    #[test]
    fn a_stream_that_never_goes_silent_stops_at_the_limit() {
        let parts = held_offset();
        let audio = AudioOutput::clock(48_000, 64).start(parts.engine).unwrap();
        let started = Instant::now();
        // No fade-out asked for: the engine never reports silence.
        assert!(!audio.stop(STOP_LIMIT));
        let waited = started.elapsed();
        assert!(waited >= STOP_LIMIT && waited < 10 * STOP_LIMIT, "{waited:?}");
    }

    /// A stepped output plays only when stepped, in whole blocks, and marks
    /// where each stream starts.
    #[test]
    fn a_stepped_output_plays_only_when_stepped() {
        let tape = Tape::new();
        let audio = AudioOutput::stepped(48_000, 64, &tape)
            .start(held_offset().engine)
            .unwrap();
        assert!(tape.is_empty());
        tape.step(100);
        assert_eq!(tape.len(), 128, "two whole blocks");
        assert_eq!(tape.left()[0], 0.0, "the fade-in starts at 0");
        drop(audio);
        tape.step(100);
        assert_eq!(tape.len(), 128, "a stopped stream plays nothing");

        let _audio = AudioOutput::stepped(48_000, 32, &tape)
            .start(held_offset().engine)
            .unwrap();
        tape.step(32);
        assert_eq!(tape.stream_starts(), vec![0, 128]);
        assert_eq!(tape.len(), 160);
    }

    /// Stopping a stepped stream plays its fade-out down to silence and the
    /// silent buffers, counted in frames, with no sleeping.
    #[test]
    fn a_stepped_stop_plays_the_fade_out() {
        let tape = Tape::new();
        let mut parts = held_offset();
        let audio = AudioOutput::stepped(48_000, 64, &tape).start(parts.engine).unwrap();
        tape.step(4800);
        assert_eq!(*tape.left().last().unwrap(), 0.5);
        parts.controller.fade_out().unwrap();
        let before = tape.len();
        assert!(audio.stop(STOP_LIMIT), "stopped on the silence");
        let left = tape.left();
        assert!(left.len() > before, "the fade-out was played");
        assert!(left.len() - before <= 4800, "within the limit, in frames");
        assert_eq!(*left.last().unwrap(), 0.0, "it ends at 0");
        assert!(!tape.is_playing());
    }

    /// With no fade-out asked for, a stepped stop ends after the limit,
    /// counted in frames.
    #[test]
    fn a_stepped_stop_that_never_goes_silent_stops_at_the_limit() {
        let tape = Tape::new();
        let audio = AudioOutput::stepped(48_000, 64, &tape)
            .start(held_offset().engine)
            .unwrap();
        assert!(!audio.stop(STOP_LIMIT));
        // 100 ms at 48 kHz, in whole 64-frame blocks.
        assert_eq!(tape.len(), 4800usize.div_ceil(64) * 64);
    }

    /// The stop limit is 100 ms for small buffers, and grows to the fade
    /// plus four buffers for large ones (output safety, rule 14).
    #[test]
    fn the_stop_limit_grows_with_the_buffer() {
        assert_eq!(stop_limit(STOP_LIMIT, 256, 48_000), STOP_LIMIT);
        assert_eq!(stop_limit(STOP_LIMIT, 0, 48_000), STOP_LIMIT, "no buffer yet");
        let large = stop_limit(STOP_LIMIT, 4096, 44_100);
        // 10 ms + 4 * 4096 / 44100 s = 381.5 ms.
        assert!((large.as_secs_f64() - 0.3815).abs() < 0.0005, "{large:?}");
    }

    /// With 4096-frame blocks at 44.1 kHz, one block is 93 ms. A stop still
    /// plays the whole fade and two silent blocks, where a fixed 100 ms
    /// limit cut it after the fade's block, before the device played it.
    #[test]
    fn a_stepped_stop_with_large_blocks_plays_the_whole_fade_out() {
        let tape = Tape::new();
        let config = EngineConfig {
            output_safety: OutputSafety::Off,
            ..EngineConfig::with_sample_rate(44_100.0)
        };
        let mut parts = create_engine(loom_modules::registry(), config);
        let c = &mut parts.controller;
        let constant = c.add_module("loom.math.constant").unwrap();
        c.set_param_named(constant, "value", 0.5).unwrap();
        let out = c.add_module("loom.io.audio-out").unwrap();
        c.connect_named(constant, "out", out, "in").unwrap();
        c.commit().unwrap();
        let audio = AudioOutput::stepped(44_100, 4096, &tape).start(parts.engine).unwrap();
        tape.step(4096);
        assert_eq!(*tape.left().last().unwrap(), 0.5);
        parts.controller.fade_out().unwrap();
        assert!(audio.stop(STOP_LIMIT), "stopped on the silence, not at the limit");
        let left = tape.left();
        assert!(
            left[left.len() - 2 * 4096..].iter().all(|&x| x == 0.0),
            "the last two blocks are silent"
        );
    }

    /// The same steps give the same tape.
    #[test]
    fn stepped_tapes_are_repeatable() {
        let run = || {
            let tape = Tape::new();
            let mut parts = held_offset();
            let audio = AudioOutput::stepped(44_100, 77, &tape).start(parts.engine).unwrap();
            tape.step(3000);
            parts.controller.fade_out().unwrap();
            audio.stop(STOP_LIMIT);
            tape.left()
        };
        assert_eq!(run(), run());
    }

    /// One recorder takes every stream at its rate, and refuses another rate.
    #[test]
    fn a_recorder_spans_streams_and_keeps_one_rate() {
        let dir = record::TestDir::new("host-recorder");
        let path = dir.0.join("out.wav");
        let recorder = Recorder::start(&path, 48_000).unwrap();
        let tape = Tape::new();
        for _ in 0..2 {
            let mut parts = held_offset();
            let mut output = AudioOutput::stepped(48_000, 64, &tape);
            output.record(&recorder).unwrap();
            let audio = output.start(parts.engine).unwrap();
            tape.step(1000);
            parts.controller.fade_out().unwrap();
            assert!(audio.stop(STOP_LIMIT));
        }
        let mut other = AudioOutput::stepped(44_100, 64, &tape);
        assert!(matches!(other.record(&recorder), Err(HostError::Record(_))));
        drop(recorder);

        let mut reader = hound::WavReader::open(&path).unwrap();
        let left: Vec<f32> = reader.samples::<f32>().map(Result::unwrap).step_by(2).collect();
        assert_eq!(left, tape.left(), "the file holds both streams, nothing between");
    }

    #[test]
    fn closing_an_input_releases_its_held_notes() {
        let config = EngineConfig {
            output_safety: OutputSafety::Off,
            ..EngineConfig::with_sample_rate(48_000.0)
        };
        let mut parts = create_engine(loom_modules::registry(), config);
        let c = &mut parts.controller;
        let note = c.add_module("loom.midi.note").unwrap();
        let out = c.add_module("loom.io.audio-out").unwrap();
        c.connect_named(note, "gate", out, "in").unwrap();
        c.commit().unwrap();
        let mut state = MidiInputState::new(share_midi(parts.midi));
        let gate = |engine: &mut Engine| {
            let (mut left, mut right) = (vec![0.0; 1024], vec![0.0; 1024]);
            engine.process(&mut left, &mut right);
            left[1023]
        };
        gate(&mut parts.engine);

        state.receive(&[0x92, 60, 100]);
        state.receive(&[0x92, 62, 100]);
        state.receive(&[0x92, 62, 0]);
        assert!(state.holds_notes());
        assert_eq!(gate(&mut parts.engine), 1.0, "the note plays");

        state.release_all();
        assert!(!state.holds_notes());
        assert_eq!(gate(&mut parts.engine), 0.0, "and is released");
    }

    /// Held notes are kept per channel, a note on with velocity 0 lets go,
    /// and other messages change nothing.
    #[test]
    fn held_notes_follow_note_on_and_off() {
        let mut held = HeldNotes::default();
        held.track(&[0x90, 60, 100]);
        held.track(&[0x93, 60, 100]);
        held.track(&[0x9F, 127, 1]);
        held.track(&[0x93, 62, 100]);
        held.track(&[0x93, 62, 0]);
        held.track(&[0xB0, 61, 100]);
        held.track(&[0x90, 200, 100]);
        held.track(&[0x90, 61]);
        assert!(held.any());
        assert_eq!(held.take().collect::<Vec<_>>(), [(0, 60), (3, 60), (15, 127)]);
        assert!(!held.any(), "none is held after take");
        held.track(&[0x90, 60, 100]);
        held.track(&[0x80, 60, 64]);
        assert_eq!(held, HeldNotes::default());
    }

    #[test]
    fn sysex_and_long_messages_are_not_sent() {
        let parts = create_engine(loom_modules::registry(), EngineConfig::default());
        let mut state = MidiInputState::new(share_midi(parts.midi));
        state.receive(&[0xF0, 1, 2]);
        state.receive(&[0x90, 60, 100, 1]);
        state.receive(&[]);
        assert!(!state.holds_notes());
    }

    /// Marking a running stream dead stops it and reports `is_dead`.
    #[test]
    fn marking_a_running_stream_dead_reports_is_dead() {
        let parts = create_engine(loom_modules::registry(), EngineConfig::default());
        let audio = AudioOutput::clock(48_000, 64).start(parts.engine).unwrap();
        assert!(!audio.is_dead());
        assert!(!audio.stats().is_dead());
        audio.mark_dead();
        assert!(audio.is_dead());
        assert!(audio.stats().is_dead());
        assert!(audio.stats().snapshot().is_dead);
        audio.stop_immediate();
    }

    /// StartError implements formatting, source, and conversion to HostError.
    #[test]
    fn start_error_formatting_and_traits() {
        let output = AudioOutput::clock(48_000, 256);
        let parts = create_engine(loom_modules::registry(), EngineConfig::default());
        let err = StartError {
            output,
            engine: parts.engine,
            error: HostError::Audio("backend failed".into()),
        };
        assert_eq!(format!("{err}"), "audio error: backend failed");
        assert!(format!("{err:?}").contains("StartError"));
        use std::error::Error;
        assert!(err.source().is_some());
        let host_err: HostError = err.into();
        assert_eq!(format!("{host_err}"), "audio error: backend failed");
    }

    /// CallbackStats supports clearing the dead flag and non-fatal error kinds.
    #[test]
    fn callback_stats_dead_flag_clearing() {
        let stats = CallbackStats::default();
        assert!(!stats.is_dead());
        stats.mark_dead();
        assert!(stats.is_dead());
        stats.clear_dead();
        assert!(!stats.is_dead());
        stats.stream_error(cpal::ErrorKind::DeviceChanged);
        assert!(!stats.is_dead());
        stats.stream_error(cpal::ErrorKind::StreamInvalidated);
        assert!(stats.is_dead());
    }

    /// StreamHandle drops its stream and safely frees the Render pointer.
    #[test]
    fn stream_handle_drops_safely() {
        let parts = create_engine(loom_modules::registry(), EngineConfig::default());
        let stats = Arc::new(CallbackStats::default());
        let render = Render::new(parts.engine, 2, 48_000, stats, None);
        let render_ptr = Box::into_raw(Box::new(render));
        let handle = StreamHandle {
            stream: None,
            render: render_ptr,
        };
        drop(handle);
    }

    /// RenderPtr safely delegates render calls.
    #[test]
    fn render_ptr_delegates_render() {
        let parts = create_engine(loom_modules::registry(), EngineConfig::default());
        let stats = Arc::new(CallbackStats::default());
        let render = Render::new(parts.engine, 2, 48_000, stats, None);
        let render_ptr = Box::into_raw(Box::new(render));
        let ptr = RenderPtr(render_ptr);
        let mut buffer = [0.0f32; 64];
        // SAFETY: render_ptr is uniquely owned in this test and freed after render completes.
        unsafe {
            ptr.render(&mut buffer);
            drop(Box::from_raw(render_ptr));
        }
    }

    /// Render::into_parts returns engine and optional tap.
    #[test]
    fn render_into_parts_returns_engine_and_tap() {
        let parts = create_engine(loom_modules::registry(), EngineConfig::default());
        let stats = Arc::new(CallbackStats::default());
        let render = Render::new(parts.engine, 2, 48_000, stats, None);
        let (engine, tap) = render.into_parts();
        assert!(tap.is_none());
        assert!(!engine.is_silent());
    }
}

/// Checks on a real audio device, ignored by default.
#[cfg(test)]
mod device_tests;

/// Checks through virtual MIDI ports (`docs/design/audio-qa.md`, Virtual MIDI controllers).
/// They need the operating system's MIDI service, so they are ignored by
/// default: run them with `cargo test -p loom-host-desktop -- --ignored` on
/// macOS, or on Linux with the ALSA sequencer.
#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod virtual_midi_tests {
    use super::*;
    use loom_core::{EngineConfig, OutputSafety, create_engine};

    /// A note played on a virtual controller reaches the engine through
    /// `connect_midi_named`, and closing the connection releases it.
    #[test]
    #[ignore = "needs virtual MIDI ports (macOS, or Linux with ALSA)"]
    fn closing_a_real_input_releases_its_held_notes() {
        let config = EngineConfig {
            output_safety: OutputSafety::Off,
            ..EngineConfig::with_sample_rate(48_000.0)
        };
        let mut parts = create_engine(loom_modules::registry(), config);
        let c = &mut parts.controller;
        let note = c.add_module("loom.midi.note").unwrap();
        let out = c.add_module("loom.io.audio-out").unwrap();
        c.connect_named(note, "gate", out, "in").unwrap();
        c.commit().unwrap();
        let mut gate = || {
            let (mut left, mut right) = (vec![0.0; 256], vec![0.0; 256]);
            parts.engine.process(&mut left, &mut right);
            left[255]
        };
        let mut wait_for = |value: f32| {
            (0..200).any(|_| {
                std::thread::sleep(Duration::from_millis(5));
                gate() == value
            })
        };

        // Without a MIDI service (a container with no ALSA sequencer), skip.
        let mut keys = match VirtualController::create(&format!("Loom test {}", std::process::id())) {
            Ok(keys) => keys,
            Err(e) if e.is_missing_hardware() => return eprintln!("skipped: {e} (docs/dev/test-devices.md)"),
            Err(e) => panic!("{e}"),
        };
        let name = keys.input_name().unwrap();
        let input = connect_midi_named(&name, share_midi(parts.midi)).unwrap();
        assert_eq!(input.port_name(), name);
        keys.note_on(0, 60, 100).unwrap();
        assert!(wait_for(1.0), "the note plays");

        input.close();
        assert!(wait_for(0.0), "closing the input releases the held note");
        assert!(keys.holds_notes(), "while the key is still held");
    }
}
