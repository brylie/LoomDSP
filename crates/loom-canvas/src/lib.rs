//! The LoomDSP patch canvas (ADR-0010).
//!
//! It draws modules, ports, and cables with its own wgpu pipelines.
//! Shapes are instanced quads and cables are triangle strips, so GPU cost
//! follows the number of things drawn, not the number of pixels. Zoomed
//! out, where text is too small to read, faint bars from the instanced
//! buffer stand in for it.
//!
//! The crate has three parts:
//!
//! - **Input, with no UI toolkit in it.** [`CanvasInput`] keeps the lasting
//!   state and handles one `loom-ui-core` event at a time, with the
//!   [`Moment`] it arrived in. [`CanvasModel`] is what the app shows this
//!   frame. The answer is a [`Reaction`]: the [`CanvasEvent`]s to report,
//!   whether the input was used, and whether to redraw. The canvas also
//!   says when it next needs to wake (`CanvasInput::next_deadline`).
//! - **Drawing, with no UI toolkit in it either.** [`CanvasInput::frame`]
//!   builds a [`CanvasFrame`], and a [`CanvasRenderer`] draws it into a
//!   `wgpu` render pass that the host gives it, at a [`Target`]. The host
//!   owns the window, the pass, and its viewport. [`CanvasInput::paint`]
//!   hands the text, and the boxes of the floating parts, to a
//!   [`Painter`], and sizes text with a [`TextMeasure`].
//! - **The Iced widget,** [`PatchCanvas`]. It converts Iced's events through
//!   [`iced_convert`], calls [`CanvasInput`], applies the [`Reaction`],
//!   schedules the next deadline, wraps the renderer as Iced shader
//!   primitives, and paints through Iced's text renderer. Iced is named
//!   only here and in [`iced_convert`].
//!
//! Positions, sizes, colours, and input events are `loom-ui-core` values.
//! They become Iced values only where they cross into an Iced call.
//!
//! The application owns the data. It builds a [`Scene`], passes it to
//! [`PatchCanvas`], and applies the [`CanvasEvent`]s the canvas reports.

mod camera;
mod controls;
mod draft;
mod gpu;
mod hover;
pub mod iced_convert;
mod input;
mod painter;
mod reaction;
mod scene;
pub mod stats;
mod text;
mod theme;
mod tile_text;
mod trace;
mod widget;

pub use camera::{Camera, MAX_ZOOM, MIN_ZOOM};
pub use controls::{
    ModulationMark, PanelValues, PartValue, Reading, WidgetValue, fader_bar_point, knob_range_ring, segment_centre,
    xy_area, xy_edge_point,
};
pub use controls::{arrow_share, arrow_step, option_bounds, part_bounds, range_bar_point};
pub use draft::{DraftSubmit, TypedDraft};
pub use gpu::{CanvasFrame, CanvasRenderer, Target};
pub use input::{
    CanvasEvent, CanvasFocusPolicy, CanvasInput, CanvasModel, CanvasPointer, CanvasTyped, CanvasView, ControlPart,
    EditGroupView, FrameCache, LearnView, Moment, PointerDrag, Still, StillDrag,
};
pub use painter::{Painter, Quad, TextMeasure, TextPainter, TextRun, TextStyle, Vertical, Weight};
pub use reaction::{CursorShape, Reaction};
pub use scene::{
    CLOCK_MARK_RADIUS, CableView, GRID, HEADER, Hit, HoverText, LABEL_COLUMNS, ModulationView, ModuleView,
    PORT_HIT_RADIUS, PanelView, PartText, PortRef, PortShape, PortView, ROW, Scene, TILE_WIDTH, TransportSignal,
    WidgetView, WidgetViewKind, fit_label, snap, tile_size,
};
pub use text::CanvasText;
pub use theme::{MIN_TEXT_CONTRAST, Palette, contrast_ratio};
pub use tile_text::{Align, TextItem, TextKind, for_each as for_each_tile_text};
pub use trace::{MAX_POINTS as MAX_TRACE_POINTS, ScopeTraces, Trace};
pub use widget::{DragLine, PatchCanvas};
