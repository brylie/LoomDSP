//! What the canvas paints above its GPU shapes: text, and the boxes of its
//! floating parts (the choice list, the typed field, hover text).
//!
//! The canvas works out what goes where and hands each piece to a
//! [`Painter`]. It asks a [`TextMeasure`] how large a piece of text is. It
//! never names a text renderer. One object does both, a [`TextPainter`],
//! so what is measured and what is drawn are set by the same code: in the
//! app, `crate::text::CanvasText` and its paint list.
//!
//! Nothing here knows about patches, so these types could serve other
//! elements. The text style and [`TextMeasure`] moved to `loom-ui-core`
//! when phase 3 began the element tree; the painting types stay here until
//! panels draw (docs/dev/leaving-iced-plan.md, step 3.1).

use crate::tile_text::Align;
use loom_ui_core::{Color, Point, Rect, Size};

// The style and the measuring trait live in `loom-ui-core`, so the layout
// in `loom-ui` measures text with the same types (docs/design/ui-layer.md,
// section 1).
pub use loom_ui_core::{TextMeasure, TextStyle, Weight};

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

/// Takes what the canvas paints, in the order it is to appear: a later
/// piece is over an earlier one in the same layer, whether it is text or a
/// box. So a selection highlight goes before its text, and a caret after.
pub trait Painter {
    /// Paints one piece of text.
    fn text(&mut self, run: &TextRun<'_>);
    /// Paints one box.
    fn quad(&mut self, quad: Quad);
    /// Starts a layer: everything until [`end_layer`](Self::end_layer) is
    /// over everything painted before, text included, and is cut to
    /// `clip`. The canvas uses it for a floating part, whose box must
    /// cover the tile text under it.
    fn begin_layer(&mut self, clip: Rect);
    /// Ends the layer last started.
    fn end_layer(&mut self);
}

/// Paints and measures with one object, so the size the canvas lays a box
/// out by is the size of the text drawn in it.
pub trait TextPainter: Painter + TextMeasure {}

impl<T: Painter + TextMeasure + ?Sized> TextPainter for T {}

#[cfg(test)]
pub(crate) mod testing;
#[cfg(test)]
mod tests;
