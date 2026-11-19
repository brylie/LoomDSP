//! The Preferences window and its messages (preferences.md).

use crate::App;
use crate::devices::{self, Host};
use crate::overlays::Overlay;
use crate::preferences::{AudioPrefs, BufferChoice, DeviceChoice, InputChoice, RateChoice};
use loom_core::OutputSafety;
use loom_host_desktop::Outputs;

pub(crate) mod access;
mod model;
mod rig;
mod view;

pub(crate) use model::{Model, Part, Picks};
pub use rig::{Rig, RigLibrary, open_output, open_store};
pub(crate) use view::author_part;

/// A message for the Preferences window.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// Selects Internal or a named MIDI clock input.
    ClockSource(Option<String>),
    /// Turns MIDI clock on or off for an output.
    SendClock(String, bool),
    Open,
    Close,
    /// Read the device lists again.
    Refresh,
    Safety(OutputSafety),
    /// Show signal activity on connected ports.
    Activity(bool),
    /// Chooses the patch map mode, or cycles it from the toolbar.
    Map(Option<crate::preferences::MapMode>),
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

/// The devices the machine offers, as read when the window opened.
pub struct DeviceLists {
    outputs: Result<Outputs, String>,
    inputs: Result<devices::Inputs, String>,
    midi: Result<Vec<String>, String>,
    midi_outputs: Result<Vec<String>, String>,
}

impl DeviceLists {
    fn read(host: &dyn Host) -> Self {
        Self {
            outputs: host.outputs(),
            inputs: host.audio_inputs(),
            midi: host.midi_inputs(),
            midi_outputs: host.midi_clock_outputs(),
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
                self.rig.midi.panic_learn = None;
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
            Message::Map(mode) => self.choose_map(mode),
            Message::Audio(change) => self.change_audio(change),
            Message::Input(choice) => self.choose_audio_input(choice),
            Message::ClockSource(name) => self.choose_clock_source(name),
            Message::SendClock(name, on) => self.choose_clock_output(name, on),
            Message::Midi(name, on) => self.set_midi_input(&name, on),
            Message::PanicLearn => self.learn_panic(),
            Message::PanicClear => self.set_panic_pref(None),
        }
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

    /// Saves an explicit choice, or cycles visibility from the canvas.
    pub(crate) fn choose_map(&mut self, mode: Option<crate::preferences::MapMode>) {
        let mut preferences = self.rig.store.saved().clone();
        let mode = mode.unwrap_or_else(|| preferences.patch_map.next());
        preferences.patch_map = mode;
        if let Some(error) = self.rig.save(preferences) {
            self.alerts.warn(error);
        } else {
            self.alerts.say(format!("Patch map: {mode}"));
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
        self.commit_maintenance();
    }
}

#[cfg(test)]
mod tests;
