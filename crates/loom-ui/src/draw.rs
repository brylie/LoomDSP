//! Drawing panels: the composed layout, painted (ui-layer.md, section 5,
//! rule 9).
//!
//! [`Runtime::paint`](crate::Runtime::paint) hands one layer's panels to a
//! [`TextPainter`], in draw order: each root of the layer in view order,
//! then the panels in its slots, each after all of its parent's elements.
//! That is the reverse of the order hit-testing tries them, so the user
//! clicks what they see on top.
//!
//! Inside a panel, elements are painted in tree order, parent before
//! children:
//!
//! - an element with a background or a border paints its box first, under
//!   everything inside it;
//! - a text element paints its text at the top-left of its content box,
//!   inside its border and padding, in its own colour or the panel style's.
//!   A text box taller than its lines keeps its text at the top, as in
//!   flexbox; centring is the parent's alignment;
//! - an element that clips starts a layer cut to its bounds, and ends it
//!   after its last child. The cut is a rectangle, even on round corners.
//!
//! Each panel is painted inside a layer cut to its slot, and to every clip
//! around the slot, so what overflows a slot shows nowhere, as it is hit
//! nowhere.
//!
//! The walk paints a whole layer each time it is asked: it runs only for
//! frames that are drawn, and a still window draws none (NFR-PERF-04).

use crate::layout::{Layout, Record, cut};
use crate::paint::{Align, PanelStyle, Quad, TextPainter, TextRun, Vertical};
use crate::runtime::Counts;
use crate::view::Layer;
use crate::window::Window;
use loom_ui_core::{Color, Point, Rect, Size, TextMeasure, Vector};

/// The window's panels, lent to a driver's drawing code with the app and
/// the target ([`Runtime::parts`](crate::Runtime::parts)): it lays them out
/// and paints them, with the typesetter it draws text with.
pub struct Panels<'a> {
    pub(crate) window: &'a mut Window,
    pub(crate) counts: &'a mut Counts,
    pub(crate) size: Size,
}

impl Panels<'_> {
    /// Lays out what changed since the last call, as
    /// [`Runtime::lay_out`](crate::Runtime::lay_out) does.
    pub fn lay_out(&mut self, text: &mut dyn TextMeasure) {
        let laid = self.window.lay_out(self.size, text);
        self.counts.panels_laid_out += laid.panels;
        self.counts.elements_laid_out += laid.elements;
    }

    /// Paints the panels of `layer`, as
    /// [`Runtime::paint`](crate::Runtime::paint) does.
    ///
    /// # Panics
    ///
    /// In a debug build, if something changed since the last
    /// [`Panels::lay_out`].
    pub fn paint(&mut self, layer: Layer, painter: &mut dyn TextPainter, style: &PanelStyle) {
        debug_assert!(
            self.window.is_settled(self.size),
            "the window changed since it was last laid out: call Runtime::lay_out first"
        );
        let painted = self.window.paint(layer, painter, style);
        self.counts.panels_painted += painted.panels;
        self.counts.elements_painted += painted.elements;
    }
}

/// What one paint call painted, for the runtime's counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Painted {
    /// Panels painted.
    pub(crate) panels: u64,
    /// Elements in them.
    pub(crate) elements: u64,
}

/// `rect` moved by `by`.
fn moved(rect: Rect, by: Vector) -> Rect {
    Rect::new(rect.position() + by, rect.size())
}

/// True when the record at `ancestor` is around the record at `index`.
fn is_around(records: &[Record], ancestor: usize, index: usize) -> bool {
    let mut at = records[index].parent;
    while let Some(parent) = at {
        if parent == ancestor {
            return true;
        }
        at = records[parent].parent;
    }
    false
}

impl Layout {
    /// Paints the panel with its corner at `origin` in the host, inside
    /// `clip`, there too. Returns how many elements it painted.
    pub(crate) fn paint(&self, origin: Point, clip: Rect, painter: &mut dyn TextPainter, style: &PanelStyle) -> u64 {
        let by = Vector::new(origin.x, origin.y);
        let records = self.records();
        // The clipping elements whose layers are open, outermost first.
        let mut open: Vec<usize> = Vec::new();
        for (index, record) in records.iter().enumerate() {
            while let Some(&clipping) = open.last() {
                if is_around(records, clipping, index) {
                    break;
                }
                open.pop();
                painter.end_layer();
            }
            let bounds = moved(record.bounds, by);
            let look = record.look;
            if look.draws_box() {
                let (border_width, border) = look.border.unwrap_or((0.0, Color::TRANSPARENT));
                painter.quad(Quad {
                    rect: bounds,
                    fill: look.background.unwrap_or(Color::TRANSPARENT),
                    border,
                    border_width,
                    radius: look.radius,
                });
            }
            if let Some(text) = self.text(index) {
                let content = moved(record.content(), by);
                // Its own clip too: a text that clips cuts its own letters.
                let shows = record.inner_clip().map_or(clip, |inner| cut(clip, moved(inner, by)));
                painter.text(&TextRun {
                    text: text.text,
                    anchor: content.position(),
                    bounds: content.size(),
                    align: Align::Left,
                    vertical: Vertical::Top,
                    style: text.style,
                    wrap: text.wrap,
                    colour: text.colour.unwrap_or(style.text),
                    clip: shows,
                });
            }
            if look.clip {
                painter.begin_layer(bounds);
                open.push(index);
            }
        }
        for _ in open {
            painter.end_layer();
        }
        records.len() as u64
    }
}

#[cfg(test)]
mod tests;
