//! The LoomDSP patch canvas (ADR-0010).
//!
//! It draws modules, ports, and cables with its own wgpu pipelines.
//! Shapes are instanced quads and cables are triangle strips, so GPU cost
//! follows the number of things drawn, not the number of pixels. Zoomed
//! out, where text is too small to read, faint bars from the instanced
//! buffer stand in for it.
//!
//! The crate has two parts, with no UI toolkit in either:
//!
//! - **Input.** [`CanvasInput`] keeps the lasting state and handles one
//!   `loom-ui-core` event at a time, with the [`Moment`] it arrived in.
//!   [`CanvasModel`] is what the app shows this frame. The answer is a
//!   [`Reaction`]: the [`CanvasEvent`]s to report, whether the input was
//!   used, and whether to redraw. The canvas also says when it next needs
//!   to wake (`CanvasInput::next_deadline`).
//! - **Drawing.** [`CanvasInput::frame`] builds a [`CanvasFrame`], and a
//!   [`CanvasRenderer`] draws it into a `wgpu` render pass that the host
//!   gives it, at a [`Target`](loom_ui::paint::Target). The host owns the
//!   window, the pass, and its viewport. [`CanvasInput::paint`] hands the
//!   text, and the boxes of the floating parts, to a
//!   [`Painter`](loom_ui::paint::Painter), and sizes text with a
//!   [`TextMeasure`](loom_ui::paint::TextMeasure).
//!
//! Positions, sizes, colours, and input events are `loom-ui-core` values.
//! The app hosts the canvas as a `loom-ui` custom element (`loom-app`'s
//! `canvas.rs`, ui-layer.md, section 9).
//!
//! The application owns the data. It builds a [`Scene`], passes it in a
//! [`CanvasModel`], and applies the [`CanvasEvent`]s the canvas reports.

mod camera;
mod controls;
mod draft;
mod gpu;
mod hover;
mod input;
mod reaction;
mod scene;
mod slot;
pub mod stats;
mod theme;
mod tile_text;
mod trace;

pub use camera::{Camera, MAX_ZOOM, MIN_ZOOM};
pub use controls::{
    ModulationMark, PanelValues, PartValue, Reading, WidgetValue, fader_bar_point, knob_range_ring, segment_centre,
    xy_area, xy_edge_point,
};
pub use controls::{arrow_share, arrow_step, option_bounds, part_bounds, range_bar_point};
pub use draft::{DraftSubmit, TypedDraft};
pub use gpu::{Cable, CanvasFrame, CanvasRenderer};
pub use hover::{HOVER_DELAY, HoverTarget};
pub use input::{
    BrowserPlacement, CanvasEvent, CanvasFocusPolicy, CanvasInput, CanvasModel, CanvasPointer, CanvasTyped, CanvasView,
    ControlPart, EditGroupView, FrameCache, LearnView, MapGraph, MapLine, MapView, Moment, PointerDrag, READABLE_ZOOM,
    Still, StillDrag, TypedEdit, TypedRequest,
};
pub use reaction::Reaction;
pub use scene::{
    CLOCK_MARK_RADIUS, CableView, CableWidth, GRID, HEADER, Hit, HoverText, LABEL_COLUMNS, ModulationView, ModuleView,
    PORT_HIT_RADIUS, PanelView, PartText, PortRef, PortShape, PortView, ROW, Scene, TILE_WIDTH, TransportSignal,
    WidgetView, WidgetViewKind, fit_label, snap, tile_size,
};
pub use slot::CanvasDrawing;
pub mod shortcuts;
pub use theme::{MIN_TEXT_CONTRAST, Palette, contrast_ratio};
pub use tile_text::{TextItem, TextKind, for_each as for_each_tile_text};
pub use trace::{MAX_POINTS as MAX_TRACE_POINTS, ScopeTraces, Trace, trace_draw_area};
