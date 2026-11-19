//! MIDI input connections, panic learn state, and clock routing.

use crate::devices::MidiPort;
use crate::{App, devices};
use loom_core::{LoomError, MidiInputId};
use loom_host_desktop::{MidiInputInfo, SharedMidi};
use std::collections::BTreeMap;

/// A change to the engine's note tracking that still needs queue space.
#[derive(Clone, Copy)]
enum InputTracking {
    Open,
    Release,
}

/// The MIDI rig and its links to the current engine.
#[derive(Default)]
pub(crate) struct MidiRig {
    /// MIDI from every open input, into the current engine. `None` until
    /// the first engine is attached.
    pub input: Option<SharedMidi>,
    /// Open MIDI inputs.
    pub ports: Vec<Box<dyn MidiPort>>,
    /// Tracking changes the current engine has yet to accept, by input ID.
    pending: BTreeMap<MidiInputId, InputTracking>,
    /// Output routing shared with the playback worker.
    pub(crate) clock_ports: loom_host_desktop::ClockPorts,
    /// Selected external name once it has taken over, including while absent.
    pub(crate) following_clock: Option<String>,
    /// Last input ID sent to the current engine. Zero means disconnected.
    clock_input: Option<MidiInputId>,
    /// Last published clock loss state and output failure count.
    clock_lost: bool,
    clock_failures: u64,
    /// Last reported discovery or connection error.
    clock_routing_error: Option<String>,
    /// Last displayed tempo and Run state, to refresh values only when needed.
    clock_display: Option<(u64, bool)>,
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
}

impl MidiRig {
    /// Starts with the saved set of outputs that send clock.
    pub(super) fn new(clock_outputs: &[String]) -> Self {
        let midi = Self::default();
        midi.clock_ports.set_enabled(clock_outputs);
        midi
    }

    /// The clock input last sent to the engine, for connection tests.
    #[cfg(test)]
    pub(in crate::settings) fn clock_input(&self) -> Option<MidiInputId> {
        self.clock_input
    }

    /// Points MIDI at a new engine. Returns true the first time, when the
    /// inputs are still to be opened.
    pub fn attach(&mut self, sender: loom_core::MidiSender) -> bool {
        self.pending.clear();
        self.clock_input = None;
        if let Some(midi) = &self.input {
            midi.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .replace(sender);
            return false;
        }
        self.input = Some(loom_host_desktop::share_midi(sender));
        true
    }

    /// Whether polling is needed even when the host's port list is unchanged.
    pub(crate) fn tracking_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// An input id no open input uses, going round from the last one.
    fn free_input(&mut self) -> Option<MidiInputId> {
        let max = loom_core::MAX_MIDI_INPUTS;
        for step in 1..max {
            let id = (self.last_input + step - 1) % (max - 1) + 1;
            if !self.pending.contains_key(&MidiInputId(id)) && self.ports.iter().all(|port| port.input().0 != id) {
                self.last_input = id;
                return Some(MidiInputId(id));
            }
        }
        None
    }
}

