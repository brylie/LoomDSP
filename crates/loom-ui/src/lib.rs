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
//!   type in its API. See `examples/layout.rs`. Panels compose through
//!   slots ([`Element::slot`]): the runtime keeps each panel's layout, and
//!   the driver calls [`Runtime::lay_out`] before it draws.
//! - **Drawing panels** ([`Runtime::paint`]): each element's box and text,
//!   in the look it sets, cut to its clips and its slot, which
//!   hit-testing cuts the same way. A shell layer lays out and paints
//!   through [`Panels`], lent with the app in [`shell::Parts`].
//! - **Focus and the accessibility tree** ([`Element::focusable`],
//!   [`Element::access`], [`access`]): a keyed element can be a focus stop
//!   and say what it is to a screen reader. The runtime keeps the focus
//!   among its own elements, moves it with Tab and Shift+Tab in reading
//!   order ([`Runtime::focus_step`]), and gives each laid-out element's
//!   node from that same element ([`Context::access_nodes`]). See
//!   `examples/focus.rs`.
//! - **Painting** ([`paint`]): text set with Parley and rounded boxes, in
//!   layers cut to their clips, drawn by [`paint::PaintRenderer`] (the
//!   `gpu` feature, which `shell` turns on) from one glyph atlas. It needs
//!   no window, so a crate that only paints, such as LoomDSP's canvas,
//!   takes `gpu` without `shell`.
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

pub mod access;
mod app;
#[cfg(feature = "clipboard")]
pub mod clipboard;
mod draw;
mod driver;
pub mod effect;
mod element;
mod event;
mod field;
mod focus;
pub mod headless;
mod hint;
mod hit;
mod layout;
pub mod paint;
mod pointer;
mod routing;
mod runtime;
mod select;
#[cfg(feature = "shell")]
pub mod shell;
mod slider;
mod still;
mod view;
mod waker;
mod window;

pub use app::{App, Context, Deadline, InputState, TabStep, WindowSpec};
pub use draw::Panels;
pub use driver::{Driver, Paste};
pub use effect::{ClipboardText, Effect, Effects};
pub use element::{Align, Element, Inline, Length, Looks, Padding, Placement, ScrollStyle, ShadowStyle, StateLook};
pub use event::{AccessEvent, DriverEvent, Ime, WindowEvent, WindowState};
pub use field::FieldEvent;
pub use focus::{AccessNodes, FocusStep, RingStyle};
pub use hint::HintStyle;
pub use hit::Hit;
pub use layout::Layout;
pub use pointer::Heard;
pub use routing::{NO_ELEMENTS, NoTarget, Routed, Target};
pub use runtime::{Counts, Next, Runtime};
pub use select::{Select, SelectStyle};
pub use slider::{Drag, Fill, SlidePhase, Slider};
pub use still::Still;
pub use view::{Layer, View};
pub use waker::{Wake, Waker};
