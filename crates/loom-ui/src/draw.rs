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
//! - a text field paints, in a layer cut to its content box, its
//!   selection, then its text slid left by its scroll, then its caret while
//!   it has focus and nothing is selected. The caret is one logical pixel
//!   wide, snapped to whole physical pixels so it stays sharp at any scale,
//!   and steady, so a still window draws nothing;
//! - an element that clips starts a layer cut to its bounds, and ends it
//!   after its last child. The cut is a rectangle, even on round corners.
//!
//! Each panel is painted inside a layer cut to its slot, and to every clip
//! around the slot, so what overflows a slot shows nowhere, as it is hit
//! nowhere.
//!
//! The walk paints a whole layer each time it is asked: it runs only for
//! frames that are drawn, and a still window draws none (NFR-PERF-04).

use crate::app::App;
use crate::field::{FieldState, Fields};
use crate::focus::Focus;
use crate::layout::{Layout, Record, cut};
use crate::paint::{Align, PanelStyle, Quad, TextPainter, TextRun, Vertical};
use crate::pointer::Pointer;
use crate::runtime::Counts;
use crate::view::Layer;
use crate::waker::Waker;
use crate::window::Window;
use loom_ui_core::{Color, Point, Rect, Size, TextMeasure, UiId, Vector};

/// The window's panels, lent to a driver's drawing code with the app and
/// the target ([`Runtime::parts`](crate::Runtime::parts)): it lays them out
/// and paints them, with the typesetter it draws text with.
pub struct Panels<'a, A: App> {
    pub(crate) window: &'a mut Window,
    pub(crate) counts: &'a mut Counts,
    pub(crate) size: Size,
    pub(crate) focus: &'a mut Focus,
    pub(crate) pointer: &'a mut Pointer,
    /// The app, for what its elements mean: a click's message.
    pub(crate) app: &'a A,
    /// Posts a message for the next turn and wakes the driver.
    pub(crate) waker: &'a Waker<A::Message>,
    /// Physical pixels per logical pixel, for the caret.
    pub(crate) scale: f32,
}

/// What painting a text field needs besides its layout: the fields'
/// state, which one has focus, and the window's scale.
pub(crate) struct FieldPaint<'a> {
    /// The fields' state.
    pub(crate) fields: &'a Fields,
    /// The element with focus.
    pub(crate) focus: Option<&'a UiId>,
    /// Physical pixels per logical pixel.
    pub(crate) scale: f32,
}

