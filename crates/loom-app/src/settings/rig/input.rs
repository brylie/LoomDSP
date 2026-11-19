//! Connects the input-device session to preferences and the current patch.

use super::Rig;
use crate::App;
use crate::devices;
use crate::input_session::Demand;
use crate::preferences::InputChoice;

impl Rig {
    /// The input that would be used for this run, without opening a stream.
    pub(in crate::settings) fn effective_input<'a>(&'a self, inputs: &'a devices::Inputs) -> Option<&'a str> {
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
}

impl App {
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
            .output
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
    pub(in crate::settings) fn choose_audio_input(&mut self, choice: InputChoice) {
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
}
