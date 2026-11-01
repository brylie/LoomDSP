//! The Preferences window and what its choices do
//! (`docs/design/preferences.md`).
//!
//! [`Rig`] is what belongs to the machine rather than the patch: the
//! preferences file, the device host, this run's flags, and the open MIDI
//! inputs. It outlives every engine: opening a patch or changing the device
//! builds a new engine and moves the rig over.

use crate::devices::{self, Host, InputPort, MidiPort, Playing, Wanted};
use crate::preferences::{AudioPrefs, BufferChoice, DeviceChoice, InputChoice, Preferences, RateChoice, Store};
use crate::style::{self, button};
use crate::{App, undo};
use iced::widget::{checkbox, column, container, opaque, pick_list, radio, row, rule, scrollable, space, text};
use iced::{Element, Length};
use loom_core::{EngineConfig, EngineParts, MidiInputId, OutputSafety, create_engine};
use loom_host_desktop::{AudioOutput, MidiInputInfo, Outputs, Recorder, STOP_LIMIT, SharedMidi};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Whether a host error describes a microphone permission refusal.
fn input_permission_denied(error: &str) -> bool {
    let lower = error.to_lowercase();
    lower.contains("denied") || lower.contains("permission") || lower.contains("not authorized")
}

/// Makes an input error actionable without reporting it from a callback.
fn input_error_message(error: &str) -> String {
    if input_permission_denied(error) {
        let place = if cfg!(target_os = "macos") {
            "System Settings, Privacy and Security, Microphone"
        } else if cfg!(target_os = "windows") {
            "Settings, Privacy and security, Microphone"
        } else {
            "your system's microphone privacy settings"
        };
        format!("Audio input: access to the microphone was denied. Allow LoomDSP in {place}.")
    } else {
        format!("Audio input: {error}.")
    }
}

/// A message for the Preferences window.
#[derive(Debug, Clone)]
pub enum Message {
    Open,
    Close,
    /// Read the device lists again.
    Refresh,
    Safety(OutputSafety),
    /// Show signal activity on connected ports.
    Activity(bool),
    Audio(Change),
    /// Choose the audio input device.
    Input(InputChoice),
    /// Turn a MIDI input on or off.
    Midi(String, bool),
    /// Start or stop learning the panic trigger.
    PanicLearn,
    /// Remove the panic trigger.
    PanicClear,
}

/// A change to one audio setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Device(DeviceChoice),
    Rate(RateChoice),
    Buffer(BufferChoice),
}

impl Change {
    fn apply(&self, audio: &mut AudioPrefs) {
        match self {
            Self::Device(device) => audio.device = device.clone(),
            Self::Rate(rate) => audio.sample_rate = *rate,
            Self::Buffer(buffer) => audio.buffer = *buffer,
        }
    }
}

/// What belongs to the machine, not the patch.
pub struct Rig {
    pub host: Rc<dyn Host>,
    pub store: Store,
    /// Device flags for this run. A flag is cleared when the window changes
    /// its setting: the window's choice is newer.
    pub flags: devices::Options,
    /// True while `--output-safety` sets the mode.
    pub safety_flag: bool,
    /// MIDI from every open input, into the current engine. `None` until
    /// the first engine is attached.
    pub midi_in: Option<SharedMidi>,
    pub ports: Vec<Box<dyn MidiPort>>,
    /// The open audio input, if the patch needs it.
    pub input: Option<Box<dyn InputPort>>,
    /// Shared drift warning history for the open input stream.
    pub(crate) input_tracker: Option<std::sync::Arc<crate::input_watch::Tracker>>,
    /// Last input open attempt, to avoid repeated device opens.
    input_attempted: Option<Instant>,
    /// Whether an input error has been reported in this run.
    input_error_reported: bool,
    /// Whether a microphone permission error has been reported in this run.
    input_permission_reported: bool,
    /// The last input error, so recovery does not overwrite another message.
    input_error_status: Option<String>,
    /// A named input was lost while playing; wait for that device to return.
    input_named_lost: bool,
    /// Whether the live-input feedback warning has been shown this run.
    pub(crate) input_warned: bool,
    /// The device playing now, when one is.
    pub playing: Option<Playing>,
    /// The window, while it is open, with the device lists read when it
    /// opened.
    pub window: Option<Window>,
    /// A mode change the engine's full queue refused, sent again on the
    /// next update.
    pub safety_pending: Option<OutputSafety>,
    /// Whether the last audio stop waited for the fade-out to end, for tests.
    pub last_stop: Option<bool>,
    /// The `--record` file. It belongs to the rig, not to one stream, so
    /// it records every engine the run plays (audio-qa.md, Recording the whole run).
    pub recorder: Option<Recorder>,
    /// While the panic trigger is learned: the serial of the last control
    /// change or note on before learn started. Only a later one sets it.
    pub panic_learn: Option<u32>,
    /// The MIDI inputs the last poll listed (device-changes.md). An input
    /// that is new since then is opened; one that is gone, or listed under
    /// a new identity, is closed.
    pub seen_midi: Vec<MidiInputInfo>,
    /// The last input id given out. Ids go round, so a closed input's id is
    /// not given to the next one at once, and its late events are dropped.
    pub last_input: u16,
    /// A lost audio output being watched for recovery (device-changes.md, Audio device loss).
    pub lost_audio: Option<LostAudio>,
    /// Every mode change sent to the engine, for tests.
    #[cfg(test)]
    pub sent: Vec<OutputSafety>,
}

/// What was playing when an audio device was lost, and reconnection attempts
/// (device-changes.md, Audio device loss).
#[derive(Debug, Clone)]
pub struct LostAudio {
    /// The device name that was lost.
    pub device: String,
    /// Whether the device was chosen as SystemDefault.
    pub was_system_default: bool,
    /// Number of consecutive reopen attempts that failed (0..10).
    pub attempts: u32,
    /// When the last poll / reopen attempt occurred.
    pub last_attempt: std::time::Instant,
    /// The error message if the last reopen attempt failed.
    pub last_error: Option<String>,
    /// The message to keep in the status bar while audio is lost.
    pub message: String,
}

/// The device lists the window shows.
pub struct Window {
    outputs: Result<Outputs, String>,
    inputs: Result<devices::Inputs, String>,
    midi: Result<Vec<String>, String>,
}

impl Window {
    fn read(host: &dyn Host) -> Self {
        Self {
            outputs: host.outputs(),
            inputs: host.audio_inputs(),
            midi: host.midi_inputs(),
        }
    }
}

impl Rig {
    pub fn new(host: Rc<dyn Host>, store: Store, flags: devices::Options, safety_flag: bool) -> Self {
        Self {
            host,
            store,
            flags,
            safety_flag,
            midi_in: None,
            ports: Vec::new(),
            input: None,
            input_tracker: None,
            input_attempted: None,
            input_error_reported: false,
            input_permission_reported: false,
            input_error_status: None,
            input_named_lost: false,
            input_warned: false,
            playing: None,
            window: None,
            safety_pending: None,
            last_stop: None,
            recorder: None,
            panic_learn: None,
            seen_midi: Vec::new(),
            last_input: 0,
            lost_audio: None,
            #[cfg(test)]
            sent: Vec::new(),
        }
    }

    /// Sends what `output` will play to the recording, if there is one.
    /// Returns the refusal when the output runs at another sample rate. The
    /// recording stays open: call [`Rig::end_recording`] once that output
    /// has started. A stream that fails to start must not end the recording,
    /// because the old device may then play again at the old rate.
    pub fn record_into(&self, output: &mut AudioOutput) -> Option<loom_host_desktop::HostError> {
        output.record(self.recorder.as_ref()?).err()
    }

