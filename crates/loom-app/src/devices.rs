//! Audio and MIDI devices in the app (`docs/design/preferences.md`, Audio
//! and MIDI devices).
//!
//! The app reaches devices only through [`Host`], so tests can give it a
//! fake list of devices, rates, and buffer ranges, and run without hardware.
//! This module also decides which device, rate, and buffer size to open from
//! the saved choice and the command-line flags, and what the window offers.

use crate::preferences::{AudioPrefs, BufferChoice, DeviceChoice, RateChoice};
use loom_core::MidiInputId;
use loom_host_desktop::{
    AudioOptions, AudioOutput, MidiConnection, MidiInputInfo, OutputInfo, Outputs, RunningAudio, SharedMidi,
};
use std::path::PathBuf;

/// The flags that set devices for one run. They are never saved.
/// No implicit group: `screenshot::Options` already uses the name.
#[derive(clap::Args, Debug, Clone, Default)]
#[group(skip)]
pub struct Options {
    /// Read and write preferences in this file instead of the per-user one
    /// (docs/design/preferences.md). For tests, and one file per rig.
    #[arg(long, value_name = "FILE")]
    pub preferences: Option<PathBuf>,
    /// For this run, play on the first output device whose name contains
    /// this, case-insensitive. Not saved.
    #[arg(long, value_name = "NAME", conflicts_with = "no_audio")]
    pub device: Option<String>,
    /// For this run, use the input whose name contains this text.
    #[arg(long, value_name = "NAME", conflicts_with = "no_input")]
    pub input: Option<String>,
    /// For this run, do not open an audio input.
    #[arg(long)]
    pub no_input: bool,
    /// For this run, use this sample rate in Hz. Not saved.
    #[arg(long, value_name = "HZ", conflicts_with = "no_audio")]
    pub sample_rate: Option<u32>,
    /// For this run, use this buffer size in frames. Not saved.
    #[arg(long, value_name = "FRAMES", conflicts_with = "no_audio")]
    pub buffer: Option<u32>,
    /// For this run, open only the MIDI inputs whose names contain this,
    /// case-insensitive, whatever the preferences say. Not saved.
    #[arg(long, value_name = "NAME", conflicts_with = "no_midi")]
    pub midi: Option<String>,
    /// For this run, open no MIDI input. Not saved.
    #[arg(long)]
    pub no_midi: bool,
}

/// An open MIDI input.
pub trait MidiPort {
    /// The input's full name, unique in the list (`Pad (2)` for a second
    /// `Pad`).
    fn name(&self) -> &str;
    /// The system's identity for the port when it was opened. A port
    /// listed again under another identity is a new connection.
    fn port_id(&self) -> &str;
    /// The id its events carry, which the engine tracks held notes by.
    fn input(&self) -> MidiInputId;
    /// Closes the input and releases the notes held on it.
    fn close(self: Box<Self>);
}

/// A running audio input owned by the rig.
pub trait InputPort {
    /// The device name.
    fn name(&self) -> &str;
    /// Whether the stream is still delivering input.
    fn is_live(&self) -> bool;
    /// Stream health for a watcher thread.
    fn status(&self) -> loom_core::audio_input::InputStatus;
}

impl InputPort for loom_host_desktop::AudioInput {
    fn name(&self) -> &str {
        self.name()
    }
    fn is_live(&self) -> bool {
        self.is_live()
    }
    fn status(&self) -> loom_core::audio_input::InputStatus {
        self.status()
    }
}

/// Input device names and the system default.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Inputs {
    /// The default device's name, if available.
    pub default: Option<String>,
    /// Full input device names.
    pub devices: Vec<String>,
}

/// A real MIDI input and the id it was opened with.
pub struct SystemPort {
    connection: MidiConnection,
    input: MidiInputId,
}

impl MidiPort for SystemPort {
    fn name(&self) -> &str {
        self.connection.port_name()
    }

    fn port_id(&self) -> &str {
        self.connection.port_id()
    }

    fn input(&self) -> MidiInputId {
        self.input
    }

    fn close(self: Box<Self>) {
        self.connection.close();
    }
}

