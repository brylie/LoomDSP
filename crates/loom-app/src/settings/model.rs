//! What the Preferences window shows, as plain data (ADR-0021). The view
//! draws it, the accessibility tree reports it, and the UI snapshot
//! carries it, so all three say the same thing.
//!
//! It is built fresh each time it is asked for, not kept in `update`: the
//! playing line and the input notes read the audio stream, which changes
//! with no message. It is small, about twenty parts and one per MIDI input.

use super::{Message, Window};
use crate::App;
use crate::devices;
use crate::preferences::{BufferChoice, DeviceChoice, InputChoice, RateChoice};
use loom_core::OutputSafety;
use serde::Serialize;

/// The Preferences window, as it shows.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct Model {
    /// The three output safety modes, in order.
    pub safety: Vec<SafetyMode>,
    /// Notes under the safety modes.
    pub safety_notes: Vec<Note>,
    /// Show signal activity on ports.
    pub activity: bool,
    /// The audio section's parts, in screen order: its lists and notes.
    pub audio: Vec<Part>,
    /// The MIDI section's parts, in screen order: a switch per input and
    /// notes.
    pub midi: Vec<Part>,
    /// The panic trigger.
    pub panic: Panic,
    /// The **Your name** field as typed.
    pub author: String,
    /// The default licence, as its short text, or "No default".
    pub licence: String,
}

/// One output safety mode.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct SafetyMode {
    /// Its key in UI IDs: `low-latency`, `clean`, or `off`.
    pub key: &'static str,
    /// What its radio says.
    pub label: &'static str,
    /// The line under it.
    pub note: &'static str,
    /// True while it is the mode in use.
    pub chosen: bool,
    /// The mode.
    #[serde(skip)]
    pub mode: OutputSafety,
}

/// A line of text, keyed by what it is about, never by its place.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct Note {
    /// Its key in UI IDs.
    pub key: &'static str,
    /// Its words.
    pub text: String,
}

/// A list that shows its chosen option and opens to choose another.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct Picks {
    /// Its key in UI IDs: `device`, `rate`, `buffer`, or `input`.
    pub key: &'static str,
    /// The label beside it.
    pub label: &'static str,
    /// What each option says, in list order.
    pub options: Vec<String>,
    /// The option chosen, if one is.
    pub chosen: Option<usize>,
    /// The option last asked for that could not be used, if it is in this
    /// list. A step goes on from it, not from the chosen one.
    pub failed: Option<usize>,
    /// The message each option sends, in the same order.
    #[serde(skip)]
    pub messages: Vec<Message>,
}

impl Picks {
    /// The message that picks option `at`.
    pub(crate) fn pick(&self, at: usize) -> Option<Message> {
        self.messages.get(at).cloned()
    }

    /// Where a step starts: the option that failed, or else the chosen
    /// one.
    pub(crate) fn step_from(&self) -> Option<usize> {
        self.failed.or(self.chosen)
    }

    /// The text of the chosen option.
    pub(crate) fn chosen_text(&self) -> Option<&str> {
        self.chosen.and_then(|at| self.options.get(at)).map(String::as_str)
    }
}

/// One part of a section, in screen order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) enum Part {
    /// A list to pick from. The output device's list has Refresh beside it.
    Picks(Picks),
    /// A MIDI input and whether it is on.
    Midi {
        /// The port's name.
        name: String,
        /// True while it is open.
        on: bool,
        /// Its UI ID. Two ports can share a name, so the second gets a
        /// number (`UniqueIds`), worked out once here for the view and the
        /// tree alike.
        #[serde(serialize_with = "crate::snapshot::display")]
        id: loom_ui_core::UiId,
    },
    /// A line of text.
    Note(Note),
}

/// The panic trigger's row.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct Panic {
    /// What the trigger is, or that none is set.
    pub line: String,
    /// True while a trigger is being learned, so the button says Cancel.
    pub learning: bool,
    /// True when a trigger is set, so Clear works.
    pub can_clear: bool,
}

/// The output safety modes, with what each one's radio and note say.
const SAFETY: [(OutputSafety, &str, &str, &str); 3] = [
    (
        OutputSafety::LowLatency,
        "low-latency",
        "Low latency (default)",
        "Catches accidents. No added latency. May distort briefly when it acts.",
    ),
    (
        OutputSafety::Clean,
        "clean",
        "Clean",
        "Transparent limiting. Adds 2 ms of latency.",
    ),
    (
        OutputSafety::Off,
        "off",
        "Off",
        "No protection. The raw patch output, clipped to -1 to 1.",
    ),
];

