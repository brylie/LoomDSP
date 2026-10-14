//! The LoomDSP patch canvas (ADR-0010).
//!
//! An Iced widget that draws modules, ports, and cables with one custom wgpu
//! primitive. Shapes are instanced quads and cables are triangle strips, so
//! GPU cost follows the number of things drawn, not the number of pixels.
//! Text goes through Iced's text system and is skipped when zoomed out.
//!
//! The application owns the data. It builds a [`Scene`], passes it to
//! [`PatchCanvas`], and applies the [`CanvasEvent`]s the canvas reports.
//! Live values travel separately: [`PortLevels`] for meters on ports and
//! cables, and the [`Scope`] widget for oscilloscope views.

mod camera;
mod gpu;
mod scene;
mod scope;
mod theme;
mod widget;

pub use camera::{Camera, MAX_ZOOM, MIN_ZOOM};
pub use scene::{CableView, GRID, Hit, ModuleView, PortLevels, PortRef, PortShape, PortView, Scene, TILE_WIDTH, snap};
pub use scope::{Scope, columns};
pub use theme::Palette;
pub use widget::{CanvasEvent, PatchCanvas};
