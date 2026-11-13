//! What Iced still runs for the app: timers, frames, keys, and the window's
//! events. The watcher threads are in `watching.rs`.

use crate::{App, FileAction, Message, browser, devices, learn, lights, recovery};
use iced::Subscription;
use loom_core::ModuleId;
use std::time::Duration;

impl App {
    /// What Iced still runs for the app: timers, frames, and the window's
    /// own events. The watcher threads run on the runtime's waker instead
    /// (`watching.rs`); the timers and frames become deadlines and effects
    /// next (plan step 2.1, item 3), and the window's events go with the
    /// `winit` shell (step 2.2).
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
        let close = iced::window::close_requests().map(|_| Message::File(FileAction::Close));
        // Frames only while a command waits to be drawn, so an idle app
        // stays idle.
        let control = self
            .control_waiting()
            .then(|| iced::window::frames().map(Message::ControlFrame));
        // The autosave timer runs only while a copy may be due: once the
        // copy matches the patch, an idle app ticks no more (#90).
        let autosave = if self.run_files.autosave_due(&self.session.document) {
            iced::time::every(recovery::TICK).map(|_| Message::Autosave)
        } else {
            Subscription::none()
        };
        // MIDI inputs plugged in and out are found by polling four times a
        // second: the systems offer no portable callback (device-changes.md).
        // A real host's list is read by a watcher thread (`watching.rs`); a
        // test host is polled from the UI.
        let midi_poll =
            if self.rig.midi_in.is_some() && !self.rig.flags.no_midi && self.rig.host.midi_lister().is_none() {
                iced::time::every(devices::MIDI_POLL).map(|_| Message::MidiPoll)
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
        Subscription::batch(
            [main, live, self.midi_watch(), close, autosave, midi_poll, audio_poll]
                .into_iter()
                .chain(control)
                .chain(std::iter::once(self.a11y_subscription().map(Message::A11y))),
        )
    }

    /// The timers for MIDI learn's ghost marks and for collecting dropped
    /// files, and the app's input listeners.
    pub(crate) fn midi_watch(&self) -> Subscription<Message> {
        // Runs only while a ghost mark shows, so an idle app stays idle.
        let ghosts = (!self.session.learn.ghosts().shown().is_empty())
            .then(|| iced::time::every(learn::GHOST_CHECK).map(|_| Message::MidiGhosts));
        let input = [self.input_subscription(), always_subscription()];
        // The timer runs only while a drop is being collected, so an idle
        // app stays idle.
        let drops_done = (!self.drops.is_empty())
            .then(|| iced::time::every(std::time::Duration::from_millis(100)).map(|_| Message::DropsDone));
        Subscription::batch(ghosts.into_iter().chain(input).chain(drops_done))
    }

    /// The listener for input that depends on who has it. Its messages say who
    /// was listening before the event (input_owner.rs).
    pub(crate) fn input_subscription(&self) -> Subscription<Message> {
        iced::event::listen_with(crate::input_owner::input_event)
            .with(self.listening())
            .map(|(listening, heard)| heard.message(listening))
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
