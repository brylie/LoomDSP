//! The Preferences window and what its choices do
//! (`docs/design/preferences.md`).
//!
//! [`Rig`] is what belongs to the machine rather than the patch: the
//! preferences file, the device host, this run's flags, and the open MIDI
//! inputs. It outlives every engine: opening a patch or changing the device
//! builds a new engine and moves the rig over.

use crate::devices::{self, Host, MidiPort, Playing, Wanted};
use crate::input_session::{Demand, InputSession};
use crate::overlays::Overlay;
use crate::preferences::{AudioPrefs, BufferChoice, DeviceChoice, InputChoice, Preferences, RateChoice, Store};
use crate::style::{self, button};
use crate::{App, alerts, undo};
use iced::widget::{
    checkbox, column, container, opaque, pick_list, radio, row, rule, scrollable, space, text, text_input,
};
use iced::{Element, Length};
use loom_core::{EngineConfig, EngineParts, MidiInputId, OutputSafety, create_engine};
use loom_host_desktop::{AudioOutput, MidiInputInfo, Outputs, Recorder, STOP_LIMIT, SharedMidi};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// A message for the Preferences window.
#[derive(Debug, Clone, PartialEq)]
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
    /// The **Your name** field's text, not yet saved.
    Author(String),
    /// Save the **Your name** field: Enter, or closing the window.
    SaveAuthor,
    /// Open the licence chooser for the default licence.
    ChooseLicence,
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

/// The library the rig reads linked sub-patches from (`Rig::library`).
pub enum RigLibrary {
    /// The library folder.
    Folder(crate::patch_file::FolderLibrary),
    /// No folder is known.
    None,
}

impl loom_patch::Library for RigLibrary {
    fn read(&self, path: &str) -> Result<loom_patch::Patch, loom_patch::LibraryError> {
        match self {
            Self::Folder(folder) => folder.read(path),
            Self::None => loom_patch::NoLibrary.read(path),
        }
    }
}

/// What belongs to the machine, not the patch.
pub struct Rig {
    pub host: Rc<dyn Host>,
    pub store: Store,
    /// The recent patches and the last folder. It outlives each engine,
    /// because every Open and New replaces the rest of the app.
    pub recent: crate::recent::RecentList,
    /// Device flags for this run. A flag is cleared when the window changes
    /// its setting: the window's choice is newer.
    pub flags: devices::Options,
    /// True while `--output-safety` sets the mode.
    pub safety_flag: bool,
    /// MIDI from every open input, into the current engine. `None` until
    /// the first engine is attached.
    pub midi_in: Option<SharedMidi>,
    pub ports: Vec<Box<dyn MidiPort>>,
    /// The audio input stream, and what the player was told about it.
    pub(crate) input: InputSession,
    /// The device playing now, when one is.
    pub playing: Option<Playing>,
    /// Lists a control channel command reads while the window is closed, so
    /// it checks a change as the window does without opening anything.
    borrowed_lists: Option<DeviceLists>,
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
    /// What the app saw when it was lost, for the status bar.
    pub reason: String,
    /// Whether the device was chosen as SystemDefault.
    pub was_system_default: bool,
    /// Number of consecutive reopen attempts that failed (0..10).
    pub attempts: u32,
    /// When the last poll / reopen attempt occurred.
    pub last_attempt: std::time::Instant,
    /// The error message if the last reopen attempt failed.
    pub last_error: Option<String>,
}

/// The devices the machine offers, as read when the window opened.
pub struct DeviceLists {
    outputs: Result<Outputs, String>,
    inputs: Result<devices::Inputs, String>,
    midi: Result<Vec<String>, String>,
}

impl DeviceLists {
    fn read(host: &dyn Host) -> Self {
        Self {
            outputs: host.outputs(),
            inputs: host.audio_inputs(),
            midi: host.midi_inputs(),
        }
    }
}

/// What the Preferences window holds while it is open.
pub struct Window {
    /// The device lists it shows.
    lists: DeviceLists,
    /// The **Your name** field as typed, saved on Enter or on close.
    author: String,
    /// The last audio choice that could not be used, and why. The window
    /// shows why under its list, since the status line is hidden under the
    /// window, and a step on that list goes on from it, so a device that
    /// fails never blocks the ones after it.
    failed: Option<Failed>,
}