/// A note keyed by what it is about.
fn note(key: &'static str, text: impl Into<String>) -> Part {
    Part::Note(Note { key, text: text.into() })
}

/// A list of `(value, text)` options, with the one equal to `chosen`
/// chosen, each sending `message` with its value. `failed` marks the
/// option that could not be used, when it is in this list.
fn picks<T: PartialEq>(
    failed: Option<&super::Failed>,
    key: &'static str,
    label: &'static str,
    options: Vec<(T, String)>,
    chosen: Option<&T>,
    message: impl Fn(T) -> Message,
) -> Picks {
    let chosen = chosen.and_then(|chosen| options.iter().position(|(value, _)| value == chosen));
    let (messages, options): (Vec<Message>, Vec<String>) =
        options.into_iter().map(|(value, text)| (message(value), text)).unzip();
    let failed = failed.and_then(|failed| messages.iter().position(|sent| *sent == failed.message));
    Picks {
        key,
        label,
        options,
        chosen,
        failed,
        messages,
    }
}

impl App {
    /// What the Preferences window shows, or `None` while it is closed.
    pub(crate) fn preferences_model(&self) -> Option<Model> {
        let window = self.overlays.preferences()?;
        let saved = self.rig.store.saved();
        let mode = self.session.engine.stream.output_safety();
        let safety = SAFETY
            .iter()
            .map(|&(value, key, label, note)| SafetyMode {
                key,
                label,
                note,
                chosen: value == mode,
                mode: value,
            })
            .collect();
        let safety_notes = self
            .rig
            .safety_flag
            .then(|| Note {
                key: "safety-flag",
                text: "Set by --output-safety for this run.".to_string(),
            })
            .into_iter()
            .collect();
        let licence = match loom_patch::metadata::Licence::of(saved.licence.as_deref()) {
            loom_patch::metadata::Licence::None => "No default".to_string(),
            other => other.short(),
        };
        Some(Model {
            safety,
            safety_notes,
            activity: saved.show_activity,
            audio: self.audio_parts(window),
            midi: self.midi_parts(window),
            panic: Panic {
                line: self.panic_line(),
                learning: self.rig.panic_learn.is_some(),
                can_clear: saved.panic.is_some(),
            },
            author: window.author.clone(),
            licence,
        })
    }

