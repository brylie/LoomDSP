//! What Iced still runs for the app: the window's own events and the
//! input listeners, until the `winit` shell (plan step 2.2). The watcher
//! threads are in `watching.rs`, and the timers and frames are deadlines
//! (`timers.rs`).

use crate::{App, FileAction, Message, browser, lights};
use iced::Subscription;
use loom_core::ModuleId;

impl App {
    /// What Iced still runs for the app: the window's own events, which go
    /// with the `winit` shell (step 2.2).
    pub(crate) fn subscription(&self) -> Subscription<Message> {
        let close = iced::window::close_requests().map(|_| Message::File(FileAction::Close));
        Subscription::batch([
            close,
            self.input_subscription(),
            always_subscription(),
            self.a11y_subscription().map(Message::A11y),
        ])
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