/// An audio choice that could not be used.
#[derive(Debug, Clone, PartialEq)]
struct Failed {
    /// The message that asked for it.
    message: Message,
    /// Why it could not be used, as the status line says it.
    error: String,
}

impl Window {
    fn read(host: &dyn Host, author: String) -> Self {
        Self {
            lists: DeviceLists::read(host),
            author,
            failed: None,
        }
    }
}

impl Rig {
    pub fn new(host: Rc<dyn Host>, store: Store, flags: devices::Options, safety_flag: bool) -> Self {
        Self {
            recent: crate::recent::RecentList::in_memory(),
            host,
            store,
            flags,
            safety_flag,
            midi_in: None,
            ports: Vec::new(),
            input: InputSession::new(),
            playing: None,
            borrowed_lists: None,
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

    /// Where linked sub-patches are read from: the library folder, or
    /// nothing when none is known, so every linked tile keeps its stored
    /// copy (sub-patches.md, rule 5).
    pub fn library(&self) -> RigLibrary {
        match self.store.saved().library_folder() {
            Some(root) => RigLibrary::Folder(crate::patch_file::FolderLibrary { root }),
            None => RigLibrary::None,
        }
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
        if let Some(input) = self.input.port().filter(|input| input.is_live()) {
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
            InputChoice::Named(_) if self.input.named_lost() => None,
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
            && let Some(input) = self.input.port()
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
                let author = self.rig.store.saved().author.clone();
                let window = Window::read(&*self.rig.host, author);
                self.overlays.open(Overlay::Preferences(Box::new(window)));
            }
            Message::Close => {
                self.save_author();
                self.overlays.close_preferences();
                // Learning a panic trigger needs the window open to show
                // "Waiting for MIDI...", so closing it cancels the learn.
                self.rig.panic_learn = None;
            }
            Message::Refresh => {
                let lists = DeviceLists::read(&*self.rig.host);
                if let Some(window) = self.overlays.preferences_mut() {
                    window.lists = lists;
                    window.failed = None;
                }
            }
            Message::Author(text) => {
                if let Some(window) = self.overlays.preferences_mut() {
                    window.author = text;
                }
            }
            Message::SaveAuthor => self.save_author(),
            Message::ChooseLicence => {
                let current = self.rig.store.saved().licence.clone();
                self.overlays.chooser = Some(crate::licence_chooser::Chooser::open(
                    crate::licence_chooser::Target::Default,
                    current.as_deref(),
                ));
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
        if self.session.engine.stream.without_device() {
            let flag = if self.session.engine.stream.playback().is_off() {
                "--no-audio"
            } else {
                "--fake-audio"
            };
            return Err(format!("there are no audio devices to choose with {flag}"));
        }
        let read;
        let lists = match self.overlays.preferences() {
            Some(window) => &window.lists,
            None => {
                read = DeviceLists::read(&*self.rig.host);
                &read
            }
        };
        let outputs = lists
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
        if self.overlays.preferences().is_some() {
            return false;
        }
        self.rig.borrowed_lists = Some(DeviceLists::read(&*self.rig.host));
        true
    }

    /// Drops the lists [`App::read_device_lists`] read.
    pub(crate) fn forget_device_lists(&mut self) {
        self.rig.borrowed_lists = None;
    }

    /// The MIDI input a control channel `midi` command names (rule S2).
    pub(crate) fn control_midi_input(&self, wanted: &str) -> Result<String, String> {
        let names = match self.overlays.preferences() {
            Some(window) => window.lists.midi.clone(),
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
        let message = if self.overlays.preferences().is_some() {
            Message::Close
        } else {
            Message::Open
        };
        self.preferences(message);
    }

    /// Picks the output safety mode (preferences.md, Changing the mode at
    /// run time).
    fn choose_output_safety(&mut self, mode: OutputSafety) {
        if !self.session.engine.stream.set_output_safety(mode) {
            return;
        }
        self.rig.safety_flag = false;
        let mut preferences = self.rig.store.saved().clone();
        preferences.output_safety = mode;
        if let Some(error) = self.rig.save(preferences) {
            self.alerts.warn(error);
        }
        // With no device there is no engine playing to tell. The next
        // engine is built with the new mode.
        if self.session.engine.stream.is_playing() {
            self.rig.safety_pending = Some(mode);
            self.send_pending_safety();
        }
    }

    /// Saves the **Your name** field, trimmed, when it changed
    /// (patch-metadata.md, Defaults). Only the name is stored, never other
    /// contact details (rule 4).
    fn save_author(&mut self) {
        let Some(window) = self.overlays.preferences() else {
            return;
        };
        let author = window.author.trim().to_string();
        if self.rig.store.saved().author == author {
            return;
        }
        let mut preferences = self.rig.store.saved().clone();
        preferences.author = author;
        if let Some(error) = self.rig.save(preferences) {
            self.alerts.warn(error);
        }
    }

    /// Saves the default licence for new patches, or "No default".
    pub(crate) fn set_default_licence(&mut self, licence: Option<String>) {
        if self.rig.store.saved().licence == licence {
            return;
        }
        let mut preferences = self.rig.store.saved().clone();
        preferences.licence = licence;
        if let Some(error) = self.rig.save(preferences) {
            self.alerts.warn(error);
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
            self.alerts.warn(error);
        }
        self.session.engine.live.set_activity(on);
        self.session.carrying.clear();
        self.sync_live();
        if let Err(error) = self.session.controller.commit() {
            self.alerts.warn(error.to_string());
        }
    }

    /// Sends a mode change the queue refused before. Runs on every update.
    pub(crate) fn send_pending_safety(&mut self) {
        let Some(mode) = self.rig.safety_pending else { return };
        if self.session.controller.set_output_safety(mode).is_ok() {
            self.rig.safety_pending = None;
            #[cfg(test)]
            self.rig.sent.push(mode);
        }
    }

    /// Whether the settings and the patch call for an audio input.
    pub(crate) fn input_wanted(&self) -> bool {
        !self.rig.flags.no_input
            && (self.rig.flags.input.is_some() || !matches!(self.rig.store.saved().audio.input, InputChoice::Off))
            && self
                .session
                .controller
                .graph()
                .nodes()
                .any(|(_, node)| node.descriptor.id == "loom.io.audio-in")
    }

    /// Keeps the input stream in step with the patch and the current output.
    pub(crate) fn sync_audio_input(&mut self) {
        let wanted = self.input_wanted();
        let needed =
            wanted && self.session.engine.stream.is_playing() && !self.session.engine.stream.playback().is_fake();
        let buffer = self
            .rig
            .playing
            .as_ref()
            .and_then(|playing| playing.buffer)
            .or_else(|| {
                self.session
                    .engine
                    .stream
                    .stats()
                    .map(|stats| stats.snapshot().largest_buffer)
                    .filter(|&frames| frames > 0)
                    .and_then(|frames| u32::try_from(frames).ok())
            });
        let demand = Demand {
            wanted,
            needed,
            flag: self.rig.flags.input.as_deref(),
            choice: &self.rig.store.saved().audio.input,
            buffer,
            sample_rate: self.session.engine.stream.sample_rate() as u32,
        };
        self.rig
            .input
            .sync(demand, &*self.rig.host, &mut self.session.controller, &mut self.alerts);
    }

    /// Changes the input without restarting the output stream.
    fn choose_audio_input(&mut self, choice: InputChoice) {
        self.alerts.set_input_drift(None);
        self.rig.flags.input = None;
        self.rig.flags.no_input = false;
        let mut preferences = self.rig.store.saved().clone();
        preferences.audio.input = choice;
        let error = self.rig.save(preferences);
        self.rig.input.choose(&mut self.session.controller, &mut self.alerts);
        self.sync_audio_input();
        if let Some(error) = error {
            self.alerts.warn(error);
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

        if !self.session.engine.stream.without_device() {
            // The new device opens first: if it fails, the old one keeps
            // playing and nothing is saved (Rule D5).
            let output = match devices::open_choice(&*self.rig.host, &play) {
                Ok(output) => output,
                Err(e) => {
                    let error = self.could_not_use(&play, &e);
                    self.audio_failed(&change, &error);
                    self.alerts.warn(error);
                    return;
                }
            };
            if let Err(e) = self.restart_audio(output) {
                self.audio_failed(&change, &e);
                self.alerts.warn(e);
                return;
            }
            self.audio_back();
        } else {
            self.audio_back();
        }
        if let Some(window) = self.overlays.preferences_mut() {
            window.failed = None;
        }
        match change {
            Change::Device(_) => self.rig.flags.device = None,
            Change::Rate(_) => self.rig.flags.sample_rate = None,
            Change::Buffer(_) => self.rig.flags.buffer = None,
        }
        // The flags that are left still set this run; what is saved is the
        // player's choice.
        if let Some(error) = self.rig.save(saved) {
            self.alerts.warn(error);
        }
    }

    /// Notes in the open window that `change` could not be used.
    fn audio_failed(&mut self, change: &Change, error: &str) {
        if let Some(window) = self.overlays.preferences_mut() {
            window.failed = Some(Failed {
                message: Message::Audio(change.clone()),
                error: error.to_string(),
            });
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
        let open = self.overlays.preferences().map(|window| &window.lists);
        open.or(self.rig.borrowed_lists.as_ref())
            .and_then(|lists| lists.outputs.as_ref().ok())
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
            output_safety: self.session.engine.stream.output_safety(),
            ..EngineConfig::with_sample_rate(rate)
        };
        let mut parts = create_engine(crate::registry(self.test_modules), config);
        let controller = &mut parts.controller;
        let current = undo::Snapshot::take(&self.session.controller, &self.session.positions);
        let empty = undo::Snapshot::take(controller, &BTreeMap::new());
        if let Some(step) = undo::diff(&empty, &current, &BTreeSet::new()) {
            undo::apply(&step, true, controller, &mut self.session.positions.clone())?;
        }
        // The transport keeps its tempo, time signature, and whether it
        // runs. Its position starts at beat 0.
        let transport = self.session.controller.transport();
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
        if self.session.engine.stream.has_engine() {
            return;
        }
        let Ok(parts) = self.rebuilt_engine(self.session.engine.stream.sample_rate()) else {
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
        self.session.controller = controller;
        // The new engine needs the panic trigger and the open MIDI inputs,
        // as in `play`, so panic and held-note release still work.
        self.send_panic_trigger();
        self.connect_midi(false, false);
        self.session.engine.monitor =
            crate::monitor::Monitor::new(telemetry, self.session.controller.config().sample_rate, self.generation);
        self.session.traces = loom_canvas::ScopeTraces::new();
        self.session.engine.live = crate::live::Live::showing_activity();
        self.session
            .engine
            .live
            .set_activity(self.rig.store.saved().show_activity);
        self.session.engine.stream.keep_idle(engine);
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
        self.session.controller = controller;
        self.send_panic_trigger();
        self.connect_midi(false, false);
        self.session.engine.monitor =
            crate::monitor::Monitor::new(telemetry, self.session.controller.config().sample_rate, self.generation);
        self.session.traces = loom_canvas::ScopeTraces::new();
        self.session.engine.live = crate::live::Live::showing_activity();
        self.session
            .engine
            .live
            .set_activity(self.rig.store.saved().show_activity);
        self.session.lights.clear();
        self.session.meter_levels.clear();
        self.session.engine.stream.prepare(output.sample_rate() as f32);
        self.session.engine.holding = false;
        let refused = self.rig.record_into(&mut output);
        let started = self.rig.host.start(output, engine);
        // Set on success; on an error this returns early.
        let record_error;
        match started {
            Ok(audio) => {
                record_error = refused.map(|e| self.rig.end_recording(&e));
                self.session.engine.stream.started(audio, label);
                self.rig.playing = Some(playing);
                self.session.engine.midi = self.rig.midi_in.clone();
            }
            Err(refused) => {
                let (engine, e) = *refused;
                self.session.engine.stream.refused(engine, &e);
                self.rig.playing = None;
                self.session.engine.midi = None;
                return Err(e);
            }
        }
        // Probes for the meters on screen, in the new engine.
        self.commit();
        self.refresh_values();
        // After the commit, which sets the status line too.
        if let Some(error) = record_error {
            self.alerts.warn(error);
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
        // A new stream gets a new watcher, and a clean first read sends
        // nothing, so nothing else would turn these off.
        self.alerts.set_output_warnings(false, false);
        if let Some(audio) = self.session.engine.stream.take_running() {
            audio.stop_immediate();
        }
        self.rig.input.stream_stopped();
        self.rig.playing = None;
    }

    /// Handles loss of the active audio output (device-changes.md, Audio device loss).
    pub(crate) fn handle_audio_loss(&mut self, reason: &str) {
        // Rule 3: The engine is never rebuilt twice for one loss. A second error
        // from the same dead stream is ignored.
        if !self.session.engine.stream.is_playing() && self.rig.lost_audio.is_some() {
            return;
        }
        if !self.session.engine.stream.is_playing() && self.rig.playing.is_none() {
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
        self.session.engine.midi = None;
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
                    self.audio_back();
                    self.alerts.warn(format!(
                        "Audio: \"{old_name}\" was lost: {reason}. Now playing on \"{new_default}\"."
                    ));
                    switched = true;
                }
            }
            if !switched {
                let message = format!("Audio: \"{old_name}\" was lost: {reason}.");
                self.audio_lost(
                    LostAudio {
                        device: old_name,
                        reason: reason.to_string(),
                        was_system_default: true,
                        attempts: 0,
                        last_attempt: Instant::now(),
                        last_error: None,
                    },
                    message,
                );
            }
        } else {
            // Named device: plays nothing, keeps engine idle
            let message = format!(
                "Audio: \"{old_name}\" was lost: {reason}. Plug it in again, or choose another device in Preferences."
            );
            self.audio_lost(
                LostAudio {
                    device: old_name,
                    reason: reason.to_string(),
                    was_system_default: false,
                    attempts: 0,
                    last_attempt: Instant::now(),
                    last_error: None,
                },
                message,
            );
        }
    }

    /// The output is lost: the app remembers it and keeps the message on
    /// screen. This and [`Self::audio_back`] are the only ways in and out, so
    /// the two cannot disagree. The retries in `check_audio` take the record
    /// out and put it back, which changes nothing the player sees.
    fn audio_lost(&mut self, lost: LostAudio, message: String) {
        self.alerts.error(alerts::Cause::AudioOutput, message);
        self.rig.lost_audio = Some(lost);
    }

    /// Sound plays again, or the player chose a device.
    pub(crate) fn audio_back(&mut self) {
        self.rig.lost_audio = None;
        self.alerts.clear(alerts::Cause::AudioOutput);
    }

    /// Checks for audio device loss, silent stalls, and device reconnection
    /// (device-changes.md, Audio device loss).
    pub(crate) fn poll_audio(&mut self) {
        self.check_audio(Instant::now());
        self.sync_audio_input();
    }

    /// Checks for audio device loss, silent stalls, and device reconnection at `now`.
    pub(crate) fn check_audio(&mut self, now: Instant) {
        if self.session.engine.stream.without_device() {
            return;
        }

        // 1. If audio is running, check for loss (dead stream, stall > 1s, missing device)
        if self.session.engine.stream.is_playing() {
            let mut callbacks_advancing = false;
            if let Some(stats) = self.session.engine.stream.stats().cloned() {
                let snap = stats.snapshot();
                if snap.is_dead {
                    self.handle_audio_loss("the audio device reported an error");
                    return;
                }
                if self.session.engine.stream.observe_callbacks(snap.callbacks, now) {
                    callbacks_advancing = true;
                } else if self.session.engine.stream.stalled(now) {
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
                    self.handle_audio_loss("it was unplugged");
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
                                    self.audio_back();
                                    self.alerts.warn(format!(
                                        "Audio: \"{}\" was lost: {}. Now playing on \"{new_default}\".",
                                        lost.device, lost.reason
                                    ));
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
                        self.alerts.error(alerts::Cause::AudioOutput, message);
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
                                self.audio_back();
                                self.alerts.say(format!("Audio: \"{}\" is back.", lost.device));
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
                        self.alerts.error(alerts::Cause::AudioOutput, message);
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
        // A new stream gets a new watcher, and a clean first read sends
        // nothing, so nothing else would turn these off.
        self.alerts.set_output_warnings(false, false);
        if let Some(audio) = self.session.engine.stream.take_running() {
            // A full queue cannot fade: the limit then stops the stream.
            let _ = self.session.controller.fade_out();
            self.rig.last_stop = Some(audio.stop(STOP_LIMIT));
        }
        self.rig.input.stream_stopped();
        self.rig.playing = None;
    }

    /// Opens a MIDI input: the engine first starts tracking its id, then
    /// the host connects it. On an error nothing stays open.
    fn open_input(&mut self, name: &str) -> Result<(), String> {
        let midi = self.rig.midi_in.clone().ok_or("no engine to play")?;
        let input = self.rig.free_input().ok_or("too many MIDI inputs are open")?;
        self.session
            .controller
            .open_midi_input(input)
            .map_err(|e| e.to_string())?;
        match self.rig.host.open_midi(name, midi, input) {
            Ok(port) => {
                self.rig.ports.push(port);
                Ok(())
            }
            Err(e) => {
                let _ = self.session.controller.release_midi_input(input);
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
        let _ = self.session.controller.release_midi_input(input);
    }

    /// After a new engine is attached: tracks the open inputs on it, or the
    /// first time opens the inputs the preferences and flags choose, when
    /// `connect` is true. Returns a status bar line about an input that
    /// failed to open; the others still play.
    pub(crate) fn connect_midi(&mut self, first: bool, connect: bool) -> Option<String> {
        for input in self.rig.ports.iter().map(|port| port.input()).collect::<Vec<_>>() {
            let _ = self.session.controller.open_midi_input(input);
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
        if let Some(window) = self.overlays.preferences_mut() {
            window.lists.midi = Ok(list.iter().map(|input| input.name.clone()).collect());
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
                    self.alerts.warn(format!("MIDI: {e}"));
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
            self.alerts.warn(error);
        }
    }

    /// The window, over the canvas, while it is open.
    pub(crate) fn preferences_view(&self) -> Option<Element<'_, crate::Message>> {
        self.preferences_model().map(preferences_panel)
    }

    /// The line under the audio controls: what is playing now.
    pub(crate) fn playing_line(&self) -> String {
        if self.session.engine.stream.playback().is_off() {
            return "Audio is off for this run (--no-audio).".to_string();
        }
        if self.session.engine.stream.playback().is_fake() {
            return "No device for this run (--fake-audio).".to_string();
        }
        match &self.rig.playing {
            Some(playing) => {
                let frames = self
                    .session
                    .engine
                    .stream
                    .stats()
                    .map(|s| s.snapshot().largest_buffer)
                    .filter(|&f| f > 0)
                    .and_then(|f| u32::try_from(f).ok());
                playing.describe(frames)
            }
            None => format!("Not playing: {}.", self.session.engine.stream.label()),
        }
    }
}

/// The id of the **Your name** field, so a screen reader's Focus reaches
/// it. It goes away with Iced's text input in phase 3.
pub(crate) const AUTHOR_INPUT: &str = "preferences-author";

/// The Preferences window, drawn from its model. Each part is named by its
/// UI ID, so the window says where it drew each one (`places.rs`).
fn preferences_panel(model: Model) -> Element<'static, crate::Message> {
    use access::{part_id, preferences_id};
    let msg = crate::Message::Preferences;
    let heading = |label: &'static str| text(label).size(style::HEADING);
    let note = |line: String| text(line).size(style::BODY).color(crate::DIM);

    let note_of = |line: model::Note| named(note(line.text), part_id("note").with(line.key));
    let mut modes = column![].spacing(6);
    for mode in &model.safety {
        let chosen = mode.chosen.then_some(mode.mode);
        modes = modes.push(
            column![
                named(
                    radio(mode.label, mode.mode, chosen, move |m| msg(Message::Safety(m)))
                        .size(style::SWITCH)
                        .text_size(style::CONTROL),
                    part_id("safety").with(mode.key),
                ),
                container(note(mode.note.to_string())).padding([0, 24]),
            ]
            .spacing(2),
        );
    }
    let mut safety = column![heading("Output safety"), named(modes, part_id("safety"))].spacing(6);
    for line in model.safety_notes {
        safety = safety.push(note_of(line));
    }

    let mut audio = column![].spacing(8);
    for part in model.audio {
        audio = match part {
            Part::Picks(picks) if picks.key == "device" => audio.push(labelled(
                picks.label,
                row![
                    pick_list_of(picks),
                    named(
                        button(text("Refresh").size(style::CONTROL)).on_press(msg(Message::Refresh)),
                        part_id("refresh"),
                    ),
                ]
                .spacing(8)
                .into(),
            )),
            Part::Picks(picks) => audio.push(labelled(picks.label, pick_list_of(picks))),
            Part::Note(line) => audio.push(note_of(line)),
            Part::Midi { .. } => audio,
        };
    }

    let mut midi = column![].spacing(6);
    for part in model.midi {
        midi = match part {
            Part::Midi { name, on, id } => midi.push(named(
                checkbox(on)
                    .label(name.clone())
                    .on_toggle(move |on| msg(Message::Midi(name.clone(), on)))
                    .size(style::SWITCH)
                    .text_size(style::CONTROL),
                id,
            )),
            Part::Note(line) => midi.push(note_of(line)),
            Part::Picks(_) => midi,
        };
    }

    let panic = column![
        row![
            container(named(text(model.panic.line).size(style::CONTROL), part_id("panic"))).width(Length::Fill),
            named(
                button(text(if model.panic.learning { "Cancel" } else { "Learn" }).size(style::CONTROL))
                    .on_press(msg(Message::PanicLearn)),
                part_id("panic").with("learn"),
            ),
            named(
                button(text("Clear").size(style::CONTROL))
                    .on_press_maybe(model.panic.can_clear.then_some(msg(Message::PanicClear))),
                part_id("panic").with("clear"),
            ),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
        note("A control or pad that runs panic on every patch.".into()),
    ]
    .spacing(6);

    let patches = column![
        labelled(
            "Your name",
            named(
                text_input("Added as author of new patches", &model.author)
                    .id(iced::widget::Id::new(AUTHOR_INPUT))
                    .on_input(move |t| msg(Message::Author(t)))
                    .on_submit(msg(Message::SaveAuthor))
                    .size(style::CONTROL),
                part_id("author"),
            ),
        ),
        note("Only the name is added to patches you share. Add more by hand if you want to.".into()),
        labelled(
            "Licence",
            row![
                container(named(text(model.licence).size(style::CONTROL), part_id("licence"))).width(Length::Fill),
                named(
                    button(text("Choose").size(style::CONTROL)).on_press(msg(Message::ChooseLicence)),
                    part_id("licence").with("choose"),
                ),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .into(),
        ),
        note("New patches start with this licence. You can change it for each patch.".into()),
    ]
    .spacing(6);

    let panel = column![
        row![
            text("Preferences").size(style::TITLE),
            space::horizontal(),
            named(
                button(text("Close").size(style::CONTROL)).on_press(msg(Message::Close)),
                part_id("close"),
            ),
        ]
        .align_y(iced::Alignment::Center),
        scrollable(
            column![
                safety,
                rule::horizontal(1),
                named(
                    checkbox(model.activity)
                        .label("Show signal activity on ports")
                        .on_toggle(move |on| msg(Message::Activity(on))),
                    part_id("activity"),
                ),
                rule::horizontal(1),
                heading("Audio output"),
                audio,
                rule::horizontal(1),
                heading("MIDI inputs"),
                midi,
                rule::horizontal(1),
                heading("Panic"),
                panic,
                rule::horizontal(1),
                heading("New patches"),
                patches
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
        .style(container::rounded_box)
        .id(crate::places::widget_id(&preferences_id()));
    opaque(
        container(panel)
            .width(Length::Fill)
            .height(Length::Fill)
            .center(Length::Fill)
            .style(|_| container::background(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.4))),
    )
}

/// `element` in a container named by `id`, so the window says where it
/// drew it.
fn named<'a>(element: impl Into<Element<'a, crate::Message>>, id: loom_ui_core::UiId) -> Element<'a, crate::Message> {
    container(element).id(crate::places::widget_id(&id)).into()
}

/// A list from its model: picking an option sends that option's message.
fn pick_list_of(picks: Picks) -> Element<'static, crate::Message> {
    let id = access::part_id(picks.key);
    let shown: Vec<Shown> = picks
        .options
        .iter()
        .enumerate()
        .map(|(at, text)| Shown { at, text: text.clone() })
        .collect();
    let selected = picks.chosen.and_then(|at| shown.get(at).cloned());
    let messages = picks.messages;
    let list = pick_list(shown, selected, move |item: Shown| {
        crate::Message::Preferences(messages[item.at].clone())
    })
    .text_size(style::CONTROL)
    .width(Length::Fill);
    container(list)
        .width(Length::Fill)
        .id(crate::places::widget_id(&id))
        .into()
}

/// A control with its label on the left.
fn labelled(label: &'static str, control: Element<'static, crate::Message>) -> Element<'static, crate::Message> {
    row![text(label).size(style::CONTROL).width(100), control]
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .into()
}

/// One option of a list as the list shows it: its place and its text.
#[derive(Debug, Clone, PartialEq)]
struct Shown {
    at: usize,
    text: String,
}

impl fmt::Display for Shown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

pub(crate) mod access;
mod model;

pub(crate) use model::{Model, Part, Picks};

#[cfg(test)]
mod tests;