    /// Ends the recording because a stream at another sample rate started
    /// (audio-qa.md, rule 6). Returns the status bar line, which is also
    /// printed to stderr.
    pub fn end_recording(&mut self, error: &loom_host_desktop::HostError) -> String {
        // Dropping the recorder closes the file with what it holds.
        self.recorder = None;
        let message = format!("The recording stopped: {error}.");
        eprintln!("loom-app: {message}");
        message
    }

    /// A rig with no file and the real devices, for a headless app and as
    /// a stand-in while a rig moves to a new app.
    pub fn empty() -> Self {
        Self::new(
            Rc::new(devices::System),
            Store::at(None),
            devices::Options::default(),
            false,
        )
    }

    /// Points MIDI at a new engine. Returns true the first time, when the
    /// inputs are still to be opened.
    pub fn attach(&mut self, sender: loom_core::MidiSender) -> bool {
        if let Some(midi) = &self.midi_in {
            *midi.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = sender;
            return false;
        }
        self.midi_in = Some(loom_host_desktop::share_midi(sender));
        true
    }

    /// An input id no open input uses, going round from the last one.
    fn free_input(&mut self) -> Option<MidiInputId> {
        let max = loom_core::MAX_MIDI_INPUTS;
        for step in 1..max {
            let id = (self.last_input + step - 1) % (max - 1) + 1;
            if self.ports.iter().all(|port| port.input().0 != id) {
                self.last_input = id;
                return Some(MidiInputId(id));
            }
        }
        None
    }

    /// The input that would be used for this run, without opening a stream.
    fn effective_input<'a>(&'a self, inputs: &'a devices::Inputs) -> Option<&'a str> {
        if self.flags.no_input {
            return None;
        }
        if let Some(input) = self.input.as_ref().filter(|input| input.is_live()) {
            return Some(input.name());
        }
        if let Some(wanted) = &self.flags.input {
            let wanted = wanted.to_lowercase();
            return inputs
                .devices
                .iter()
                .find(|name| name.to_lowercase().contains(&wanted))
                .map(String::as_str);
        }
        match &self.store.saved().audio.input {
            InputChoice::Off => None,
            InputChoice::SystemDefault => inputs.default.as_deref(),
            InputChoice::Named(name) if inputs.devices.contains(name) => Some(name),
            InputChoice::Named(_) if self.input_named_lost => None,
            InputChoice::Named(_) => inputs.default.as_deref(),
        }
    }

    /// The audio choice the window shows: the saved one, with the flags for
    /// this run on top.
    pub fn shown_audio(&self) -> AudioPrefs {
        let mut audio = self.store.saved().audio.clone();
        if self.flags.no_input {
            audio.input = InputChoice::Off;
        } else if self.flags.input.is_some()
            && let Some(input) = &self.input
        {
            audio.input = InputChoice::Named(input.name().to_string());
        }
        if self.flags.device.is_some()
            && let Some(playing) = &self.playing
        {
            audio.device = DeviceChoice::Named(playing.device.clone());
        }
        if let Some(rate) = self.flags.sample_rate {
            audio.sample_rate = RateChoice::Hz(rate);
        }
        if let Some(frames) = self.flags.buffer {
            audio.buffer = BufferChoice::Frames(frames);
        }
        audio
    }

    /// What to open now: the saved choice with the flags on top.
    pub fn wanted(&self) -> Wanted {
        Wanted::new(&self.store.saved().audio, &self.flags)
    }

    /// Saves `preferences`. Returns the status bar line when it fails.
    fn save(&mut self, preferences: Preferences) -> Option<String> {
        self.store.save(preferences).err()
    }
}

/// Opens the audio output for a run: none with `--no-audio`, a timer with
/// `--fake-audio`, and otherwise the saved device with the flags on top.
/// Returns the output and status bar lines about fallbacks.
pub fn open_output(rig: &Rig, no_audio: bool, fake_audio: bool) -> (Result<AudioOutput, String>, Vec<String>) {
    if no_audio {
        (Err("audio off".to_string()), Vec::new())
    } else if fake_audio {
        let rate = rig.flags.sample_rate.unwrap_or(48_000);
        let block = rig.flags.buffer.unwrap_or(256);
        (Ok(AudioOutput::clock(rate, block)), Vec::new())
    } else {
        devices::open_at_start(&*rig.host, &rig.wanted())
    }
}

/// The preferences file for a run: `--preferences`, or the per-user file.
/// Tests never read the real file.
pub fn open_store(flags: &devices::Options) -> (Store, Vec<String>) {
    let path = match &flags.preferences {
        Some(path) => Some(path.clone()),
        None if cfg!(test) => None,
        None => crate::preferences::default_path(),
    };
    Store::open(path)
}

impl App {
    /// Handles a message for the Preferences window.
    pub(crate) fn preferences(&mut self, message: Message) {
        match message {
            Message::Open => {
                self.rig.window = Some(Window::read(&*self.rig.host));
                self.browser = None;
            }
            Message::Close => {
                self.rig.window = None;
                // Learning a panic trigger needs the window open to show
                // "Waiting for MIDI...", so closing it cancels the learn.
                self.rig.panic_learn = None;
            }
            Message::Refresh => {
                if self.rig.window.is_some() {
                    self.rig.window = Some(Window::read(&*self.rig.host));
                }
            }
            Message::Safety(mode) => self.choose_output_safety(mode),
            Message::Activity(on) => self.choose_activity(on),
            Message::Audio(change) => self.change_audio(change),
            Message::Input(choice) => self.choose_audio_input(choice),
            Message::Midi(name, on) => self.set_midi_input(&name, on),
            Message::PanicLearn => self.learn_panic(),
            Message::PanicClear => self.set_panic_pref(None),
        }
    }

    /// The change a control channel `audio` command asks for, checked
    /// against what the window offers (control-channel.md, rule S2). Give
    /// exactly one of the three. The device lists are the window's, or read
    /// now when the window is closed, as opening it would.
    pub(crate) fn control_audio_change(
        &self,
        device: Option<&str>,
        rate: Option<Option<u32>>,
        buffer: Option<Option<u32>>,
    ) -> Result<Change, String> {
        if self.stream.without_device() {
            let flag = if self.stream.playback().is_off() {
                "--no-audio"
            } else {
                "--fake-audio"
            };
            return Err(format!("there are no audio devices to choose with {flag}"));
        }
        let read;
        let window = match &self.rig.window {
            Some(window) => window,
            None => {
                read = Window::read(&*self.rig.host);
                &read
            }
        };
        let outputs = window
            .outputs
            .as_ref()
            .map_err(|e| format!("could not list the audio outputs: {e}"))?;
        let shown = self.rig.shown_audio();
        let info = || {
            let name = match &shown.device {
                DeviceChoice::SystemDefault => outputs.default.as_ref(),
                DeviceChoice::Named(name) => Some(name),
            };
            name.and_then(|name| outputs.devices.iter().find(|d| d.name == *name))
                .ok_or_else(|| "the chosen device is not connected; choose a device first".to_string())
        };
        let offered = |values: Vec<u32>| values.iter().map(u32::to_string).collect::<Vec<_>>().join(", ");
        match (device, rate, buffer) {
            (Some("default"), None, None) => Ok(Change::Device(DeviceChoice::SystemDefault)),
            (Some(wanted), None, None) => {
                let names: Vec<String> = outputs.devices.iter().map(|d| d.name.clone()).collect();
                devices::match_name(&names, wanted, "audio output")
                    .map(|name| Change::Device(DeviceChoice::Named(name)))
            }
            (None, Some(None), None) => Ok(Change::Rate(RateChoice::DeviceDefault)),
            (None, Some(Some(hz)), None) => {
                let rates = devices::offered_rates(info()?);
                if rates.contains(&hz) {
                    Ok(Change::Rate(RateChoice::Hz(hz)))
                } else {
                    Err(format!(
                        "{hz} Hz is not offered; rates: \"default\", {}",
                        offered(rates)
                    ))
                }
            }
            (None, None, Some(None)) => Ok(Change::Buffer(BufferChoice::DeviceDefault)),
            (None, None, Some(Some(frames))) => {
                let sizes = devices::offered_buffers(info()?);
                if sizes.contains(&frames) {
                    Ok(Change::Buffer(BufferChoice::Frames(frames)))
                } else {
                    Err(format!(
                        "{frames} frames is not offered; sizes: \"default\", {}",
                        offered(sizes)
                    ))
                }
            }
            _ => Err("give exactly one of `device`, `rate`, or `buffer`".to_string()),
        }
    }

