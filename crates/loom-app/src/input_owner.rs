//! Who has the input, and which messages a window event becomes.
//!
//! Iced keeps subscriptions in a hash map, so several listeners that hear one
//! event send their messages in no set order. So the app has one listener,
//! [`input_event`], and [`Listening::route`] turns what it hears into messages
//! in one fixed order. The real window, the control channel's `pointer::Injector`,
//! and the headless `Ui` all go through this route, so a test sees what the
//! app does.

use crate::{App, Message, panic, patches, question, recent, settings};
use crate::{controls, matrix, subscriptions};
use iced::Task;
use iced::event::Status;
use iced::keyboard::{self, Key, key::Named};
use iced::{mouse, window};

/// Who the keys and clicks over the window belong to. Listed from the top
/// layer down: when two are open, the earlier one has the input.
///
/// To add an overlay, add it here and to [`App::input_owner`]. The canvas, the
/// clipboard keys, and key handling then follow, and the compiler points to
/// [`App::overlay_key`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InputOwner {
    /// A question must be answered first.
    Question,
    /// The help overlay.
    Help,
    /// The licence chooser, over Your patches or the inspector.
    Chooser,
    /// The Your patches browser.
    Patches,
    /// The Preferences window.
    Preferences,
    /// The module browser.
    Browser,
    /// The Recent list.
    RecentList,
    /// The probe menu on an output.
    ProbeMenu,
    /// The tile's context menu.
    TileMenu,
    /// Nothing is over the canvas.
    Canvas,
}

/// A listener: what an event becomes, if anything. Each one also gets whether
/// a widget used the event.
type Listener = fn(iced::Event, Status, window::Id) -> Option<Message>;

/// Which listeners hear events. It is read before an event arrives and travels
/// with the event's message, because widgets act on an event first. Without
/// it, an Enter that makes a widget open a question would also answer the
/// question it just opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Listening {
    /// No question is open.
    open: bool,
    /// The matrix table has the keyboard.
    matrix: bool,
    /// The module browser is on top of what it shares a layer with.
    browser: bool,
    /// The patches browser is on top of what it shares a layer with.
    patches: bool,
    /// An inspector value field has a draft.
    value_draft: bool,
}

/// An event the input listener heard, and whether a widget used it.
#[derive(Debug, Clone)]
pub(crate) struct Heard {
    event: iced::Event,
    status: Status,
}

impl Heard {
    /// The message for this event, with who was listening when it was heard.
    pub(crate) fn message(self, listening: Listening) -> Message {
        Message::Input {
            event: self.event,
            status: self.status,
            listening,
        }
    }
}

/// The app's one input listener. It lets through only what a row of
/// [`Listening::rows`] can use, so moving the pointer and redrawing send no
/// message and an idle app stays idle.
pub(crate) fn input_event(event: iced::Event, status: Status, _window: window::Id) -> Option<Heard> {
    let used = matches!(
        event,
        iced::Event::Keyboard(_)
            | iced::Event::Mouse(mouse::Event::ButtonPressed(_))
            | iced::Event::Window(window::Event::FileDropped(_))
    );
    used.then_some(Heard { event, status })
}

impl Listening {
    /// The listeners, each with whether it hears events, in the order their
    /// messages are applied. `Key` comes first, so a key no widget used
    /// reaches the overlay that has it before anything closes that overlay.
    fn rows(self) -> [(bool, Listener); 10] {
        // An open question has the keys, except Panic's. One key must not
        // answer it and also reach what is behind it, once the answer has
        // closed it (files.md, Asking before losing work).
        [
            (self.open, app_key),
            // The matrix owns arrows and Escape even when a child widget uses
            // the key. Its handler ignores them outside table focus.
            (self.open && self.matrix, matrix::key_listener),
            // The search field takes Escape for itself, so the browser hears
            // keys whether or not a widget used them.
            (self.open && self.browser, subscriptions::browser_key),
            (self.open, subscriptions::help_key),
            // The patches browser hears Up, Down, and Escape over its fields.
            (self.open && self.patches, patches::key),
            // A draft in a value field ends on Escape, and the app has to know
            // when text focus may have moved (panel-widgets.md, Typing a value).
            (self.open && self.value_draft, controls::value_draft_event),
            (self.open, controls::text_focus_event),
            (!self.open, question::key),
            // The panic key works whatever has focus (panic.md).
            (true, panic::key),
            // Files dropped on the window.
            (true, recent::drop_event),
        ]
    }