/// Where the app finds audio outputs and MIDI inputs.
pub trait Host {
    /// The output devices and what each offers.
    fn outputs(&self) -> Result<Outputs, String>;
    /// Chooses an output, without starting it.
    fn open(&self, options: &AudioOptions) -> Result<AudioOutput, String>;
    /// Lists audio input devices.
    fn audio_inputs(&self) -> Result<Inputs, String> {
        Ok(Inputs::default())
    }
    /// An input device lister that can run on a watcher thread.
    fn audio_input_lister(&self) -> Option<fn() -> Result<Inputs, String>> {
        None
    }
    /// Reads microphone permission without opening a stream.
    fn audio_input_permission_denied(&self) -> bool {
        false
    }
    /// A permission check that can run on the input watcher thread.
    fn audio_input_permission_checker(&self) -> Option<fn() -> bool> {
        None
    }
    /// Opens an input stream and returns the engine side of its ring.
    fn open_audio_input(
        &self,
        _options: &AudioOptions,
        _output_buffer: usize,
    ) -> Result<(Box<dyn InputPort>, loom_core::audio_input::InputReader), String> {
        Err("no audio input device available".into())
    }
    /// Starts an output with an engine. A device can still refuse here,
    /// after it opened. On refusal, returns the [`loom_core::Engine`] so it
    /// can be kept idle and retried, along with the error reason. Boxed,
    /// because the engine is large.
    fn start(
        &self,
        output: AudioOutput,
        engine: loom_core::Engine,
    ) -> Result<RunningAudio, Box<(loom_core::Engine, String)>> {
        match output.start(engine) {
            Ok(running) => Ok(running),
            Err(e) => Err(Box::new((e.engine, e.error.to_string()))),
        }
    }
    /// The MIDI inputs now: names made unique by order, and port
    /// identities (`docs/design/device-changes.md`).
    fn midi_list(&self) -> Result<Vec<MidiInputInfo>, String>;
    /// The names of the MIDI inputs, unique by order.
    fn midi_inputs(&self) -> Result<Vec<String>, String> {
        Ok(self.midi_list()?.into_iter().map(|input| input.name).collect())
    }
    /// Opens the MIDI input with this name from [`Host::midi_list`]. Its
    /// events carry `input`.
    fn open_midi(&self, name: &str, midi: SharedMidi, input: MidiInputId) -> Result<Box<dyn MidiPort>, String>;
    /// A function that lists the MIDI inputs as [`Host::midi_list`] does,
    /// from any thread. The app then watches the list on a thread of its
    /// own, and wakes the UI only when it changes (#90). `None`, the
    /// default, has the UI poll [`Host::midi_list`] itself.
    fn midi_lister(&self) -> Option<MidiLister> {
        None
    }
}

/// Lists the MIDI inputs from any thread. See [`Host::midi_lister`].
pub type MidiLister = fn() -> Result<Vec<MidiInputInfo>, String>;

/// How often the MIDI input list is read: four times a second
/// (`docs/design/device-changes.md`).
pub const MIDI_POLL: std::time::Duration = std::time::Duration::from_millis(250);

/// The MIDI input list to watch on a thread, and the list the UI last saw.
#[derive(Clone)]
pub struct MidiWatch {
    /// Reads the list.
    pub list: MidiLister,
    /// What the UI last saw. The thread sends nothing until the list
    /// differs from it.
    pub seen: Vec<MidiInputInfo>,
    /// The engine generation: a new engine restarts the watch with the
    /// list the UI saw then.
    pub generation: u64,
}

impl std::hash::Hash for MidiWatch {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        "loom-app MIDI watch".hash(state);
        self.generation.hash(state);
    }
}

/// Reads the MIDI input list every [`MIDI_POLL`] on a thread, and yields
/// once each time it changes. The UI then runs its usual poll. A failed
/// read is not a change: the UI's poll skips those too.
pub fn midi_stream(watch: &MidiWatch) -> iced::futures::channel::mpsc::Receiver<()> {
    let (mut tx, rx) = iced::futures::channel::mpsc::channel(1);
    let (list, mut seen) = (watch.list, watch.seen.clone());
    std::thread::spawn(move || {
        while !tx.is_closed() {
            if let Ok(now) = list()
                && now != seen
            {
                match tx.try_send(()) {
                    Ok(()) => seen = now,
                    // The UI has not taken the last one: it reads the
                    // list itself when it does.
                    Err(e) if e.is_full() => seen = now,
                    Err(_) => return,
                }
            }
            std::thread::sleep(MIDI_POLL);
        }
    });
    rx
}

/// The real devices, through `cpal` and `midir`.
pub struct System;

