//! Who has the input, and which messages a window event becomes.
//!
//! Iced keeps subscriptions in a hash map, so several listeners that hear one
//! event send their messages in no set order. So one listener, [`input_event`],
//! hears what depends on who has the input, and [`Listening::route`] turns that
//! into messages in one fixed order. A second listener, [`always_event`], hears
//! what does not. The real window, the control channel's `pointer::Injector`, and
//! the headless `Ui` all go through [`heard_messages`] or the same two listeners,
//! so a test sees what the app does.
//!
//! The listeners hear `loom-ui-core` events. Iced still delivers them until
//! the window leaves Iced (plan step P4), and `subscriptions::Listeners`
//! converts each one once on the way in.

use crate::{App, Message, panic, settings};
use crate::{controls, subscriptions};
use loom_ui_core::{Event, Key, KeyEvent, PointerEvent};

/// Who the keys and clicks over the window belong to. When two are open, the
/// earlier one has the input. From `Question` down to `Preferences` this is the
/// order the view draws them, top layer first. The rest are small and open
/// over the canvas, and do not normally share the screen.
///
/// To add an overlay, add it here and to [`Overlays`](crate::overlays::Overlays):
/// a panel is one more [`Overlay`](crate::overlays::Overlay) variant, and the
/// compiler points to `Overlays::owner` and [`App::overlay_key`]. The canvas, the
/// clipboard keys, and key handling then follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InputOwner {
    /// A question must be answered first.
    Question,
    /// The help overlay.
    Help,
    /// The licence chooser, over Preferences or the inspector.
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
type Listener = fn(&Event, bool) -> Option<Message>;

/// Which listeners hear events. It is read before an event arrives and travels
/// with the event's message, because widgets act on an event first. Without
/// it, an Enter that makes a widget open a question would also answer the
/// question it just opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Listening {
    /// No question is open.
    open: bool,
    /// The module browser has the input.
    browser: bool,
    /// The patches browser has the input.
    patches: bool,
    /// An inspector value field has a draft.
    value_draft: bool,
}

#[cfg(test)]
impl Listening {
    /// Every combination of who listens, for tests that must hold for all.
    pub(crate) fn all() -> Vec<Self> {
        (0..16u8)
            .map(|bits| Self {
                open: bits & 1 != 0,
                browser: bits & 2 != 0,
                patches: bits & 4 != 0,
                value_draft: bits & 8 != 0,
            })
            .collect()
    }
}

/// An event the input listener heard, and whether a widget used it.
#[derive(Debug, Clone)]
pub(crate) struct Heard {
    event: Event,
    /// A widget used the event.
    used: bool,
}

impl Heard {
    /// The message for this event, with who was listening when it was heard.
    pub(crate) fn message(self, listening: Listening) -> Message {
        Message::Input {
            event: self.event,
            used: self.used,
            listening,
        }
    }
}

/// The input listener. It lets through only what a row of [`Listening::rows`]
/// can use, so moving the pointer and redrawing send no message and an idle app
/// stays idle.
///
/// Its subscription restarts when [`Listening`] changes, and an event still
/// queued for the old one is lost. That is rare: the window is usually very
/// short, though longer while the main thread is busy, and a person just presses
/// the key again. What must never be lost goes through [`always_event`].
pub(crate) fn input_event(event: &Event, used: bool) -> Option<Heard> {
    let heard = matches!(event, Event::Key(_) | Event::Pointer(PointerEvent::Pressed(_)));
    heard.then(|| Heard {
        event: event.clone(),
        used,
    })
}

/// The listener for what works whoever has the input: the panic key
/// (panic.md). Its subscription never restarts, so it never loses one.
/// A file dropped on the window is a window event, which the runtime hands
/// the app (`subscriptions::window_message`).
pub(crate) fn always_event(event: &Event, used: bool) -> Option<Message> {
    panic::key(event, used)
}

/// What the two listeners send for an event, for the paths that hand events on
/// themselves, through `subscriptions::Listeners`.
pub(crate) fn heard_messages(event: &Event, used: bool, listening: Listening) -> Vec<Message> {
    input_event(event, used)
        .map(|heard| heard.message(listening))
        .into_iter()
        .chain(always_event(event, used))
        .collect()
}

impl Listening {
    /// The listeners, each with whether it hears events, in the order their
    /// messages are applied. `Key` comes first, so a key no widget used
    /// reaches the overlay that has it before anything closes that overlay.
    fn rows(self) -> [(bool, Listener); 4] {
        // An open question has every key, Panic's included: the runtime
        // keeps each press from Iced and gives it to `question::key_of`
        // (ui-layer.md, section 6, rule 6, step 2), so no listener here
        // hears one. They stay off while it is open anyway, so one key
        // cannot answer it and also reach what is behind it once the answer
        // has closed it (files.md, Asking before losing work).
        [
            (self.open, app_key),
            // The matrix's keys are the runtime's: its card list takes the
            // grid's keys before its cells (`matrix/panel.rs`).
            // The module browser's keys are the runtime's: its search field
            // leaves them to the browser before it edits (`browser/panel.rs`).
            (self.open, subscriptions::help_key),
            // Your patches' keys are the runtime's too (`patches/panel.rs`).
            // A draft in a value field ends on Escape, and the app has to know
            // when text focus may have moved (panel-widgets.md, Typing a value).
            (self.open && self.value_draft, controls::value_draft_event),
            (self.open, controls::text_focus_event),
        ]
    }

