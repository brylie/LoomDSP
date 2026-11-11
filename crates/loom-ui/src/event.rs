//! What a driver tells the runtime.
//!
//! There are two levels of event. What an element receives is
//! [`loom_ui_core::Event`]: the pointer, the keys, and the window losing
//! focus. What a driver sends the runtime is a [`DriverEvent`]: one of
//! those element events, or something about the window as a whole, or a
//! reason to run a turn. No element handles a resize or a dropped file
//! itself, so those live here and not in `loom-ui-core`.

use loom_ui_core::{Event, Point, Size};
use std::path::PathBuf;

/// One thing a driver tells the runtime.
#[derive(Debug, Clone, PartialEq)]
pub enum DriverEvent {
    /// An event for the elements. The pointer's place travels beside it, in
    /// logical pixels, because a key event can depend on where the pointer
    /// is and only the driver knows when something covers it. `None` means
    /// no element may see the pointer (see `loom_ui_core`'s input module).
    Input {
        /// The event.
        event: Event,
        /// Where the pointer is, or `None`.
        pointer: Option<Point>,
    },
    /// Something happened to the window as a whole.
    Window(WindowEvent),
    /// The system asks for the window to be drawn: it was uncovered or
    /// resized. Runs no update.
    RedrawRequested,
    /// One animation frame. A driver sends one per display refresh, only
    /// while the runtime asks for frames. Draws, and runs no update.
    Frame,
    /// The frame the runtime last asked for has been drawn and shown.
    Presented,
    /// A waker sent a message, which is already in the queue.
    Woken,
    /// The driver woke at the time the runtime asked for.
    DeadlineReached,
}

/// Something that happened to the window as a whole.
#[derive(Debug, Clone, PartialEq)]
pub enum WindowEvent {
    /// The window's inside has this size, in logical pixels.
    Resized(Size),
    /// The display's scale factor changed: the window moved to another
    /// display, or its settings changed.
    ScaleChanged(f32),
    /// The window became the one that takes the keys (`true`), or stopped
    /// being it (`false`).
    Focused(bool),
    /// The user asked to close the window.
    CloseRequested,
    /// A file was dropped on the window.
    FileDropped(PathBuf),
}

/// What the runtime knows about its window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowState {
    /// The inside of the window, in logical pixels.
    pub size: Size,
    /// Physical pixels per logical pixel.
    pub scale: f32,
    /// True while the window takes the keys.
    pub focused: bool,
}

impl WindowState {
    /// A window of `size` logical pixels at scale 1, with focus.
    pub fn new(size: Size) -> Self {
        Self {
            size,
            scale: 1.0,
            focused: true,
        }
    }
}