impl App {
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
        self.rig.midi.ports.iter().map(|port| port.name().to_string()).collect()
    }

    /// Opens a MIDI input: the engine first starts tracking its id, then
    /// the host connects it. On an error nothing stays open.
    fn open_input(&mut self, name: &str) -> Result<(), String> {
        let midi = self.rig.midi.input.clone().ok_or("no engine to play")?;
        let input = self.rig.midi.free_input().ok_or("too many MIDI inputs are open")?;
        self.session
            .controller
            .open_midi_input(input)
            .map_err(|e| e.to_string())?;
        midi.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear_input(input);
        match self.rig.host.open_midi(name, midi, input) {
            Ok(port) => {
                self.rig.midi.ports.push(port);
                self.sync_clock_source(false);
                Ok(())
            }
            Err(e) => {
                self.release_input_tracking(input);
                Err(e)
            }
        }
    }

    /// Closes open input `index`, and has the engine release every note it
    /// held (device-changes.md, Held notes belong to an input).
    fn close_input(&mut self, index: usize) {
        let port = self.rig.midi.ports.remove(index);
        let input = port.input();
        port.close();
        if let Some(midi) = &self.rig.midi.input {
            midi.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clear_input(input);
        }
        self.release_input_tracking(input);
        self.sync_clock_source(false);
    }

    /// Releases notes as soon as the engine can take the command. The
    /// pending ID stays reserved so its notes cannot affect a new port.
    fn release_input_tracking(&mut self, input: MidiInputId) {
        self.rig.midi.pending.insert(input, InputTracking::Release);
        self.retry_input_tracking();
    }

    /// Sends pending tracking changes. A sent command leaves the list
    /// because commands on the same engine are processed in order.
    fn retry_input_tracking(&mut self) {
        let controller = &mut self.session.controller;
        let alerts = &mut self.alerts;
        self.rig.midi.pending.retain(|&input, tracking| {
            let result = match tracking {
                InputTracking::Open => controller.open_midi_input(input),
                InputTracking::Release => controller.release_midi_input(input),
            };
            match result {
                Ok(()) => false,
                Err(LoomError::QueueFull) => true,
                Err(error) => {
                    alerts.warn(format!("MIDI: {error}"));
                    false
                }
            }
        });
    }

    /// After a new engine is attached: tracks the open inputs on it, or the
    /// first time opens the inputs the preferences and flags choose, when
    /// `connect` is true. Returns a status bar line about an input that
    /// failed to open; the others still play.
    pub(crate) fn connect_midi(&mut self, first: bool, connect: bool) -> Option<String> {
        let rig = &mut self.rig.midi;
        rig.pending
            .extend(rig.ports.iter().map(|port| (port.input(), InputTracking::Open)));
        self.retry_input_tracking();
        self.sync_clock_source(true);
        if !(first && connect) {
            return None;
        }
        let list = match self.rig.host.midi_list() {
            Ok(list) => list,
            Err(e) => return Some(format!("MIDI: {e}")),
        };
        let names: Vec<String> = list.iter().map(|input| input.name.clone()).collect();
        self.rig.midi.seen_midi = list;
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
        self.retry_input_tracking();
        self.poll_midi_clock();
        let Ok(list) = self.rig.host.midi_list() else { return };
        if let Some(window) = self.overlays.preferences_mut() {
            window.lists.midi = Ok(list.iter().map(|input| input.name.clone()).collect());
        }
        if self.rig.midi.input.is_none() || list == self.rig.midi.seen_midi {
            self.rig.midi.seen_midi = list;
            return;
        }
        let mut lines = Vec::new();
        let mut reopen = Vec::new();
        let mut index = 0;
        while index < self.rig.midi.ports.len() {
            let port = &self.rig.midi.ports[index];
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
            let new = !self.rig.midi.seen_midi.iter().any(|seen| seen.name == name);
            let open = self.rig.midi.ports.iter().any(|port| port.name() == name);
            if open || !(new || reopen.contains(&name)) {
                continue;
            }
            match self.open_input(&name) {
                Ok(()) => lines.push(format!("MIDI: \"{name}\" connected.")),
                Err(e) => lines.push(format!("MIDI: could not open \"{name}\": {e}")),
            }
        }
        self.rig.midi.seen_midi = list;
        if !lines.is_empty() {
            self.alerts.say(lines.join(" "));
        }
    }

    /// Turns a MIDI input on or off. Notes held on an input that is turned
    /// off are released. Audio does not stop.
    pub(in crate::settings) fn set_midi_input(&mut self, name: &str, on: bool) {
        let open = self.rig.midi.ports.iter().position(|port| port.name() == name);
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

    /// Applies an explicit clock source choice and remembers it for this rig.
    pub(in crate::settings) fn choose_clock_source(&mut self, name: Option<String>) {
        if name
            .as_ref()
            .is_some_and(|name| !self.rig.midi.ports.iter().any(|port| port.name() == name))
        {
            return;
        }
        let mut prefs = self.rig.store.saved().clone();
        prefs.midi.clock_source = name;
        if let Some(error) = self.rig.save(prefs) {
            self.alerts.warn(error);
        }
        self.sync_clock_source(false);
    }

    /// Keeps the display name across reconnects and assigns the engine's input ID.
    fn sync_clock_source(&mut self, new_engine: bool) {
        let wanted = self.rig.store.saved().midi.clock_source.as_ref();
        let same = wanted.is_some() && wanted == self.rig.midi.following_clock.as_ref();
        let open = wanted.and_then(|name| self.rig.midi.ports.iter().find(|port| port.name() == name));
        let input = open
            .map(|port| port.input())
            .or_else(|| (same && !new_engine).then_some(MidiInputId(0)));
        if !new_engine && input == self.rig.midi.clock_input {
            return;
        }
        if let Err(error) = self.session.controller.select_midi_clock(input, same && !new_engine) {
            self.alerts.warn(error.to_string());
            return;
        }
        let following = input.and(wanted).cloned();
        let changed = self.rig.midi.following_clock != following;
        self.rig.midi.clock_input = input;
        self.rig.midi.following_clock = following;
        if changed || (new_engine && self.rig.midi.following_clock.is_some()) {
            self.rig.midi.clock_display = None;
            self.refresh();
        }
    }

    /// Persists and applies one output's Send clock setting.
    pub(in crate::settings) fn choose_clock_output(&mut self, name: String, on: bool) {
        let mut prefs = self.rig.store.saved().clone();
        prefs.midi.send_clock.retain(|output| output != &name);
        if on {
            prefs.midi.send_clock.push(name);
        }
        if let Some(error) = self.rig.save(prefs) {
            self.alerts.warn(error);
        }
        self.rig
            .midi
            .clock_ports
            .set_enabled(&self.rig.store.saved().midi.send_clock);
    }

    /// Reads new clock status from the engine and desktop worker.
    fn poll_midi_clock(&mut self) {
        self.sync_clock_source(false);
        if let Some(window) = self.overlays.preferences_mut() {
            window.lists.midi_outputs = self.rig.host.midi_clock_outputs();
        }
        let stats = self.session.controller.stats();
        let lost = stats.clock_lost.load(std::sync::atomic::Ordering::Relaxed);
        let tempo = stats.transport_tempo.load(std::sync::atomic::Ordering::Relaxed);
        let display = self.rig.midi.following_clock.as_ref().map(|_| {
            (
                (f64::from_bits(tempo) * 10.0).round() as u64,
                crate::transport_running(&self.session.controller).unwrap_or(false),
            )
        });
        if let Some(name) = &self.rig.midi.following_clock {
            if lost && !self.rig.midi.clock_lost {
                self.alerts.say(format!(
                    "MIDI clock from \"{name}\" stopped. Running at {:.1} BPM.",
                    f64::from_bits(tempo)
                ));
            } else if !lost && self.rig.midi.clock_lost {
                self.alerts.say(format!("Following {name} clock"));
            }
        }
        self.rig.midi.clock_lost = lost;
        if display != self.rig.midi.clock_display {
            self.rig.midi.clock_display = display;
            self.refresh_values();
        }
        let routing_error = self.rig.midi.clock_ports.routing_error();
        if routing_error != self.rig.midi.clock_routing_error {
            if let Some(error) = &routing_error {
                self.alerts.warn(error.clone());
            }
            self.rig.midi.clock_routing_error = routing_error;
        }
        let failures = self.rig.midi.clock_ports.failures();
        if failures != self.rig.midi.clock_failures {
            self.rig.midi.clock_failures = failures;
            self.alerts
                .warn("MIDI clock output missed its deadline or could not send. Resynchronizing.");
        }
    }
}
