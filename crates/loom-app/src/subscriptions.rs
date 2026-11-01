//! What the app listens to: timers, keys, MIDI, the control channel, and
//! the watcher threads that wake it.

use crate::matrix::Tab as RightTab;
use crate::{
    App, FileAction, Message, audio_watch, bench, browser, control, devices, input_watch, learn, lights, matrix,
    monitor, panic, preferences, recovery,
};
use iced::Subscription;
use loom_core::ModuleId;
use std::time::Duration;

impl App {
    pub(crate) fn subscription(&self) -> Subscription<Message> {
        let main = if self.measuring {
            // Redraw every frame, so the counter measures real render cost.
            iced::window::frames().map(Message::Frame)
        } else if self.rig.recorder.is_some() {
            // Otherwise the window redraws only when something changes
            // (#90). A recording, a QA tool, ticks to notice a write error.
            iced::time::every(Duration::from_millis(500)).map(|_| Message::Tick)
        } else {
            Subscription::none()
        };
        // The audio numbers and stalled callbacks are read on a thread,
        // which wakes the UI only when the stats bar changes (#90).
        let audio_watch = match self.stream.stats() {
            Some(stats) => Subscription::run_with(
                audio_watch::Watch {
                    stats: stats.clone(),
                    engine: self.controller.stats().clone(),
                },
                audio_watch::stream,
            )
            .map(Message::Audio),
            None => Subscription::none(),
        };
        // An open input always has a drift tracker: both are set and cleared
        // together.
        let input_watch = self
            .rig
            .input
            .as_ref()
            .zip(self.rig.input_tracker.as_ref())
            .map(|(input, tracker)| {
                let stats = self.controller.stats().clone();
                let tracker = tracker.clone();
                Subscription::run_with(
                    input_watch::Watch {
                        status: input.status(),
                        stats,
                        tracker,
                        device: input.name().to_string(),
                        system_default: self.rig.flags.input.is_none()
                            && matches!(
                                self.rig.store.saved().audio.input,
                                preferences::InputChoice::SystemDefault
                            ),
                        list: self.rig.host.audio_input_lister(),
                        permission_denied: self.rig.host.audio_input_permission_checker(),
                        active: self
                            .controller
                            .graph()
                            .nodes()
                            .filter_map(|(id, node)| {
                                (node.descriptor.id == "loom.io.audio-in")
                                    .then(|| self.controller.display_source(id, 0))
                                    .flatten()
                            })
                            .collect(),
                        displays: self.controller.display_reader(),
                    },
                    input_watch::stream,
                )
                .map(Message::InputAudio)
            });
        // Live modulated values move every block, so a patch with an
        // assignment that is on follows them every frame.
        // Without audio the engine does not run, so nothing moves.
        let live = if !self.measuring
            && self.stream.is_playing()
            && self.controller.graph().assignments().any(|(_, a)| a.enabled)
        {
            iced::window::frames().map(|_| Message::LiveFrame)
        } else {
            Subscription::none()
        };
        let telemetry = Subscription::run_with(self.monitor.clone(), monitor::stream).map(Message::Telemetry);
        let close = iced::window::close_requests().map(|_| Message::File(FileAction::Close));
        let control = self
            .control
            .iter()
            .map(|listener| Subscription::run_with(listener.clone(), control::stream).map(Message::Control))
            // Frames only while a command waits to be drawn, so an idle app
            // stays idle.
            .chain(
                self.control_waiting()
                    .then(|| iced::window::frames().map(Message::ControlFrame)),
            );
        // The autosave timer runs only while a copy may be due: once the
        // copy matches the patch, an idle app ticks no more (#90).
        let autosave = if self.document.autosave_due() {
            iced::time::every(recovery::TICK).map(|_| Message::Autosave)
        } else {
            Subscription::none()
        };
        // MIDI inputs plugged in and out are found by polling four times a
        // second: the systems offer no portable callback (device-changes.md).
        // With a real host the list is read on a thread, which wakes the
        // UI only when it changes. A test host is polled from the UI.
        let midi_poll = if self.rig.midi_in.is_some() && !self.rig.flags.no_midi {
            match self.rig.host.midi_lister() {
                Some(list) => Subscription::run_with(
                    devices::MidiWatch {
                        list,
                        seen: self.rig.seen_midi.clone(),
                        generation: self.generation,
                    },
                    devices::midi_stream,
                )
                .map(|()| Message::MidiPoll),
                None => iced::time::every(devices::MIDI_POLL).map(|_| Message::MidiPoll),
            }
        } else {
            Subscription::none()
        };
        // While audio plays, the audio watcher reports stalled callbacks.
        // Once a device is lost, reconnection is tried by polling four
        // times a second (device-changes.md, Audio device loss).
        let waiting_for_input = self.rig.input.is_none()
            && self.stream.is_playing()
            && !self.rig.flags.no_input
            && (self.rig.flags.input.is_some()
                || !matches!(self.rig.store.saved().audio.input, preferences::InputChoice::Off))
            && self
                .controller
                .graph()
                .nodes()
                .any(|(_, node)| node.descriptor.id == "loom.io.audio-in");
        let audio_poll = if !self.stream.without_device() && (!self.stream.is_playing() || waiting_for_input) {
            iced::time::every(Duration::from_millis(250)).map(|_| Message::AudioPoll)
        } else {
            Subscription::none()
        };
        let report = match &self.reporter {
            Some(reporter) => Subscription::run_with(reporter.clone(), bench::stream).map(|()| Message::BenchDone),
            None => Subscription::none(),
        };
        Subscription::batch(
            [
                main,
                audio_watch,
                live,
                self.light_watch(),
                telemetry,
                self.midi_watch(),
                close,
                report,
                autosave,
                midi_poll,
                audio_poll,
            ]
            .into_iter()
            .chain(control)
            .chain(input_watch),
        )
    }

