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
//! elements. They stay in this crate until phase 3 builds the element tree
//! (docs/dev/leaving-iced-plan.md).

use crate::tile_text::Align;
use loom_ui_core::{Color, Point, Rect, Size};

/// Where a run's anchor is, top to bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vertical {
    /// The anchor is at the middle of the run's height.
    Center,
    /// The anchor is at the run's top.
    Top,
}

/// How heavy the letters are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weight {
    /// The interface font as it is.
    Regular,
    /// Bold, for a label that must stand apart from the line under it.
    Bold,
}

/// How a piece of text is set, for drawing and for measuring. Every run is
/// shaped in full, with fallback fonts for a symbol or a script the
/// interface font lacks, so no style asks for a cheaper path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    /// The font size in logical pixels.
    pub size: f32,
    /// The height of a line, as a multiple of `size`.
    pub line_height: f32,
    /// How heavy the letters are.
    pub weight: Weight,
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

/// Says how large a piece of text is when set.
pub trait TextMeasure {
    /// The size of `text` in `style`. With `wrap_width`, lines break
    /// between words to fit that width, and the size is that of the lines
    /// together. Without it, the text is one line.
    ///
    /// It takes `&mut self` because measuring sets the text, and a measure
    /// keeps what it set for the painting that follows.
    fn measure(&mut self, text: &str, style: TextStyle, wrap_width: Option<f32>) -> Size;
}

/// Paints and measures with one object, so the size the canvas lays a box
/// out by is the size of the text drawn in it.
pub trait TextPainter: Painter + TextMeasure {}

impl<T: Painter + TextMeasure + ?Sized> TextPainter for T {}

#[cfg(test)]
pub(crate) mod testing;
#[cfg(test)]
mod tests;