impl Host for System {
    fn outputs(&self) -> Result<Outputs, String> {
        loom_host_desktop::outputs().map_err(|e| e.to_string())
    }

    fn open(&self, options: &AudioOptions) -> Result<AudioOutput, String> {
        AudioOutput::open(options).map_err(|e| e.to_string())
    }

    fn audio_inputs(&self) -> Result<Inputs, String> {
        system_audio_inputs()
    }

    fn audio_input_lister(&self) -> Option<fn() -> Result<Inputs, String>> {
        Some(system_audio_inputs)
    }

    fn audio_input_permission_denied(&self) -> bool {
        loom_host_desktop::microphone_access_denied()
    }

    fn audio_input_permission_checker(&self) -> Option<fn() -> bool> {
        Some(loom_host_desktop::microphone_access_denied)
    }

    fn open_audio_input(
        &self,
        options: &AudioOptions,
        output_buffer: usize,
    ) -> Result<(Box<dyn InputPort>, loom_core::audio_input::InputReader), String> {
        let (input, reader) =
            loom_host_desktop::AudioInput::start(options, output_buffer).map_err(|error| match error {
                loom_host_desktop::HostError::UnsupportedSampleRate { device, rate } => {
                    format!("\"{device}\" does not offer {rate} Hz")
                }
                other => other.to_string(),
            })?;
        Ok((Box::new(input), reader))
    }

    fn midi_list(&self) -> Result<Vec<MidiInputInfo>, String> {
        system_midi_list()
    }

    fn open_midi(&self, name: &str, midi: SharedMidi, input: MidiInputId) -> Result<Box<dyn MidiPort>, String> {
        loom_host_desktop::connect_midi_input(name, midi, input)
            .map(|connection| Box::new(SystemPort { connection, input }) as Box<dyn MidiPort>)
            .map_err(|e| e.to_string())
    }

    fn midi_lister(&self) -> Option<MidiLister> {
        Some(system_midi_list)
    }
}

/// Lists audio inputs on any thread.
fn system_audio_inputs() -> Result<Inputs, String> {
    Ok(Inputs {
        default: loom_host_desktop::default_input_device_name(),
        devices: loom_host_desktop::input_device_names().map_err(|e| e.to_string())?,
    })
}

/// The system's MIDI inputs, through `midir`.
fn system_midi_list() -> Result<Vec<MidiInputInfo>, String> {
    loom_host_desktop::midi_inputs().map_err(|e| e.to_string())
}

/// The rates a menu offers, when a device's range includes them (Rule D4).
pub const COMMON_RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];

/// Buffer sizes offered when a device does not report its range (Rule D4).
const UNREPORTED_BUFFERS: (u32, u32) = (64, 2048);

/// The common rates that fall in one of the device's ranges.
pub fn offered_rates(info: &OutputInfo) -> Vec<u32> {
    COMMON_RATES
        .into_iter()
        .filter(|&rate| offers_rate(info, rate))
        .collect()
}

fn offers_rate(info: &OutputInfo, rate: u32) -> bool {
    info.rates.iter().any(|&(lo, hi)| (lo..=hi).contains(&rate))
}

/// The powers of two from 16 to 4096 in the device's buffer range, or from
/// 64 to 2048 when it reports none.
pub fn offered_buffers(info: &OutputInfo) -> Vec<u32> {
    let (lo, hi) = info.buffer.unwrap_or(UNREPORTED_BUFFERS);
    (4..=12)
        .map(|power| 1u32 << power)
        .filter(|size| (lo..=hi).contains(size))
        .collect()
}

/// A buffer's length in milliseconds at `rate`, as the window shows it:
/// `1.3 ms`.
pub fn buffer_time(frames: u32, rate: u32) -> String {
    format!("{:.1} ms", f64::from(frames) * 1000.0 / f64::from(rate.max(1)))
}

/// Which device to play on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    /// The system default.
    SystemDefault,
    /// A saved device, by its full name. Missing at start: the system
    /// default plays instead.
    Named(String),
    /// `--device`: the first device whose name contains this.
    Matching(String),
}

/// The device, rate, and buffer size to open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wanted {
    pub device: Pick,
    pub rate: Option<u32>,
    pub buffer: Option<u32>,
}