    /// The window's device lists, read now when it is closed, so a control
    /// channel change is checked as the window checks it. Returns true when
    /// it read them, and the caller then calls [`App::forget_device_lists`].
    pub(crate) fn read_device_lists(&mut self) -> bool {
        if self.rig.window.is_some() {
            return false;
        }
        self.rig.window = Some(Window::read(&*self.rig.host));
        true
    }

    /// Drops the lists [`App::read_device_lists`] read, so the window stays
    /// closed.
    pub(crate) fn forget_device_lists(&mut self) {
        self.rig.window = None;
    }

    /// The MIDI input a control channel `midi` command names (rule S2).
    pub(crate) fn control_midi_input(&self, wanted: &str) -> Result<String, String> {
        let names = match &self.rig.window {
            Some(window) => window.midi.clone(),
            None => self.rig.host.midi_inputs(),
        }
        .map_err(|e| format!("could not list the MIDI inputs: {e}"))?;
        devices::match_name(&names, wanted, "MIDI input")
    }

    /// The MIDI inputs open now, by name.
    pub(crate) fn open_midi_inputs(&self) -> Vec<String> {
        self.rig.ports.iter().map(|port| port.name().to_string()).collect()
    }

    /// Opens or closes the window, for Cmd+, or Ctrl+,.
    pub(crate) fn toggle_preferences(&mut self) {
        let message = if self.rig.window.is_some() {
            Message::Close
        } else {
            Message::Open
        };
        self.preferences(message);
    }

    /// Picks the output safety mode (preferences.md, Changing the mode at
    /// run time).
    fn choose_output_safety(&mut self, mode: OutputSafety) {
        if !self.stream.set_output_safety(mode) {
            return;
        }
        self.rig.safety_flag = false;
        let mut preferences = self.rig.store.saved().clone();
        preferences.output_safety = mode;
        if let Some(error) = self.rig.save(preferences) {
            self.alerts.say(error);
        }
        // With no device there is no engine playing to tell. The next
        // engine is built with the new mode.
        if self.stream.is_playing() {
            self.rig.safety_pending = Some(mode);
            self.send_pending_safety();
        }
    }

    /// Saves the port activity choice and updates the probes in this engine.
    fn choose_activity(&mut self, on: bool) {
        if self.rig.store.saved().show_activity == on {
            return;
        }
        let mut preferences = self.rig.store.saved().clone();
        preferences.show_activity = on;
        if let Some(error) = self.rig.save(preferences) {
            self.alerts.say(error);
        }
        self.live.set_activity(on);
        self.carrying.clear();
        self.sync_live();
        if let Err(error) = self.controller.commit() {
            self.alerts.say(format!("error: {error}"));
        }
    }

    /// Sends a mode change the queue refused before. Runs on every update.
    pub(crate) fn send_pending_safety(&mut self) {
        let Some(mode) = self.rig.safety_pending else { return };
        if self.controller.set_output_safety(mode).is_ok() {
            self.rig.safety_pending = None;
            #[cfg(test)]
            self.rig.sent.push(mode);
        }
    }

    /// Keeps the input stream in step with the patch and the current output.
    pub(crate) fn sync_audio_input(&mut self) {
        let needed = self.stream.is_playing()
            && !self.stream.playback().is_fake()
            && !self.rig.flags.no_input
            && (self.rig.flags.input.is_some() || !matches!(self.rig.store.saved().audio.input, InputChoice::Off))
            && self
                .controller
                .graph()
                .nodes()
                .any(|(_, node)| node.descriptor.id == "loom.io.audio-in");
        if !needed {
            if self.rig.input.take().is_some() {
                let _ = self.controller.set_audio_input(None);
            }
            self.alerts.set_input_drift(None);
            self.rig.input_tracker = None;
            self.rig.input_attempted = None;
            self.rig.input_named_lost = false;
            return;
        }
        if self.rig.host.audio_input_permission_denied() {
            if self.rig.input.take().is_some() {
                let _ = self.controller.set_audio_input(None);
            }
            self.rig.input_tracker = None;
            self.alerts.set_input_drift(None);
            if !self.rig.input_permission_reported {
                self.alerts.say(input_error_message("microphone permission denied"));
                self.rig.input_error_status = Some(self.alerts.status().to_string());
                self.rig.input_permission_reported = true;
            }
            return;
        }
        if self.rig.input.as_ref().is_some_and(|input| input.is_live()) {
            let live = self.controller.graph().nodes().any(|(id, node)| {
                node.descriptor.id == "loom.io.audio-in"
                    && node
                        .params
                        .get(2)
                        .is_some_and(|&mute| self.controller.modulated(id, 2).unwrap_or(mute) < 0.5)
                    && self.controller.display(id, 0).is_some_and(|active| active >= 0.5)
            });
            if live && !self.rig.input_warned {
                self.alerts
                    .say("Audio input is live. Use headphones, or keep the gain low, to avoid feedback.");
                self.rig.input_warned = true;
            }
            return;
        }
        if let Some(input) = self.rig.input.take() {
            let name = input.name().to_string();
            drop(input);
            self.rig.input_tracker = None;
            self.alerts.set_input_drift(None);
            let _ = self.controller.set_audio_input(None);
            self.rig.input_attempted = None;
            self.rig.input_named_lost =
                self.rig.flags.input.is_some() || matches!(self.rig.store.saved().audio.input, InputChoice::Named(_));
            if !self.rig.input_error_reported {
                self.alerts
                    .say(format!("Audio input: \"{name}\" was lost. Trying to reopen it."));
                self.rig.input_error_status = Some(self.alerts.status().to_string());
                self.rig.input_error_reported = true;
            }
        }
        let now = Instant::now();
        if self
            .rig
            .input_attempted
            .is_some_and(|at| now.duration_since(at) < Duration::from_secs(1))
        {
            return;
        }
        // With the output's default buffer, wait for its first callback so
        // the ring is sized for the actual device rather than max_block.
        let buffer = self
            .rig
            .playing
            .as_ref()
            .and_then(|playing| playing.buffer)
            .or_else(|| {
                self.stream
                    .stats()
                    .map(|stats| stats.snapshot().largest_buffer)
                    .filter(|&frames| frames > 0)
                    .and_then(|frames| u32::try_from(frames).ok())
            });
        let Some(buffer) = buffer else { return };
        self.rig.input_attempted = Some(now);
        let mut options = loom_host_desktop::AudioOptions {
            sample_rate: Some(self.stream.sample_rate() as u32),
            buffer_size: Some(buffer),
            ..Default::default()
        };
        let mut fallback = None;
        if let Some(wanted) = &self.rig.flags.input {
            options.device = Some(wanted.clone());
        } else if let InputChoice::Named(name) = &self.rig.store.saved().audio.input {
            if !self
                .rig
                .host
                .audio_inputs()
                .is_ok_and(|inputs| !inputs.devices.contains(name))
            {
                options.device = Some(name.clone());
                options.exact = true;
            } else {
                if self.rig.input_named_lost {
                    return;
                }
                fallback = Some(name.clone());
            }
        }
        match self.rig.host.open_audio_input(&options, buffer as usize) {
            Ok((port, reader)) => match self.controller.set_audio_input(Some(reader)) {
                Ok(()) => {
                    let stats = self.controller.stats();
                    let counts = (
                        stats.input_underruns.load(std::sync::atomic::Ordering::Relaxed),
                        stats.input_overruns.load(std::sync::atomic::Ordering::Relaxed),
                    );
                    self.rig.input_tracker = Some(std::sync::Arc::new(crate::input_watch::Tracker::new(counts)));
                    self.rig.input = Some(port);
                    self.rig.input_named_lost = false;
                    self.rig.input_error_reported = false;
                    let error_was_shown = self
                        .rig
                        .input_error_status
                        .take()
                        .is_some_and(|status| status == self.alerts.status());
                    if let Some(name) = fallback {
                        self.alerts.say(format!(
                            "Audio input: \"{name}\" is not available. Using the system default."
                        ));
                    } else if error_was_shown && let Some(input) = self.rig.input.as_ref() {
                        let name = input.name();
                        self.alerts.say(format!("Audio input: \"{name}\" is back."));
                    }
                }
                Err(error) => self.alerts.say(format!("Audio input: {error}.")),
            },
            Err(error) => {
                let reported = if input_permission_denied(&error) {
                    &mut self.rig.input_permission_reported
                } else {
                    &mut self.rig.input_error_reported
                };
                if !*reported {
                    self.alerts.say(input_error_message(&error));
                    self.rig.input_error_status = Some(self.alerts.status().to_string());
                    *reported = true;
                }
            }
        }
    }

