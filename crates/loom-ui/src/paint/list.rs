//! What an element paints, as plain data for the renderer: boxes and
//! glyphs in layers, in the order they were painted.
//!
//! [`ListPainter`] is the [`TextPainter`] a host paints through. It
//! sets text with [`Typesetter`], so a measured box and the glyphs drawn
//! in it come from one layout.
//!
//! Order (the `Painter` contract): a later piece is over an earlier one.
//! Inside a layer, pieces of one kind in a row make one batch, so a box
//! after some text is a new batch over it. A layer is over every layer
//! before it. [`Painter::end_layer`] starts a new layer with the clip of
//! the one before, so what follows is still over what came before.

use super::{Align, Painter, Quad, Segment, Span, TextMeasure, TextRun, TextStyle, Typesetter, Vertical};
use loom_ui_core::{Color, Point, Rect, Size};
use parley::{FontData, PositionedLayoutItem};
use std::cell::RefCell;
use std::ops::Range;

/// A face at one size: what a glyph's image is drawn from.
#[derive(Debug, Clone)]
pub(crate) struct Face {
    /// The font file and the face's index in it.
    pub font: FontData,
    /// The size in logical pixels.
    pub size: f32,
    /// The variation axes' positions, normalised, as Parley chose them.
    pub coords: Vec<i16>,
    /// Bold made by thickening, for a family with no bold face.
    pub embolden: bool,
    /// Italic made by slanting, in degrees, for a family with no italic.
    pub skew: Option<f32>,
}

impl Face {
    /// True when `other` draws the same images.
    fn same(&self, other: &Face) -> bool {
        self.font.data.id() == other.font.data.id()
            && self.font.index == other.font.index
            && self.size.to_bits() == other.size.to_bits()
            && self.coords == other.coords
            && self.embolden == other.embolden
            && self.skew.map(f32::to_bits) == other.skew.map(f32::to_bits)
    }
}

/// One glyph to draw.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PlacedGlyph {
    /// Its face, an index into [`PaintList::faces`].
    pub face: u32,
    /// The glyph's number in its face.
    pub id: u32,
    /// Where its origin is on the baseline, in the host's logical pixels.
    pub origin: Point,
    /// The colour of its letters. A colour glyph keeps its own colours.
    pub colour: Color,
    /// Nothing of it is drawn outside this, in the host's logical pixels.
    pub clip: Rect,
}

/// A run of pieces of one kind, drawn together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Batch {
    /// Boxes: a range of [`PaintList::boxes`].
    Boxes(Range<u32>),
    /// Glyphs: a range of [`PaintList::glyphs`].
    Glyphs(Range<u32>),
}

/// A layer: its batches in order, cut to its clip.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Layer {
    /// Nothing in the layer is drawn outside this, in the host's logical
    /// pixels.
    pub clip: Rect,
    /// The batches, first to last.
    pub batches: Vec<Batch>,
}

/// Shapes in the same box batch, in paint order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PaintBox {
    /// An axis-aligned box.
    Quad(Quad),
    /// A rotated box.
    Segment(Segment),
}

/// Everything painted in one frame, ready for the renderer.
#[derive(Debug, Clone, Default)]
pub struct PaintList {
    /// Every box, in the order painted.
    pub(crate) boxes: Vec<PaintBox>,
    /// Every glyph, in the order painted.
    pub(crate) glyphs: Vec<PlacedGlyph>,
    /// The faces the glyphs name.
    pub(crate) faces: Vec<Face>,
    /// The layers, back to front. The first is cut to the bounds the list
    /// was made with.
    pub(crate) layers: Vec<Layer>,
    /// The clips of the layers begun and not ended, outermost first.
    open: Vec<Rect>,
}

impl PaintList {
    /// True when `other` paints exactly what this list paints: the same
    /// boxes, glyphs, faces, and layers, in the same order. A renderer that
    /// prepared this list draws `other` as it is, so a host can skip
    /// preparing a frame that did not change.
    pub fn same_as(&self, other: &PaintList) -> bool {
        self.boxes == other.boxes
            && self.glyphs == other.glyphs
            && self.layers == other.layers
            && self.faces.len() == other.faces.len()
            && self.faces.iter().zip(&other.faces).all(|(a, b)| a.same(b))
    }

    /// An empty list whose first layer is cut to `bounds`.
    pub fn new(bounds: Rect) -> Self {
        Self {
            layers: vec![Layer {
                clip: bounds,
                batches: Vec::new(),
            }],
            open: vec![bounds],
            ..Self::default()
        }
    }

    /// Starts a layer cut to `clip`, over everything before it.
    fn start_layer(&mut self, clip: Rect) {
        self.layers.push(Layer {
            clip,
            batches: Vec::new(),
        });
    }

    /// True when nothing was painted.
    pub fn is_empty(&self) -> bool {
        self.boxes.is_empty() && self.glyphs.is_empty()
    }

    /// The layer being painted into. A list made with `default` starts
    /// one with no clip.
    fn layer(&mut self) -> &mut Layer {
        if self.layers.is_empty() {
            self.start_layer(Rect::new(Point::ORIGIN, Size::new(f32::MAX, f32::MAX)));
        }
        let last = self.layers.len() - 1;
        &mut self.layers[last]
    }

    /// Adds one box to the current layer.
    fn push_box(&mut self, shape: PaintBox) {
        let index = self.boxes.len() as u32;
        self.boxes.push(shape);
        match self.layer().batches.last_mut() {
            Some(Batch::Boxes(range)) => range.end = index + 1,
            _ => self.layer().batches.push(Batch::Boxes(index..index + 1)),
        }
    }

