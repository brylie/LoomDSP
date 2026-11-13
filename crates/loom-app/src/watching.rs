//! Which watcher threads the app runs, and keeping them in step with its
//! state (ui-layer.md, section 2, rule 7; `watcher.rs`).
//!
//! After each update, [`App::sync_watchers`] works out what each watcher
//! should read, as a key, and starts, stops, or restarts only the ones
//! whose key changed. The handles live where what they watch lives: those
//! that read one engine on the session's engine, which New, Open, and a
//! device change replace, and those that outlive a patch (the control
//! channel, the MIDI ports, the bench report) on the `App`.
//!
//! Watchers start once the window is open ([`Message::StartWatching`], sent
//! by the bridge as it starts). The tests and the offscreen paths never
//! send it, so no thread delivers a message a test did not ask for.

use crate::watcher::{Ledger, Watcher, keep};
use crate::{App, Message, audio_watch, bench, control, devices, input_watch, learn, lights, monitor, preferences};
use loom_ui::Waker;

/// How many audio or input events may be in flight, as their channels held
/// before: a reading and a warning can come together.
const EVENTS: usize = 4;
/// One at a time: fresh values follow once the app takes this one.
const LATEST: usize = 1;

/// The watchers that read one engine. On the session's engine, so a new
/// engine drops them with the one they read.
#[derive(Debug, Default)]
pub(crate) struct EngineWatchers {
    /// The audio stats and stalled callbacks (`audio_watch.rs`).
    audio: Option<Watcher>,
    /// The audio input's health (`input_watch.rs`).
    input: Option<Watcher>,
    /// The lights and readouts on screen (`lights.rs`).
    lights: Option<Watcher>,
    /// Meters, scopes, spectra, and pairs (`monitor.rs`).
    monitor: Option<Watcher>,
    /// Control changes, for mappings and learn mode (`learn.rs`).
    learn: Option<Watcher>,
}

#[cfg(test)]
impl EngineWatchers {
    /// True while none runs, as for a new engine.
    pub(crate) fn none_running(&self) -> bool {
        [&self.audio, &self.input, &self.lights, &self.monitor, &self.learn]
            .iter()
            .all(|watcher| watcher.is_none())
    }
}

/// One ledger per kind of watched message (`watcher::Ledger`): who sent
/// each one still in the queue, so taking it frees only its sender.
#[derive(Debug, Default)]
pub(crate) struct Ledgers {
    audio: Ledger,
    input: Ledger,
    lights: Ledger,
    monitor: Ledger,
    learn: Ledger,
    control: Ledger,
    midi_ports: Ledger,
    bench: Ledger,
}

/// The watchers that outlive a patch, and the ledgers, which outlive every
/// watcher. On the `App`.
#[derive(Debug, Default)]
pub(crate) struct AppWatchers {
    /// Who sent each watched message still in the queue.
    ledgers: Ledgers,
    /// One per control channel listener.
    control: Vec<Watcher>,
    /// The MIDI input list, read on a thread with a real host.
    midi_ports: Option<Watcher>,
    /// The `--bench-seconds` report.
    bench: Option<Watcher>,
}

impl App {
    /// The app took `message`: if a watcher sent it, there is room for its
    /// next one. Called first in `apply`, before anything can return, so no
    /// watcher waits on a message the app dropped.
    pub(crate) fn watched_message_taken(&self, message: &Message) {
        let ledgers = &self.watchers.ledgers;
        let ledger = match message {
            Message::Audio(_) => &ledgers.audio,
            Message::InputAudio(_) => &ledgers.input,
            Message::Lights(_) => &ledgers.lights,
            Message::Telemetry(_) => &ledgers.monitor,
            Message::Midi => &ledgers.learn,
            Message::Control(_) => &ledgers.control,
            Message::MidiPoll => &ledgers.midi_ports,
            Message::BenchDone => &ledgers.bench,
            _ => return,
        };
        ledger.taken();
    }

