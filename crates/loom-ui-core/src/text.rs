//! How a piece of text is set, and the trait that says how large it is.
//!
//! Layout needs the size of a text before it can place it, and drawing
//! needs the same size, or a label would not fit the box laid out for it.
//! So one object both measures and sets text: in LoomDSP, the Parley text
//! of `loom-canvas`. This crate only names the style and the question, so
//! the layout in `loom-ui` and the canvas can share them without either
//! depending on the other.
//!
//! A paragraph whose words differ in weight, face, or colour, such as a
//! help page's text with its links, is one text with [`Span`]s over it:
//! set once, so its lines break as one paragraph, and asked where a range
//! of it lies ([`TextMeasure::span_rects`]), so a link in it can be hit and
//! focused.

use crate::{Color, Point, Rect, Size};
use std::ops::Range;

/// How heavy the letters are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Weight {
    /// The interface font as it is.
    #[default]
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

/// Which family of letters a span is set in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Face {
    /// The interface font, the system's own.
    #[default]
    Interface,
    /// The system's monospace font, for code.
    Mono,
}

/// How one piece of a text is set and painted, over the next `len` bytes
/// after the span before it. The spans of a text cover it from its start;
/// bytes past the last span take the text's own style.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Span {
    /// How many bytes of the text it covers.
    pub len: usize,
    /// How heavy its letters are.
    pub weight: Weight,
    /// Its family of letters.
    pub face: Face,
    /// The colour of its letters, or `None` for the text's own.
    pub colour: Option<Color>,
    /// A line under its letters, in their colour, as a link has.
    pub underline: bool,
}

impl Span {
    /// `len` bytes in the text's own style.
    pub fn plain(len: usize) -> Self {
        Self { len, ..Self::default() }
    }

    /// The byte range of each span in a text `text_len` bytes long, cut
    /// to the text, in order.
    pub fn ranges(spans: &[Span], text_len: usize) -> impl Iterator<Item = Range<usize>> + '_ {
        spans.iter().scan(0, move |start, span| {
            let from = (*start).min(text_len);
            *start = start.saturating_add(span.len);
            Some(from..(*start).min(text_len))
        })
    }
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

    /// The size of `text` in `style` with `spans` over it, as
    /// [`measure`](Self::measure) gives it for plain text. The default
    /// ignores the spans, for a measure that does not set text, such as a
    /// test's.
    fn measure_spans(&mut self, text: &str, spans: &[Span], style: TextStyle, wrap_width: Option<f32>) -> Size {
        let _ = spans;
        self.measure(text, style, wrap_width)
    }

    /// Where the bytes `range` of `text` lie when it is set as
    /// [`measure_spans`](Self::measure_spans) sets it: one rectangle per
    /// line the range is on, top to bottom, relative to the text's
    /// top-left corner. Empty for an empty range.
    ///
    /// The default puts the text on one line and measures the text before
    /// the range and up to its end, which is right only for a measure that
    /// does not wrap; a measure that sets text answers from its layout.
    fn span_rects(
        &mut self,
        text: &str,
        spans: &[Span],
        style: TextStyle,
        wrap_width: Option<f32>,
        range: Range<usize>,
    ) -> Vec<Rect> {
        let _ = (spans, wrap_width);
        let (Some(before), Some(through)) = (text.get(..range.start), text.get(..range.end)) else {
            return Vec::new();
        };
        if range.is_empty() {
            return Vec::new();
        }
        let left = self.measure(before, style, None).width;
        let right = self.measure(through, style, None).width;
        let height = self.measure(text, style, None).height;
        vec![Rect::new(Point::new(left, 0.0), Size::new(right - left, height))]
    }
}

#[cfg(test)]
mod tests;
