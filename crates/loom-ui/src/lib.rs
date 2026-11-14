//! A UI runtime: the loop that takes events, calls the app, runs what the
//! app asks for, and decides when to draw.
//!
//! This crate is the middle of LoomDSP's own UI layer (ADR-0041,
//! `docs/design/ui-layer.md`, section 2). It sits on [`loom_ui_core`] and
//! knows nothing about patches, so another tool can use it.
//!
//! # The shape
//!
//! - **The app** is one message type and one `update` that applies one
//!   message and returns its effects. No other path changes app state.
//! - **Effects** are plain values: focus a field, write the clipboard, open
//!   a dialog, run work on another thread, quit. A test can read each one,
//!   run it, or refuse it.
//! - **A driver** feeds the runtime events and carries out its effects: a
//!   window shell, or the headless driver tests use. The runtime names no
//!   window toolkit, so a third driver, such as a plugin host's window, can
//!   be added without changing it.
//! - **The window shell** ([`shell`], the `shell` feature, on by default)
//!   is the driver for a desktop window: `winit` and `wgpu`, in that one
//!   module. The app's side draws each frame through [`shell::Layers`].
//! - **Layout** ([`Element`], [`Layout`], [`Layout::hit`]): a panel builds
//!   a tree of elements keyed by UI ID, and the layout places it, gives each
//!   keyed element's bounds by UI ID, and finds the element under a point.
//!   Sizes follow flexbox's defaults. It needs no GPU, and names no taffy
//!   type in its API. See `examples/layout.rs`.
//! - **Time comes from the driver.** Every call takes the current moment,
//!   and the runtime never reads the clock, so a test decides what time it
//!   is.
//!
//! # Rules
//!
//! - **One message gives one `update`.** Effects run after it returns. A
//!   message an effect produces waits for the next turn.
//! - **Nothing redraws while nothing visible changes.** The runtime asks
//!   for a redraw only when a panel's model changed, a routing target asks
//!   for one, or an effect does.
//! - **The loop sleeps** when nothing is queued, nothing animates, and no
//!   deadline is near.

mod app;
mod driver;
pub mod effect;
mod element;
mod event;
pub mod headless;
mod hit;
mod layout;
mod routing;
mod runtime;
#[cfg(feature = "shell")]
pub mod shell;
mod view;
mod waker;

pub use app::{App, Context, Deadline, WindowSpec};
pub use driver::Driver;
pub use effect::{Effect, Effects};
pub use element::{Align, Element, Length, Padding};
pub use event::{DriverEvent, WindowEvent, WindowState};
pub use hit::{Hit, hit_layers};
pub use layout::Layout;
pub use routing::{NO_ELEMENTS, NoTarget, Target};
pub use runtime::{Counts, Next, Runtime};
pub use view::{Layer, View};
pub use waker::{Wake, Waker};
