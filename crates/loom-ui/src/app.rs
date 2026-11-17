//! The app, as the runtime sees it.

use crate::effect::Effects;
use crate::event::{AccessEvent, WindowEvent, WindowState};
use crate::focus::AccessNodes;
use crate::view::View;
use crate::waker::Waker;
use crate::window::Window;
use loom_ui_core::{IdTable, Size, UiId};
use std::time::Instant;

/// The program built on the runtime: one message type and one `update`.
///
/// `update` is the only place app state changes (ui-layer.md, section 2,
/// rule 1). The other methods only read it.
pub trait App {
    /// The app's message: the only way its state changes. It is `Send`
    /// because watcher threads and background work send messages.
    type Message: Send + 'static;

    /// Applies one message and returns what should happen next.
    fn update(&mut self, message: Self::Message, cx: &Context<'_, Self::Message>) -> Effects<Self::Message>;

    /// Describes the window, one panel at a time.
    fn view(&self, view: &mut View);

    /// The window's title and limits. Read after each turn that ran an
    /// update, and passed to the driver only when it changed.
    fn window(&self) -> WindowSpec;

    /// The next moment the app needs to wake with no event, and the message
    /// it gets then. Built from state after each turn that ran an update.
    /// Handling that message must move the deadline on, or it is due again
    /// at once.
    fn next_deadline(&self) -> Option<Deadline<Self::Message>> {
        None
    }

    /// The message for a window event, if the app cares about it: a close
    /// request, a dropped file. It reads state and changes none.
    fn window_event(&self, event: &WindowEvent) -> Option<Self::Message> {
        let _ = event;
        None
    }

    /// The message for something a screen reader did, if the app wants
    /// one. An app with no accessibility tree wants none.
    fn accessibility_event(&self, event: &AccessEvent) -> Option<Self::Message> {
        let _ = event;
        None
    }

    /// True while something on screen moves and needs a frame each display
    /// refresh. A frame draws and runs no update.
    fn animating(&self) -> bool {
        false
    }
}

/// When the app next needs to wake, and the message it gets then.
#[derive(Debug, Clone, PartialEq)]
pub struct Deadline<M> {
    /// When.
    pub at: Instant,
    /// The message queued when that time has come.
    pub message: M,
}

/// The window's title and limits, as the app describes them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WindowSpec {
    /// The title.
    pub title: String,
    /// The smallest inside size the window may have, in logical pixels.
    pub min_size: Option<Size>,
}

/// What `update` may read besides its message. Everything here is read
/// only: app state still changes only in `update`.
pub struct Context<'a, M> {
    pub(crate) now: Instant,
    pub(crate) waker: &'a Waker<M>,
    pub(crate) focused: Option<UiId>,
    pub(crate) window: WindowState,
    /// The runtime's panels, as last laid out, for their nodes. None
    /// outside a runtime.
    pub(crate) panels: Option<&'a Window>,
}

impl<'a, M> Context<'a, M> {
    /// A context for a driver or a test that calls `update` itself, outside
    /// a [`Runtime`](crate::Runtime): the bridge to a toolkit that owns the
    /// event loop, or a test that steps one app with no window. A
    /// [`Runtime`](crate::Runtime) makes its own for every update.
    pub fn new(now: Instant, waker: &'a Waker<M>, focused: Option<UiId>, window: WindowState) -> Self {
        Self {
            now,
            waker,
            focused,
            window,
            panels: None,
        }
    }

    /// The moment this turn runs, from the driver.
    pub fn now(&self) -> Instant {
        self.now
    }

    /// A waker for a watcher thread to send messages with. The app keeps
    /// the watcher's handle in its state; dropping the handle stops it.
    pub fn waker(&self) -> Waker<M> {
        self.waker.clone()
    }

    /// The element that has keyboard focus, if any.
    pub fn focused(&self) -> Option<&UiId> {
        self.focused.as_ref()
    }

    /// The window's size, scale, and focus.
    pub fn window(&self) -> WindowState {
        self.window
    }

    /// The nodes of the runtime's own elements, as last laid out, for the
    /// app to put in the tree it sends (ui-layer.md, section 7). Node IDs
    /// come from `ids`, the app's one table for every node in the window,
    /// and bounds are in physical pixels at the window's scale. The focus is
    /// the runtime's focused element, when it has a node. It reads the last
    /// layout, and layout runs after the turn, so in the turn that opens a
    /// panel its nodes are not there yet: an app whose panels changed builds
    /// the tree after the draw ([`Effects::after_draw`](crate::Effects::after_draw)).
    ///
    /// A screen reader's action on one of these nodes reaches the app as
    /// any other does, through [`App::accessibility_event`]. The app turns
    /// it into a message by the UI ID its table gives (section 7, rule 6),
    /// and a Focus action into [`Effects::focus`](crate::Effects::focus).
    /// Empty outside a runtime.
    pub fn access_nodes(&self, ids: &mut IdTable) -> AccessNodes {
        self.panels.map_or_else(AccessNodes::default, |panels| {
            panels.access_nodes(ids, self.window.scale, self.focused.as_ref())
        })
    }
}

#[cfg(test)]
mod tests;
