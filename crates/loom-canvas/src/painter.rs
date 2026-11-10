//! What the canvas paints above its GPU shapes: text, and the boxes of its
//! floating parts (the choice list, the typed field, hover text).
//!
//! The canvas works out what goes where and hands each piece to a
//! [`Painter`]. It asks a [`TextMeasure`] how large a piece of text is. It
//! never names a text renderer. Whoever hosts the canvas supplies both:
//! today the Iced widget, later our own text renderer
//! (docs/dev/leaving-iced-plan.md, step 2.4).
//!
//! Nothing here knows about patches, so these types could serve other
//! elements. They stay in this crate until the text renderer is chosen,
//! because that choice may reshape them.

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

/// What the text may hold, which decides how much work shaping it needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shaping {
    /// Plain interface text: one glyph for each character, left to right,
    /// from the interface font. The cheap path, for labels and numbers.
    Simple,
    /// Text that may hold a symbol or a script the interface font lacks,
    /// such as a check mark. It needs full shaping and fallback fonts.
    Full,
}

/// How a piece of text is set, for drawing and for measuring.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    /// The font size in logical pixels.
    pub size: f32,
    /// The height of a line, as a multiple of `size`.
    pub line_height: f32,
    /// How heavy the letters are.
    pub weight: Weight,
    /// What the text may hold.
    pub shaping: Shaping,
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
    /// Size, line height, weight, and shaping.
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
/// piece is over an earlier one in the same layer.
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
    fn measure(&self, text: &str, style: TextStyle, wrap_width: Option<f32>) -> Size;
}

#[cfg(test)]
pub(crate) mod testing;
#[cfg(test)]
mod tests;
