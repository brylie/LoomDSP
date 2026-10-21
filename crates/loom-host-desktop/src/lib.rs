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
//! - MIDI ports are chosen at start, or opened and closed by name. Hot-plugging
//!   comes in P1 (FR-MIDI-08).
//!
//! For development and tests, [`AudioOutput::clock`] plays the engine from
//! a timer thread instead of a device, and [`AudioOutput::record`] saves
//! what the engine plays to a WAV file.

#![warn(missing_docs)]

mod record;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use loom_core::{Engine, MidiEvent, MidiSender};
use loom_rt::ScopedFlushDenormals;
use record::{Recording, Tap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

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
    /// The recording file could not be created or written.
    #[error("recording: {0}")]
    Record(String),
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
    kind: Kind,
    name: String,
    record: Option<PathBuf>,
}

/// What plays the engine.
enum Kind {
    Device {
        device: cpal::Device,
        config: cpal::StreamConfig,
    },
    /// A timer thread, with no device.
    Clock { sample_rate: u32, block: u32 },
}

impl AudioOutput {
    /// Chooses a device and a 32-bit float configuration.
    pub fn open(options: &AudioOptions) -> Result<Self, HostError> {
        let host = cpal::default_host();
        let device = match &options.device {
            None => host.default_output_device(),
            Some(wanted) if options.exact => host
                .output_devices()
                .map_err(audio_error)?
                .find(|d| device_name(d) == *wanted),
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

    /// Gives the output another name, for an output with no device that
    /// stands in for one in tests.
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Also saves what the engine plays to a WAV file (32-bit float,
    /// stereo). The file is created when audio starts.
    pub fn record(&mut self, path: impl Into<PathBuf>) {
        self.record = Some(path.into());
    }

    /// Device name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Sample rate in Hz. Create the engine with this rate.
    pub fn sample_rate(&self) -> u32 {
        match &self.kind {
            Kind::Device { config, .. } => config.sample_rate,
            Kind::Clock { sample_rate, .. } => *sample_rate,
        }
    }

    /// Number of output channels.
    pub fn channels(&self) -> u16 {
        match &self.kind {
            Kind::Device { config, .. } => config.channels,
            Kind::Clock { .. } => 2,
        }
    }

    /// Requested buffer size, if fixed.
    pub fn buffer_size(&self) -> Option<u32> {
        match &self.kind {
            Kind::Device { config, .. } => match config.buffer_size {
                cpal::BufferSize::Fixed(frames) => Some(frames),
                cpal::BufferSize::Default => None,
            },
            Kind::Clock { block, .. } => Some(*block),
        }
    }

    /// Starts audio. The engine moves to the audio thread.
    pub fn start(self, engine: Engine) -> Result<RunningAudio, HostError> {
        let stats = Arc::new(CallbackStats::default());
        let sample_rate = self.sample_rate();
        let (recording, tap) = match &self.record {
            Some(path) => {
                let (recording, tap) = Recording::start(path, sample_rate)?;
                (Some(recording), Some(tap))
            }
            None => (None, None),
        };
        let mut render = Render::new(
            engine,
            usize::from(self.channels().max(1)),
            sample_rate,
            stats.clone(),
            tap,
        );
        // A late block on the clock cannot be heard: it is not a dropout.
        render.count_overruns = matches!(self.kind, Kind::Device { .. });

        let player = match self.kind {
            Kind::Device { device, config } => {
                let data_callback = move |data: &mut [f32], _: &cpal::OutputCallbackInfo| render.render(data);
                let error_stats = stats.clone();
                let error_callback = move |_error: cpal::Error| {
                    // Printing here could block the audio thread on some backends.
                    // Count it; the control thread reports it.
                    error_stats.stream_errors.fetch_add(1, Ordering::Relaxed);
                };
                let stream = device
                    .build_output_stream::<f32, _, _>(config, data_callback, error_callback, None)
                    .map_err(audio_error)?;
                stream.play().map_err(audio_error)?;
                Player::Device(stream)
            }
            Kind::Clock { sample_rate, block } => Player::Clock(Clock::start(render, sample_rate, block)?),
        };
        Ok(RunningAudio {
            player: Some(player),
            recording,
            stats,
            name: self.name,
            sample_rate,
        })
    }
}

/// Renders the engine into interleaved output buffers, and records it if
/// asked. Runs on the audio thread: it must not allocate (NFR-RT-01).
struct Render {
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

    fn render(&mut self, data: &mut [f32]) {
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
}

/// What keeps audio playing. Never read: dropping it stops audio.
#[allow(dead_code)]
enum Player {
    Device(cpal::Stream),
    Clock(Clock),
}

/// A timer thread that plays the engine in real time, with no device.
struct Clock {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Clock {
    fn start(mut render: Render, sample_rate: u32, block: u32) -> Result<Self, HostError> {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let mut buffer = vec![0.0f32; block as usize * 2];
        let period = Duration::from_secs_f64(f64::from(block) / f64::from(sample_rate));
        let thread = std::thread::Builder::new()
            .name("loom-audio-clock".to_string())
            .spawn(move || {
                let mut next = Instant::now();
                while !thread_stop.load(Ordering::Relaxed) {
                    render.render(&mut buffer);
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
            })
            .map_err(audio_error)?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for Clock {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Buffers of silence the device must be handed after the engine reports
/// silence, before the stream stops (output safety, rule 14). The engine
/// renders ahead of what the device plays, so stopping at the report could
/// drop the end of the fade.
pub const SILENT_BUFFERS_BEFORE_STOP: u64 = 2;

/// The longest [`RunningAudio::stop`] waits for the fade-out.
pub const STOP_LIMIT: Duration = Duration::from_millis(100);

/// Audio that is playing. Audio stops when this is dropped, and then a
/// recording is closed. To stop without a click, ask the engine to fade out
/// ([`loom_core::Controller::fade_out`]) and call [`RunningAudio::stop`].
pub struct RunningAudio {
    // Fields drop in order: audio stops before the recording closes.
    player: Option<Player>,
    recording: Option<Recording>,
    stats: Arc<CallbackStats>,
    name: String,
    sample_rate: u32,
}

impl RunningAudio {
    /// Stops audio once the engine has faded out: after it reported silence
    /// and the device was handed [`SILENT_BUFFERS_BEFORE_STOP`] more
    /// buffers, or after `limit`, whichever comes first. Returns true if it
    /// stopped on the silence. Ask the engine to fade out first.
    pub fn stop(mut self, limit: Duration) -> bool {
        let started = Instant::now();
        let silent = loop {
            if self.stats.silent_buffers.load(Ordering::Relaxed) >= SILENT_BUFFERS_BEFORE_STOP {
                break true;
            }
            if started.elapsed() >= limit {
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

    /// Frames left out of the recording because the writer fell behind.
    /// Zero when not recording.
    pub fn recording_dropped(&self) -> u64 {
        self.recording.as_ref().map_or(0, Recording::dropped)
    }

    /// Why the recording stopped early (for example a full disk), if it
    /// did. `None` when it is fine or there is none.
    pub fn recording_error(&self) -> Option<String> {
        self.recording.as_ref().and_then(Recording::error)
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
    /// Buffers rendered after the engine reported silence.
    silent_buffers: AtomicU64,
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
    /// Buffers handed to the device after the engine faded out to silence.
    pub silent_buffers: u64,
}

impl CallbackStats {
    fn record(&self, frames: usize, load: f64, count_overrun: bool) {
        let permille = (load * 1000.0) as u64;
        self.callbacks.fetch_add(1, Ordering::Relaxed);
        self.frames.fetch_add(frames as u64, Ordering::Relaxed);
        self.last_load_permille.store(permille, Ordering::Relaxed);
        self.max_load_permille.fetch_max(permille, Ordering::Relaxed);
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
            silent_buffers: self.silent_buffers.load(Ordering::Relaxed),
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

/// One MIDI input's path into the engine. It remembers which notes are held
/// on that input, so closing the input can release them.
pub struct MidiInputState {
    midi: SharedMidi,
    /// One bit per note, per channel.
    held: [u128; 16],
}

impl MidiInputState {
    /// A path into `midi` with no notes held.
    pub fn new(midi: SharedMidi) -> Self {
        Self { midi, held: [0; 16] }
    }

    /// Sends one message from the input to the engine, applied at the start
    /// of the next audio buffer. SysEx and longer messages are dropped.
    pub fn receive(&mut self, bytes: &[u8]) {
        let Some(&status) = bytes.first() else { return };
        if status == 0xF0 || bytes.len() > 3 {
            return;
        }
        if let [status, note, velocity] = *bytes
            && note < 128
        {
            let channel = usize::from(status & 0x0F);
            let bit = 1u128 << note;
            match status & 0xF0 {
                0x90 if velocity > 0 => self.held[channel] |= bit,
                0x80 | 0x90 => self.held[channel] &= !bit,
                _ => {}
            }
        }
        // Frame 0: applied at the start of the next audio buffer
        // (NFR-PERF-07). Sub-buffer timestamps come later.
        send_midi(&self.midi, MidiEvent::from_bytes(0, bytes));
    }

    /// Sends a note off for every note held on this input.
    pub fn release_all(&mut self) {
        for (channel, held) in self.held.iter_mut().enumerate() {
            for note in 0..128u8 {
                if *held & (1u128 << note) != 0 {
                    send_midi(&self.midi, MidiEvent::note_off(0, channel as u8, note));
                }
            }
            *held = 0;
        }
    }

    /// True if any note is held on this input.
    pub fn holds_notes(&self) -> bool {
        self.held.iter().any(|&h| h != 0)
    }
}

/// An open MIDI input. MIDI stops when this is dropped. [`close`] it to
/// also release the notes held on it.
///
/// [`close`]: MidiConnection::close
pub struct MidiConnection {
    connection: midir::MidiInputConnection<MidiInputState>,
    port_name: String,
}

impl MidiConnection {
    /// Name of the connected port.
    pub fn port_name(&self) -> &str {
        &self.port_name
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
    let port_name = input.port_name(port).unwrap_or_else(|_| "<unnamed port>".into());
    let connection = input
        .connect(
            port,
            "loomdsp-in",
            |_timestamp, bytes, state: &mut MidiInputState| state.receive(bytes),
            MidiInputState::new(midi),
        )
        .map_err(|e| HostError::Midi(format!("{port_name}: {e}")))?;
    Ok(MidiConnection { connection, port_name })
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

    /// A held offset into Audio Out with no safety stage, so the output
    /// stays at 0.5 until it fades: a thump if the stream stopped mid-way.
    fn held_offset() -> EngineParts {
        let config = EngineConfig {
            output_safety: OutputSafety::Off,
            ..EngineConfig::with_sample_rate(48_000.0)
        };
        let mut parts = create_engine(loom_modules::registry(), config);
        let c = &mut parts.controller;
        let constant = c.add_module("loom.math.constant").unwrap();
        c.set_param_named(constant, "value", 0.5).unwrap();
        let out = c.add_module("loom.io.audio-out").unwrap();
        c.connect_named(constant, "out", out, "left").unwrap();
        c.commit().unwrap();
        parts
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
        c.connect_named(note, "gate", out, "left").unwrap();
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

    #[test]
    fn sysex_and_long_messages_are_not_sent() {
        let parts = create_engine(loom_modules::registry(), EngineConfig::default());
        let mut state = MidiInputState::new(share_midi(parts.midi));
        state.receive(&[0xF0, 1, 2]);
        state.receive(&[0x90, 60, 100, 1]);
        state.receive(&[]);
        assert!(!state.holds_notes());
    }
}