impl<A: App> Panels<'_, A> {
    /// Lays out what changed since the last call, and settles the focus
    /// on what is laid out, as [`Runtime::lay_out`](crate::Runtime::lay_out)
    /// does. When that gives one of the runtime's elements the focus, the
    /// routing target lets go of its own before the runtime routes again.
    pub fn lay_out(&mut self, text: &mut dyn TextMeasure) {
        let laid = self.window.lay_out(self.size, text);
        self.counts.panels_laid_out += laid.panels;
        self.counts.elements_laid_out += laid.elements;
        self.focus.settle(self.window);
        self.place_hint(text);
        self.report_focus();
    }

    /// Chooses the hover text to show, from the hovered element and the
    /// focused one in the layers that take input, and lays it out
    /// (section 5, rule 11).
    fn place_hint(&mut self, text: &mut dyn TextMeasure) {
        let input = self.app.input().layer;
        let window = &*self.window;
        let with_text = |id: &UiId| {
            window
                .layer_of(id)
                .filter(|layer| *layer >= input)
                .and_then(|_| window.hover_text_of(id))
                .map(|hover| (id.clone(), hover.to_string()))
        };
        let hovered = self.pointer.hovered.as_ref().and_then(|(_, id)| with_text(id));
        let focused = self.focus.id().and_then(with_text);
        let chosen = self.pointer.hint.choose(hovered, focused, self.focus.id());
        let style = self.app.hint_style();
        let shown = chosen.and_then(|(source, hover)| {
            let anchor = self.window.bounds(&source)?;
            Some(crate::hint::place(source, hover, anchor, self.size, &style, text))
        });
        self.window.set_hint(shown);
    }

    /// Tells the app the focused element changed since it was last told,
    /// through [`App::focus_moved`], for the next turn. Each change is told
    /// once; a ring alone is no change.
    pub(crate) fn report_focus(&mut self) {
        if let Some(message) = self.focus_report() {
            self.waker.send(message);
        }
    }

    /// The app's message for a focus move it has not been told of yet,
    /// marking it told. `None` when nothing moved, or the app does not ask.
    pub(crate) fn focus_report(&mut self) -> Option<A::Message> {
        if self.focus.id() == self.focus.reported.as_ref() {
            return None;
        }
        self.focus.reported = self.focus.id().cloned();
        self.app.focus_moved(self.focus.id())
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
        let painted = self.window.paint(layer, painter, style, self.focus.id(), self.scale);
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
    pub(crate) fn paint(
        &self,
        origin: Point,
        clip: Rect,
        painter: &mut dyn TextPainter,
        style: &PanelStyle,
        fields: Option<&FieldPaint<'_>>,
    ) -> u64 {
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
                let content = moved(record.content, by);
                // Its own clip too: a text that clips cuts its own letters.
                let shows = record.inner_clip().map_or(clip, |inner| cut(clip, moved(inner, by)));
                let field = record
                    .id
                    .as_ref()
                    .filter(|_| text.field)
                    .zip(fields)
                    .and_then(|(id, fields)| {
                        let state = fields.fields.get(id)?;
                        Some((state, fields.focus == Some(id), fields.scale))
                    });
                if let Some((state, focused, scale)) = field {
                    paint_field(
                        state,
                        focused,
                        content,
                        cut(shows, content),
                        text,
                        painter,
                        style,
                        scale,
                    );
                    if look.clip {
                        painter.begin_layer(bounds);
                        open.push(index);
                    }
                    continue;
                }
                painter.text(&TextRun {
                    text: text.text,
                    anchor: content.position(),
                    // A wrapping text breaks its lines at the width it was
                    // measured at, so it fits the height it was given.
                    bounds: Size::new(
                        if text.wrap { record.wrap_width } else { content.width },
                        content.height,
                    ),
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

/// `value` in logical pixels, moved to the nearest whole physical pixel
/// at `scale`.
fn snap(value: f32, scale: f32) -> f32 {
    (value * scale).round() / scale
}

/// Paints a text field: its selection, its text, and its caret when
/// `focused`, from its editor's layout, in a layer cut to `shows`, with its
/// text's start at the corner of `content`, slid left by its scroll.
#[allow(clippy::too_many_arguments)]
fn paint_field(
    state: &FieldState,
    focused: bool,
    content: Rect,
    shows: Rect,
    text: crate::layout::TextPaint<'_>,
    painter: &mut dyn TextPainter,
    style: &PanelStyle,
    scale: f32,
) {
    let start = Point::new(content.x - state.scroll(), content.y);
    painter.begin_layer(shows);
    for rect in state.selection() {
        painter.quad(Quad {
            rect: Rect::new(Point::new(start.x + rect.x, start.y + rect.y), rect.size()),
            fill: style.selection,
            border: Color::TRANSPARENT,
            border_width: 0.0,
            radius: 0.0,
        });
    }
    painter.text(&TextRun {
        text: state.text(),
        anchor: start,
        bounds: Size::new(state.text_width().max(content.width), content.height),
        align: Align::Left,
        vertical: Vertical::Top,
        style: text.style,
        wrap: false,
        colour: style.text,
        clip: shows,
    });
    if focused && let Some(caret) = state.caret() {
        let scale = if scale > 0.0 { scale } else { 1.0 };
        let width = scale.round().max(1.0) / scale;
        painter.quad(Quad {
            rect: Rect::new(
                Point::new(snap(start.x + caret.x, scale), snap(start.y + caret.y, scale)),
                Size::new(width, snap(caret.height, scale)),
            ),
            fill: style.caret,
            border: Color::TRANSPARENT,
            border_width: 0.0,
            radius: 0.0,
        });
    }
    painter.end_layer();
}

#[cfg(test)]
mod tests;