    /// Changes the input without restarting the output stream.
    fn choose_audio_input(&mut self, choice: InputChoice) {
        self.alerts.set_input_drift(None);
        self.rig.input_tracker = None;
        self.rig.flags.input = None;
        self.rig.flags.no_input = false;
        let mut preferences = self.rig.store.saved().clone();
        preferences.audio.input = choice;
        let error = self.rig.save(preferences);
        if self.rig.input.take().is_some() {
            let _ = self.controller.set_audio_input(None);
        }
        self.rig.input_attempted = None;
        self.rig.input_error_reported = false;
        self.rig.input_permission_reported = false;
        self.rig.input_error_status = None;
        self.rig.input_named_lost = false;
        self.sync_audio_input();
        if let Some(error) = error {
            self.alerts.say(error);
        }
    }

    /// Changes the device, rate, or buffer size (preferences.md, What a
    /// change does).
    fn change_audio(&mut self, change: Change) {
        let shown = self.rig.shown_audio();
        let mut play = shown.clone();
        change.apply(&mut play);
        if play == shown {
            return;
        }
        let mut saved = self.rig.store.saved().clone();
        change.apply(&mut saved.audio);
        // A new device starts from its own rate and buffer size when it
        // does not offer the ones chosen for the old one (Rule D4).
        if let Change::Device(_) = change
            && let Some(info) = self.device_info(&play.device)
        {
            for audio in [&mut play, &mut saved.audio] {
                if audio
                    .sample_rate
                    .hz()
                    .is_some_and(|hz| !devices::offered_rates(&info).contains(&hz))
                {
                    audio.sample_rate = RateChoice::DeviceDefault;
                }
                if audio
                    .buffer
                    .frames()
                    .is_some_and(|f| !devices::offered_buffers(&info).contains(&f))
                {
                    audio.buffer = BufferChoice::DeviceDefault;
                }
            }
        }

        if !self.stream.without_device() {
            // The new device opens first: if it fails, the old one keeps
            // playing and nothing is saved (Rule D5).
            let output = match devices::open_choice(&*self.rig.host, &play) {
                Ok(output) => output,
                Err(e) => {
                    self.alerts.say(self.could_not_use(&play, &e));
                    return;
                }
            };
            if let Err(e) = self.restart_audio(output) {
                self.alerts.say(e);
                return;
            }
            self.rig.lost_audio = None;
        } else {
            self.rig.lost_audio = None;
        }
        match change {
            Change::Device(_) => self.rig.flags.device = None,
            Change::Rate(_) => self.rig.flags.sample_rate = None,
            Change::Buffer(_) => self.rig.flags.buffer = None,
        }
        // The flags that are left still set this run; what is saved is the
        // player's choice.
        if let Some(error) = self.rig.save(saved) {
            self.alerts.say(error);
        }
    }

    /// `Could not use "MOTU M4" at 192000 Hz: <reason>. Still playing on
    /// "MacBook Pro Speakers".`
    fn could_not_use(&self, choice: &AudioPrefs, reason: &str) -> String {
        let name = match &choice.device {
            DeviceChoice::Named(name) => name.clone(),
            DeviceChoice::SystemDefault => self
                .default_device_name()
                .unwrap_or_else(|| "the system default".into()),
        };
        let rate = choice
            .sample_rate
            .hz()
            .or_else(|| self.device_info(&choice.device).map(|info| info.default_rate));
        let at = rate.map(|hz| format!(" at {hz} Hz")).unwrap_or_default();
        let still = match &self.rig.playing {
            Some(playing) => format!(" Still playing on \"{}\".", playing.device),
            None => String::new(),
        };
        format!("Could not use \"{name}\"{at}: {reason}.{still}")
    }

    fn outputs(&self) -> Option<&Outputs> {
        self.rig.window.as_ref().and_then(|w| w.outputs.as_ref().ok())
    }

    fn default_device_name(&self) -> Option<String> {
        self.outputs().and_then(|o| o.default.clone())
    }

    /// What a device offers, from the lists the window read.
    fn device_info(&self, device: &DeviceChoice) -> Option<loom_host_desktop::OutputInfo> {
        let outputs = self.outputs()?;
        let name = match device {
            DeviceChoice::SystemDefault => outputs.default.as_ref()?,
            DeviceChoice::Named(name) => name,
        };
        outputs.devices.iter().find(|d| d.name == *name).cloned()
    }

    /// Builds a new engine from the patch as it is now, with every unsaved
    /// edit, at the new device's rate, fades the old one out, and plays the
    /// new one (preferences.md, Restarting audio). Module state starts
    /// again; the patch, undo history, selection, view, MIDI mappings, and
    /// MIDI inputs are kept (Rule D6). On an error before the old engine
    /// stops, nothing changes. If the new stream then fails to start, the
    /// old device plays again, so a failed change leaves sound playing
    /// (Rule D5).
    fn restart_audio(&mut self, output: AudioOutput) -> Result<(), String> {
        self.finish_edits();
        let parts = self
            .rebuilt_engine(output.sample_rate() as f32)
            .map_err(|e| format!("Could not restart audio: {e}. The audio is unchanged."))?;
        let previous = self.rig.playing.clone();
        self.stop_audio();
        let device = output.name().to_string();
        let error = match self.play(output, parts) {
            Ok(()) => return Ok(()),
            Err(e) => e,
        };
        // Back to the device that played before, as it was.
        let restored = previous.as_ref().and_then(|old| {
            let options = loom_host_desktop::AudioOptions {
                device: Some(old.device.clone()),
                exact: true,
                sample_rate: Some(old.rate),
                buffer_size: old.buffer,
            };
            let output = self.rig.host.open(&options).ok()?;
            let parts = self.rebuilt_engine(output.sample_rate() as f32).ok()?;
            self.play(output, parts).ok()
        });
        Err(match (restored, previous) {
            (Some(()), Some(old)) => {
                format!(
                    "Could not use \"{device}\": {error}. Still playing on \"{}\".",
                    old.device
                )
            }
            _ => format!("Could not use \"{device}\": {error}. Audio is off."),
        })
    }

