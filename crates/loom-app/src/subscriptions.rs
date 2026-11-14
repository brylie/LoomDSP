//! Who hears the window's events: the close request and the input
//! listeners, as plain calls ([`Listeners`]). The window's layers on the
//! `winit` shell (`iced_bridge/window.rs`), the `Ui` harness, and the
//! control channel's pointer hand each event on through them. The watcher
//! threads are in `watching.rs`, and the timers and frames are deadlines
//! (`timers.rs`).
//!
//! The same listeners as Iced's subscriptions stay, for tests only, as the
//! reference the plain calls are checked against (`subscriptions/tests.rs`).

use crate::input_owner::Listening;
use crate::{App, FileAction, Message, browser, lights};
#[cfg(test)]
use iced::Subscription;
use iced::event::Status;
use loom_core::ModuleId;

impl App {
    /// What Iced's runtime ran for the app, as subscriptions. The window
    /// on the shell hears the same through [`Listeners`]; this stays for
    /// the tests that compare the two, until phase 3.
    #[cfg(test)]
    pub(crate) fn subscription(&self) -> Subscription<Message> {
        let close = iced::window::close_requests().map(|_| Message::File(FileAction::Close));
        Subscription::batch([
            close,
            self.input_subscription(),
            always_subscription(),
            self.a11y_subscription().map(Message::A11y),
        ])
    }

    /// Who hears the window's events now, for [`Listeners::messages`].
    /// Take it before a batch of events, as Iced's subscriptions hear a
    /// batch as the app stood before it.
    pub(crate) fn listeners(&self) -> Listeners {
        Listeners {
            listening: self.listening(),
            screen_reader: self.a11y.is_active(),
        }
    }

    /// The listener for input that depends on who has it. Its messages say who
    /// was listening before the event (input_owner.rs).
    #[cfg(test)]
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
#[cfg(test)]
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

/// The window's listeners as the app stood at one moment: the same ones
/// Iced's subscriptions heard (`App::subscription`, kept for the tests), as
/// plain calls. The screen reader adapter's events are not here: they come
/// through the runtime's waker (`a11y/macos.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Listeners {
    /// Who has the input (input_owner.rs).
    listening: Listening,
    /// A screen reader listens, so a change of scale is heard.
    screen_reader: bool,
}

impl Listeners {
    /// The messages the listeners send for `event`, which a widget used or
    /// not as `status` says: the close request, then the input listeners
    /// (input_owner.rs), then the scale for a screen reader.
    pub(crate) fn messages(self, event: &iced::Event, status: Status) -> Vec<Message> {
        let mut messages = Vec::new();
        if matches!(event, iced::Event::Window(iced::window::Event::CloseRequested)) {
            messages.push(Message::File(FileAction::Close));
        }
        messages.extend(crate::input_owner::heard_messages(event, status, self.listening));
        if self.screen_reader {
            let window = iced::window::Id::unique();
            messages.extend(crate::a11y::rescaled(event.clone(), status, window).map(Message::A11y));
        }
        messages
    }
}

#[cfg(test)]
mod tests;