impl Wanted {
    /// The saved choice, with the flags for this run on top.
    pub fn new(saved: &AudioPrefs, flags: &Options) -> Self {
        let device = match (&flags.device, &saved.device) {
            (Some(part), _) => Pick::Matching(part.clone()),
            (None, DeviceChoice::SystemDefault) => Pick::SystemDefault,
            (None, DeviceChoice::Named(name)) => Pick::Named(name.clone()),
        };
        Self {
            device,
            rate: flags.sample_rate.or(saved.sample_rate.hz()),
            buffer: flags.buffer.or(saved.buffer.frames()),
        }
    }
}

/// What to open, and the status bar lines about values that could not be
/// used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub options: AudioOptions,
    pub notes: Vec<String>,
}

/// Decides what to open at start (preferences.md, At start). A saved device
/// that is not plugged in, or a rate or buffer size the device does not
/// offer, falls back to the default, with a note. The saved choice is never
/// changed here (Rule D2). Only a `--device` that matches nothing is an
/// error: the app then starts with no audio.
pub fn plan(outputs: &Outputs, wanted: &Wanted) -> Result<Plan, String> {
    let mut notes = Vec::new();
    let default = || {
        outputs
            .default
            .as_ref()
            .and_then(|name| outputs.devices.iter().find(|d| d.name == *name))
            .ok_or_else(|| "no audio output device found".to_string())
    };
    let info = match &wanted.device {
        Pick::SystemDefault => default()?,
        Pick::Named(name) => match outputs.devices.iter().find(|d| d.name == *name) {
            Some(info) => info,
            None => {
                let info = default()?;
                notes.push(format!(
                    "Audio: \"{name}\" is not connected. Playing on \"{}\". Choose a device in Preferences.",
                    info.name
                ));
                info
            }
        },
        Pick::Matching(part) => {
            let lower = part.to_lowercase();
            outputs
                .devices
                .iter()
                .find(|d| d.name.to_lowercase().contains(&lower))
                .ok_or_else(|| format!("no audio output device matches \"{part}\""))?
        }
    };
    let sample_rate = match wanted.rate {
        Some(rate) if !offers_rate(info, rate) => {
            notes.push(format!(
                "Audio: \"{}\" does not offer {rate} Hz. Using {} Hz.",
                info.name, info.default_rate
            ));
            None
        }
        rate => rate,
    };
    let buffer_size = match wanted.buffer {
        Some(frames) if !offered_buffers(info).contains(&frames) => {
            notes.push(format!(
                "Audio: \"{}\" does not offer {frames} frames. Using the device default.",
                info.name
            ));
            None
        }
        frames => frames,
    };
    Ok(Plan {
        options: AudioOptions {
            device: Some(info.name.clone()),
            exact: true,
            sample_rate,
            buffer_size,
        },
        notes,
    })
}

/// Opens the output at start. When the device list cannot be read, the
/// system default opens as before. A buffer size the device refuses falls
/// back to the device default (Rule D4).
pub fn open_at_start(host: &dyn Host, wanted: &Wanted) -> (Result<AudioOutput, String>, Vec<String>) {
    let plan = match host.outputs() {
        Ok(outputs) => plan(&outputs, wanted),
        // No list: let the backend choose, with the rate and buffer asked for.
        Err(_) => Ok(Plan {
            options: AudioOptions {
                device: match &wanted.device {
                    Pick::SystemDefault => None,
                    Pick::Named(name) | Pick::Matching(name) => Some(name.clone()),
                },
                exact: matches!(wanted.device, Pick::Named(_)),
                sample_rate: wanted.rate,
                buffer_size: wanted.buffer,
            },
            notes: Vec::new(),
        }),
    };
    let Plan { options, mut notes } = match plan {
        Ok(plan) => plan,
        Err(e) => return (Err(e), Vec::new()),
    };
    let output = open_with_fallback(host, options, &mut notes);
    (output, notes)
}

/// Opens `options`; if the device refuses the buffer size, opens it again
/// with its default buffer size, and notes it.
fn open_with_fallback(host: &dyn Host, options: AudioOptions, notes: &mut Vec<String>) -> Result<AudioOutput, String> {
    match host.open(&options) {
        Err(_) if options.buffer_size.is_some() => {
            let frames = options.buffer_size.unwrap_or_default();
            let output = host.open(&AudioOptions {
                buffer_size: None,
                ..options
            })?;
            notes.push(format!(
                "Audio: \"{}\" refused {frames} frames. Using the device default.",
                output.name()
            ));
            Ok(output)
        }
        result => result,
    }
}