    /// A new engine at `rate` holding the patch as it is now. The patch is
    /// replayed as one undo step from nothing, so module and assignment IDs
    /// stay the same and the history still applies.
    fn rebuilt_engine(&self, rate: f32) -> Result<EngineParts, loom_core::LoomError> {
        let config = EngineConfig {
            output_safety: self.stream.output_safety(),
            ..EngineConfig::with_sample_rate(rate)
        };
        let mut parts = create_engine(crate::registry(self.test_modules), config);
        let controller = &mut parts.controller;
        let current = undo::Snapshot::take(&self.controller, &self.positions);
        let empty = undo::Snapshot::take(controller, &BTreeMap::new());
        if let Some(step) = undo::diff(&empty, &current, &BTreeSet::new()) {
            undo::apply(&step, true, controller, &mut self.positions.clone())?;
        }
        // The transport keeps its tempo, time signature, and whether it
        // runs. Its position starts at beat 0.
        let transport = self.controller.transport();
        controller.set_tempo(transport.tempo)?;
        controller.set_time_signature(transport.time_signature)?;
        controller.set_running(transport.running)?;
        controller.commit()?;
        Ok(parts)
    }

    /// Gives the app an idle engine after a stream failed to start. The
    /// failed stream dropped the engine it was given, so nothing would drain
    /// the controller's queue, and edits would fail once it is full (#220).
    /// The new engine holds the patch as it is now, at the current rate.
    /// Does nothing while a device plays or an idle engine is in place.
    pub(crate) fn keep_engine_idle(&mut self) {
        if self.stream.has_engine() {
            return;
        }
        let Ok(parts) = self.rebuilt_engine(self.stream.sample_rate()) else {
            return;
        };
        let EngineParts {
            controller,
            engine,
            midi,
            telemetry,
        } = parts;
        self.rig.attach(midi);
        self.generation += 1;
        self.controller = controller;
        // The new engine needs the panic trigger and the open MIDI inputs,
        // as in `play`, so panic and held-note release still work.
        self.send_panic_trigger();
        self.connect_midi(false, false);
        self.monitor = crate::monitor::Monitor::new(telemetry, self.controller.config().sample_rate, self.generation);
        self.traces = loom_canvas::ScopeTraces::new();
        self.live = crate::live::Live::showing_activity();
        self.live.set_activity(self.rig.store.saved().show_activity);
        self.stream.keep_idle(engine);
        // The stopped engine's live values are gone; show the new one's.
        self.refresh_values();
    }

    /// Puts a new engine in place of the old one, which has stopped, and
    /// plays it on `output`. On an error the app is left with no audio,
    /// and an idle engine that takes edits.
    fn play(&mut self, mut output: AudioOutput, parts: EngineParts) -> Result<(), String> {
        let EngineParts {
            controller,
            engine,
            midi,
            telemetry,
        } = parts;
        self.rig.attach(midi);
        let playing = Playing::of(&output);
        let label = format!("{} at {} Hz", output.name(), output.sample_rate());
        self.generation += 1;
        self.controller = controller;
        self.send_panic_trigger();
        self.connect_midi(false, false);
        self.monitor = crate::monitor::Monitor::new(telemetry, self.controller.config().sample_rate, self.generation);
        self.traces = loom_canvas::ScopeTraces::new();
        self.live = crate::live::Live::showing_activity();
        self.live.set_activity(self.rig.store.saved().show_activity);
        self.lights.clear();
        self.meter_levels.clear();
        self.stream.prepare(output.sample_rate() as f32);
        self.holding = false;
        let refused = self.rig.record_into(&mut output);
        let started = self.rig.host.start(output, engine);
        // Set on success; on an error this returns early.
        let record_error;
        match started {
            Ok(audio) => {
                record_error = refused.map(|e| self.rig.end_recording(&e));
                self.stream.started(audio, label);
                self.rig.playing = Some(playing);
                self.midi = self.rig.midi_in.clone();
            }
            Err(refused) => {
                let (engine, e) = *refused;
                self.stream.refused(engine, &e);
                self.rig.playing = None;
                self.midi = None;
                return Err(e);
            }
        }
        // Probes for the meters on screen, in the new engine.
        self.commit();
        self.refresh_values();
        // After the commit, which sets the status line too.
        if let Some(error) = record_error {
            self.alerts.say(error);
        }
        Ok(())
    }

    /// Extracts the underlying reason from an audio restart error message for
    /// reporting in the status bar.
    pub(crate) fn unwrap_reopen_error(raw: &str) -> &str {
        let mut s = raw;
        if let Some(rest) = s.strip_prefix("Could not use \"")
            && let Some(idx) = rest.find("\": ")
        {
            s = &rest[idx + 3..];
        }
        let without_suffix = s.strip_suffix(". Audio is off.").unwrap_or(s);
        without_suffix.strip_suffix('.').unwrap_or(without_suffix)
    }

    /// Stops audio immediately without fading out. Used when the audio device
    /// was lost or disconnected (device-changes.md, Audio device loss).
    pub(crate) fn stop_audio_immediate(&mut self) {
        self.alerts.set_input_drift(None);
        if let Some(audio) = self.stream.take_running() {
            audio.stop_immediate();
        }
        self.rig.input.take();
        self.rig.input_tracker = None;
        self.rig.input_attempted = None;
        self.rig.playing = None;
    }

    /// Handles loss of the active audio output (device-changes.md, Audio device loss).
    pub(crate) fn handle_audio_loss(&mut self, _reason: &str) {
        // Rule 3: The engine is never rebuilt twice for one loss. A second error
        // from the same dead stream is ignored.
        if !self.stream.is_playing() && self.rig.lost_audio.is_some() {
            return;
        }
        if !self.stream.is_playing() && self.rig.playing.is_none() {
            return;
        }
        let was_system_default = matches!(self.rig.wanted().device, devices::Pick::SystemDefault);
        let old_name = self
            .rig
            .playing
            .as_ref()
            .map(|p| p.device.clone())
            .or_else(|| match &self.rig.store.saved().audio.device {
                DeviceChoice::Named(n) => Some(n.clone()),
                DeviceChoice::SystemDefault => None,
            })
            .unwrap_or_else(|| "audio device".into());

        self.stop_audio_immediate();
        self.midi = None;
        self.keep_engine_idle();

        if was_system_default {
            // System default: at once, open the current system default
            let mut switched = false;
            if let Ok(outputs) = self.rig.host.outputs()
                && let Some(new_default) = outputs.default
            {
                let options = loom_host_desktop::AudioOptions {
                    device: Some(new_default.clone()),
                    exact: true,
                    sample_rate: None,
                    buffer_size: None,
                };
                if let Ok(output) = self.rig.host.open(&options)
                    && self.restart_audio(output).is_ok()
                {
                    self.alerts.say(format!(
                        "Audio: \"{old_name}\" was lost. Now playing on \"{new_default}\"."
                    ));
                    self.rig.lost_audio = None;
                    switched = true;
                }
            }
            if !switched {
                let message = format!("Audio: \"{old_name}\" was lost.");
                self.alerts.say(message.clone());
                self.rig.lost_audio = Some(LostAudio {
                    device: old_name,
                    was_system_default: true,
                    attempts: 0,
                    last_attempt: Instant::now(),
                    last_error: None,
                    message,
                });
            }
        } else {
            // Named device: plays nothing, keeps engine idle
            let message =
                format!("Audio: \"{old_name}\" was lost. Plug it in again, or choose another device in Preferences.");
            self.alerts.say(message.clone());
            self.rig.lost_audio = Some(LostAudio {
                device: old_name,
                was_system_default: false,
                attempts: 0,
                last_attempt: Instant::now(),
                last_error: None,
                message,
            });
        }
    }

