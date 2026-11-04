//! The LoomDSP patch canvas (ADR-0010).
//!
//! An Iced widget that draws modules, ports, and cables with one custom wgpu
//! primitive. Shapes are instanced quads and cables are triangle strips, so
//! GPU cost follows the number of things drawn, not the number of pixels.
//! Text goes through Iced's text system. Zoomed out, where it is too small
//! to read, faint bars from the instanced buffer stand in for it.
//!
//! The application owns the data. It builds a [`Scene`], passes it to
//! [`PatchCanvas`], and applies the [`CanvasEvent`]s the canvas reports.

mod camera;
mod controls;
mod drag_line;
mod gpu;
mod hover;
mod scene;
pub mod stats;
mod theme;
mod tile_text;
mod trace;
mod widget;

pub use camera::{Camera, MAX_ZOOM, MIN_ZOOM};
pub use controls::{
    ModulationMark, PanelValues, PartValue, WidgetValue, fader_bar_point, knob_range_ring, segment_centre, xy_area,
    xy_edge_point,
};
pub use controls::{part_bounds, range_bar_point};
pub use drag_line::DragLine;
pub use scene::{
    CLOCK_MARK_RADIUS, CableView, GRID, HEADER, Hit, HoverText, LABEL_COLUMNS, ModulationView, ModuleView,
    PORT_HIT_RADIUS, PanelView, PartText, PortRef, PortShape, PortView, ROW, Scene, TILE_WIDTH, TransportSignal,
    WidgetView, WidgetViewKind, fit_label, snap, tile_size,
};
pub use theme::{MIN_TEXT_CONTRAST, Palette, contrast_ratio};
pub use tile_text::{Align, TextItem, TextKind, for_each as for_each_tile_text};
pub use trace::{MAX_POINTS as MAX_TRACE_POINTS, ScopeTraces, Trace};
pub use widget::{
    CanvasEvent, CanvasPointer, CanvasTyped, CanvasView, ControlPart, LearnView, PatchCanvas, PointerDrag, Still,
    StillDrag,
};
