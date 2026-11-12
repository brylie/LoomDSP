//! The app, as the runtime sees it.

use crate::effect::Effects;
use crate::event::{WindowEvent, WindowState};
use crate::view::View;
use crate::waker::Waker;
use loom_ui_core::{Size, UiId};
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
}

#[cfg(test)]
mod tests;
