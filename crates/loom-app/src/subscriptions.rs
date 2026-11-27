//! Who hears the window's events: the input listeners, as plain calls
//! ([`Listeners`]), and the window events the runtime hands the app
//! ([`window_message`]): the close request and a dropped file. The window's layers on the
//! `winit` shell (`iced_bridge/window.rs`), the `Ui` harness, and the
//! control channel's pointer hand each event on through them. The watcher
//! threads are in `watching.rs`, and the timers and frames are deadlines
//! (`timers.rs`).
//!
//! The same listeners as Iced's subscriptions stay, for tests only, as the
//! reference the plain calls are checked against (`subscriptions/tests.rs`).
//! Plan step P4 deletes them with the bridge.

use crate::input_owner::Listening;
use crate::{App, FileAction, Message, lights};
#[cfg(test)]
use iced::Subscription;
use iced::event::Status;
use loom_core::ModuleId;
use loom_ui_core::{Event, Key, KeyEvent};

impl App {
    /// What Iced's runtime ran for the app, as subscriptions, less the
    /// close request, which the runtime now hands the app as a window event
    /// ([`window_message`]). The window on the shell hears the same through
    /// [`Listeners`]; this stays for the tests that compare the two. Plan
    /// step P4 deletes it.
    #[cfg(test)]
    pub(crate) fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([self.input_subscription(), always_subscription()])
    }

    /// Who hears the window's events now, for [`Listeners::messages`].
    /// Take it before a batch of events, as Iced's subscriptions hear a
    /// batch as the app stood before it.
    pub(crate) fn listeners(&self) -> Listeners {
        Listeners {
            listening: self.listening(),
        }
    }

    /// The listener for input that depends on who has it. Its messages say who
    /// was listening before the event (input_owner.rs).
    #[cfg(test)]
    pub(crate) fn input_subscription(&self) -> Subscription<Message> {
        iced::event::listen_with(iced_input_event)
            .with(self.listening())
            .map(|(listening, heard)| heard.message(listening))
    }

    /// The lights on screen and where their values live, with the reader of
    /// this engine's table, or `None` when there are none.
    pub(crate) fn watched_lights(&self) -> Option<lights::OnScreen> {
        let graph = self.session.controller.graph();
        let lights: Vec<lights::Entry> = self
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
                Some(lights::Entry {
                    light,
                    source,
                    range,
                    readout,
                })
            })
            .collect();
        (!lights.is_empty()).then(|| lights::OnScreen {
            reader: self.session.controller.display_reader(),
            lights,
        })
    }

    /// The trigger lights on the watched list now.
    fn watched_triggers(&self) -> std::collections::BTreeSet<lights::Light> {
        self.watched_lights()
            .into_iter()
            .flat_map(|on| on.lights)
            .filter(|entry| entry.source.kind() == loom_core::DisplayKind::Trigger)
            .map(|entry| entry.light)
            .collect()
    }

    /// Whether a light shows a trigger display value, by its module's
    /// descriptor. A light that no longer exists is not one.
    fn is_trigger_light(&self, (id, widget): lights::Light) -> bool {
        let Some(node) = self.session.controller.graph().node(ModuleId(id)) else {
            return false;
        };
        let d = node.descriptor;
        d.panel
            .widgets
            .get(widget)
            .filter(|w| !w.is_control())
            .and_then(|w| d.display_index(w.bind()))
            .and_then(|index| d.displays.get(index))
            .is_some_and(|spec| spec.kind == loom_core::DisplayKind::Trigger)
    }

    /// Takes the light readings the watcher thread sent. A reading for a
    /// trigger light off the watched list is ignored: it was sent before the
    /// light left the screen, and would light it again after
    /// [`App::set_visible_displays`] set it dark. A level light takes every
    /// reading, as its last value is the best guess until the next one
    /// (`docs/design/panels.md`, Redraw).
    pub(crate) fn take_lights(&mut self, readings: Vec<(lights::Light, f32)>) {
        let mut watched = None;
        for (light, value) in readings {
            if self.is_trigger_light(light) && !watched.get_or_insert_with(|| self.watched_triggers()).contains(&light)
            {
                continue;
            }
            self.session.lights.insert(light, value);
        }
    }

    /// Says which modules' displays are on screen, as the canvas reports. A
    /// trigger light that leaves the screen is set dark, and so is one new
    /// to the list, so it does not draw lit for a frame when it comes back,
    /// before its first reading. A level light keeps its last value, the
    /// best guess until then (`docs/design/panels.md`, Redraw).
    pub(crate) fn set_visible_displays(&mut self, ids: Vec<u32>) {
        let before = self.watched_triggers();
        self.session.visible_displays = ids;
        let after = self.watched_triggers();
        for light in before.symmetric_difference(&after) {
            self.session.lights.insert(*light, 0.0);
        }
    }
}

