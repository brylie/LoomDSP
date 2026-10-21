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
use loom_core::{EngineConfig, EngineParts, OutputSafety, create_engine};
use loom_host_desktop::{AudioOutput, Outputs, STOP_LIMIT, SharedMidi};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::rc::Rc;

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
    /// Every mode change sent to the engine, for tests.
    #[cfg(test)]
    pub sent: Vec<OutputSafety>,
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
            #[cfg(test)]
            sent: Vec::new(),
        }
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

    /// Points MIDI at a new engine. The first time, it also opens the MIDI
    /// inputs, when `connect` is true. Returns a status bar line about an
    /// input that failed to open.
    pub fn attach(&mut self, sender: loom_core::MidiSender, connect: bool) -> Option<String> {
        if let Some(midi) = &self.midi_in {
            *midi.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = sender;
            return None;
        }
        let midi = loom_host_desktop::share_midi(sender);
        self.midi_in = Some(midi.clone());
        if !connect {
            return None;
        }
        let names = match self.host.midi_inputs() {
            Ok(names) => names,
            Err(e) => return Some(format!("MIDI: {e}")),
        };
        let mut error = None;
        for name in devices::midi_at_start(&names, &self.store.saved().midi.off, &self.flags) {
            // A port that fails is reported, and the others still play.
            match self.host.open_midi(&name, midi.clone()) {
                Ok(port) => self.ports.push(port),
                Err(e) => {
                    error.get_or_insert(format!("MIDI: {e}"));
                }
            }
        }
        error
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
pub fn open_output(
    rig: &Rig,
    no_audio: bool,
    fake_audio: bool,
    record: Option<&std::path::Path>,
) -> (Result<AudioOutput, String>, Vec<String>) {
    let (output, notes) = if no_audio {
        (Err("audio off".to_string()), Vec::new())
    } else if fake_audio {
        let rate = rig.flags.sample_rate.unwrap_or(48_000);
        let block = rig.flags.buffer.unwrap_or(256);
        (Ok(AudioOutput::clock(rate, block)), Vec::new())
    } else {
        devices::open_at_start(&*rig.host, &rig.wanted())
    };
    let output = output.map(|mut output| {
        if let Some(path) = record {
            output.record(path);
        }
        output
    });
    (output, notes)
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
            Message::Close => self.rig.window = None,
            Message::Refresh => {
                if self.rig.window.is_some() {
                    self.rig.window = Some(Window::read(&*self.rig.host));
                }
            }
            Message::Safety(mode) => self.choose_output_safety(mode),
            Message::Audio(change) => self.change_audio(change),
            Message::Midi(name, on) => self.set_midi_input(&name, on),
        }
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
    /// stops, nothing changes.
    fn restart_audio(&mut self, output: AudioOutput) -> Result<(), String> {
        self.finish_edits();
        let rate = output.sample_rate() as f32;
        let config = EngineConfig {
            output_safety: self.output_safety,
            ..EngineConfig::with_sample_rate(rate)
        };
        let EngineParts {
            mut controller,
            engine,
            midi,
            telemetry,
        } = create_engine(loom_modules::registry(), config);
        // The patch is replayed as one undo step from nothing, so module and
        // assignment IDs stay the same and the history still applies.
        let current = undo::Snapshot::take(&self.controller, &self.positions);
        let empty = undo::Snapshot::take(&controller, &BTreeMap::new());
        let mut positions = self.positions.clone();
        let mut rebuild = |controller: &mut loom_core::Controller| -> Result<(), loom_core::LoomError> {
            if let Some(step) = undo::diff(&empty, &current, &BTreeSet::new()) {
                undo::apply(&step, true, controller, &mut positions)?;
            }
            // The transport keeps its tempo, time signature, and whether it
            // runs. Its position starts at beat 0.
            let transport = self.controller.transport();
            controller.set_tempo(transport.tempo)?;
            controller.set_time_signature(transport.time_signature)?;
            controller.set_running(transport.running)?;
            controller.commit()?;
            Ok(())
        };
        rebuild(&mut controller).map_err(|e| format!("Could not restart audio: {e}. The audio is unchanged."))?;

        self.stop_audio();
        self.rig.attach(midi, false);
        let playing = Playing::of(&output);
        let label = format!("{} at {} Hz", output.name(), output.sample_rate());
        self.generation += 1;
        self.controller = controller;
        self.monitor = crate::monitor::Monitor::new(telemetry, self.generation);
        self.live = crate::live::Live::new();
        self.lights.clear();
        self.meter_levels.clear();
        self.sample_rate = rate;
        self.idle_engine = None;
        self.holding = false;
        match output.start(engine) {
            Ok(audio) => {
                self.audio_stats = Some(audio.stats().clone());
                self._audio = Some(audio);
                self.audio_name = label;
                self.rig.playing = Some(playing);
                self.midi = self.rig.midi_in.clone();
            }
            Err(e) => {
                self.audio_name = format!("no audio ({e})");
                self.rig.playing = None;
                self.midi = None;
                return Err(format!("Could not start \"{}\": {e}. Audio is off.", playing.device));
            }
        }
        // Probes for the meters on screen, in the new engine.
        self.commit();
        self.refresh_values();
        Ok(())
    }

    /// Stops audio without a click: the engine fades out, and the stream
    /// stops after two more silent buffers, or after 100 ms (output safety,
    /// rule 14). Used by Open, New, quit, and device changes.
    pub(crate) fn stop_audio(&mut self) {
        if let Some(audio) = self._audio.take() {
            // A full queue cannot fade: the limit then stops the stream.
            let _ = self.controller.fade_out();
            self.rig.last_stop = Some(audio.stop(STOP_LIMIT));
        }
        self.audio_stats = None;
        self.rig.playing = None;
    }

    /// Turns a MIDI input on or off. Notes held on an input that is turned
    /// off are released. Audio does not stop.
    fn set_midi_input(&mut self, name: &str, on: bool) {
        let open = self.rig.ports.iter().position(|port| port.name() == name);
        match (on, open) {
            (true, Some(_)) | (false, None) => return,
            (true, None) => {
                let Some(midi) = self.rig.midi_in.clone() else { return };
                match self.rig.host.open_midi(name, midi) {
                    Ok(port) => self.rig.ports.push(port),
                    Err(e) => {
                        self.status = format!("MIDI: {e}");
                        return;
                    }
                }
            }
            (false, Some(index)) => self.rig.ports.remove(index).close(),
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
                    midi
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
        assert_eq!(app.history.len(), 3);
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
        assert_eq!(app.history.len(), 3);
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
        let mut fake = Fake::new(&[fake::device("Fake A"), fake::device("Fake B")], &[]);
        fake.broken.push("Fake B".into());
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
}