    /// Wakes the UI when control changes arrive, while there is something to
    /// update: mapped parameters to follow, or learn mode waiting for MIDI.
    /// In learn mode, it also listens for Escape.
    pub(crate) fn midi_watch(&self) -> Subscription<Message> {
        let mapped = self.controller.graph().midi_mappings().next().is_some();
        let watch = (mapped || self.learn.is_on() || self.rig.panic_learn.is_some()).then(|| {
            let watch = learn::Watch {
                reader: self.controller.midi_reader(),
                generation: self.generation,
            };
            Subscription::run_with(watch, learn::stream).map(|_| Message::Midi)
        });
        let keys = Some(iced::keyboard::listen().map(Message::Key));
        // The matrix owns arrows and Escape even when a child widget uses
        // the key. Its handler ignores them outside table focus.
        let matrix_keys = (self.matrix.tab == RightTab::Matrix && self.matrix.focused)
            .then(|| iced::event::listen_with(matrix::key_listener));
        // The search field takes Escape for itself, so the browser listens
        // to keys whether or not a widget used them.
        let browser = (self.browser.is_some() && self.help.is_none()).then(|| iced::event::listen_with(browser_key));
        let help_keys = Some(iced::event::listen_with(help_key));
        // The panic key works whatever has focus (panic.md).
        let panic = Some(iced::event::listen_with(panic::key));
        Subscription::batch(
            watch
                .into_iter()
                .chain(keys)
                .chain(matrix_keys)
                .chain(browser)
                .chain(help_keys)
                .chain(panic),
        )
    }

    /// Watches the lights on screen, and sends a message only when one
    /// visibly changes. No lights on screen: no watcher.
    pub(crate) fn light_watch(&self) -> Subscription<Message> {
        match self.watched_lights() {
            Some(watch) => Subscription::run_with(watch, lights::stream).map(Message::Lights),
            None => Subscription::none(),
        }
    }

    /// The lights on screen and where their values live, or `None` when
    /// there are none.
    pub(crate) fn watched_lights(&self) -> Option<lights::Watch> {
        let graph = self.controller.graph();
        let watched: Vec<_> = self
            .visible_displays
            .iter()
            .filter_map(|&id| Some((id, graph.node(ModuleId(id))?.descriptor)))
            .flat_map(|(id, d)| {
                d.panel.widgets.iter().enumerate().filter_map(move |(index, w)| {
                    let display = (!w.is_control()).then(|| d.display_index(w.bind))??;
                    Some(((id, index), display))
                })
            })
            .filter_map(|(light, display)| {
                let source = self.controller.display_source(ModuleId(light.0), display)?;
                Some((light, source))
            })
            .collect();
        (!watched.is_empty()).then(|| lights::Watch {
            reader: self.controller.display_reader(),
            lights: watched,
            generation: self.generation,
        })
    }
}

/// F1 and Escape reach help even while a text field has focus.
pub(crate) fn help_key(event: iced::Event, _status: iced::event::Status, _window: iced::window::Id) -> Option<Message> {
    use iced::keyboard::{Event, Key, key::Named};
    match event {
        iced::Event::Keyboard(Event::KeyPressed {
            key: Key::Named(Named::F1),
            ..
        }) => Some(Message::ShowHelp),
        iced::Event::Keyboard(Event::KeyPressed {
            key: Key::Named(Named::Escape),
            ..
        }) => Some(Message::CloseHelp),
        _ => None,
    }
}

/// The keys the open browser handles, whether or not a widget used them.
pub(crate) fn browser_key(
    event: iced::Event,
    _status: iced::event::Status,
    _window: iced::window::Id,
) -> Option<Message> {
    use iced::keyboard::{Event, Key, key::Named};
    let iced::Event::Keyboard(Event::KeyPressed { key, .. }) = event else {
        return None;
    };
    let nav = match key {
        Key::Named(Named::ArrowUp) => browser::Nav::Up,
        Key::Named(Named::ArrowDown) => browser::Nav::Down,
        Key::Named(Named::PageUp) => browser::Nav::PageUp,
        Key::Named(Named::PageDown) => browser::Nav::PageDown,
        Key::Named(Named::Escape) => browser::Nav::Escape,
        _ => return None,
    };
    Some(Message::Browser(browser::Message::Key(nav)))
}