/// The listener for input that works whoever has it. It reads no app state, so
/// its identity never changes and it never restarts (input_owner.rs).
#[cfg(test)]
pub(crate) fn always_subscription() -> Subscription<Message> {
    iced::event::listen_with(iced_always_event)
}

/// [`crate::input_owner::input_event`] as Iced's listener, hearing Iced's
/// event as ours. Plan step P4 deletes it.
#[cfg(test)]
fn iced_input_event(
    event: iced::Event,
    status: Status,
    _window: iced::window::Id,
) -> Option<crate::input_owner::Heard> {
    crate::input_owner::input_event(&loom_canvas::iced_convert::event_to_core(&event)?, used(status))
}

/// [`crate::input_owner::always_event`] as Iced's listener, hearing Iced's
/// event as ours. Plan step P4 deletes it.
#[cfg(test)]
fn iced_always_event(event: iced::Event, status: Status, _window: iced::window::Id) -> Option<Message> {
    crate::input_owner::always_event(&loom_canvas::iced_convert::event_to_core(&event)?, used(status))
}

/// Whether a widget used an event, from Iced's word for it.
fn used(status: Status) -> bool {
    status == Status::Captured
}

/// F1 and Escape reach help even while a text field has focus.
pub(crate) fn help_key(event: &Event, _used: bool) -> Option<Message> {
    match event {
        Event::Key(KeyEvent::Pressed { key: Key::F1, .. }) => Some(Message::ShowHelp),
        Event::Key(KeyEvent::Pressed { key: Key::Escape, .. }) => Some(Message::CloseHelp),
        _ => None,
    }
}

/// The app's message for a window event the runtime hands it, if it wants
/// one: the close request becomes the app's close, so the window stays open
/// until the app has asked about unsaved work (files.md); a dropped file
/// becomes a message, one per file, which the app collects for a moment;
/// a new scale redraws.
pub(crate) fn window_message(event: &loom_ui::WindowEvent) -> Option<Message> {
    match event {
        loom_ui::WindowEvent::CloseRequested => Some(Message::File(FileAction::Close)),
        loom_ui::WindowEvent::FileDropped(path) => Some(Message::FileDropped(path.clone())),
        loom_ui::WindowEvent::ScaleChanged(_) => Some(Message::WindowRescaled),
        _ => None,
    }
}

/// The window's listeners as the app stood at one moment: the same ones
/// Iced's subscriptions heard (`App::subscription`, kept for the tests), as
/// plain calls. The screen reader adapter's events are not here: they come
/// through the runtime's waker (`a11y/macos.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Listeners {
    /// Who has the input (input_owner.rs).
    listening: Listening,
}

impl Listeners {
    /// The messages the listeners send for Iced's `event`, which a widget
    /// used or not as `status` says (input_owner.rs). This is the one place
    /// an event Iced delivers becomes ours, for the window, the `Ui`
    /// harness, and the control channel's injector alike. An event with no
    /// meaning to the listeners, such as a window event, sends nothing: the
    /// runtime hands the app those itself ([`window_message`]). Plan step
    /// P4 deletes it, when the runtime delivers every event.
    pub(crate) fn messages(self, event: &iced::Event, status: Status) -> Vec<Message> {
        loom_canvas::iced_convert::event_to_core(event)
            .map(|event| self.heard(&event, used(status)))
            .unwrap_or_default()
    }

    /// The messages the listeners send for `event`, which a widget used or
    /// not as `used` says (input_owner.rs).
    pub(crate) fn heard(self, event: &Event, used: bool) -> Vec<Message> {
        crate::input_owner::heard_messages(event, used, self.listening)
    }
}

#[cfg(test)]
mod tests;