    /// The audio section: the output's lists, the input's list, their
    /// notes, and the playing line.
    fn audio_parts(&self, window: &Window) -> Vec<Part> {
        let mut parts = Vec::new();
        let shown = self.rig.shown_audio();
        let failed = window.failed.as_ref();
        match &window.lists.outputs {
            Err(error) => parts.push(note(
                "output-error",
                format!("Could not list the audio outputs: {error}"),
            )),
            Ok(outputs) => {
                let default_name = outputs.default.clone().unwrap_or_else(|| "none".into());
                let devices =
                    std::iter::once((DeviceChoice::SystemDefault, format!("System default ({default_name})")))
                        .chain(
                            outputs
                                .devices
                                .iter()
                                .map(|d| (DeviceChoice::Named(d.name.clone()), d.name.clone())),
                        )
                        .collect();
                parts.push(Part::Picks(picks(
                    failed,
                    "device",
                    "Device",
                    devices,
                    Some(&shown.device),
                    |d| Message::Audio(super::Change::Device(d)),
                )));
                if self.rig.flags.device.is_some() {
                    parts.push(note("device-flag", "Set by --device for this run."));
                }
                if let Some(info) = self.device_info(&shown.device) {
                    let rates = devices::rate_choices(&info)
                        .into_iter()
                        .map(|value| {
                            let text = match value {
                                RateChoice::DeviceDefault => format!("Device default ({} Hz)", info.default_rate),
                                RateChoice::Hz(hz) => format!("{hz} Hz"),
                            };
                            (value, text)
                        })
                        .collect();
                    parts.push(Part::Picks(picks(
                        failed,
                        "rate",
                        "Sample rate",
                        rates,
                        Some(&shown.sample_rate),
                        |r| Message::Audio(super::Change::Rate(r)),
                    )));
                    if self.rig.flags.sample_rate.is_some() {
                        parts.push(note("rate-flag", "Set by --sample-rate for this run."));
                    }
                    let rate = devices::effective_rate(&info, shown.sample_rate);
                    let buffers = devices::buffer_choices(&info)
                        .into_iter()
                        .map(|value| {
                            let text = match value {
                                BufferChoice::DeviceDefault => "Device default".to_string(),
                                BufferChoice::Frames(f) => format!("{f} frames · {}", devices::buffer_time(f, rate)),
                            };
                            (value, text)
                        })
                        .collect();
                    parts.push(Part::Picks(picks(
                        failed,
                        "buffer",
                        "Buffer size",
                        buffers,
                        Some(&shown.buffer),
                        |b| Message::Audio(super::Change::Buffer(b)),
                    )));
                    if self.rig.flags.buffer.is_some() {
                        parts.push(note("buffer-flag", "Set by --buffer for this run."));
                    }
                }
            }
        }
        // Why the last choice could not be used, under the lists, since the
        // status line is hidden under the window.
        if let Some(failed) = failed {
            parts.push(note("audio-error", failed.error.clone()));
        }
        match &window.lists.inputs {
            Err(error) => parts.push(note("input-error", format!("Could not list the audio inputs: {error}"))),
            Ok(inputs) => {
                let choices: Vec<(InputChoice, String)> = std::iter::once((
                    InputChoice::SystemDefault,
                    format!("System default ({})", inputs.default.as_deref().unwrap_or("none")),
                ))
                .chain(
                    inputs
                        .devices
                        .iter()
                        .map(|name| (InputChoice::Named(name.clone()), name.clone())),
                )
                .chain(std::iter::once((InputChoice::Off, "Off".to_string())))
                .collect();
                let chosen = if self.rig.flags.no_input {
                    Some(InputChoice::Off)
                } else if self.rig.flags.input.is_some() {
                    self.rig
                        .effective_input(inputs)
                        .map(|name| InputChoice::Named(name.to_string()))
                } else {
                    Some(shown.input.clone())
                };
                parts.push(Part::Picks(picks(
                    failed,
                    "input",
                    "Audio input",
                    choices,
                    chosen.as_ref(),
                    Message::Input,
                )));
                if self.rig.flags.input.is_some() || self.rig.flags.no_input {
                    parts.push(note("input-flag", "Set by an input flag for this run."));
                }
                if let (Some(input), Some(output)) = (self.rig.effective_input(inputs), &self.rig.playing) {
                    if input == output.device {
                        parts.push(note(
                            "same-device",
                            "Input and output use the same device. Speakers can feed the microphone.",
                        ));
                    }
                    let buffer = output.buffer.or_else(|| {
                        self.session
                            .engine
                            .stream
                            .stats()
                            .map(|stats| stats.snapshot().largest_buffer)
                            .filter(|&frames| frames > 0)
                            .and_then(|frames| u32::try_from(frames).ok())
                    });
                    if let Some(buffer) = buffer {
                        parts.push(note(
                            "input-latency",
                            format!(
                                "Input latency about {}. The patch adds no delay of its own.",
                                devices::buffer_time(buffer.saturating_mul(2), output.rate)
                            ),
                        ));
                    }
                }
            }
        }
        parts.push(note("playing", self.playing_line()));
        parts
    }

    /// The MIDI section: a switch per input, and its notes.
    fn midi_parts(&self, window: &Window) -> Vec<Part> {
        let mut parts = Vec::new();
        match &window.lists.midi {
            Err(error) => parts.push(note("midi-error", format!("Could not list the MIDI inputs: {error}"))),
            Ok(names) if names.is_empty() => parts.push(note("midi-none", "No MIDI inputs found.")),
            Ok(names) => {
                let mut taken = crate::a11y::items::UniqueIds::default();
                for name in names {
                    let on = self.rig.ports.iter().any(|port| port.name() == name);
                    let id = taken.take(super::access::preferences_id().with("midi").with(name.as_str()));
                    parts.push(Part::Midi {
                        name: name.clone(),
                        on,
                        id,
                    });
                }
                parts.push(note("midi-new", "New MIDI inputs are on until you turn them off."));
            }
        }
        if self.rig.flags.no_midi {
            parts.push(note("midi-flag", "Set by --no-midi for this run."));
        } else if self.rig.flags.midi.is_some() {
            parts.push(note("midi-flag", "Set by --midi for this run."));
        }
        parts
    }
}