    /// The messages an event becomes, in the order they are applied.
    pub(crate) fn route(self, event: &Event, used: bool) -> Vec<Message> {
        self.rows()
            .into_iter()
            .filter(|(on, _)| *on)
            .filter_map(|(_, listener)| listener(event, used))
            .collect()
    }
}

/// A key no widget used goes to the app, as `keyboard::listen` would send it.
fn app_key(event: &Event, used: bool) -> Option<Message> {
    match event {
        Event::Key(
            key @ KeyEvent::Pressed {
                key: Key::Tab,
                modifiers,
                ..
            },
        ) if modifiers.control => Some(Message::Key(key.clone())),
        Event::Key(key) if !used => Some(Message::Key(key.clone())),
        _ => None,
    }
}

impl App {
    /// Who has the input now.
    pub(crate) fn input_owner(&self) -> InputOwner {
        self.overlays.owner()
    }

    /// Whether the canvas owns input, including its editor and shortcuts.
    pub(crate) fn no_overlay(&self) -> bool {
        self.input_owner() == InputOwner::Canvas
    }

    /// True while a pointer press reaches the window's own parts: a tile
    /// and its controls, the toolbar, the right panel's tabs, and the stats
    /// bar. That is only while nothing is open. Every panel and menu the
    /// view draws puts a layer over the whole window (`view.rs`), either
    /// to close itself on a press outside or to hold the press, so the
    /// press never reaches what is below. The small menus on the canvas do
    /// this too.
    ///
    /// A screen reader follows the pointer: what a press cannot reach is
    /// hidden from it and refuses its actions (ui-layer.md, section 7,
    /// rule 24). A test presses under every panel and checks that the tree
    /// agrees (`a11y::tests::the_tree_follows_the_pointer_under_every_panel`).
    pub(crate) fn pointer_reaches_window(&self) -> bool {
        self.input_owner() == InputOwner::Canvas
    }

    /// Who else has the keyboard, for the canvas: the overlays, the matrix,
    /// and the app's text fields. One place decides it, so the canvas never
    /// infers it from loose flags.
    pub(crate) fn canvas_focus_policy(&self) -> loom_canvas::CanvasFocusPolicy {
        loom_canvas::CanvasFocusPolicy {
            interactive: self.input_owner() == InputOwner::Canvas,
            // The matrix keeps no flag: while the runtime's focus is on one
            // of its parts, the canvas reports a press on it
            // (`PointerDown`), which gives the keys back to the canvas.
            matrix_keyboard: self.focus_in_matrix()
                || self.session.outline.active
                || self.toolbar_has_focus()
                || self.session.outline.inspector_action.is_some(),
            jump_keyboard: self.session.outline.has_jump(),
            text_field_keys: self.focus.is_some(),
            text_field_keeps_tab: self.text_field_tabs(self.focus.as_ref()),
        }
    }

    /// Which listeners hear events now.
    pub(crate) fn listening(&self) -> Listening {
        Listening {
            open: self.overlays.question.is_none(),
            browser: self.input_owner() == InputOwner::Browser,
            patches: self.input_owner() == InputOwner::Patches,
            value_draft: self.session.value_draft.is_some(),
        }
    }

    /// A key no widget used while an overlay has the input. An overlay's keys
    /// stop here, so none reaches the canvas or a window behind it.
    pub(crate) fn overlay_key(&mut self, event: &KeyEvent) {
        let KeyEvent::Pressed { key, modifiers, .. } = event else {
            return;
        };
        let escape = *key == Key::Escape;
        match self.input_owner() {
            // These have listeners or widgets of their own, and the menus
            // their own keys (`menus.rs`), which `update` gives them first.
            InputOwner::Question
            | InputOwner::Help
            | InputOwner::Patches
            | InputOwner::Browser
            | InputOwner::RecentList
            | InputOwner::ProbeMenu
            | InputOwner::TileMenu
            | InputOwner::Canvas => {}
            InputOwner::Chooser if escape => self.overlays.chooser = None,
            InputOwner::Chooser => {}
            InputOwner::Preferences => match key {
                Key::Escape => self.preferences(settings::Message::Close),
                Key::Character(',') if modifiers.shortcut() && !modifiers.shift => self.toggle_preferences(),
                _ => {}
            },
        }
    }
}

/// The messages an input event becomes, routed before `update` by whoever
/// runs the app (the bridge, the `Ui` harness, `App::send`): each is its
/// own update, in order (ui-layer.md, section 2, rule 5). A route with no
/// message still redraws, so it counts as input for `--measure-idle`.
pub(crate) fn route(message: Message) -> Vec<Message> {
    match message {
        Message::Input { event, used, listening } => {
            let messages = listening.route(&event, used);
            if messages.is_empty() {
                loom_canvas::stats::request_frame(loom_canvas::stats::FrameCause::Input);
            }
            messages
        }
        message => vec![message],
    }
}

#[cfg(test)]
pub(crate) mod tests;
