//! A small oscilloscope widget (FR-VIS-02).
//!
//! The application finds the trigger point and passes the samples to show.
//! The widget draws one thin quad per pixel column, spanning the lowest and
//! highest value of the trace inside that column. Neighbouring columns
//! overlap by one sample, so the trace reads as a connected line, and a
//! dense signal reads as a filled band instead of aliasing. The cost is one
//! quad per column, whatever the number of samples.

use iced::advanced::Widget;
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::Tree;
use iced::{Border, Color, Element, Length, Point, Rectangle, Size, mouse};

/// Thinnest the trace is drawn, in logical pixels.
const TRACE_WIDTH: f32 = 1.5;

/// An oscilloscope view of a short run of samples.
pub struct Scope<'a> {
    trace: &'a [f32],
    offset: f32,
    span: f32,
    range: f32,
    colour: Color,
    width: Length,
    height: Length,
}

impl<'a> Scope<'a> {
    /// A scope showing `trace`. The first sample is at the left edge and
    /// `span` samples fill the width, so pass `span + 1` samples to reach
    /// the right edge.
    pub fn new(trace: &'a [f32], span: f32) -> Self {
        Self {
            trace,
            offset: 0.0,
            span: span.max(1.0),
            range: 1.0,
            colour: Color::WHITE,
            width: Length::Fill,
            height: Length::Fixed(96.0),
        }
    }

    /// Shifts the trace left by a fraction of a sample, so a trigger found
    /// between two samples lands exactly on the left edge. This keeps a
    /// periodic wave still even when its period is not a whole number of
    /// samples.
    pub fn offset(mut self, offset: f32) -> Self {
        self.offset = offset;
        self
    }

    /// The value drawn at the top edge. The bottom edge is `-range`.
    pub fn range(mut self, range: f32) -> Self {
        self.range = range.max(f32::MIN_POSITIVE);
        self
    }

    /// Trace colour.
    pub fn colour(mut self, colour: Color) -> Self {
        self.colour = colour;
        self
    }

    /// Widget width.
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    /// Widget height.
    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }
}

/// The lowest and highest trace value inside each of `width` pixel columns.
///
/// Sample `i` sits at x = `(i - offset) * width / span`. Each column includes
/// the trace value at both of its edges (linear between samples), so
/// neighbouring columns always touch.
pub fn columns(trace: &[f32], offset: f32, span: f32, width: usize) -> Vec<(f32, f32)> {
    let mut out = Vec::with_capacity(width);
    let Some(&last) = trace.last() else {
        return out;
    };
    let at = |position: f32| -> f32 {
        if position <= 0.0 {
            return trace[0];
        }
        let i = position.floor() as usize;
        if i + 1 >= trace.len() {
            return last;
        }
        let t = position - i as f32;
        trace[i] + (trace[i + 1] - trace[i]) * t
    };
    let per_column = span / width.max(1) as f32;
    for column in 0..width {
        // Both edges from the same formula, so a shared edge has exactly the
        // same value in both columns.
        let start = column as f32 * per_column + offset;
        let end = (column + 1) as f32 * per_column + offset;
        let (a, b) = (at(start), at(end));
        let (mut low, mut high) = (a.min(b), a.max(b));
        let first = (start.floor() as isize + 1).max(0) as usize;
        let past = (end.ceil().max(0.0) as usize).min(trace.len());
        for &x in trace.get(first..past).unwrap_or_default() {
            low = low.min(x);
            high = high.max(x);
        }
        out.push((low, high));
    }
    out
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Scope<'_>
where
    Renderer: renderer::Renderer,
{
    fn size(&self) -> Size<Length> {
        Size::new(self.width, self.height)
    }

    fn layout(&mut self, _tree: &mut Tree, _renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        layout::atomic(limits, self.width, self.height)
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let quad = |bounds: Rectangle| renderer::Quad {
            bounds,
            snap: false,
            ..renderer::Quad::default()
        };
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: Border {
                    color: Color::from_rgb8(0x3C, 0x40, 0x4A),
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..renderer::Quad::default()
            },
            Color::from_rgb8(0x16, 0x18, 0x1C),
        );

        let middle = bounds.y + bounds.height * 0.5;
        let scale = (bounds.height * 0.5 - 2.0).max(1.0) / self.range;
        // Zero line, and faint lines at half range.
        for (fraction, alpha) in [(0.0, 0.18), (0.5, 0.07), (-0.5, 0.07)] {
            let y = middle - scale * self.range * fraction;
            renderer.fill_quad(
                quad(Rectangle::new(
                    Point::new(bounds.x, y - 0.5),
                    Size::new(bounds.width, 1.0),
                )),
                Color::from_rgba(1.0, 1.0, 1.0, alpha),
            );
        }

        renderer.with_layer(bounds, |renderer| {
            let width = bounds.width.max(0.0) as usize;
            for (column, (low, high)) in columns(self.trace, self.offset, self.span, width)
                .into_iter()
                .enumerate()
            {
                let top = middle - high * scale;
                let bottom = middle - low * scale;
                let extra = (TRACE_WIDTH - (bottom - top)).max(0.0) * 0.5;
                renderer.fill_quad(
                    quad(Rectangle::new(
                        Point::new(bounds.x + column as f32, top - extra),
                        Size::new(1.0, bottom - top + 2.0 * extra),
                    )),
                    self.colour,
                );
            }
        });
    }
}

impl<'a, Message, Theme, Renderer> From<Scope<'a>> for Element<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer + 'a,
{
    fn from(scope: Scope<'a>) -> Self {
        Element::new(scope)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_column_per_pixel() {
        assert!(columns(&[], 0.0, 10.0, 8).is_empty());
        assert_eq!(columns(&[0.0; 11], 0.0, 10.0, 8).len(), 8);
    }

    #[test]
    fn dense_trace_keeps_every_peak() {
        // 1000 samples into 10 columns: each column holds its own extremes.
        let trace: Vec<f32> = (0..=1000).map(|i| if i == 555 { 1.0 } else { 0.0 }).collect();
        let cols = columns(&trace, 0.0, 1000.0, 10);
        assert_eq!(cols[5], (0.0, 1.0));
        assert_eq!(cols[4], (0.0, 0.0));
    }

    #[test]
    fn sparse_trace_interpolates() {
        // A ramp from 0 to 1 over one sample, across four columns.
        let cols = columns(&[0.0, 1.0], 0.0, 1.0, 4);
        assert_eq!(cols, vec![(0.0, 0.25), (0.25, 0.5), (0.5, 0.75), (0.75, 1.0)]);
    }

    #[test]
    fn neighbouring_columns_touch() {
        let trace: Vec<f32> = (0..=64).map(|i| (i as f32 * 0.7).sin()).collect();
        let cols = columns(&trace, 0.3, 64.0, 37);
        for pair in cols.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            assert!(a.0 <= b.1 && b.0 <= a.1, "columns {a:?} and {b:?} do not overlap");
        }
    }

    #[test]
    fn offset_shifts_left() {
        // With an offset of half a sample, the left edge is halfway up the ramp.
        let cols = columns(&[0.0, 1.0, 1.0], 0.5, 1.0, 1);
        assert_eq!(cols, vec![(0.5, 1.0)]);
    }
}