    /// Checks for audio device loss, silent stalls, and device reconnection
    /// (device-changes.md, Audio device loss).
    pub(crate) fn poll_audio(&mut self) {
        self.check_audio(Instant::now());
        self.sync_audio_input();
    }

    /// Checks for audio device loss, silent stalls, and device reconnection at `now`.
    pub(crate) fn check_audio(&mut self, now: Instant) {
        if self.stream.without_device() {
            return;
        }

        // 1. If audio is running, check for loss (dead stream, stall > 1s, missing device)
        if self.stream.is_playing() {
            let mut callbacks_advancing = false;
            if let Some(stats) = self.stream.stats().cloned() {
                let snap = stats.snapshot();
                if snap.is_dead {
                    self.handle_audio_loss("the audio device reported an error");
                    return;
                }
                if self.stream.observe_callbacks(snap.callbacks, now) {
                    callbacks_advancing = true;
                } else if self.stream.stalled(now) {
                    let mut rate_changed = false;
                    let device_default = matches!(
                        self.rig.store.saved().audio.sample_rate,
                        crate::preferences::RateChoice::DeviceDefault
                    ) && self.rig.flags.sample_rate.is_none();
                    let current = self.rig.playing.as_ref().map(|p| (p.device.clone(), p.rate));
                    if device_default
                        && let Some((device_name, current_rate)) = current
                        && let Ok(outputs) = self.rig.host.outputs()
                        && let Some(info) = outputs.devices.iter().find(|d| d.name == device_name)
                        && info.default_rate != current_rate
                    {
                        let new_rate = info.default_rate;
                        let mut choice = self.rig.store.saved().audio.clone();
                        choice.sample_rate = crate::preferences::RateChoice::Hz(new_rate);
                        if let Ok(output) = devices::open_choice(&*self.rig.host, &choice)
                            && self.restart_audio(output).is_ok()
                        {
                            self.alerts.say(format!(
                                "Audio: \"{device_name}\" sample rate changed to {new_rate} Hz."
                            ));
                            rate_changed = true;
                        }
                    }
                    if !rate_changed {
                        self.handle_audio_loss("the device stopped calling back");
                        return;
                    }
                }
            }
            if !callbacks_advancing {
                let playing_name = self.rig.playing.as_ref().map(|p| p.device.clone());
                if let Some(device_name) = playing_name
                    && let Ok(outputs) = self.rig.host.outputs()
                    && !outputs.devices.iter().any(|d| d.name == device_name)
                {
                    self.handle_audio_loss(&format!("\"{device_name}\" was unplugged"));
                    return;
                }
            }
        }

        // 2. If audio was lost, watch and attempt recovery
        let Some(mut lost) = self.rig.lost_audio.take() else {
            return;
        };

        if lost.was_system_default {
            if now.saturating_duration_since(lost.last_attempt) >= Duration::from_secs(1) {
                lost.last_attempt = now;
                if lost.attempts < 10 {
                    if let Ok(outputs) = self.rig.host.outputs()
                        && let Some(new_default) = outputs.default
                    {
                        let options = loom_host_desktop::AudioOptions {
                            device: Some(new_default.clone()),
                            exact: true,
                            sample_rate: None,
                            buffer_size: None,
                        };
                        match self.rig.host.open(&options) {
                            Ok(output) => match self.restart_audio(output) {
                                Ok(()) => {
                                    self.alerts.say(format!(
                                        "Audio: \"{}\" was lost. Now playing on \"{new_default}\".",
                                        lost.device
                                    ));
                                    self.rig.lost_audio = None;
                                    return;
                                }
                                Err(e) => {
                                    lost.attempts += 1;
                                    lost.last_error = Some(e);
                                }
                            },
                            Err(e) => {
                                lost.attempts += 1;
                                lost.last_error = Some(e);
                            }
                        }
                    }
                    if lost.attempts >= 10 {
                        let raw = lost.last_error.as_deref().unwrap_or("could not reopen device");
                        let reason = Self::unwrap_reopen_error(raw);
                        let message = format!(
                            "Audio: could not reopen \"{}\": {reason}. Choose a device in Preferences.",
                            lost.device
                        );
                        self.alerts.say(message.clone());
                        lost.message = message;
                    }
                }
            }
            self.rig.lost_audio = Some(lost);
        } else {
            // Named device: repeats once a second for up to 10 tries
            if now.saturating_duration_since(lost.last_attempt) >= Duration::from_secs(1) {
                lost.last_attempt = now;
                if lost.attempts < 10
                    && let Ok(outputs) = self.rig.host.outputs()
                    && outputs.devices.iter().any(|d| d.name == lost.device)
                {
                    let saved = self.rig.store.saved().audio.clone();
                    match devices::open_choice(&*self.rig.host, &saved) {
                        Ok(output) => match self.restart_audio(output) {
                            Ok(()) => {
                                self.alerts.say(format!("Audio: \"{}\" is back.", lost.device));
                                self.rig.lost_audio = None;
                                return;
                            }
                            Err(e) => {
                                lost.attempts += 1;
                                lost.last_error = Some(e);
                            }
                        },
                        Err(e) => {
                            lost.attempts += 1;
                            lost.last_error = Some(e);
                        }
                    }
                    if lost.attempts >= 10 {
                        let raw = lost.last_error.as_deref().unwrap_or("could not start stream");
                        let reason = Self::unwrap_reopen_error(raw);
                        let message = format!(
                            "Audio: could not reopen \"{}\": {reason}. Choose a device in Preferences.",
                            lost.device
                        );
                        self.alerts.say(message.clone());
                        lost.message = message;
                    }
                }
            }
            self.rig.lost_audio = Some(lost);
        }
    }

    /// Stops audio without a click: the engine fades out, and the stream
    /// stops after two more silent buffers, or at the stop limit (100 ms,
    /// longer for large buffers; output safety, rule 14). Used by Open, New, quit, and device changes.
    pub(crate) fn stop_audio(&mut self) {
        self.alerts.set_input_drift(None);
        if let Some(audio) = self.stream.take_running() {
            // A full queue cannot fade: the limit then stops the stream.
            let _ = self.controller.fade_out();
            self.rig.last_stop = Some(audio.stop(STOP_LIMIT));
        }
        self.rig.input.take();
        self.rig.input_tracker = None;
        self.rig.input_attempted = None;
        self.rig.playing = None;
    }

    /// Opens a MIDI input: the engine first starts tracking its id, then
    /// the host connects it. On an error nothing stays open.
    fn open_input(&mut self, name: &str) -> Result<(), String> {
        let midi = self.rig.midi_in.clone().ok_or("no engine to play")?;
        let input = self.rig.free_input().ok_or("too many MIDI inputs are open")?;
        self.controller.open_midi_input(input).map_err(|e| e.to_string())?;
        match self.rig.host.open_midi(name, midi, input) {
            Ok(port) => {
                self.rig.ports.push(port);
                Ok(())
            }
            Err(e) => {
                let _ = self.controller.release_midi_input(input);
                Err(e)
            }
        }
    }

    /// Closes open input `index`, and has the engine release every note it
    /// held (device-changes.md, Held notes belong to an input).
    fn close_input(&mut self, index: usize) {
        let port = self.rig.ports.remove(index);
        let input = port.input();
        port.close();
        let _ = self.controller.release_midi_input(input);
    }