/// Opens the device the player picked in the window: exactly that device,
/// rate, and buffer size, with no fallback to another device.
pub fn open_choice(host: &dyn Host, choice: &AudioPrefs) -> Result<AudioOutput, String> {
    let device = match &choice.device {
        DeviceChoice::SystemDefault => host.outputs()?.default.ok_or("no audio output device found")?,
        DeviceChoice::Named(name) => name.clone(),
    };
    host.open(&AudioOptions {
        device: Some(device),
        exact: true,
        sample_rate: choice.sample_rate.hz(),
        buffer_size: choice.buffer.frames(),
    })
}

/// The MIDI inputs to open at start: none with `--no-midi`, the matching
/// ones with `--midi`, and otherwise every input that is not turned off.
pub fn midi_at_start(inputs: &[String], off: &[String], flags: &Options) -> Vec<String> {
    if flags.no_midi {
        return Vec::new();
    }
    inputs
        .iter()
        .filter(|name| match &flags.midi {
            Some(part) => name.to_lowercase().contains(&part.to_lowercase()),
            None => !off.contains(name),
        })
        .cloned()
        .collect()
}

/// What is playing now, for the line under the audio controls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Playing {
    pub device: String,
    pub rate: u32,
    /// The buffer size asked for, or `None` for the device default.
    pub buffer: Option<u32>,
}

impl Playing {
    pub fn of(output: &AudioOutput) -> Self {
        Self {
            device: output.name().to_string(),
            rate: output.sample_rate(),
            buffer: output.buffer_size(),
        }
    }

    /// `Playing on "MOTU M4" at 96000 Hz, 128 frames (1.3 ms).` `frames` is
    /// the buffer size the device asks for, when the device chose it.
    pub fn describe(&self, frames: Option<u32>) -> String {
        match self.buffer.or(frames) {
            Some(frames) => format!(
                "Playing on \"{}\" at {} Hz, {frames} frames ({}).",
                self.device,
                self.rate,
                buffer_time(frames, self.rate)
            ),
            None => format!("Playing on \"{}\" at {} Hz.", self.device, self.rate),
        }
    }
}

/// The rate shown for "Device default" in the menu, and the rate the buffer
/// times are worked out at.
pub fn effective_rate(info: &OutputInfo, choice: RateChoice) -> u32 {
    choice.hz().unwrap_or(info.default_rate)
}

/// Every choice the Buffer size menu offers.
pub fn buffer_choices(info: &OutputInfo) -> Vec<BufferChoice> {
    std::iter::once(BufferChoice::DeviceDefault)
        .chain(offered_buffers(info).into_iter().map(BufferChoice::Frames))
        .collect()
}

/// The entry in `names` that `wanted` names: the one equal to it, or else
/// the only one that contains it, case-insensitive. Several matches are
/// refused, never the first one picked (control-channel.md, rule S2).
/// `what` names the list in errors, such as "audio output".
pub fn match_name(names: &[String], wanted: &str, what: &str) -> Result<String, String> {
    if let Some(name) = names.iter().find(|name| *name == wanted) {
        return Ok(name.clone());
    }
    let lower = wanted.to_lowercase();
    let found: Vec<&String> = names
        .iter()
        .filter(|name| name.to_lowercase().contains(&lower))
        .collect();
    match found.as_slice() {
        [name] => Ok((*name).clone()),
        [] => Err(format!("no {what} matches `{wanted}`; there are: {}", quoted(names))),
        _ => Err(format!(
            "several {what}s match `{wanted}`: {}; give more of the name",
            quoted(&found.into_iter().cloned().collect::<Vec<_>>())
        )),
    }
}

/// Names in quotes, separated by commas, or "none".
fn quoted(names: &[String]) -> String {
    if names.is_empty() {
        return "none".to_string();
    }
    names.iter().map(|n| format!("\"{n}\"")).collect::<Vec<_>>().join(", ")
}

/// Every choice the Sample rate menu offers.
pub fn rate_choices(info: &OutputInfo) -> Vec<RateChoice> {
    std::iter::once(RateChoice::DeviceDefault)
        .chain(offered_rates(info).into_iter().map(RateChoice::Hz))
        .collect()
}

/// A fake host for tests: a list of devices, and MIDI inputs whose notes a
/// test plays by hand.
#[cfg(test)]
pub mod fake;

#[cfg(test)]
mod tests;
