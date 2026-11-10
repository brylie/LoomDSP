//! Plain geometry, colour, and input event types for a UI layer.
//!
//! This crate is the bottom of LoomDSP's own UI layer (ADR-0041,
//! `docs/design/ui-layer.md`). It holds the values every other part passes
//! around: where something is, how big it is, what colour it has, and what
//! the pointer and the keys did. It depends on no UI toolkit and on no
//! LoomDSP crate, so a tool that has nothing to do with patches can use it.
//!
//! # Units
//!
//! Every length is an `f32`. The crate does not say what one unit is. A
//! caller that lays out a window uses logical pixels; a caller that draws a
//! zoomable canvas may use its own world units.
//!
//! The Y axis points down, as on a screen: a larger `y` is lower.
//!
//! # Rules
//!
//! The types follow a few rules, chosen for hit-testing and layout:
//!
//! - **A rectangle is half-open.** It holds its left and top edges, and not
//!   its right and bottom edges. Two rectangles side by side then never both
//!   claim the point on their shared edge.
//! - **Rectangles that only touch do not overlap.** An overlap with no area
//!   is no overlap, for the same reason.
//! - **A colour is four channels as given.** The crate does no colour space
//!   conversion. What a renderer does with the channels is its own concern.
//!
//! # Example
//!
//! ```
//! use loom_ui_core::{Point, Rect, Size};
//!
//! let button = Rect::new(Point::new(10.0, 10.0), Size::new(80.0, 24.0));
//! assert!(button.contains(Point::new(10.0, 10.0)));
//! assert!(!button.contains(Point::new(90.0, 10.0)));
//! ```

mod color;
mod geometry;
mod input;

pub use color::Color;
pub use geometry::{Point, Rect, Size, Vector};
pub use input::{Event, Key, KeyEvent, Modifiers, Platform, PointerButton, PointerEvent, ScrollDelta};
