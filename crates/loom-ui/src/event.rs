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
use std::time::Duration;

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
    /// The system wants a frame the runtime did not ask for: the window was
    /// uncovered or resized. The turn runs no update and says to draw, and
    /// the driver draws that frame at once, asking for no other. A frame
    /// the driver asked for because a turn said to draw is drawn when it
    /// comes, with no `RedrawRequested` (see [`crate::Next`]).
    RedrawRequested,
    /// One animation frame. A driver sends one per display refresh, only
    /// while the runtime asks for frames. Draws, and runs no update.
    Frame,
    /// A frame has been drawn and shown. Sent once for each frame drawn,
    /// after it is on screen, and only for a frame that began after the
    /// latest turn that said to draw. A message waiting for the next frame
    /// to be shown arrives on this event, so a `Presented` for an older
    /// frame would deliver it before its frame is on screen.
    Presented,
    /// A waker sent a message, which is already in the queue.
    Woken,
    /// The driver woke at the time the runtime asked for.
    DeadlineReached,
}

/// Something that happened to the window as a whole.
#[derive(Debug, Clone, PartialEq)]
pub enum WindowEvent {
    /// The window's inside has this size, in logical pixels. It updates the
    /// runtime's window state and does not draw: the drawing comes from the
    /// `RedrawRequested` the window system sends after a resize, as macOS,
    /// Windows, and Linux window systems do through `winit`. A driver whose
    /// system sends none, such as a host's window, sends `RedrawRequested`
    /// itself after the resize.
    Resized(Size),
    /// The display's scale factor changed: the window moved to another
    /// display, or its settings changed. Like a resize, it does not draw by
    /// itself.
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
    /// How long one frame of its display lasts. A driver that knows the
    /// display's rate sets it; the rest keep [`WindowState::FRAME`].
    pub frame: Duration,
}

impl WindowState {
    /// The frame period of a display whose rate is not known: 16 ms, about
    /// 60 frames a second.
    pub const FRAME: Duration = Duration::from_millis(16);

    /// A window of `size` logical pixels at scale 1, with focus, on a
    /// display whose rate is not known.
    pub fn new(size: Size) -> Self {
        Self {
            size,
            scale: 1.0,
            focused: true,
            frame: Self::FRAME,
        }
    }
}
