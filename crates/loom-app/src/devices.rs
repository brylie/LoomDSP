//! Audio and MIDI devices in the app (`docs/design/preferences.md`, Audio
//! and MIDI devices).
//!
//! The app reaches devices only through [`Host`], so tests can give it a
//! fake list of devices, rates, and buffer ranges, and run without hardware.
//! This module also decides which device, rate, and buffer size to open from
//! the saved choice and the command-line flags, and what the window offers.

use crate::preferences::{AudioPrefs, BufferChoice, DeviceChoice, RateChoice};
use loom_host_desktop::{AudioOptions, AudioOutput, MidiConnection, OutputInfo, Outputs, SharedMidi};
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
    /// The input's full name.
    fn name(&self) -> &str;
    /// Closes the input and releases the notes held on it.
    fn close(self: Box<Self>);
}

impl MidiPort for MidiConnection {
    fn name(&self) -> &str {
        self.port_name()
    }

    fn close(self: Box<Self>) {
        MidiConnection::close(*self);
    }
}

/// Where the app finds audio outputs and MIDI inputs.
pub trait Host {
    /// The output devices and what each offers.
    fn outputs(&self) -> Result<Outputs, String>;
    /// Chooses an output, without starting it.
    fn open(&self, options: &AudioOptions) -> Result<AudioOutput, String>;
    /// The names of the MIDI inputs.
    fn midi_inputs(&self) -> Result<Vec<String>, String>;
    /// Opens the MIDI input with this exact name.
    fn open_midi(&self, name: &str, midi: SharedMidi) -> Result<Box<dyn MidiPort>, String>;
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

    fn midi_inputs(&self) -> Result<Vec<String>, String> {
        loom_host_desktop::midi_input_names().map_err(|e| e.to_string())
    }

    fn open_midi(&self, name: &str, midi: SharedMidi) -> Result<Box<dyn MidiPort>, String> {
        loom_host_desktop::connect_midi_named(name, midi)
            .map(|c| Box::new(c) as Box<dyn MidiPort>)
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

    /// What happened, in order.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Event {
        Opened(String),
        MidiOpened(String),
        MidiClosed(String),
    }

    #[derive(Default)]
    pub struct Fake {
        pub outputs: Outputs,
        /// Devices that fail to open.
        pub broken: Vec<String>,
        pub midi: Vec<String>,
        pub log: RefCell<Vec<Event>>,
        /// The open MIDI inputs, so a test can play notes on them.
        pub ports: RefCell<Vec<(String, Rc<RefCell<MidiInputState>>)>>,
    }

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
                outputs: Outputs {
                    default: devices.first().map(|d| d.name.clone()),
                    devices: devices.to_vec(),
                },
                midi: midi.iter().map(|s| s.to_string()).collect(),
                ..Self::default()
            }
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
        state: Rc<RefCell<MidiInputState>>,
        host: Rc<Fake>,
    }

    impl MidiPort for Port {
        fn name(&self) -> &str {
            &self.name
        }

        fn close(self: Box<Self>) {
            self.host.ports.borrow_mut().retain(|(name, _)| *name != self.name);
            self.state.borrow_mut().release_all();
            self.host.log.borrow_mut().push(Event::MidiClosed(self.name.clone()));
        }
    }

    /// The fake behind an `Rc`, so its MIDI ports can reach it.
    pub struct Shared(pub Rc<Fake>);

    impl Host for Shared {
        fn outputs(&self) -> Result<Outputs, String> {
            Ok(self.0.outputs.clone())
        }

        fn open(&self, options: &AudioOptions) -> Result<AudioOutput, String> {
            let fake = &self.0;
            let name = options
                .device
                .clone()
                .or(fake.outputs.default.clone())
                .ok_or("no device")?;
            let info = fake
                .outputs
                .devices
                .iter()
                .find(|d| d.name == name)
                .ok_or_else(|| format!("no audio output device found matching `{name}`"))?;
            if fake.broken.contains(&name) {
                return Err("the device is busy".to_string());
            }
            let rate = options.sample_rate.unwrap_or(info.default_rate);
            if !offers_rate(info, rate) {
                return Err(format!("audio device `{name}` does not support {rate} Hz"));
            }
            fake.log.borrow_mut().push(Event::Opened(name.clone()));
            let block = options.buffer_size.unwrap_or(256);
            Ok(AudioOutput::clock(rate, block).named(name))
        }

        fn midi_inputs(&self) -> Result<Vec<String>, String> {
            Ok(self.0.midi.clone())
        }

        fn open_midi(&self, name: &str, midi: SharedMidi) -> Result<Box<dyn MidiPort>, String> {
            if !self.0.midi.iter().any(|n| n == name) {
                return Err(format!("no MIDI input port found matching `{name}`"));
            }
            let state = Rc::new(RefCell::new(MidiInputState::new(midi)));
            self.0.ports.borrow_mut().push((name.to_string(), state.clone()));
            self.0.log.borrow_mut().push(Event::MidiOpened(name.to_string()));
            Ok(Box::new(Port {
                name: name.to_string(),
                state,
                host: self.0.clone(),
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::device;
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
}