    /// After a new engine is attached: tracks the open inputs on it, or the
    /// first time opens the inputs the preferences and flags choose, when
    /// `connect` is true. Returns a status bar line about an input that
    /// failed to open; the others still play.
    pub(crate) fn connect_midi(&mut self, first: bool, connect: bool) -> Option<String> {
        for input in self.rig.ports.iter().map(|port| port.input()).collect::<Vec<_>>() {
            let _ = self.controller.open_midi_input(input);
        }
        if !(first && connect) {
            return None;
        }
        let list = match self.rig.host.midi_list() {
            Ok(list) => list,
            Err(e) => return Some(format!("MIDI: {e}")),
        };
        let names: Vec<String> = list.iter().map(|input| input.name.clone()).collect();
        self.rig.seen_midi = list;
        let mut error = None;
        for name in devices::midi_at_start(&names, &self.rig.store.saved().midi.off, &self.rig.flags) {
            if let Err(e) = self.open_input(&name) {
                error.get_or_insert(format!("MIDI: {e}"));
            }
        }
        error
    }

    /// Follows MIDI inputs plugged in and out (device-changes.md, MIDI
    /// inputs). Runs four times a second. A new input that the preferences
    /// and flags allow is opened. An input that is gone, or is listed under
    /// a new identity, is closed and its notes released; one with a new
    /// identity is opened again as a new connection. The window's list
    /// follows too.
    pub(crate) fn poll_midi(&mut self) {
        let Ok(list) = self.rig.host.midi_list() else { return };
        if let Some(window) = &mut self.rig.window {
            window.midi = Ok(list.iter().map(|input| input.name.clone()).collect());
        }
        if self.rig.midi_in.is_none() || list == self.rig.seen_midi {
            self.rig.seen_midi = list;
            return;
        }
        let mut lines = Vec::new();
        let mut reopen = Vec::new();
        let mut index = 0;
        while index < self.rig.ports.len() {
            let port = &self.rig.ports[index];
            let name = port.name().to_string();
            match list.iter().find(|input| input.name == name) {
                Some(input) if input.id == port.port_id() => index += 1,
                found => {
                    self.close_input(index);
                    lines.push(format!("MIDI: \"{name}\" disconnected."));
                    if found.is_some() {
                        reopen.push(name);
                    }
                }
            }
        }
        let names: Vec<String> = list.iter().map(|input| input.name.clone()).collect();
        let wanted = devices::midi_at_start(&names, &self.rig.store.saved().midi.off, &self.rig.flags);
        for name in wanted {
            let new = !self.rig.seen_midi.iter().any(|seen| seen.name == name);
            let open = self.rig.ports.iter().any(|port| port.name() == name);
            if open || !(new || reopen.contains(&name)) {
                continue;
            }
            match self.open_input(&name) {
                Ok(()) => lines.push(format!("MIDI: \"{name}\" connected.")),
                Err(e) => lines.push(format!("MIDI: could not open \"{name}\": {e}")),
            }
        }
        self.rig.seen_midi = list;
        if !lines.is_empty() {
            self.alerts.say(lines.join(" "));
        }
    }

    /// Turns a MIDI input on or off. Notes held on an input that is turned
    /// off are released. Audio does not stop.
    fn set_midi_input(&mut self, name: &str, on: bool) {
        let open = self.rig.ports.iter().position(|port| port.name() == name);
        match (on, open) {
            (true, Some(_)) | (false, None) => return,
            (true, None) => {
                if let Err(e) = self.open_input(name) {
                    self.alerts.say(format!("MIDI: {e}"));
                    return;
                }
            }
            (false, Some(index)) => self.close_input(index),
        }
        self.rig.flags.midi = None;
        self.rig.flags.no_midi = false;
        let mut preferences = self.rig.store.saved().clone();
        preferences.midi.set(name, on);
        if let Some(error) = self.rig.save(preferences) {
            self.alerts.say(error);
        }
    }

    /// The window, over the canvas, while it is open.
    pub(crate) fn preferences_view(&self) -> Option<Element<'_, crate::Message>> {
        let window = self.rig.window.as_ref()?;
        let heading = |label: &'static str| text(label).size(style::HEADING);
        let note = |line: String| text(line).size(style::BODY).color(crate::DIM);

        let mut safety = column![heading("Output safety")].spacing(6);
        for (mode, line) in [
            (
                OutputSafety::LowLatency,
                "Catches accidents. No added latency. May distort briefly when it acts.",
            ),
            (OutputSafety::Clean, "Transparent limiting. Adds 2 ms of latency."),
            (
                OutputSafety::Off,
                "No protection. The raw patch output, clipped to -1 to 1.",
            ),
        ] {
            let label = match mode {
                OutputSafety::LowLatency => "Low latency (default)",
                OutputSafety::Clean => "Clean",
                OutputSafety::Off => "Off",
            };
            safety = safety.push(
                column![
                    radio(label, mode, Some(self.stream.output_safety()), |m| {
                        crate::Message::Preferences(Message::Safety(m))
                    })
                    .size(style::SWITCH)
                    .text_size(style::CONTROL),
                    container(note(line.to_string())).padding([0, 24]),
                ]
                .spacing(2),
            );
        }
        if self.rig.safety_flag {
            safety = safety.push(note("Set by --output-safety for this run.".into()));
        }