    /// Starts, stops, or restarts each watcher so it reads what the app's
    /// state says now. Only a watcher whose key changed restarts. Nothing
    /// runs before [`Message::StartWatching`].
    pub(crate) fn sync_watchers(&mut self, waker: &Waker<Message>) {
        if !self.watching {
            return;
        }
        let audio = self.session.engine.stream.stats().map(|stats| audio_watch::Watch {
            stats: stats.clone(),
            engine: self.session.controller.stats().clone(),
        });
        let input = self.input_watch();
        let lights = self.watched_lights();
        let monitor = Some(self.session.engine.monitor.clone());
        let learn = self.learn_watch();
        let midi_ports = self.midi_port_watch();
        let bench = self.reporter.clone();

        let engine = &mut self.session.engine.watchers;
        let ledgers = &self.watchers.ledgers;
        keep(&mut engine.audio, audio, |key, watch| {
            Watcher::start(
                &ledgers.audio,
                key,
                EVENTS,
                waker.clone(),
                Message::Audio,
                move |outlet| {
                    audio_watch::run(&watch, outlet);
                },
            )
        });
        keep(&mut engine.input, input, |key, watch| {
            Watcher::start(
                &ledgers.input,
                key,
                EVENTS,
                waker.clone(),
                Message::InputAudio,
                move |outlet| {
                    input_watch::run(&watch, outlet);
                },
            )
        });
        keep(&mut engine.lights, lights, |key, watch| {
            Watcher::start(
                &ledgers.lights,
                key,
                LATEST,
                waker.clone(),
                Message::Lights,
                move |outlet| {
                    lights::run(&watch, outlet);
                },
            )
        });
        keep(&mut engine.monitor, monitor, |key, watch| {
            Watcher::start(
                &ledgers.monitor,
                key,
                LATEST,
                waker.clone(),
                Message::Telemetry,
                move |outlet| {
                    monitor::run(&watch, outlet);
                },
            )
        });
        keep(&mut engine.learn, learn, |key, watch| {
            Watcher::start(
                &ledgers.learn,
                key,
                LATEST,
                waker.clone(),
                |_: u32| Message::Midi,
                move |outlet| {
                    learn::run(&watch, outlet);
                },
            )
        });
        keep(&mut self.watchers.midi_ports, midi_ports, |key, watch| {
            Watcher::start(
                &ledgers.midi_ports,
                key,
                LATEST,
                waker.clone(),
                |()| Message::MidiPoll,
                move |outlet| {
                    devices::watch_midi(&watch, outlet);
                },
            )
        });
        keep(&mut self.watchers.bench, bench, |key, reporter| {
            Watcher::start(
                &ledgers.bench,
                key,
                LATEST,
                waker.clone(),
                |()| Message::BenchDone,
                move |outlet| {
                    bench::run(&reporter, outlet);
                },
            )
        });
        // The listeners are fixed for the run: started once.
        if self.watchers.control.len() != self.control.len() {
            self.watchers.control = self
                .control
                .iter()
                .map(|listener| {
                    let listener = listener.clone();
                    Watcher::start(
                        &ledgers.control,
                        crate::watcher::key_of(&listener),
                        LATEST,
                        waker.clone(),
                        Message::Control,
                        move |outlet| control::watch(&listener, outlet),
                    )
                })
                .collect();
        }
    }

    /// The audio input's watch, while an input is open.
    fn input_watch(&self) -> Option<input_watch::Watch> {
        let (input, tracker) = self.rig.input.open()?;
        Some(input_watch::Watch {
            status: input.status(),
            stats: self.session.controller.stats().clone(),
            tracker: tracker.clone(),
            device: input.name().to_string(),
            system_default: self.rig.flags.input.is_none()
                && matches!(
                    self.rig.store.saved().audio.input,
                    preferences::InputChoice::SystemDefault
                ),
            list: self.rig.host.audio_input_lister(),
            permission_denied: self.rig.host.audio_input_permission_checker(),
            active: self
                .session
                .controller
                .graph()
                .nodes()
                .filter_map(|(id, node)| {
                    (node.descriptor.id == "loom.io.audio-in")
                        .then(|| self.session.controller.display_source(id, 0))
                        .flatten()
                })
                .collect(),
            displays: self.session.controller.display_reader(),
        })
    }

    /// The control change watch, while there is something to update: mapped
    /// parameters to follow, or learn mode waiting for MIDI.
    fn learn_watch(&self) -> Option<learn::Watch> {
        let mapped = self.session.controller.graph().midi_mappings().next().is_some();
        (mapped || self.session.learn.is_on() || self.rig.panic_learn.is_some()).then(|| learn::Watch {
            reader: self.session.controller.midi_reader(),
            generation: self.generation,
        })
    }

    /// The MIDI input list's watch. With a real host the list is read on a
    /// thread, which wakes the UI only when it changes; a test host is
    /// polled from the UI by a timer instead (`subscriptions.rs`).
    fn midi_port_watch(&self) -> Option<devices::MidiWatch> {
        if self.rig.midi_in.is_none() || self.rig.flags.no_midi {
            return None;
        }
        self.rig.host.midi_lister().map(|list| devices::MidiWatch {
            list,
            seen: self.rig.seen_midi.clone(),
            generation: self.generation,
        })
    }
}

#[cfg(test)]
mod tests;
