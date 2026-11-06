//! What the app listens to: timers, keys, MIDI, the control channel, and
//! the watcher threads that wake it.

use crate::{
    App, FileAction, Message, audio_watch, bench, browser, control, devices, input_watch, learn, lights, monitor,
    preferences, recovery,
};
use iced::{Subscription, Task};
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
        let audio_watch = match self.session.engine.stream.stats() {
            Some(stats) => Subscription::run_with(
                audio_watch::Watch {
                    stats: stats.clone(),
                    engine: self.session.controller.stats().clone(),
                },
                audio_watch::stream,
            )
            .map(Message::Audio),
            None => Subscription::none(),
        };
        let input_watch = self.rig.input.open().map(|(input, tracker)| {
            let stats = self.session.controller.stats().clone();
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
                },
                input_watch::stream,
            )
            .map(Message::InputAudio)
        });
        // Live modulated values move every block, so a patch with an
        // assignment that is on follows them every frame.
        // Without audio the engine does not run, so nothing moves.
        let live = if !self.measuring
            && self.session.engine.stream.is_playing()
            && self.session.controller.graph().assignments().any(|(_, a)| a.enabled)
        {
            iced::window::frames().map(|_| Message::LiveFrame)
        } else {
            Subscription::none()
        };
        let telemetry =
            Subscription::run_with(self.session.engine.monitor.clone(), monitor::stream).map(Message::Telemetry);
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
        let autosave = if self.run_files.autosave_due(&self.session.document) {
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
        let waiting_for_input =
            self.rig.input.port().is_none() && self.session.engine.stream.is_playing() && self.input_wanted();
        let audio_poll = if !self.session.engine.stream.without_device()
            && (!self.session.engine.stream.is_playing() || waiting_for_input)
        {
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
            .chain(input_watch)
            .chain(self.a11y_subscription()),
        )
    }

    /// Wakes the UI when control changes arrive, while there is something to
    /// update: mapped parameters to follow, or learn mode waiting for MIDI.
    /// Also the app's input listeners.
    pub(crate) fn midi_watch(&self) -> Subscription<Message> {
        let mapped = self.session.controller.graph().midi_mappings().next().is_some();
        let watch = (mapped || self.session.learn.is_on() || self.rig.panic_learn.is_some()).then(|| {
            let watch = learn::Watch {
                reader: self.session.controller.midi_reader(),
                generation: self.generation,
            };
            Subscription::run_with(watch, learn::stream).map(|_| Message::Midi)
        });
        // Runs only while a ghost mark shows, so an idle app stays idle.
        let ghosts = (!self.session.learn.ghosts().shown().is_empty())
            .then(|| iced::time::every(learn::GHOST_CHECK).map(|_| Message::MidiGhosts));
        let input = [self.input_subscription(), always_subscription()];
        // The timer runs only while a drop is being collected, so an idle
        // app stays idle.
        let drops_done = (!self.drops.is_empty())
            .then(|| iced::time::every(std::time::Duration::from_millis(100)).map(|_| Message::DropsDone));
        Subscription::batch(watch.into_iter().chain(ghosts).chain(input).chain(drops_done))
    }

    /// The listener for input that depends on who has it. Its messages say who
    /// was listening before the event (input_owner.rs).
    pub(crate) fn input_subscription(&self) -> Subscription<Message> {
        iced::event::listen_with(crate::input_owner::input_event)
            .with(self.listening())
            .map(|(listening, heard)| heard.message(listening))
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
        let graph = self.session.controller.graph();
        let (lights, (ranges, readouts)): (Vec<_>, (Vec<_>, Vec<_>)) = self
            .session
            .visible_displays
            .iter()
            .filter_map(|&id| Some((id, graph.node(ModuleId(id))?.descriptor)))
            .flat_map(|(id, d)| {
                d.panel.widgets.iter().enumerate().filter_map(move |(index, w)| {
                    let display = (!w.is_control()).then(|| d.display_index(w.bind()))??;
                    Some(((id, index), display, lights::range(d, w), lights::readout(d, w)))
                })
            })
            .filter_map(|(light, display, range, readout)| {
                let source = self.session.controller.display_source(ModuleId(light.0), display)?;
                Some(((light, source), (range, readout)))
            })
            .unzip();
        (!lights.is_empty()).then(|| lights::Watch {
            reader: self.session.controller.display_reader(),
            readouts,
            lights,
            ranges,
            generation: self.generation,
        })
    }
}

/// The listener for input that works whoever has it. It reads no app state, so
/// its identity never changes and it never restarts (input_owner.rs).
pub(crate) fn always_subscription() -> Subscription<Message> {
    iced::event::listen_with(crate::input_owner::always_event)
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

impl App {
    /// Spike: the accessibility layer's events, when the feature is on.
    fn a11y_subscription(&self) -> Option<Subscription<Message>> {
        #[cfg(all(feature = "a11y-spike", target_os = "macos"))]
        return Some(crate::a11y::subscription().map(Message::A11y));
        #[cfg(not(all(feature = "a11y-spike", target_os = "macos")))]
        None
    }

    /// The task that runs once the window opens. With the spike feature it
    /// attaches the accessibility layer; otherwise it does nothing.
    pub(crate) fn start_task() -> Task<Message> {
        #[cfg(all(feature = "a11y-spike", target_os = "macos"))]
        return crate::a11y::attach().map(Message::A11y);
        #[cfg(not(all(feature = "a11y-spike", target_os = "macos")))]
        Task::none()
    }
}