        let audio = self.audio_section(window, note);
        let midi = self.midi_section(window, note);
        let panic = column![
            row![
                text(self.panic_line()).size(style::CONTROL).width(Length::Fill),
                button(
                    text(if self.rig.panic_learn.is_some() {
                        "Cancel"
                    } else {
                        "Learn"
                    })
                    .size(style::CONTROL)
                )
                .on_press(crate::Message::Preferences(Message::PanicLearn)),
                button(text("Clear").size(style::CONTROL)).on_press_maybe(
                    self.rig
                        .store
                        .saved()
                        .panic
                        .map(|_| crate::Message::Preferences(Message::PanicClear))
                ),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
            note("A control or pad that runs panic on every patch.".into()),
        ]
        .spacing(6);

        let panel = column![
            row![
                text("Preferences").size(style::TITLE),
                space::horizontal(),
                button(text("Close").size(style::CONTROL)).on_press(crate::Message::Preferences(Message::Close)),
            ]
            .align_y(iced::Alignment::Center),
            scrollable(
                column![
                    safety,
                    rule::horizontal(1),
                    checkbox(self.rig.store.saved().show_activity)
                        .label("Show signal activity on ports")
                        .on_toggle(|on| crate::Message::Preferences(Message::Activity(on))),
                    rule::horizontal(1),
                    heading("Audio output"),
                    audio,
                    rule::horizontal(1),
                    heading("MIDI inputs"),
                    midi,
                    rule::horizontal(1),
                    heading("Panic"),
                    panic
                ]
                .spacing(12)
                .padding([0, 12])
            )
        ]
        .spacing(12);
        let panel = container(panel)
            .padding(16)
            .width(560)
            .max_height(720)
            .style(container::rounded_box);
        Some(opaque(
            container(panel)
                .width(Length::Fill)
                .height(Length::Fill)
                .center(Length::Fill)
                .style(|_| container::background(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.4))),
        ))
    }

    fn audio_section<'a>(
        &'a self,
        window: &'a Window,
        note: impl Fn(String) -> iced::widget::Text<'a>,
    ) -> Element<'a, crate::Message> {
        let mut section = column![].spacing(8);
        let shown = self.rig.shown_audio();
        match &window.outputs {
            Err(e) => section = section.push(note(format!("Could not list the audio outputs: {e}"))),
            Ok(outputs) => {
                let default_name = outputs.default.clone().unwrap_or_else(|| "none".into());
                let devices: Vec<Item<DeviceChoice>> = std::iter::once(Item {
                    value: DeviceChoice::SystemDefault,
                    label: format!("System default ({default_name})"),
                })
                .chain(outputs.devices.iter().map(|d| Item {
                    value: DeviceChoice::Named(d.name.clone()),
                    label: d.name.clone(),
                }))
                .collect();
                let selected = Item::find(&devices, &shown.device);
                section = section.push(labelled(
                    "Device",
                    row![
                        pick_list(devices, selected, |item: Item<DeviceChoice>| {
                            crate::Message::Preferences(Message::Audio(Change::Device(item.value)))
                        })
                        .text_size(style::CONTROL)
                        .width(Length::Fill),
                        button(text("Refresh").size(style::CONTROL))
                            .on_press(crate::Message::Preferences(Message::Refresh)),
                    ]
                    .spacing(8)
                    .into(),
                ));
                if self.rig.flags.device.is_some() {
                    section = section.push(note("Set by --device for this run.".into()));
                }
                if let Some(info) = self.device_info(&shown.device) {
                    let rates: Vec<Item<RateChoice>> = devices::rate_choices(&info)
                        .into_iter()
                        .map(|value| Item {
                            label: match value {
                                RateChoice::DeviceDefault => format!("Device default ({} Hz)", info.default_rate),
                                RateChoice::Hz(hz) => format!("{hz} Hz"),
                            },
                            value,
                        })
                        .collect();
                    let selected = Item::find(&rates, &shown.sample_rate);
                    section = section.push(labelled(
                        "Sample rate",
                        pick_list(rates, selected, |item: Item<RateChoice>| {
                            crate::Message::Preferences(Message::Audio(Change::Rate(item.value)))
                        })
                        .text_size(style::CONTROL)
                        .width(Length::Fill)
                        .into(),
                    ));
                    if self.rig.flags.sample_rate.is_some() {
                        section = section.push(note("Set by --sample-rate for this run.".into()));
                    }
                    let rate = devices::effective_rate(&info, shown.sample_rate);
                    let buffers: Vec<Item<BufferChoice>> = devices::buffer_choices(&info)
                        .into_iter()
                        .map(|value| Item {
                            label: match value {
                                BufferChoice::DeviceDefault => "Device default".to_string(),
                                BufferChoice::Frames(f) => format!("{f} frames · {}", devices::buffer_time(f, rate)),
                            },
                            value,
                        })
                        .collect();
                    let selected = Item::find(&buffers, &shown.buffer);
                    section = section.push(labelled(
                        "Buffer size",
                        pick_list(buffers, selected, |item: Item<BufferChoice>| {
                            crate::Message::Preferences(Message::Audio(Change::Buffer(item.value)))
                        })
                        .text_size(style::CONTROL)
                        .width(Length::Fill)
                        .into(),
                    ));
                    if self.rig.flags.buffer.is_some() {
                        section = section.push(note("Set by --buffer for this run.".into()));
                    }
                }
            }
        }
        match &window.inputs {
            Err(error) => section = section.push(note(format!("Could not list the audio inputs: {error}"))),
            Ok(inputs) => {
                let choices: Vec<Item<InputChoice>> = std::iter::once(Item {
                    value: InputChoice::SystemDefault,
                    label: format!("System default ({})", inputs.default.as_deref().unwrap_or("none")),
                })
                .chain(inputs.devices.iter().map(|name| Item {
                    value: InputChoice::Named(name.clone()),
                    label: name.clone(),
                }))
                .chain(std::iter::once(Item {
                    value: InputChoice::Off,
                    label: "Off".into(),
                }))
                .collect();
                let selected = if self.rig.flags.no_input {
                    Item::find(&choices, &InputChoice::Off)
                } else if self.rig.flags.input.is_some() {
                    self.rig
                        .effective_input(inputs)
                        .and_then(|name| Item::find(&choices, &InputChoice::Named(name.to_string())))
                } else {
                    Item::find(&choices, &shown.input)
                };
                section = section.push(labelled(
                    "Audio input",
                    pick_list(choices, selected, |item: Item<InputChoice>| {
                        crate::Message::Preferences(Message::Input(item.value))
                    })
                    .text_size(style::CONTROL)
                    .width(Length::Fill)
                    .into(),
                ));
                if self.rig.flags.input.is_some() || self.rig.flags.no_input {
                    section = section.push(note("Set by an input flag for this run.".into()));
                }
                if let (Some(input), Some(output)) = (self.rig.effective_input(inputs), &self.rig.playing) {
                    if input == output.device {
                        section = section.push(note(
                            "Input and output use the same device. Speakers can feed the microphone.".into(),
                        ));
                    }
                    let buffer = output.buffer.or_else(|| {
                        self.stream
                            .stats()
                            .map(|stats| stats.snapshot().largest_buffer)
                            .filter(|&frames| frames > 0)
                            .and_then(|frames| u32::try_from(frames).ok())
                    });
                    if let Some(buffer) = buffer {
                        section = section.push(note(format!(
                            "Input latency about {}. The patch adds no delay of its own.",
                            devices::buffer_time(buffer.saturating_mul(2), output.rate)
                        )));
                    }
                }
            }
        }
        section.push(note(self.playing_line())).into()
    }

    /// The line under the audio controls: what is playing now.
    pub(crate) fn playing_line(&self) -> String {
        if self.stream.playback().is_off() {
            return "Audio is off for this run (--no-audio).".to_string();
        }
        if self.stream.playback().is_fake() {
            return "No device for this run (--fake-audio).".to_string();
        }
        match &self.rig.playing {
            Some(playing) => {
                let frames = self
                    .stream
                    .stats()
                    .map(|s| s.snapshot().largest_buffer)
                    .filter(|&f| f > 0)
                    .and_then(|f| u32::try_from(f).ok());
                playing.describe(frames)
            }
            None => format!("Not playing: {}.", self.stream.label()),
        }
    }

    fn midi_section<'a>(
        &'a self,
        window: &'a Window,
        note: impl Fn(String) -> iced::widget::Text<'a>,
    ) -> Element<'a, crate::Message> {
        let mut section = column![].spacing(6);
        match &window.midi {
            Err(e) => section = section.push(note(format!("Could not list the MIDI inputs: {e}"))),
            Ok(names) if names.is_empty() => section = section.push(note("No MIDI inputs found.".into())),
            Ok(names) => {
                for name in names {
                    let on = self.rig.ports.iter().any(|port| port.name() == name);
                    let name = name.clone();
                    section = section.push(
                        checkbox(on)
                            .label(name.clone())
                            .on_toggle(move |on| crate::Message::Preferences(Message::Midi(name.clone(), on)))
                            .size(style::SWITCH)
                            .text_size(style::CONTROL),
                    );
                }
                section = section.push(note("New MIDI inputs are on until you turn them off.".into()));
            }
        }
        if self.rig.flags.no_midi {
            section = section.push(note("Set by --no-midi for this run.".into()));
        } else if self.rig.flags.midi.is_some() {
            section = section.push(note("Set by --midi for this run.".into()));
        }
        section.into()
    }
}

/// A control with its label on the left.
fn labelled<'a>(label: &'a str, control: Element<'a, crate::Message>) -> Element<'a, crate::Message> {
    row![text(label).size(style::CONTROL).width(100), control]
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .into()
}

/// A menu entry: a value and its text.
#[derive(Debug, Clone, PartialEq)]
struct Item<T> {
    value: T,
    label: String,
}

impl<T: PartialEq + Clone> Item<T> {
    fn find(items: &[Self], value: &T) -> Option<Self> {
        items.iter().find(|item| item.value == *value).cloned()
    }
}

impl<T> fmt::Display for Item<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

#[cfg(test)]
mod tests;
