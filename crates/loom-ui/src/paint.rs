//! Painting: text and rounded boxes, in layers, for a GPU to draw.
//!
//! An element works out what goes where and hands each piece to a
//! [`Painter`], in the order it is to appear. It asks a [`TextMeasure`]
//! how large a piece of text is. One object does both, a [`TextPainter`],
//! so what is measured and what is drawn are set by the same code.
//!
//! The parts, from the element down to the GPU:
//!
//! - [`Typesetter`] sets text with Parley and keeps each layout while
//!   frames use it (docs/design/ui-layer.md, section 8).
//! - [`ListPainter`] paints into a [`PaintList`]: boxes and glyphs in
//!   layers, each cut to its clip, in the order painted.
//! - The glyph atlas (`atlas.rs`) rasterises glyphs with `swash` and packs
//!   them into two pages, one of coverage and one of colour.
//! - The plan (`plan.rs`) turns a list into draw steps, each with its
//!   scissor in the target's pixels. It is plain data, so its order is
//!   tested with no GPU.
//! - [`PaintRenderer`] uploads what changed and draws the steps with the
//!   shaders in `paint.wgsl` ([`SHADER`]).
//!
//! The atlas, the plan, and the renderer build with the `gpu` feature,
//! which the window shell turns on. Without it, an element can still paint
//! into a list and measure text.
//!
//! None of it knows what it paints. LoomDSP's canvas paints its text and
//! floating parts through it, and panels will too (plan for leaving Iced,
//! step 3.1).

use loom_ui_core::{Color, Point, Rect, Size};

// The atlas and the plan serve only the GPU renderer, so they build with
// it. Both are plain code, and their tests need no GPU device.
#[cfg(feature = "gpu")]
mod atlas;
#[cfg(feature = "gpu")]
mod gpu;
mod list;
#[cfg(feature = "gpu")]
mod plan;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
mod typesetter;

#[cfg(feature = "gpu")]
pub use gpu::{InstanceBuffer, PaintCounts, PaintRenderer, SHAPE_ATTRIBUTES};
pub use list::{ListPainter, PaintList};
#[cfg(feature = "gpu")]
pub use plan::{Shape, Target, packed};
pub use typesetter::Typesetter;
pub(crate) use typesetter::{editing_contexts, properties};

// The style and the measuring trait live in `loom-ui-core`, so the layout
// measures text with the same types (docs/design/ui-layer.md, section 1).
pub use loom_ui_core::{TextMeasure, TextStyle, Weight};

/// The shaders that draw painted boxes and glyphs, and what a host's own
/// shaders may build on: the globals, the move to clip space, and
/// premultiplied colour. A host that draws more puts its own stages after
/// this source in one module, as LoomDSP's canvas does, so the shared
/// part exists once.
pub const SHADER: &str = include_str!("paint/paint.wgsl");

/// What a host gives the panels it paints, so `loom-ui` decides no
/// colour of its own: the app fills it from its own style tokens, which
/// are what its contrast tests check.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelStyle {
    /// The colour of text that sets none of its own.
    pub text: Color,
    /// The colour of a text field's caret.
    pub caret: Color,
    /// The colour behind a text field's selected text.
    pub selection: Color,
    /// How the focus ring looks, around the element the keyboard focused
    /// (ui-layer.md, section 6, rule 3).
    pub ring: crate::focus::RingStyle,
    /// The colour of quiet text, such as a radio's note, that sets none of
    /// its own. It needs a contrast of 4.5:1 or more against the panel
    /// (WCAG 2.2, SC 1.4.3).
    pub quiet: Color,
    /// The colour of a radio's mark: its ring, and its dot when chosen. It
    /// needs a contrast of 3:1 or more against the panel (SC 1.4.11).
    pub mark: Color,
}

/// How text sits on its anchor, left to right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    /// Starts at the anchor.
    Left,
    /// Centred on the anchor.
    Center,
    /// Ends at the anchor.
    Right,
}

/// Where a run's anchor is, top to bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vertical {
    /// The anchor is at the middle of the run's height.
    Center,
    /// The anchor is at the run's top.
    Top,
}

/// One piece of text to paint. The text is borrowed: a painter that keeps
/// it copies it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextRun<'a> {
    /// The text.
    pub text: &'a str,
    /// The point the run is placed by, in the host's logical pixels.
    /// `align` and `vertical` say which point of the run sits there.
    pub anchor: Point,
    /// The room the run has. Text wider than it is cut, or wrapped when
    /// `wrap` is set.
    pub bounds: Size,
    /// Where the anchor is, left to right.
    pub align: Align,
    /// Where the anchor is, top to bottom.
    pub vertical: Vertical,
    /// Size, line height, and weight.
    pub style: TextStyle,
    /// Break lines between words to fit `bounds`. Otherwise one line.
    pub wrap: bool,
    /// The colour of the letters.
    pub colour: Color,
    /// Nothing is painted outside this rectangle.
    pub clip: Rect,
}

/// One rounded box to paint, in the host's logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quad {
    /// Where the box is.
    pub rect: Rect,
    /// The colour inside.
    pub fill: Color,
    /// The colour of the outline.
    pub border: Color,
    /// The width of the outline. Zero for none.
    pub border_width: f32,
    /// The radius of the corners.
    pub radius: f32,
}

impl Quad {
    /// A box with no outline.
    pub fn filled(rect: Rect, fill: Color, radius: f32) -> Self {
        Self {
            rect,
            fill,
            border: Color::TRANSPARENT,
            border_width: 0.0,
            radius,
        }
    }
}

/// A straight mark between two points in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    /// The start of the mark.
    pub from: Point,
    /// The end of the mark.
    pub to: Point,
    /// The thickness across the mark.
    pub width: f32,
    /// The fill colour.
    pub colour: Color,
}

/// Takes what an element paints, in the order it is to appear: a later
/// piece is over an earlier one in the same layer, whether it is text or a
/// box. So a selection highlight goes before its text, and a caret after.
pub trait Painter {
    /// Paints one piece of text.
    fn text(&mut self, run: &TextRun<'_>);
    /// Paints one box.
    fn quad(&mut self, quad: Quad);
    /// Paints one straight mark.
    fn segment(&mut self, segment: Segment);
    /// Starts a layer: everything until [`end_layer`](Self::end_layer) is
    /// over everything painted before, text included, and is cut to
    /// `clip`. A floating part uses it, so its box covers the text under
    /// it.
    fn begin_layer(&mut self, clip: Rect);
    /// Ends the layer last started.
    fn end_layer(&mut self);
}

/// Paints and measures with one object, so the size an element lays a box
/// out by is the size of the text drawn in it.
pub trait TextPainter: Painter + TextMeasure {}

impl<T: Painter + TextMeasure + ?Sized> TextPainter for T {}

#[cfg(test)]
mod tests;