    /// Adds one glyph to the current layer.
    fn push_glyph(&mut self, glyph: PlacedGlyph) {
        let index = self.glyphs.len() as u32;
        self.glyphs.push(glyph);
        match self.layer().batches.last_mut() {
            Some(Batch::Glyphs(range)) => range.end = index + 1,
            _ => self.layer().batches.push(Batch::Glyphs(index..index + 1)),
        }
    }

    /// The index of `face`, adding it if no glyph has named it yet.
    fn face(&mut self, face: Face) -> u32 {
        match self.faces.iter().position(|known| known.same(&face)) {
            Some(index) => index as u32,
            None => {
                self.faces.push(face);
                (self.faces.len() - 1) as u32
            }
        }
    }
}

/// Paints into a [`PaintList`], setting text with a [`Typesetter`].
///
/// It borrows the text only inside each call, never across one, so a
/// paint step may measure between two pieces of text, and nothing it calls
/// can find the text already borrowed.
pub struct ListPainter<'a> {
    text: &'a RefCell<Typesetter>,
    list: &'a mut PaintList,
}

impl<'a> ListPainter<'a> {
    /// Paints into `list` with `text`.
    pub fn new(text: &'a RefCell<Typesetter>, list: &'a mut PaintList) -> Self {
        Self { text, list }
    }
}

impl Painter for ListPainter<'_> {
    fn text(&mut self, run: &TextRun<'_>) {
        self.text_spans(run, &[]);
    }

    fn text_spans(&mut self, run: &TextRun<'_>, spans: &[Span]) {
        let wrap = run.wrap.then_some(run.bounds.width);
        let mut text = self.text.borrow_mut();
        let layout = text.layout_spans(run.text, spans, run.style, wrap);
        // The anchor is the point `align` and `vertical` name on the
        // text's own box, as the text is set.
        let size = Size::new(layout.width(), layout.height());
        let left = match run.align {
            Align::Left => run.anchor.x,
            Align::Center => run.anchor.x - size.width / 2.0,
            Align::Right => run.anchor.x - size.width,
        };
        let top = match run.vertical {
            Vertical::Top => run.anchor.y,
            Vertical::Center => run.anchor.y - size.height / 2.0,
        };
        let mut underlines = Vec::new();
        for line in layout.lines() {
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(glyphs) = item else {
                    continue;
                };
                // The brush is the span's index (`typesetter.rs`).
                let span = spans.get(usize::from(glyphs.style().brush));
                let colour = span.and_then(|span| span.colour).unwrap_or(run.colour);
                let shaped = glyphs.run();
                let synthesis = shaped.synthesis();
                let face = self.list.face(Face {
                    font: shaped.font().clone(),
                    size: shaped.font_size(),
                    coords: shaped.normalized_coords().iter().map(|c| c.to_bits()).collect(),
                    embolden: synthesis.embolden(),
                    skew: synthesis.skew(),
                });
                for glyph in glyphs.positioned_glyphs() {
                    self.list.push_glyph(PlacedGlyph {
                        face,
                        id: glyph.id,
                        origin: Point::new(left + glyph.x, top + glyph.y),
                        colour,
                        clip: run.clip,
                    });
                }
                if span.is_some_and(|span| span.underline) {
                    // At the font's own underline place and thickness, so a
                    // link's line sits as the system's own links' do.
                    // The offset is to the line's top, upward from the
                    // baseline as fonts count it; a segment is placed by
                    // its middle.
                    let metrics = shaped.font_metrics();
                    let width = metrics.underline_size.max(1.0);
                    let y = top + glyphs.baseline() - metrics.underline_offset + width / 2.0;
                    let from = left + glyphs.offset();
                    underlines.push(Segment {
                        from: Point::new(from, y),
                        to: Point::new(from + glyphs.advance(), y),
                        width,
                        colour,
                    });
                }
            }
        }
        // After the glyphs, so a line is over the letters it crosses.
        for line in underlines {
            self.list.push_box(PaintBox::Segment(line));
        }
    }

    fn quad(&mut self, quad: Quad) {
        self.list.push_box(PaintBox::Quad(quad));
    }

    fn segment(&mut self, segment: Segment) {
        self.list.push_box(PaintBox::Segment(segment));
    }

    fn begin_layer(&mut self, clip: Rect) {
        // A layer inside another is cut to both. One that does not overlap
        // its parent shows nothing.
        let clip = match self.list.open.last() {
            Some(parent) => parent
                .intersection(clip)
                .unwrap_or(Rect::new(clip.position(), Size::new(0.0, 0.0))),
            None => clip,
        };
        self.list.open.push(clip);
        self.list.start_layer(clip);
    }

    fn end_layer(&mut self) {
        // What follows is over the layer that ended, cut as before it.
        self.list.open.pop();
        if let Some(&clip) = self.list.open.last() {
            self.list.start_layer(clip);
        }
    }
}

impl TextMeasure for ListPainter<'_> {
    fn measure(&mut self, text: &str, style: TextStyle, wrap_width: Option<f32>) -> Size {
        self.text.borrow_mut().measure(text, style, wrap_width)
    }

    fn measure_spans(&mut self, text: &str, spans: &[Span], style: TextStyle, wrap_width: Option<f32>) -> Size {
        self.text.borrow_mut().measure_spans(text, spans, style, wrap_width)
    }

    fn span_rects(
        &mut self,
        text: &str,
        spans: &[Span],
        style: TextStyle,
        wrap_width: Option<f32>,
        range: Range<usize>,
    ) -> Vec<Rect> {
        self.text.borrow_mut().span_rects(text, spans, style, wrap_width, range)
    }
}

#[cfg(test)]
mod tests;
