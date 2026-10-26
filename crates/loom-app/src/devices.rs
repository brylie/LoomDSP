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
    /// Starts an output with an engine. A device can still refuse here,
    /// after it opened. On refusal, returns the [`loom_core::Engine`] so it
    /// can be kept idle and retried, along with the error reason.
    #[allow(clippy::result_large_err)]
    fn start(
        &self,
        output: AudioOutput,
        engine: loom_core::Engine,
    ) -> Result<RunningAudio, (loom_core::Engine, String)> {
        match output.start(engine) {
            Ok(running) => Ok(running),
            Err(e) => Err((e.engine, e.error.to_string())),
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

    fn midi_list(&self) -> Result<Vec<MidiInputInfo>, String> {
        loom_host_desktop::midi_inputs().map_err(|e| e.to_string())
    }

    fn open_midi(&self, name: &str, midi: SharedMidi, input: MidiInputId) -> Result<Box<dyn MidiPort>, String> {
        loom_host_desktop::connect_midi_input(name, midi, input)
            .map(|connection| Box::new(SystemPort { connection, input }) as Box<dyn MidiPort>)
            .map_err(|e| e.to_string())
    }
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
pub mod fake {
    use super::*;
    use loom_host_desktop::MidiInputState;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;

    /// What happened, in order.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Event {
        Opened(String),
        MidiOpened(String),
        MidiClosed(String),
    }

    #[derive(Default)]
    pub struct Fake {
        pub outputs: RefCell<Outputs>,
        /// Devices that fail to open.
        pub broken: RefCell<Vec<String>>,
        /// Devices that open, then fail to start.
        pub wont_start: RefCell<Vec<String>>,
        /// The active running streams' callback stats, so tests can emit lost errors.
        pub active_stats: RefCell<Vec<(String, Arc<loom_host_desktop::CallbackStats>)>>,
        /// The MIDI inputs plugged in now, as (name, port identity). A
        /// test plugs and unplugs them with [`Fake::plug`] and
        /// [`Fake::unplug`].
        pub midi: RefCell<Vec<(String, String)>>,
        /// Counts identities, so a port plugged in again gets a new one.
        pub next_id: std::cell::Cell<u32>,
        /// MIDI inputs, by listed name, that fail to open.
        pub broken_midi: RefCell<Vec<String>>,
        /// True while listing the MIDI inputs fails.
        pub midi_list_fails: std::cell::Cell<bool>,
        pub log: RefCell<Vec<Event>>,
        /// The open MIDI inputs, so a test can play notes on them.
        pub ports: RefCell<Vec<(String, Rc<RefCell<MidiInputState>>)>>,
        /// With a tape, every device is a stepped output on it: it plays
        /// only when the test steps the tape, and keeps every sample
        /// (audio-qa.md, Stepped test output). Without one, devices run on a timer.
        pub tape: Option<loom_host_desktop::Tape>,
        /// With a text, MIDI goes through the real `midir` inputs instead
        /// of `midi`, but only those whose names contain the text, so a
        /// test with virtual ports never opens a real controller
        /// (audio-qa.md, Virtual MIDI controllers).
        pub real_midi: Option<String>,
        /// Devices that start, make [`STALL_AFTER`] callbacks, and then
        /// stop calling back with no error, as a macOS device did after a
        /// rate change. They play on [`Fake::stall_tape`].
        pub stalls: Vec<String>,
        /// The tape the stalling devices play on. Tests leave it alone.
        pub stall_tape: loom_host_desktop::Tape,
    }

    /// Callbacks a stalling device makes before it stops.
    pub const STALL_AFTER: usize = 4;

    /// A device that offers 44.1 to 48 kHz, buffers of 32 to 1024 frames,
    /// and defaults to 48 kHz.
    pub fn device(name: &str) -> OutputInfo {
        OutputInfo {
            name: name.to_string(),
            default_rate: 48_000,
            rates: vec![(44_100, 48_000)],
            buffer: Some((32, 1024)),
        }
    }

    impl Fake {
        /// Devices by name, the first the system default, and MIDI inputs.
        pub fn new(devices: &[OutputInfo], midi: &[&str]) -> Self {
            Self {
                outputs: RefCell::new(Outputs {
                    default: devices.first().map(|d| d.name.clone()),
                    devices: devices.to_vec(),
                }),
                midi: RefCell::new(midi.iter().map(|s| (s.to_string(), format!("id-{s}"))).collect()),
                ..Self::default()
            }
        }

        /// Dynamically removes a device from the output list.
        pub fn remove_device(&self, name: &str) {
            let mut outputs = self.outputs.borrow_mut();
            outputs.devices.retain(|d| d.name != name);
            if outputs.default.as_deref() == Some(name) {
                outputs.default = outputs.devices.first().map(|d| d.name.clone());
            }
        }

        /// Dynamically adds a device to the output list.
        pub fn add_device(&self, info: OutputInfo) {
            let mut outputs = self.outputs.borrow_mut();
            if !outputs.devices.iter().any(|d| d.name == info.name) {
                outputs.devices.push(info);
            }
            if outputs.default.is_none() {
                outputs.default = outputs.devices.first().map(|d| d.name.clone());
            }
        }

        /// Emits a device loss error for the running output on `name`.
        pub fn emit_lost(&self, name: &str) {
            for (dev_name, stats) in self.active_stats.borrow().iter() {
                if dev_name == name {
                    stats.mark_dead();
                }
            }
        }

        /// Plugs in an input with a new port identity, at the end of the
        /// list.
        pub fn plug(&self, name: &str) {
            let id = self.next_id.get() + 1;
            self.next_id.set(id);
            self.midi.borrow_mut().push((name.to_string(), format!("{name}#{id}")));
        }

        /// Unplugs the last input listed with this name. Its connection, if
        /// open, stays until the app closes it, as a real one does.
        pub fn unplug(&self, name: &str) {
            let mut midi = self.midi.borrow_mut();
            if let Some(index) = midi.iter().rposition(|(n, _)| n == name) {
                midi.remove(index);
            }
        }

        /// How many inputs are open, for leak checks.
        pub fn open_count(&self) -> usize {
            self.ports.borrow().len()
        }

        pub fn opened(&self) -> Vec<String> {
            self.log
                .borrow()
                .iter()
                .filter_map(|e| match e {
                    Event::Opened(name) => Some(name.clone()),
                    _ => None,
                })
                .collect()
        }

        /// Plays raw MIDI bytes on an open input.
        pub fn play(&self, input: &str, bytes: &[u8]) {
            let ports = self.ports.borrow();
            let (_, state) = ports.iter().find(|(name, _)| name == input).expect("the input is open");
            state.borrow_mut().receive(bytes);
        }

        pub fn is_open(&self, input: &str) -> bool {
            self.ports.borrow().iter().any(|(name, _)| name == input)
        }
    }

    struct Port {
        name: String,
        id: String,
        input: MidiInputId,
        state: Rc<RefCell<MidiInputState>>,
        host: Rc<Fake>,
    }

    impl MidiPort for Port {
        fn name(&self) -> &str {
            &self.name
        }

        fn port_id(&self) -> &str {
            &self.id
        }

        fn input(&self) -> MidiInputId {
            self.input
        }

        fn close(self: Box<Self>) {
            self.host.ports.borrow_mut().retain(|(name, _)| *name != self.name);
            self.state.borrow_mut().release_all();
            self.host.log.borrow_mut().push(Event::MidiClosed(self.name.clone()));
        }
    }

    /// The host `App::new` uses in tests: the real audio outputs, but no
    /// MIDI inputs, so a controller plugged into the machine cannot change
    /// a result. Opening a MIDI port through it is a test bug.
    pub struct NoMidi;

    impl Host for NoMidi {
        fn outputs(&self) -> Result<Outputs, String> {
            System.outputs()
        }

        fn open(&self, options: &AudioOptions) -> Result<AudioOutput, String> {
            System.open(options)
        }

        #[allow(clippy::result_large_err)]
        #[allow(clippy::result_large_err)]
        fn start(
            &self,
            output: AudioOutput,
            engine: loom_core::Engine,
        ) -> Result<RunningAudio, (loom_core::Engine, String)> {
            System.start(output, engine)
        }

        fn midi_list(&self) -> Result<Vec<MidiInputInfo>, String> {
            Ok(Vec::new())
        }

        fn open_midi(&self, name: &str, _midi: SharedMidi, _input: MidiInputId) -> Result<Box<dyn MidiPort>, String> {
            panic!("a test opened the MIDI port `{name}`; use fake::Shared with its own ports");
        }
    }

    /// The fake behind an `Rc`, so its MIDI ports can reach it.
    pub struct Shared(pub Rc<Fake>);

    impl Host for Shared {
        fn outputs(&self) -> Result<Outputs, String> {
            Ok(self.0.outputs.borrow().clone())
        }

        fn open(&self, options: &AudioOptions) -> Result<AudioOutput, String> {
            let fake = &self.0;
            let outputs = fake.outputs.borrow();
            let name = options
                .device
                .clone()
                .or_else(|| outputs.default.clone())
                .ok_or("no device")?;
            let info = outputs
                .devices
                .iter()
                .find(|d| d.name == name)
                .ok_or_else(|| format!("no audio output device found matching `{name}`"))?;
            if fake.broken.borrow().contains(&name) {
                return Err("the device is busy".to_string());
            }
            let rate = options.sample_rate.unwrap_or(info.default_rate);
            if !offers_rate(info, rate) {
                return Err(format!("audio device `{name}` does not support {rate} Hz"));
            }
            fake.log.borrow_mut().push(Event::Opened(name.clone()));
            let block = options.buffer_size.unwrap_or(256);
            Ok(match &fake.tape {
                _ if fake.stalls.contains(&name) => AudioOutput::stepped(rate, block, &fake.stall_tape),
                Some(tape) => AudioOutput::stepped(rate, block, tape),
                None => AudioOutput::clock(rate, block),
            }
            .named(name))
        }

        #[allow(clippy::result_large_err)]
        fn start(
            &self,
            output: AudioOutput,
            engine: loom_core::Engine,
        ) -> Result<RunningAudio, (loom_core::Engine, String)> {
            if self.0.wont_start.borrow().iter().any(|name| name == output.name()) {
                return Err((engine, "the stream did not start".to_string()));
            }
            let stalls = self.0.stalls.iter().any(|name| name == output.name());
            let block = output.buffer_size().unwrap_or(256) as usize;
            let name = output.name().to_string();
            let running = match output.start(engine) {
                Ok(r) => r,
                Err(e) => return Err((e.engine, e.error.to_string())),
            };
            self.0.active_stats.borrow_mut().push((name, running.stats().clone()));
            if stalls {
                self.0.stall_tape.step(block * STALL_AFTER);
            }
            Ok(running)
        }

        fn midi_list(&self) -> Result<Vec<MidiInputInfo>, String> {
            match &self.0.real_midi {
                Some(only) => Ok(System
                    .midi_list()?
                    .into_iter()
                    .filter(|i| i.name.contains(only))
                    .collect()),
                None if self.0.midi_list_fails.get() => Err("the MIDI service is not running".to_string()),
                None => {
                    let (names, ids) = self.0.midi.borrow().iter().cloned().unzip();
                    Ok(loom_host_desktop::input_infos(names, ids))
                }
            }
        }

        fn open_midi(&self, name: &str, midi: SharedMidi, input: MidiInputId) -> Result<Box<dyn MidiPort>, String> {
            if let Some(only) = &self.0.real_midi {
                assert!(name.contains(only), "a test opens only its own MIDI ports");
                return System.open_midi(name, midi, input);
            }
            if self.0.broken_midi.borrow().iter().any(|broken| broken == name) {
                return Err(format!("{name}: the port is busy"));
            }
            let Some(info) = self.midi_list()?.into_iter().find(|i| i.name == name) else {
                return Err(format!("no MIDI input port found matching `{name}`"));
            };
            let state = Rc::new(RefCell::new(MidiInputState::for_input(midi, input)));
            self.0.ports.borrow_mut().push((name.to_string(), state.clone()));
            self.0.log.borrow_mut().push(Event::MidiOpened(name.to_string()));
            Ok(Box::new(Port {
                name: name.to_string(),
                id: info.id,
                input,
                state,
                host: self.0.clone(),
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::{NoMidi, device};
    use super::*;

    fn outputs(names: &[&str]) -> Outputs {
        Outputs {
            default: names.first().map(|n| n.to_string()),
            devices: names.iter().map(|n| device(n)).collect(),
        }
    }

    fn wanted(device: Pick) -> Wanted {
        Wanted {
            device,
            rate: None,
            buffer: None,
        }
    }

    #[test]
    fn only_common_rates_in_the_range_are_offered() {
        let wide = OutputInfo {
            rates: vec![(8_000, 384_000)],
            ..device("Wide")
        };
        assert_eq!(offered_rates(&wide), COMMON_RATES);
        assert_eq!(
            rate_choices(&wide).len(),
            7,
            "Device default and the six common rates, and nothing else"
        );
        assert_eq!(offered_rates(&device("Small")), [44_100, 48_000]);
    }

    #[test]
    fn buffers_are_powers_of_two_in_the_range() {
        assert_eq!(offered_buffers(&device("A")), [32, 64, 128, 256, 512, 1024]);
        let unreported = OutputInfo {
            buffer: None,
            ..device("B")
        };
        assert_eq!(offered_buffers(&unreported), [64, 128, 256, 512, 1024, 2048]);
        let huge = OutputInfo {
            buffer: Some((1, 1 << 20)),
            ..device("C")
        };
        assert_eq!(offered_buffers(&huge).first(), Some(&16));
        assert_eq!(offered_buffers(&huge).last(), Some(&4096));
        assert_eq!(buffer_time(128, 96_000), "1.3 ms");
    }

    #[test]
    fn a_saved_device_plays_when_plugged_in() {
        let plan = plan(&outputs(&["Fake A", "Fake B"]), &wanted(Pick::Named("Fake B".into()))).unwrap();
        assert_eq!(plan.options.device.as_deref(), Some("Fake B"));
        assert!(plan.options.exact);
        assert!(plan.notes.is_empty());
    }

    #[test]
    fn a_missing_saved_device_falls_back_to_the_default() {
        let plan = plan(&outputs(&["Fake A"]), &wanted(Pick::Named("Fake B".into()))).unwrap();
        assert_eq!(plan.options.device.as_deref(), Some("Fake A"));
        assert_eq!(
            plan.notes,
            ["Audio: \"Fake B\" is not connected. Playing on \"Fake A\". Choose a device in Preferences."]
        );
    }

    #[test]
    fn a_rate_the_device_does_not_offer_falls_back() {
        let plan = plan(
            &outputs(&["Fake A"]),
            &Wanted {
                rate: Some(96_000),
                buffer: Some(100),
                ..wanted(Pick::SystemDefault)
            },
        )
        .unwrap();
        assert_eq!(plan.options.sample_rate, None);
        assert_eq!(plan.options.buffer_size, None);
        assert_eq!(
            plan.notes,
            [
                "Audio: \"Fake A\" does not offer 96000 Hz. Using 48000 Hz.",
                "Audio: \"Fake A\" does not offer 100 frames. Using the device default.",
            ]
        );
    }

    #[test]
    fn the_device_flag_matches_part_of_a_name() {
        let list = outputs(&["Fake A", "MOTU M4"]);
        let plan = plan(&list, &wanted(Pick::Matching("motu".into()))).unwrap();
        assert_eq!(plan.options.device.as_deref(), Some("MOTU M4"));
        let error = super::plan(&list, &wanted(Pick::Matching("RME".into()))).unwrap_err();
        assert_eq!(error, "no audio output device matches \"RME\"");
    }

    #[test]
    fn flags_win_over_the_saved_choice() {
        let saved = AudioPrefs {
            device: DeviceChoice::Named("Fake B".into()),
            sample_rate: RateChoice::Hz(44_100),
            buffer: BufferChoice::Frames(64),
        };
        let flags = Options {
            device: Some("Fake A".into()),
            buffer: Some(128),
            ..Options::default()
        };
        assert_eq!(
            Wanted::new(&saved, &flags),
            Wanted {
                device: Pick::Matching("Fake A".into()),
                rate: Some(44_100),
                buffer: Some(128),
            }
        );
    }

    #[test]
    fn midi_inputs_at_start() {
        let inputs = ["Pad".to_string(), "Keys".to_string(), "Keys 2".to_string()];
        let off = ["Pad".to_string(), "Keys 2".to_string()];
        assert_eq!(midi_at_start(&inputs, &off, &Options::default()), ["Keys"]);
        let only_keys = Options {
            midi: Some("KEYS".into()),
            ..Options::default()
        };
        assert_eq!(midi_at_start(&inputs, &off, &only_keys), ["Keys", "Keys 2"]);
        let none = Options {
            no_midi: true,
            ..Options::default()
        };
        assert!(midi_at_start(&inputs, &[], &none).is_empty());
    }

    #[test]
    fn the_playing_line() {
        let playing = Playing {
            device: "MOTU M4".into(),
            rate: 96_000,
            buffer: Some(128),
        };
        assert_eq!(
            playing.describe(None),
            "Playing on \"MOTU M4\" at 96000 Hz, 128 frames (1.3 ms)."
        );
        let chosen_by_device = Playing {
            buffer: None,
            ..playing
        };
        assert_eq!(
            chosen_by_device.describe(Some(512)),
            "Playing on \"MOTU M4\" at 96000 Hz, 512 frames (5.3 ms)."
        );
        assert_eq!(chosen_by_device.describe(None), "Playing on \"MOTU M4\" at 96000 Hz.");
    }

    /// Verifies that NoMidi host implements start by delegating to output.start.
    #[test]
    fn no_midi_host_start() {
        let host = NoMidi;
        let output = AudioOutput::clock(48_000, 256);
        let parts = loom_core::create_engine(loom_modules::registry(), loom_core::EngineConfig::default());
        let started = host.start(output, parts.engine);
        assert!(started.is_ok());
    }

    /// Verifies that the default Host::start implementation handles outputs.
    #[test]
    fn host_start_default_implementation() {
        let output = AudioOutput::clock(48_000, 256);
        let parts = loom_core::create_engine(loom_modules::registry(), loom_core::EngineConfig::default());
        let started = System.start(output, parts.engine);
        assert!(started.is_ok());
    }
}