    /// The messages an event becomes, in the order they are applied.
    pub(crate) fn route(self, event: iced::Event, status: Status) -> Vec<Message> {
        self.rows()
            .into_iter()
            .filter(|(on, _)| *on)
            .filter_map(|(_, listener)| listener(event.clone(), status, window::Id::unique()))
            .collect()
    }
}

/// A key no widget used goes to the app, as `keyboard::listen` would send it.
fn app_key(event: iced::Event, status: Status, _window: window::Id) -> Option<Message> {
    match event {
        iced::Event::Keyboard(key) if status == Status::Ignored => Some(Message::Key(key)),
        _ => None,
    }
}

impl App {
    /// Who has the input now.
    pub(crate) fn input_owner(&self) -> InputOwner {
        if self.question.is_some() {
            InputOwner::Question
        } else if self.help.is_some() {
            InputOwner::Help
        } else if self.chooser.is_some() {
            InputOwner::Chooser
        } else if self.patches.is_some() {
            InputOwner::Patches
        } else if self.rig.window.is_some() {
            InputOwner::Preferences
        } else if self.browser.is_some() {
            InputOwner::Browser
        } else if self.recent_rows.is_some() {
            InputOwner::RecentList
        } else if self.probe_menu.is_some() {
            InputOwner::ProbeMenu
        } else if self.tile_menu.is_some() {
            InputOwner::TileMenu
        } else {
            InputOwner::Canvas
        }
    }

    /// True when no overlay or dialog panel is open over the canvas. A small
    /// menu on the canvas does not count. That is why the menus come last in
    /// [`InputOwner`].
    pub(crate) fn no_overlay(&self) -> bool {
        matches!(
            self.input_owner(),
            InputOwner::Canvas | InputOwner::ProbeMenu | InputOwner::TileMenu
        )
    }

    /// Which listeners hear events now.
    pub(crate) fn listening(&self) -> Listening {
        Listening {
            open: self.question.is_none(),
            matrix: self.matrix_has_keys(),
            browser: self.browser.is_some() && self.help.is_none(),
            patches: self.patches.is_some() && self.chooser.is_none(),
            value_draft: self.value_draft.is_some(),
        }
    }

    /// Applies the messages an event becomes, one after the other. A route with
    /// no message still redraws, so it counts as input for `--measure-idle`.
    pub(crate) fn input_message(&mut self, event: iced::Event, status: Status, listening: Listening) -> Task<Message> {
        let messages = listening.route(event, status);
        if messages.is_empty() {
            loom_canvas::stats::request_frame(loom_canvas::stats::FrameCause::Input);
            return Task::none();
        }
        Task::batch(messages.into_iter().map(|message| self.update(message)))
    }

    /// A key no widget used while an overlay has the input. An overlay's keys
    /// stop here, so none reaches the canvas or a window behind it.
    pub(crate) fn overlay_key(&mut self, event: &keyboard::Event) {
        let keyboard::Event::KeyPressed { key, modifiers, .. } = event else {
            return;
        };
        let escape = *key == Key::Named(Named::Escape);
        match self.input_owner() {
            // These have listeners or widgets of their own.
            InputOwner::Question
            | InputOwner::Help
            | InputOwner::Patches
            | InputOwner::Browser
            | InputOwner::Canvas => {}
            InputOwner::Chooser if escape => self.chooser = None,
            InputOwner::Chooser => {}
            InputOwner::Preferences => match key.as_ref() {
                Key::Named(Named::Escape) => self.preferences(settings::Message::Close),
                Key::Character(",") if modifiers.command() => self.toggle_preferences(),
                _ => {}
            },
            InputOwner::RecentList if escape => self.recent_rows = None,
            InputOwner::ProbeMenu if escape => self.probe_menu = None,
            InputOwner::TileMenu if escape => self.tile_menu = None,
            InputOwner::RecentList | InputOwner::ProbeMenu | InputOwner::TileMenu => {}
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
