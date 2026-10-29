//! The Preferences window and what its choices do
//! (`docs/design/preferences.md`).
//!
//! [`Rig`] is what belongs to the machine rather than the patch: the
//! preferences file, the device host, this run's flags, and the open MIDI
//! inputs. It outlives every engine: opening a patch or changing the device
//! builds a new engine and moves the rig over.

use crate::devices::{self, Host, MidiPort, Playing, Wanted};
use crate::preferences::{AudioPrefs, BufferChoice, DeviceChoice, Preferences, RateChoice, Store};
use crate::{App, undo};
use iced::widget::{button, checkbox, column, container, opaque, pick_list, radio, row, rule, scrollable, space, text};
use iced::{Element, Length};
use loom_core::{EngineConfig, EngineParts, MidiInputId, OutputSafety, create_engine};
use loom_host_desktop::{AudioOutput, MidiInputInfo, Outputs, Recorder, STOP_LIMIT, SharedMidi};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// A message for the Preferences window.
#[derive(Debug, Clone)]
pub enum Message {
    Open,
    Close,
    /// Read the device lists again.
    Refresh,
    Safety(OutputSafety),
    Audio(Change),
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
    midi: Result<Vec<String>, String>,
}

impl Window {
    fn read(host: &dyn Host) -> Self {
        Self {
            outputs: host.outputs(),
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

    /// The audio choice the window shows: the saved one, with the flags for
    /// this run on top.
    pub fn shown_audio(&self) -> AudioPrefs {
        let mut audio = self.store.saved().audio.clone();
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
            Message::Audio(change) => self.change_audio(change),
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
        if self.no_audio || self.fake_audio {
            let flag = if self.no_audio { "--no-audio" } else { "--fake-audio" };
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
        if mode == self.output_safety {
            return;
        }
        self.output_safety = mode;
        self.rig.safety_flag = false;
        let mut preferences = self.rig.store.saved().clone();
        preferences.output_safety = mode;
        if let Some(error) = self.rig.save(preferences) {
            self.status = error;
        }
        // With no device there is no engine playing to tell. The next
        // engine is built with the new mode.
        if self._audio.is_some() {
            self.rig.safety_pending = Some(mode);
            self.send_pending_safety();
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

        if !self.no_audio && !self.fake_audio {
            // The new device opens first: if it fails, the old one keeps
            // playing and nothing is saved (Rule D5).
            let output = match devices::open_choice(&*self.rig.host, &play) {
                Ok(output) => output,
                Err(e) => {
                    self.status = self.could_not_use(&play, &e);
                    return;
                }
            };
            if let Err(e) = self.restart_audio(output) {
                self.status = e;
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
            self.status = error;
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
            output_safety: self.output_safety,
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
        if self._audio.is_some() || self.idle_engine.is_some() {
            return;
        }
        let Ok(parts) = self.rebuilt_engine(self.sample_rate) else {
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
        self.live = crate::live::Live::new();
        self.idle_engine = Some(engine);
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
        self.live = crate::live::Live::new();
        self.lights.clear();
        self.meter_levels.clear();
        self.sample_rate = output.sample_rate() as f32;
        self.idle_engine = None;
        self.holding = false;
        let refused = self.rig.record_into(&mut output);
        let started = self.rig.host.start(output, engine);
        // Set on success; on an error this returns early.
        let record_error;
        match started {
            Ok(audio) => {
                record_error = refused.map(|e| self.rig.end_recording(&e));
                self.audio_stats = Some(audio.stats().clone());
                self._audio = Some(audio);
                self.audio_name = label;
                self.rig.playing = Some(playing);
                self.midi = self.rig.midi_in.clone();
                self.audio_callbacks_seen = 0;
                self.audio_callbacks_advanced = Instant::now();
            }
            Err((engine, e)) => {
                self.audio_name = format!("no audio ({e})");
                self.rig.playing = None;
                self.midi = None;
                self.idle_engine = Some(engine);
                return Err(e);
            }
        }
        // Probes for the meters on screen, in the new engine.
        self.commit();
        self.refresh_values();
        // After the commit, which sets the status line too.
        if let Some(error) = record_error {
            self.status = error;
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
        if let Some(audio) = self._audio.take() {
            audio.stop_immediate();
        }
        self.audio_stats = None;
        self.rig.playing = None;
    }

    /// Handles loss of the active audio output (device-changes.md, Audio device loss).
    pub(crate) fn handle_audio_loss(&mut self, _reason: &str) {
        // Rule 3: The engine is never rebuilt twice for one loss. A second error
        // from the same dead stream is ignored.
        if self._audio.is_none() && self.rig.lost_audio.is_some() {
            return;
        }
        if self._audio.is_none() && self.rig.playing.is_none() {
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
                    self.status = format!("Audio: \"{old_name}\" was lost. Now playing on \"{new_default}\".");
                    self.rig.lost_audio = None;
                    switched = true;
                }
            }
            if !switched {
                let message = format!("Audio: \"{old_name}\" was lost.");
                self.status = message.clone();
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
            self.status = message.clone();
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
    }

    /// Checks for audio device loss, silent stalls, and device reconnection at `now`.
    pub(crate) fn check_audio(&mut self, now: Instant) {
        if self.no_audio || self.fake_audio {
            return;
        }

        // 1. If audio is running, check for loss (dead stream, stall > 1s, missing device)
        if self._audio.is_some() {
            let mut callbacks_advancing = false;
            if let Some(stats) = &self.audio_stats {
                let snap = stats.snapshot();
                if snap.is_dead {
                    self.handle_audio_loss("the audio device reported an error");
                    return;
                }
                if snap.callbacks > self.audio_callbacks_seen {
                    self.audio_callbacks_seen = snap.callbacks;
                    self.audio_callbacks_advanced = now;
                    callbacks_advancing = true;
                } else if now.saturating_duration_since(self.audio_callbacks_advanced) > Duration::from_secs(1) {
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
                            self.status = format!("Audio: \"{device_name}\" sample rate changed to {new_rate} Hz.");
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
                                    self.status = format!(
                                        "Audio: \"{}\" was lost. Now playing on \"{new_default}\".",
                                        lost.device
                                    );
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
                        self.status = message.clone();
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
                                self.status = format!("Audio: \"{}\" is back.", lost.device);
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
                        self.status = message.clone();
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
        if let Some(audio) = self._audio.take() {
            // A full queue cannot fade: the limit then stops the stream.
            let _ = self.controller.fade_out();
            self.rig.last_stop = Some(audio.stop(STOP_LIMIT));
        }
        self.audio_stats = None;
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
            self.status = lines.join(" ");
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
                    self.status = format!("MIDI: {e}");
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
            self.status = error;
        }
    }

    /// The window, over the canvas, while it is open.
    pub(crate) fn preferences_view(&self) -> Option<Element<'_, crate::Message>> {
        let window = self.rig.window.as_ref()?;
        let heading = |label: &'static str| text(label).size(15);
        let note = |line: String| text(line).size(12).color(crate::DIM);

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
                    radio(label, mode, Some(self.output_safety), |m| crate::Message::Preferences(
                        Message::Safety(m)
                    ))
                    .size(14)
                    .text_size(13),
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
                text(self.panic_line()).size(13).width(Length::Fill),
                button(
                    text(if self.rig.panic_learn.is_some() {
                        "Cancel"
                    } else {
                        "Learn"
                    })
                    .size(13)
                )
                .on_press(crate::Message::Preferences(Message::PanicLearn)),
                button(text("Clear").size(13)).on_press_maybe(
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
                text("Preferences").size(18),
                space::horizontal(),
                button(text("Close").size(13)).on_press(crate::Message::Preferences(Message::Close)),
            ]
            .align_y(iced::Alignment::Center),
            scrollable(
                column![
                    safety,
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
                        .text_size(13)
                        .width(Length::Fill),
                        button(text("Refresh").size(13)).on_press(crate::Message::Preferences(Message::Refresh)),
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
                        .text_size(13)
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
                        .text_size(13)
                        .width(Length::Fill)
                        .into(),
                    ));
                    if self.rig.flags.buffer.is_some() {
                        section = section.push(note("Set by --buffer for this run.".into()));
                    }
                }
            }
        }
        section.push(note(self.playing_line())).into()
    }

    /// The line under the audio controls: what is playing now.
    pub(crate) fn playing_line(&self) -> String {
        if self.no_audio {
            return "Audio is off for this run (--no-audio).".to_string();
        }
        if self.fake_audio {
            return "No device for this run (--fake-audio).".to_string();
        }
        match &self.rig.playing {
            Some(playing) => {
                let frames = self
                    .audio_stats
                    .as_ref()
                    .map(|s| s.snapshot().largest_buffer)
                    .filter(|&f| f > 0)
                    .and_then(|f| u32::try_from(f).ok());
                playing.describe(frames)
            }
            None => format!("Not playing: {}.", self.audio_name),
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
                            .size(14)
                            .text_size(13),
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
    row![text(label).size(13).width(100), control]
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
mod tests {
    use super::*;
    use crate::devices::fake::{self, Event, Fake};
    use crate::preferences::FILE_NAME;
    use crate::{Args, Source, recovery};
    use std::path::PathBuf;
    use std::time::Duration;

    /// A folder for one test's preferences file.
    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("loom-settings-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Args for a run that plays on the fake devices, with the preferences
    /// file in `dir`.
    fn args(dir: &std::path::Path) -> Args {
        Args {
            no_audio: false,
            no_recovery: true,
            devices: devices::Options {
                preferences: Some(dir.join(FILE_NAME)),
                ..devices::Options::default()
            },
            ..crate::tests::app_args()
        }
    }

    fn start(args: Args, fake: &Rc<Fake>) -> App {
        App::boot_with(args, &mut recovery::Dialogs, Rc::new(fake::Shared(fake.clone())))
    }

    fn write(dir: &std::path::Path, text: &str) -> PathBuf {
        let path = dir.join(FILE_NAME);
        std::fs::write(&path, text).unwrap();
        path
    }

    fn two_devices() -> Rc<Fake> {
        Rc::new(Fake::new(&[fake::device("Fake A"), fake::device("Fake B")], &[]))
    }

    fn change(app: &mut App, change: Change) {
        app.preferences(Message::Open);
        app.preferences(Message::Audio(change));
    }

    #[test]
    fn no_saved_devices_play_on_the_defaults_with_every_midi_input() {
        let dir = folder("defaults");
        let fake = Rc::new(Fake::new(
            &[fake::device("Fake A"), fake::device("Fake B")],
            &["Keys", "Pad"],
        ));
        let app = start(args(&dir), &fake);
        assert_eq!(fake.opened(), ["Fake A"]);
        assert_eq!(app.sample_rate, 48_000.0);
        assert!(app._audio.is_some());
        assert!(fake.is_open("Keys") && fake.is_open("Pad"));
        assert_eq!(app.controller.config().output_safety, OutputSafety::LowLatency);
        assert!(!dir.join(FILE_NAME).exists(), "starting writes nothing");
        assert_eq!(
            app.playing_line(),
            "Playing on \"Fake A\" at 48000 Hz, 256 frames (5.3 ms)."
        );
    }

    #[test]
    fn a_saved_device_plays_when_it_is_there() {
        let dir = folder("saved");
        write(&dir, "(audio: (device: Named(\"Fake B\")))");
        let fake = two_devices();
        let _app = start(args(&dir), &fake);
        assert_eq!(fake.opened(), ["Fake B"]);
    }

    #[test]
    fn a_missing_device_falls_back_and_the_file_is_unchanged() {
        let dir = folder("missing");
        let text = "(audio: (device: Named(\"Fake B\")))";
        let path = write(&dir, text);
        let fake = Rc::new(Fake::new(&[fake::device("Fake A")], &[]));
        let app = start(args(&dir), &fake);
        assert_eq!(fake.opened(), ["Fake A"]);
        assert!(app.status.contains("\"Fake B\" is not connected"), "{}", app.status);
        assert!(app.status.contains("Playing on \"Fake A\""), "{}", app.status);
        assert_eq!(std::fs::read_to_string(path).unwrap(), text);
        assert_eq!(
            app.rig.shown_audio().device,
            DeviceChoice::Named("Fake B".into()),
            "the window still shows the saved choice"
        );
    }

    #[test]
    fn a_rate_the_device_does_not_offer_plays_at_its_default() {
        let dir = folder("rate");
        let text = "(audio: (sample_rate: Hz(96000)))";
        let path = write(&dir, text);
        let app = start(args(&dir), &two_devices());
        assert_eq!(app.sample_rate, 48_000.0);
        assert!(
            app.status.contains("does not offer 96000 Hz. Using 48000 Hz."),
            "{}",
            app.status
        );
        assert_eq!(std::fs::read_to_string(path).unwrap(), text);
    }

    #[test]
    fn a_rate_change_keeps_the_patch_and_its_history() {
        let dir = folder("restart");
        let mut app = start(args(&dir), &two_devices());
        let constant = loom_modules::registry().resolve("loom.math.constant").unwrap();
        for x in [100.0, 200.0, 300.0] {
            app.add_module(constant, iced::Point::new(x, 100.0));
        }
        app.controller.set_tempo(90.0).unwrap();
        let before = undo::Snapshot::take(&app.controller, &app.positions);
        let ids: Vec<_> = app.controller.graph().nodes().map(|(id, _)| id).collect();
        assert_eq!(app.edits.history.len(), 3);
        assert!(app.unsaved());

        change(&mut app, Change::Rate(RateChoice::Hz(44_100)));
        assert_eq!(app.controller.config().sample_rate, 44_100.0);
        assert_eq!(app.sample_rate, 44_100.0);
        assert_eq!(
            app.rig.last_stop,
            Some(true),
            "the old engine faded out before it stopped"
        );
        assert_eq!(
            app.controller.graph().nodes().map(|(id, _)| id).collect::<Vec<_>>(),
            ids
        );
        assert!(
            undo::diff(
                &before,
                &undo::Snapshot::take(&app.controller, &app.positions),
                &BTreeSet::new()
            )
            .is_none(),
            "the same patch"
        );
        assert_eq!(app.edits.history.len(), 3);
        assert!(app.unsaved());
        assert_eq!(app.controller.transport().tempo, 90.0);
        assert_eq!(
            crate::preferences::load(&dir.join(FILE_NAME))
                .preferences
                .audio
                .sample_rate,
            RateChoice::Hz(44_100)
        );
        // The undo history still applies to the new engine.
        app.undo();
        assert_eq!(app.controller.graph().len(), 2);

        // Picking it again does nothing.
        let generation = app.generation;
        change(&mut app, Change::Rate(RateChoice::Hz(44_100)));
        assert_eq!(app.generation, generation);
    }

    #[test]
    fn a_device_that_fails_to_open_leaves_the_old_one_playing() {
        let dir = folder("broken-device");
        let fake = Fake::new(&[fake::device("Fake A"), fake::device("Fake B")], &[]);
        fake.broken.borrow_mut().push("Fake B".into());
        let fake = Rc::new(fake);
        let mut app = start(args(&dir), &fake);
        let generation = app.generation;
        change(&mut app, Change::Device(DeviceChoice::Named("Fake B".into())));
        assert_eq!(app.generation, generation);
        assert!(app._audio.is_some());
        assert_eq!(app.rig.playing.as_ref().unwrap().device, "Fake A");
        assert!(!dir.join(FILE_NAME).exists(), "nothing saved");
        assert_eq!(
            app.status,
            "Could not use \"Fake B\" at 48000 Hz: the device is busy. Still playing on \"Fake A\"."
        );
    }

    #[test]
    fn a_device_that_fails_to_start_hands_back_to_the_old_one() {
        let dir = folder("wont-start");
        let fake = Fake::new(&[fake::device("Fake A"), fake::device("Fake B")], &["Keys"]);
        fake.wont_start.borrow_mut().push("Fake B".into());
        let fake = Rc::new(fake);
        let mut app = start(args(&dir), &fake);
        let constant = loom_modules::registry().resolve("loom.math.constant").unwrap();
        app.add_module(constant, iced::Point::new(100.0, 100.0));
        let modules = app.controller.graph().len();

        change(&mut app, Change::Device(DeviceChoice::Named("Fake B".into())));
        assert_eq!(fake.opened(), ["Fake A", "Fake B", "Fake A"], "back to the old device");
        assert!(app._audio.is_some(), "sound is still playing");
        assert_eq!(app.rig.playing.as_ref().unwrap().device, "Fake A");
        assert_eq!(app.controller.graph().len(), modules, "with the same patch");
        assert_eq!(app.edits.history.len(), 1);
        assert!(fake.is_open("Keys"));
        assert!(!dir.join(FILE_NAME).exists(), "nothing saved");
        assert_eq!(
            app.status,
            "Could not use \"Fake B\": the stream did not start. Still playing on \"Fake A\"."
        );
    }

    #[test]
    fn when_neither_device_starts_audio_is_off_and_nothing_is_saved() {
        let dir = folder("neither-starts");
        let fake = Fake::new(&[fake::device("Fake A"), fake::device("Fake B")], &[]);
        // Fake A started at boot, which does not go through the host's
        // start; it refuses only when it is started again.
        fake.wont_start.borrow_mut().extend(["Fake A".into(), "Fake B".into()]);
        let fake = Rc::new(fake);
        let mut app = start(args(&dir), &fake);
        assert!(app._audio.is_some());

        change(&mut app, Change::Device(DeviceChoice::Named("Fake B".into())));
        assert_eq!(
            fake.opened(),
            ["Fake A", "Fake B", "Fake A"],
            "it tried the old device again"
        );
        assert!(app._audio.is_none(), "audio is off");
        assert!(app.rig.playing.is_none());
        assert!(!dir.join(FILE_NAME).exists(), "nothing saved");
        assert_eq!(
            app.status,
            "Could not use \"Fake B\": the stream did not start. Audio is off."
        );

        // The failed stream took its engine; an idle one takes its place,
        // so edits still work, well past the queue's size (#220).
        assert!(app.idle_engine.is_some());
        let constant = loom_modules::registry().resolve("loom.math.constant").unwrap();
        app.add_module(constant, iced::Point::new(100.0, 100.0));
        let id = *app.selection.first().unwrap();
        for i in 0..2_000 {
            let _ = app.update(crate::Message::Param {
                module: id,
                index: 0,
                position: (i % 100) as f32 / 100.0,
            });
            assert!(!app.status.contains("queue is full"), "move {i}: {}", app.status);
        }
    }

    #[test]
    fn a_device_change_opens_the_new_device_and_fades_the_old_one() {
        let dir = folder("device");
        let fake = two_devices();
        let mut app = start(args(&dir), &fake);
        change(&mut app, Change::Device(DeviceChoice::Named("Fake B".into())));
        assert_eq!(fake.opened(), ["Fake A", "Fake B"]);
        assert_eq!(app.rig.last_stop, Some(true));
        assert_eq!(app.rig.playing.as_ref().unwrap().device, "Fake B");
        assert_eq!(
            crate::preferences::load(&dir.join(FILE_NAME)).preferences.audio.device,
            DeviceChoice::Named("Fake B".into())
        );
    }

    #[test]
    fn open_new_and_quit_fade_out_first() {
        let dir = folder("fade");
        let mut app = start(args(&dir), &two_devices());
        assert!(app.replace(Source::Empty));
        assert_eq!(app.rig.last_stop, Some(true), "New faded out");
        app.rig.last_stop = None;
        app.quit = true;
        let _ = app.update(crate::Message::Tick);
        assert_eq!(app.rig.last_stop, Some(true), "quit faded out");
    }

    #[test]
    fn turning_an_input_off_releases_its_notes_and_is_saved() {
        let dir = folder("midi");
        let fake = Rc::new(Fake::new(&[fake::device("Fake A")], &["Pad", "Keys"]));
        let mut app = start(args(&dir), &fake);
        let note = loom_modules::registry().resolve("loom.midi.note").unwrap();
        app.add_module(note, iced::Point::new(100.0, 100.0));
        let id = *app.selection.iter().next().unwrap();
        let gate = app
            .controller
            .graph()
            .node(loom_core::ModuleId(id))
            .unwrap()
            .descriptor
            .display_index("gate")
            .unwrap();
        let wait_for = |app: &App, value: f32| {
            for _ in 0..200 {
                if app.controller.display(loom_core::ModuleId(id), gate) == Some(value) {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            false
        };
        fake.play("Pad", &[0x90, 60, 100]);
        assert!(wait_for(&app, 1.0), "the note plays");

        app.preferences(Message::Midi("Pad".into(), false));
        assert!(wait_for(&app, 0.0), "and is released");
        assert!(!fake.is_open("Pad"));
        assert!(fake.is_open("Keys"));
        assert!(fake.log.borrow().contains(&Event::MidiClosed("Pad".into())));
        assert_eq!(
            crate::preferences::load(&dir.join(FILE_NAME)).preferences.midi.off,
            ["Pad"]
        );
        fake.play("Keys", &[0x90, 62, 100]);
        assert!(wait_for(&app, 1.0), "the other input still plays");
        assert!(app._audio.is_some(), "audio did not stop");
    }

    /// The panic trigger (panic.md, MIDI trigger): Learn takes the next
    /// control change, the mapping goes in `preferences.ron` and not in
    /// the patch, a value of 0 does nothing, and 127 runs panic, which
    /// releases a held note. Clear removes it.
    #[test]
    fn a_learned_panic_trigger_is_saved_and_runs_panic() {
        let dir = folder("panic");
        let fake = Rc::new(Fake::new(&[fake::device("Fake A")], &["Pad"]));
        let mut app = start(args(&dir), &fake);
        let note = loom_modules::registry().resolve("loom.midi.note").unwrap();
        app.add_module(note, iced::Point::new(100.0, 100.0));
        let id = loom_core::ModuleId(*app.selection.iter().next().unwrap());
        let gate = app
            .controller
            .graph()
            .node(id)
            .unwrap()
            .descriptor
            .display_index("gate")
            .unwrap();
        let wait_for = |app: &App, value: f32| {
            (0..400).any(|_| {
                std::thread::sleep(Duration::from_millis(5));
                app.controller.display(id, gate) == Some(value)
            })
        };

        app.preferences(Message::Open);
        app.preferences(Message::PanicLearn);
        assert_eq!(app.panic_line(), "Waiting for MIDI...");
        fake.play("Pad", &[0xB0, 123, 127]);
        assert!(
            (0..400).any(|_| {
                std::thread::sleep(Duration::from_millis(5));
                app.controller.last_control_change().is_some()
            }),
            "the control change reached the engine"
        );
        let _ = app.update(crate::Message::Midi);
        assert_eq!(app.panic_line(), "Channel 1, controller 123");
        let saved = crate::preferences::load(&dir.join(FILE_NAME)).preferences.panic;
        assert_eq!(saved, Some(crate::preferences::PanicPref::Cc(0, 123)));
        let patch = app.current_patch().to_ron().unwrap();
        assert!(!patch.contains("123"), "the trigger is not in the patch: {patch}");

        fake.play("Pad", &[0x90, 60, 100]);
        assert!(wait_for(&app, 1.0), "the note plays");
        fake.play("Pad", &[0xB0, 123, 0]);
        assert!(!wait_for(&app, 0.0), "a value of 0 does nothing");
        fake.play("Pad", &[0xB0, 123, 127]);
        assert!(wait_for(&app, 0.0), "127 runs panic and the note is released");

        app.preferences(Message::PanicClear);
        assert_eq!(app.panic_line(), "None");
        assert_eq!(crate::preferences::load(&dir.join(FILE_NAME)).preferences.panic, None);
    }

    /// An app on the fake host with a MIDI Note module, and a check that
    /// waits for its gate light to read a value.
    fn note_app(dir: &std::path::Path, fake: &Rc<Fake>) -> (App, impl Fn(&App, f32) -> bool + use<>) {
        let mut app = start(args(dir), fake);
        let note = loom_modules::registry().resolve("loom.midi.note").unwrap();
        app.add_module(note, iced::Point::new(100.0, 100.0));
        let id = loom_core::ModuleId(*app.selection.iter().next().unwrap());
        let gate = note.display_index("gate").unwrap();
        let wait_for = move |app: &App, value: f32| {
            (0..400).any(|_| {
                std::thread::sleep(Duration::from_millis(5));
                app.controller.display(id, gate) == Some(value)
            })
        };
        (app, wait_for)
    }

    /// An input plugged in while the app runs is opened without opening
    /// Preferences, and plays; one in `midi.off` is not opened.
    #[test]
    fn an_input_plugged_in_is_opened_unless_it_is_off() {
        let dir = folder("plug");
        write(&dir, "(midi: (off: [\"Drums\"]))");
        let fake = Rc::new(Fake::new(&[fake::device("Fake A")], &[]));
        let (mut app, wait_for) = note_app(&dir, &fake);
        assert!(app.rig.ports.is_empty());
        app.preferences(Message::Open);
        fake.plug("Keys");
        fake.plug("Drums");
        app.poll_midi();
        assert_eq!(app.open_midi_inputs(), ["Keys"]);
        let listed = &app.rig.window.as_ref().unwrap().midi;
        assert_eq!(
            listed.as_deref(),
            Ok(&["Keys".to_string(), "Drums".to_string()][..]),
            "the window follows"
        );
        assert_eq!(app.status, "MIDI: \"Keys\" connected.");
        fake.play("Keys", &[0x90, 60, 100]);
        assert!(wait_for(&app, 1.0), "a note on it plays");
    }

    /// Unplugging an input with a held note releases it in the engine, and
    /// the app keeps running. The input comes back as a new connection,
    /// and notes play once.
    #[test]
    fn an_input_unplugged_releases_its_notes_and_comes_back() {
        let dir = folder("unplug");
        let fake = Rc::new(Fake::new(&[fake::device("Fake A")], &["Keys"]));
        let (mut app, wait_for) = note_app(&dir, &fake);
        fake.play("Keys", &[0x90, 60, 100]);
        assert!(wait_for(&app, 1.0));
        fake.unplug("Keys");
        app.poll_midi();
        assert!(wait_for(&app, 0.0), "the held note is released");
        assert_eq!(app.status, "MIDI: \"Keys\" disconnected.");
        assert!(!fake.is_open("Keys"));
        assert!(app._audio.is_some(), "the app keeps playing");

        fake.plug("Keys");
        app.poll_midi();
        assert_eq!(fake.open_count(), 1, "one connection, not two");
        fake.play("Keys", &[0x90, 60, 100]);
        assert!(wait_for(&app, 1.0));
        fake.play("Keys", &[0x80, 60, 0]);
        assert!(wait_for(&app, 0.0), "one note off ends it: it was not doubled");
    }

    /// Two inputs hold the same note. Removing one keeps the note until the
    /// other lets go.
    #[test]
    fn a_shared_note_ends_with_its_last_input() {
        let dir = folder("shared-note");
        let fake = Rc::new(Fake::new(&[fake::device("Fake A")], &["Keys", "Pad"]));
        let (mut app, wait_for) = note_app(&dir, &fake);
        fake.play("Keys", &[0x90, 60, 100]);
        fake.play("Pad", &[0x90, 60, 100]);
        assert!(wait_for(&app, 1.0));
        fake.unplug("Keys");
        app.poll_midi();
        assert!(!wait_for(&app, 0.0), "the pad still holds the note");
        fake.play("Pad", &[0x80, 60, 0]);
        assert!(wait_for(&app, 0.0));
    }

    /// A quick replug between two polls, with a new port identity, counts as
    /// a removal: the old connection closes, its note is released, and the
    /// new one plays. Ten quick cycles leak nothing and leave no stuck note.
    #[test]
    fn a_quick_replug_is_a_new_connection() {
        let dir = folder("replug");
        let fake = Rc::new(Fake::new(&[fake::device("Fake A")], &["Keys"]));
        let (mut app, wait_for) = note_app(&dir, &fake);
        fake.play("Keys", &[0x90, 60, 100]);
        assert!(wait_for(&app, 1.0));
        fake.unplug("Keys");
        fake.plug("Keys");
        app.poll_midi();
        assert!(wait_for(&app, 0.0), "the old connection's note is released");
        assert_eq!(fake.open_count(), 1);
        for _ in 0..10 {
            fake.play("Keys", &[0x90, 62, 100]);
            fake.unplug("Keys");
            app.poll_midi();
            fake.plug("Keys");
            app.poll_midi();
        }
        assert!(wait_for(&app, 0.0), "no stuck note");
        assert_eq!(fake.open_count(), 1, "no leaked connection");
        assert_eq!(app.open_midi_inputs(), ["Keys"]);
        fake.play("Keys", &[0x90, 64, 100]);
        assert!(wait_for(&app, 1.0), "and it still plays");
    }

    /// Two inputs with the same name are told apart by order.
    #[test]
    fn same_named_inputs_are_numbered() {
        let dir = folder("same-names");
        let fake = Rc::new(Fake::new(&[fake::device("Fake A")], &["Pad", "Pad"]));
        let app = start(args(&dir), &fake);
        assert_eq!(app.open_midi_inputs(), ["Pad", "Pad (2)"]);
    }

    /// An input that fails to open is reported, at start, when it is
    /// plugged in, and when it is turned on, and the others still play. A
    /// poll that finds nothing new says nothing, and a list that cannot be
    /// read changes nothing.
    #[test]
    fn inputs_that_fail_are_reported_and_the_others_play() {
        let dir = folder("broken-midi");
        let fake = Rc::new(Fake::new(&[fake::device("Fake A")], &["Keys", "Busy"]));
        fake.broken_midi.borrow_mut().push("Busy".into());
        fake.broken_midi.borrow_mut().push("Late".into());
        let mut app = start(args(&dir), &fake);
        assert_eq!(app.open_midi_inputs(), ["Keys"]);
        assert!(app.status.contains("Busy: the port is busy"), "{}", app.status);

        app.status.clear();
        app.poll_midi();
        assert!(app.status.is_empty(), "nothing changed: {}", app.status);
        fake.plug("Late");
        app.poll_midi();
        assert!(app.status.contains("could not open \"Late\""), "{}", app.status);
        assert_eq!(app.open_midi_inputs(), ["Keys"]);

        app.preferences(Message::Midi("Busy".into(), true));
        assert!(app.status.starts_with("MIDI: Busy"), "{}", app.status);
        assert_eq!(app.open_midi_inputs(), ["Keys"]);

        fake.midi_list_fails.set(true);
        fake.unplug("Keys");
        app.poll_midi();
        assert_eq!(app.open_midi_inputs(), ["Keys"], "an unreadable list closes nothing");
    }

    /// When the MIDI inputs cannot be listed at start, the status says so.
    #[test]
    fn an_unreadable_input_list_at_start_is_reported() {
        let dir = folder("no-midi-service");
        let fake = Rc::new(Fake::new(&[fake::device("Fake A")], &["Keys"]));
        fake.midi_list_fails.set(true);
        let app = start(args(&dir), &fake);
        assert!(app.rig.ports.is_empty());
        assert!(app.status.contains("the MIDI service is not running"), "{}", app.status);
    }

    /// The Panic section of the window: Learn shows it is waiting and turns
    /// into Cancel, a second press cancels, and nothing arrived means no
    /// trigger. The window draws in each state.
    #[test]
    fn the_panic_section_learns_and_cancels() {
        let dir = folder("panic-window");
        let fake = Rc::new(Fake::new(&[fake::device("Fake A")], &["Pad"]));
        let mut app = start(args(&dir), &fake);
        app.preferences(Message::Open);
        assert!(app.preferences_view().is_some());
        assert_eq!(app.panic_line(), "None");
        app.preferences(Message::PanicLearn);
        assert!(app.preferences_view().is_some());
        let _ = app.update(crate::Message::Midi);
        assert_eq!(app.panic_line(), "Waiting for MIDI...", "no MIDI came yet");
        app.preferences(Message::PanicLearn);
        assert!(app.status.contains("cancelled"), "{}", app.status);
        assert_eq!(app.panic_line(), "None");

        // A note on is learned too.
        app.preferences(Message::PanicLearn);
        fake.play("Pad", &[0x99, 36, 100]);
        assert!(
            (0..400).any(|_| {
                std::thread::sleep(Duration::from_millis(5));
                app.controller.last_note_on().is_some()
            }),
            "the note reached the engine"
        );
        let _ = app.update(crate::Message::Midi);
        assert_eq!(app.panic_line(), "Channel 10, note 36");
        assert!(app.preferences_view().is_some());

        // Closing the window cancels a learn, so a later note on is not
        // saved as the trigger where the player cannot see it.
        app.preferences(Message::PanicLearn);
        app.preferences(Message::Close);
        assert_eq!(app.rig.panic_learn, None, "closing cancels the learn");
    }

    /// The same check through real MIDI inputs: two virtual controllers,
    /// opened by `midir` (audio-qa.md, Virtual MIDI controllers). It needs the operating
    /// system's MIDI service, so it is ignored by default: run it with
    /// `cargo test -p loom-app -- --ignored` on macOS, or on Linux with the
    /// ALSA sequencer.
    #[test]
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[ignore = "needs virtual MIDI ports (macOS, or Linux with ALSA)"]
    fn turning_a_real_input_off_releases_its_notes_and_is_saved() {
        use loom_host_desktop::VirtualController;
        let dir = folder("real-midi");
        let run = format!("Loom test {}", std::process::id());
        // Without a MIDI service (a container with no ALSA sequencer), skip.
        let create = |name: String| match VirtualController::create(&name) {
            Ok(port) => Some(port),
            Err(e) if e.is_missing_hardware() => {
                eprintln!("skipped: {e} (docs/dev/test-devices.md)");
                None
            }
            Err(e) => panic!("{e}"),
        };
        let (Some(mut pad), Some(mut keys)) = (create(format!("{run} Pad")), create(format!("{run} Keys"))) else {
            return;
        };
        let (pad_name, keys_name) = (pad.input_name().unwrap(), keys.input_name().unwrap());
        let fake = Rc::new(Fake {
            real_midi: Some(run),
            ..Fake::new(&[fake::device("Fake A")], &[])
        });
        let mut app = start(args(&dir), &fake);
        let open = |app: &App| app.rig.ports.iter().map(|p| p.name().to_string()).collect::<Vec<_>>();
        assert_eq!(open(&app).len(), 2, "both ports open: {:?}", open(&app));

        let note = loom_modules::registry().resolve("loom.midi.note").unwrap();
        app.add_module(note, iced::Point::new(100.0, 100.0));
        let id = loom_core::ModuleId(*app.selection.iter().next().unwrap());
        let gate = app
            .controller
            .graph()
            .node(id)
            .unwrap()
            .descriptor
            .display_index("gate")
            .unwrap();
        let wait_for = |app: &App, value: f32| {
            (0..400).any(|_| {
                std::thread::sleep(Duration::from_millis(5));
                app.controller.display(id, gate) == Some(value)
            })
        };
        pad.note_on(0, 60, 100).unwrap();
        assert!(wait_for(&app, 1.0), "the note plays");

        app.preferences(Message::Midi(pad_name.clone(), false));
        assert!(wait_for(&app, 0.0), "and is released, though the key is held");
        assert!(pad.holds_notes());
        assert_eq!(open(&app), std::slice::from_ref(&keys_name));
        assert_eq!(
            crate::preferences::load(&dir.join(FILE_NAME)).preferences.midi.off,
            [pad_name]
        );
        pad.note_on(0, 64, 100).unwrap();
        assert!(!wait_for(&app, 1.0), "the input that is off does not play");
        keys.note_on(0, 62, 100).unwrap();
        assert!(wait_for(&app, 1.0), "the other input still plays");
        assert!(app._audio.is_some(), "audio did not stop");
    }

    #[test]
    fn inputs_that_are_off_are_not_opened() {
        let dir = folder("midi-off");
        write(&dir, "(midi: (off: [\"Pad\"]))");
        let fake = Rc::new(Fake::new(&[fake::device("Fake A")], &["Pad", "Keys"]));
        let _app = start(args(&dir), &fake);
        assert!(!fake.is_open("Pad"));
        assert!(fake.is_open("Keys"));
    }

    #[test]
    fn midi_inputs_stay_open_across_a_new_patch() {
        let dir = folder("midi-new");
        let fake = Rc::new(Fake::new(&[fake::device("Fake A")], &["Keys"]));
        let mut app = start(args(&dir), &fake);
        assert!(app.replace(Source::Empty));
        assert!(fake.is_open("Keys"));
        assert_eq!(fake.log.borrow().len(), 3, "opened once: {:?}", fake.log.borrow());
    }

    #[test]
    fn the_device_flag_wins_for_this_run_only() {
        let dir = folder("flag");
        let path = write(&dir, "(audio: (device: Named(\"Fake B\")))");
        let fake = two_devices();
        let mut args = args(&dir);
        args.devices.device = Some("Fake A".into());
        let mut app = start(args, &fake);
        assert_eq!(fake.opened(), ["Fake A"]);
        assert_eq!(app.rig.shown_audio().device, DeviceChoice::Named("Fake A".into()));
        assert!(app.rig.flags.device.is_some(), "the window shows the flag note");
        app.quit = true;
        let _ = app.update(crate::Message::Tick);
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            "(audio: (device: Named(\"Fake B\")))"
        );
    }

    #[test]
    fn the_midi_flag_opens_matching_inputs_even_when_off() {
        let dir = folder("midi-flag");
        write(&dir, "(midi: (off: [\"Keys 2\"]))");
        let fake = Rc::new(Fake::new(&[fake::device("Fake A")], &["Pad", "Keys", "Keys 2"]));
        let mut args = args(&dir);
        args.devices.midi = Some("Keys".into());
        let _app = start(args, &fake);
        assert!(!fake.is_open("Pad"));
        assert!(fake.is_open("Keys") && fake.is_open("Keys 2"));
    }

    #[test]
    fn the_saved_output_safety_applies_unless_a_flag_sets_it() {
        let dir = folder("safety");
        write(&dir, "(output_safety: \"clean\")");
        let fake = two_devices();
        let app = start(args(&dir), &fake);
        assert_eq!(app.controller.config().output_safety, OutputSafety::Clean);

        let mut flagged = args(&dir);
        flagged.output_safety = Some(OutputSafety::LowLatency);
        let app = start(flagged, &fake);
        assert_eq!(app.controller.config().output_safety, OutputSafety::LowLatency);
        assert!(app.rig.safety_flag);
    }

    #[test]
    fn the_safety_flag_is_shown_and_never_saved() {
        let dir = folder("safety-flag");
        let path = write(&dir, "(output_safety: \"clean\")");
        let mut flagged = args(&dir);
        flagged.output_safety = Some(OutputSafety::Off);
        let mut app = start(flagged, &two_devices());
        assert_eq!(app.controller.config().output_safety, OutputSafety::Off);
        assert_eq!(app.output_safety, OutputSafety::Off, "the window shows Off");
        assert!(app.rig.safety_flag, "with the flag note");
        app.quit = true;
        let _ = app.update(crate::Message::Tick);
        assert_eq!(std::fs::read_to_string(path).unwrap(), "(output_safety: \"clean\")");
    }

    #[test]
    fn a_failed_save_still_changes_the_mode() {
        let dir = folder("unwritable");
        // A file where the settings folder should be: nothing can be saved.
        std::fs::write(dir.join("blocked"), "").unwrap();
        let mut args = args(&dir);
        args.devices.preferences = Some(dir.join("blocked").join(FILE_NAME));
        let mut app = start(args, &two_devices());
        app.preferences(Message::Safety(OutputSafety::Clean));
        assert_eq!(app.rig.sent, [OutputSafety::Clean], "the engine runs Clean");
        assert!(
            app.status.ends_with("The change applies until you quit."),
            "{}",
            app.status
        );
    }

    #[test]
    fn picking_a_mode_tells_the_engine_and_saves_it() {
        let dir = folder("pick");
        let path = dir.join(FILE_NAME);
        let mut app = start(args(&dir), &two_devices());
        app.preferences(Message::Safety(OutputSafety::Off));
        assert_eq!(app.rig.sent, [OutputSafety::Off]);
        assert_eq!(
            crate::preferences::load(&path).preferences.output_safety,
            OutputSafety::Off
        );
        let written = std::fs::metadata(&path).unwrap().modified().unwrap();

        // The mode in effect: nothing sent, nothing written.
        std::thread::sleep(Duration::from_millis(20));
        app.preferences(Message::Safety(OutputSafety::Off));
        assert_eq!(app.rig.sent, [OutputSafety::Off]);
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), written);

        assert!(app.replace(Source::Empty));
        assert_eq!(
            app.controller.config().output_safety,
            OutputSafety::Off,
            "the next engine too"
        );
    }

    #[test]
    fn a_broken_file_still_starts_with_safety_on() {
        let dir = folder("broken-file");
        let path = write(&dir, "(output_safety: \"off\"");
        let mut app = start(args(&dir), &two_devices());
        assert_eq!(app.controller.config().output_safety, OutputSafety::LowLatency);
        assert!(app.status.contains("Could not read preferences"), "{}", app.status);
        app.preferences(Message::Safety(OutputSafety::Clean));
        assert_eq!(
            std::fs::read_to_string(dir.join("preferences.broken.ron")).unwrap(),
            "(output_safety: \"off\""
        );
        assert_eq!(
            crate::preferences::load(&path).preferences.output_safety,
            OutputSafety::Clean
        );
    }

    #[test]
    fn with_no_audio_a_choice_is_only_saved() {
        let dir = folder("no-audio");
        let fake = two_devices();
        let mut app = start(
            Args {
                no_audio: true,
                ..args(&dir)
            },
            &fake,
        );
        assert_eq!(app.playing_line(), "Audio is off for this run (--no-audio).");
        change(&mut app, Change::Device(DeviceChoice::Named("Fake B".into())));
        assert!(fake.opened().is_empty());
        assert_eq!(
            crate::preferences::load(&dir.join(FILE_NAME)).preferences.audio.device,
            DeviceChoice::Named("Fake B".into())
        );
    }

    #[test]
    fn the_window_opens_and_closes_with_its_shortcut_and_escape() {
        use iced::keyboard::{Event, Key, Modifiers, key::Named};
        let mut app = start(args(&folder("keys")), &two_devices());
        let press = |key: Key, modifiers: Modifiers| {
            crate::Message::Key(Event::KeyPressed {
                key: key.clone(),
                modified_key: key,
                physical_key: iced::keyboard::key::Physical::Unidentified(
                    iced::keyboard::key::NativeCode::Unidentified,
                ),
                location: iced::keyboard::Location::Standard,
                modifiers,
                text: None,
                repeat: false,
            })
        };
        let _ = app.update(press(Key::Character(",".into()), Modifiers::COMMAND));
        assert!(app.rig.window.is_some());
        let _ = app.update(press(Key::Named(Named::Escape), Modifiers::empty()));
        assert!(app.rig.window.is_none());
    }

    /// unwrap_reopen_error strips the restart wrapper while preserving raw reasons.
    #[test]
    fn unwrap_reopen_error_normalizes_various_formats() {
        assert_eq!(
            App::unwrap_reopen_error("Could not use \"Fake B\": the stream did not start. Audio is off."),
            "the stream did not start"
        );
        assert_eq!(
            App::unwrap_reopen_error("Could not use \"Fake A\": the device is busy. Audio is off."),
            "the device is busy"
        );
        assert_eq!(
            App::unwrap_reopen_error("the stream did not start."),
            "the stream did not start"
        );
        assert_eq!(
            App::unwrap_reopen_error("the stream did not start"),
            "the stream did not start"
        );
        assert_eq!(
            App::unwrap_reopen_error("no audio output device found"),
            "no audio output device found"
        );
    }

    /// When the initial device fails to open at boot, the app reports the error and preserves the idle engine.
    #[test]
    fn boot_open_failure_leaves_engine_idle_and_reports_error() {
        let dir = folder("boot-open-fail");
        let fake = Fake::new(&[fake::device("Fake A"), fake::device("Fake B")], &[]);
        fake.broken.borrow_mut().push("Fake A".into());
        let fake = Rc::new(fake);
        let app = start(args(&dir), &fake);
        assert!(app._audio.is_none());
        assert!(app.idle_engine.is_some());
        assert!(
            app.audio_name.contains("the device is busy"),
            "audio_name: {:?}",
            app.audio_name
        );
    }

    /// AudioPoll subscription is active when audio is enabled, and Message::AudioPoll triggers poll_audio.
    #[test]
    fn audio_poll_subscription_and_message_handling() {
        let dir = folder("audio-poll-sub");
        let fake = two_devices();
        let mut app = start(args(&dir), &fake);
        let _ = app.subscription();
        let _ = app.update(crate::Message::AudioPoll);
    }

    /// check_audio returns early when no_audio or fake_audio is active.
    #[test]
    fn check_audio_returns_early_when_audio_disabled() {
        let dir = folder("check-audio-disabled");
        let fake = two_devices();
        let mut app = start(args(&dir), &fake);
        app.no_audio = true;
        app.check_audio(Instant::now());
        app.no_audio = false;
        app.fake_audio = true;
        app.check_audio(Instant::now());
    }

    /// handle_audio_loss guards against redundant rebuilds and inactive audio.
    #[test]
    fn handle_audio_loss_guards_and_named_fallback() {
        let dir = folder("handle-audio-loss-guards");
        write(&dir, "(audio: (device: Named(\"Fake A\")))");
        let fake = two_devices();
        let mut app = start(args(&dir), &fake);

        // When audio is running on a named device, loss puts it in lost_audio
        app.handle_audio_loss("initial");
        assert!(app.rig.lost_audio.is_some());

        // Second call while lost_audio is Some returns early without duplicate handling (Rule 3)
        app.handle_audio_loss("duplicate");
        assert!(app.rig.lost_audio.is_some());

        // When audio was never playing and not lost, return early
        app.rig.lost_audio = None;
        app.rig.playing = None;
        assert!(app._audio.is_none());
        app.handle_audio_loss("ignored");
        assert!(app.rig.lost_audio.is_none());

        // When _audio is present but playing is None, falls back to saved device name
        let output = loom_host_desktop::AudioOutput::clock(48_000, 256);
        let parts = loom_core::create_engine(loom_modules::registry(), loom_core::EngineConfig::default());
        let running = output.start(parts.engine).unwrap();
        app._audio = Some(running);
        let mut saved = app.rig.store.saved().clone();
        saved.audio.device = DeviceChoice::Named("Saved Device".into());
        app.rig.save(saved);
        app.handle_audio_loss("unplugged");
        assert_eq!(app.rig.lost_audio.as_ref().unwrap().device, "Saved Device");

        // When _audio is present, playing is None, and saved is SystemDefault, falls back to "audio device"
        let output2 = loom_host_desktop::AudioOutput::clock(48_000, 256);
        let parts2 = loom_core::create_engine(loom_modules::registry(), loom_core::EngineConfig::default());
        app._audio = Some(output2.start(parts2.engine).unwrap());
        app.rig.lost_audio = None;
        let mut saved2 = app.rig.store.saved().clone();
        saved2.audio.device = DeviceChoice::SystemDefault;
        app.rig.save(saved2);
        fake.outputs.borrow_mut().default = None;
        app.handle_audio_loss("unplugged");
        assert_eq!(app.rig.lost_audio.as_ref().unwrap().device, "audio device");

        // When check_audio runs sooner than 1s after attempt, it waits without retrying
        let recent = Instant::now() + Duration::from_millis(100);
        app.check_audio(recent);
        assert_eq!(app.rig.lost_audio.as_ref().unwrap().attempts, 0);
    }
}
