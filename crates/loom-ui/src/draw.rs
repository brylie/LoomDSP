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
//!   after its last child. The cut is a rectangle, even on round corners;
//! - a scroll container's thumb is painted after its layer ends, so the
//!   content never covers it (section 5, rule 12);
//! - a radio's mark is a ring in the panel style's mark colour, with a dot
//!   inside when the radio is chosen, and a radio's note is in its quiet
//!   colour (section 6, rule 9).
//!
//! Each panel is painted inside a layer cut to its slot, and to every clip
//! around the slot, so what overflows a slot shows nowhere, as it is hit
//! nowhere.
//!
//! The walk paints a whole layer each time it is asked: it runs only for
//! frames that are drawn, and a still window draws none (NFR-PERF-04).

use crate::access::Access;
use crate::app::App;
use crate::element::State;
use crate::field::{FieldState, Fields};
use crate::focus::Focus;
use crate::layout::{Layout, RadioPart, Record, cut};
use crate::paint::{Align, PanelStyle, Quad, Segment, TextPainter, TextRun, Vertical};
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
    /// The custom elements' state, made, kept, or dropped at each layout.
    pub(crate) customs: &'a mut crate::custom::Customs<A>,
}

/// What painting a panel needs besides its layout: the fields' state,
/// which element has focus, which is hovered and which pressed (for the
/// [`Looks`](crate::Looks) of each), and the window's scale.
pub(crate) struct PaintState<'a> {
    /// The fields' state.
    pub(crate) fields: &'a Fields,
    /// The element with focus.
    pub(crate) focus: Option<&'a UiId>,
    /// The element under the pointer.
    pub(crate) hovered: Option<&'a UiId>,
    /// The element that shows as pressed ([`Panels::shown_pressed`]).
    pub(crate) pressed: Option<&'a UiId>,
    /// Physical pixels per logical pixel.
    pub(crate) scale: f32,
}

impl PaintState<'_> {
    /// The state `record` is painted in: off when its access is turned
    /// off, whatever else holds; then pressed, then hovered, then normal.
    fn state_of(&self, record: &Record) -> State {
        if record.access.as_ref().is_some_and(Access::is_disabled) {
            return State::Off;
        }
        let Some(id) = record.id.as_ref() else {
            return State::Normal;
        };
        if self.pressed == Some(id) {
            State::Pressed
        } else if self.hovered == Some(id) {
            State::Hovered
        } else {
            State::Normal
        }
    }
}

impl<A: App> Panels<'_, A> {
    /// The focused element when it is one of the runtime's text fields, as
    /// [`Runtime::focused_field`](crate::Runtime::focused_field) gives it,
    /// for a layer that drives the window between turns.
    pub fn focused_field(&self) -> Option<&UiId> {
        self.focus.id().filter(|id| self.window.fields.has(id))
    }

    /// Lays out what changed since the last call, and settles the focus
    /// on what is laid out, as [`Runtime::lay_out`](crate::Runtime::lay_out)
    /// does. When that gives one of the runtime's elements the focus, the
    /// routing target lets go of its own before the runtime routes again.
    pub fn lay_out(&mut self, text: &mut dyn TextMeasure) {
        let laid = self.window.lay_out(self.size, text);
        self.counts.panels_laid_out += laid.panels;
        self.counts.elements_laid_out += laid.elements;
        self.customs.settle(self.window.customs(), self.app);
        self.focus.settle(self.window);
        self.reveal();
        self.reveal_caret();
        self.place_hint(text);
        self.report_focus();
        self.report_hidden_anchors();
    }

    /// Tells the app of each root placed under an element that stopped
    /// showing, scrolled out of view or cut away, once, on the next turn
    /// (section 5, rule 13). Only a settled layout says, since where things
    /// are is known only then. A root whose anchor shows again, or that
    /// leaves the view, is told again when it next hides.
    fn report_hidden_anchors(&mut self) {
        if !self.window.is_settled(self.size) {
            return;
        }
        let hidden = self.window.hidden_anchors();
        for root in hidden.iter().filter(|root| !self.window.anchors_hidden.contains(root)) {
            if let Some(message) = self.app.anchor_hidden(root) {
                self.waker.send(message);
            }
        }
        self.window.anchors_hidden = hidden;
    }

    /// Brings the element a keyboard focus move or a scroll effect named
    /// into view, once the layout is current (section 5, rule 12). Returns
    /// true when a container moved.
    pub(crate) fn reveal(&mut self) -> bool {
        if !self.window.is_settled(self.size) {
            return false;
        }
        let Some(id) = self.window.reveal.take() else {
            return false;
        };
        self.window.scroll_to_show(&id)
    }

    /// Brings a multi-line field's caret into view after an edit or a caret
    /// move in it, or its start or end after Home or End on a Mac
    /// (section 8, rule 7). It runs only right after a layout,
    /// never in the turn of the edit: there the view is not yet rebuilt
    /// with the edit, so a box that grows a line would still have its old
    /// height. The caret's place is read now, from the field as this layout
    /// left it. Returns true when a container moved.
    fn reveal_caret(&mut self) -> bool {
        if !self.window.is_settled(self.size) {
            return false;
        }
        let Some((id, show)) = self.window.caret_reveal.take() else {
            return false;
        };
        match show {
            crate::window::FieldShow::Edge(edge) => self.window.scroll_to_show_edge(&id, edge),
            crate::window::FieldShow::Caret => {
                let Some(caret) = self
                    .window
                    .fields
                    .get(&id)
                    .and_then(crate::field::FieldState::focus_rect)
                else {
                    return false;
                };
                self.window.scroll_to_show_inside(&id, caret)
            }
        }
    }

    /// Finds a named element or visible text in the current layout.
    pub fn locate(&self, id: Option<&UiId>, text: &str) -> Option<Rect> {
        id.and_then(|id| self.window.bounds(id))
            .or_else(|| self.window.text_bounds(text))
    }

    /// Chooses the hover text to show, from the hovered element and the
    /// focused one in the layers that take input, and lays it out
    /// (section 5, rule 11).
    fn place_hint(&mut self, text: &mut dyn TextMeasure) {
        // A layout may have moved the hovered element out from under a
        // pointer that did not move: it is hovered no more, unless the
        // pointer is on its hover text.
        if let (Some((_, id)), Some(at)) = (&self.pointer.hovered, self.pointer.at) {
            let keeps_hover = self.window.bounds(id).is_some_and(|anchor| {
                anchor.contains(at)
                    || self
                        .window
                        .hint()
                        .is_some_and(|hint| hint.source == *id && hint.keeps(at, anchor))
            });
            if !keeps_hover {
                self.pointer.set_hover(None);
            }
        }
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
        let chosen = self
            .pointer
            .hint
            .choose(hovered, focused, self.pointer.hover_moves, self.focus.moves);
        let style = self.app.hint_style();
        let shown = chosen.and_then(|(source, hover)| {
            let anchor = self.window.bounds(&source)?;
            // Above everything but a question, and above a question when
            // its element is in one, so the question's panel never covers
            // it.
            let layer = self
                .window
                .layer_of(&source)
                .filter(|layer| *layer == Layer::Question)
                .unwrap_or(Layer::Overlays);
            Some(crate::hint::place(
                source, hover, anchor, self.size, &style, text, layer,
            ))
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
        let hovered = self.pointer.hovered.as_ref().map(|(_, id)| id.clone());
        let pressed = self.shown_pressed();
        let painted = self.window.paint(
            layer,
            painter,
            style,
            self.focus.id(),
            (hovered.as_ref(), pressed.as_ref()),
            self.scale,
        );
        self.counts.panels_painted += painted.panels;
        self.counts.elements_painted += painted.elements;
        // The focus ring, over every panel of the focused element's layer,
        // so a panel painted later in the layer does not cover it, and cut
        // only by the window and the scroll containers around the element,
        // so a part scrolled out of view leaves no ring outside its
        // container (section 6, rule 3; section 5, rule 12).
        if let Some((id, bounds)) = crate::runtime::shown_ring(self.window, self.app, self.focus)
            && self.window.layer_of(&id) == Some(layer)
        {
            let whole = Rect::new(Point::ORIGIN, self.size);
            let shows = self.window.ring_clip(&id).map_or(whole, |clip| cut(whole, clip));
            painter.begin_layer(shows);
            // A link that wraps has a ring around its words on each line,
            // not one around both lines and the words between.
            let pieces = self.window.pieces(&id);
            let radius = self.window.radius_of(&id);
            if pieces.is_empty() {
                painter.quad(ring(bounds, radius, style.ring));
            }
            for piece in pieces {
                painter.quad(ring(piece, radius, style.ring));
            }
            painter.end_layer();
        }
    }
}

/// The focus ring around an element with `bounds` and corner `radius`: an
/// outline `style.width` wide, `style.offset` outside the bounds, with
/// corners that follow the element's. The outline is drawn inside the
/// quad's box, so the box grows by the offset and the width.
pub(crate) fn ring(bounds: Rect, radius: f32, style: crate::focus::RingStyle) -> Quad {
    let grow = style.offset + style.width;
    Quad {
        rect: Rect::new(
            Point::new(bounds.x - grow, bounds.y - grow),
            Size::new(bounds.width + 2.0 * grow, bounds.height + 2.0 * grow),
        ),
        fill: Color::TRANSPARENT,
        border: style.color,
        border_width: style.width,
        radius: radius + grow,
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
        fields: Option<&PaintState<'_>>,
    ) -> u64 {
        let by = Vector::new(origin.x, origin.y);
        let records = self.records();
        // The clipping elements whose layers are open, outermost first.
        let mut open: Vec<usize> = Vec::new();
        // The elements with looks around this one, innermost last, with
        // the text colour of the state each shows.
        let mut tints: Vec<(usize, Option<Color>)> = Vec::new();
        for (index, record) in records.iter().enumerate() {
            while let Some(&clipping) = open.last() {
                if is_around(records, clipping, index) {
                    break;
                }
                open.pop();
                painter.end_layer();
                paint_thumb(&records[clipping], by, painter);
            }
            while let Some(&(tinting, _)) = tints.last() {
                if is_around(records, tinting, index) {
                    break;
                }
                tints.pop();
            }
            let bounds = moved(record.bounds, by);
            let look = record.look;
            let shown = look.states.map(|states| {
                let state = fields.map_or(State::Normal, |live| live.state_of(record));
                states.of(state)
            });
            if let Some(shown) = shown {
                tints.push((index, shown.text));
            }
            let tint = tints.last().and_then(|(_, colour)| *colour);
            if let Some(shadow) = look.shadow {
                painter.shadow(crate::paint::Shadow {
                    rect: Rect::new(bounds.position() + shadow.offset, bounds.size()),
                    radius: look.radius,
                    colour: shadow.colour,
                    blur: shadow.blur,
                });
            }
            if look.draws_box() {
                let (border_width, border) = look.border.unwrap_or((0.0, Color::TRANSPARENT));
                painter.quad(Quad {
                    rect: bounds,
                    fill: shown
                        .and_then(|shown| shown.background)
                        .or(look.background)
                        .unwrap_or(Color::TRANSPARENT),
                    border,
                    border_width,
                    radius: look.radius,
                });
            }
            if let Some(RadioPart::Mark { selected }) = record.radio {
                paint_mark(bounds, selected, painter, style);
            }
            if let Some(on) = record.check {
                paint_check(bounds, on, painter, style);
            }
            if let Some(slider) = &record.slider {
                paint_slider(bounds, slider, painter, style);
            }
            if let Some(glyph) = record.glyph {
                paint_glyph(bounds, glyph, painter);
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
                let run = TextRun {
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
                    colour: text
                        .colour
                        .or(text.quiet.then_some(style.quiet))
                        .or(tint)
                        .unwrap_or(style.text),
                    clip: shows,
                };
                if text.spans.is_empty() {
                    painter.text(&run);
                } else {
                    painter.text_spans(&run, text.spans);
                }
            }
            if look.clip {
                painter.begin_layer(bounds);
                open.push(index);
            }
        }
        for clipping in open.into_iter().rev() {
            painter.end_layer();
            paint_thumb(&records[clipping], by, painter);
        }
        records.len() as u64
    }
}

/// The thickness of a radio mark's ring, in logical pixels.
const MARK_RING: f32 = 1.5;

/// The size of the dot in a chosen radio's mark, in logical pixels.
const MARK_DOT: f32 = 6.0;

/// Paints a scroll container's thumb, moved by `by`, when its content
/// overflows: over its own clip's layer, which has ended, so the content
/// never covers it. A record that is no container paints nothing.
fn paint_thumb(record: &Record, by: Vector, painter: &mut dyn TextPainter) {
    let (Some(scroll), Some(thumb)) = (record.scroll, record.thumb()) else {
        return;
    };
    painter.quad(Quad {
        rect: moved(thumb, by),
        fill: scroll.style.thumb,
        border: Color::TRANSPARENT,
        border_width: 0.0,
        radius: scroll.style.radius,
    });
}

/// Paints a radio's mark in `bounds`: a ring, and a dot inside it when the
/// radio is `selected`.
fn paint_mark(bounds: Rect, selected: bool, painter: &mut dyn TextPainter, style: &PanelStyle) {
    painter.quad(Quad {
        rect: bounds,
        fill: Color::TRANSPARENT,
        border: style.mark,
        border_width: MARK_RING,
        radius: bounds.width.min(bounds.height) / 2.0,
    });
    if selected {
        let centre = Point::new(bounds.x + bounds.width / 2.0, bounds.y + bounds.height / 2.0);
        painter.quad(Quad {
            rect: Rect::new(
                Point::new(centre.x - MARK_DOT / 2.0, centre.y - MARK_DOT / 2.0),
                Size::new(MARK_DOT, MARK_DOT),
            ),
            fill: style.mark,
            border: Color::TRANSPARENT,
            border_width: 0.0,
            radius: MARK_DOT / 2.0,
        });
    }
}

/// The thickness of a check box's border, in logical pixels (section 5,
/// rule 17), as Iced's check box drew it.
const CHECK_BORDER: f32 = 1.0;

/// The radius of a check box's corners, in logical pixels.
const CHECK_RADIUS: f32 = 2.0;

/// The thickness of a check box's check mark, in logical pixels.
const CHECK_STROKE: f32 = 2.0;

/// Paints a check box's box in `bounds`: off, its edge; on, filled, with
/// a check mark on the fill (`PanelStyle::check`).
fn paint_check(bounds: Rect, on: bool, painter: &mut dyn TextPainter, style: &PanelStyle) {
    let look = style.check;
    painter.quad(Quad {
        rect: bounds,
        fill: if on { look.fill } else { Color::TRANSPARENT },
        border: if on { look.fill } else { look.edge },
        border_width: CHECK_BORDER,
        radius: CHECK_RADIUS,
    });
    if on {
        // A tick: down from the left, then up to the right.
        let at = |x: f32, y: f32| Point::new(bounds.x + x * bounds.width, bounds.y + y * bounds.height);
        let (start, low, end) = (at(0.24, 0.52), at(0.42, 0.70), at(0.76, 0.32));
        for (from, to) in [(start, low), (low, end)] {
            painter.segment(Segment {
                from,
                to,
                width: CHECK_STROKE,
                colour: look.mark,
            });
        }
    }
}

/// Paints a drawn mark in `bounds`: a select's V, or a chosen option's
/// tick, in two strokes.
fn paint_glyph(bounds: Rect, glyph: crate::select::Glyph, painter: &mut dyn TextPainter) {
    use crate::select::Glyph;
    let at = |x: f32, y: f32| Point::new(bounds.x + x * bounds.width, bounds.y + y * bounds.height);
    let (points, colour) = match glyph {
        Glyph::Opens(colour) => ([at(0.15, 0.35), at(0.5, 0.7), at(0.85, 0.35)], colour),
        Glyph::Tick(colour) => ([at(0.1, 0.55), at(0.4, 0.85), at(0.9, 0.2)], colour),
    };
    for (from, to) in [(points[0], points[1]), (points[1], points[2])] {
        painter.segment(Segment {
            from,
            to,
            width: CHECK_STROKE * 0.75,
            colour,
        });
    }
}

/// Paints a slider in `bounds`: its track across the middle, the part of
/// it filled from the minimum or the middle to the thumb, and the round
/// thumb at its position.
fn paint_slider(bounds: Rect, slider: &crate::slider::Slider, painter: &mut dyn TextPainter, style: &PanelStyle) {
    use crate::slider::{Fill, THUMB, TRACK, thumb_centre, travel};
    let middle = bounds.y + bounds.height / 2.0;
    let (left, length) = travel(bounds);
    let track = |from: f32, to: f32| {
        Rect::new(
            Point::new(from.min(to), middle - TRACK / 2.0),
            Size::new((to - from).abs(), TRACK),
        )
    };
    painter.quad(Quad::filled(track(left, left + length), style.track, TRACK / 2.0));
    let thumb = thumb_centre(bounds, slider.position());
    let start = match slider.fill() {
        Fill::FromMin => left,
        Fill::FromMiddle => left + length / 2.0,
    };
    painter.quad(Quad::filled(track(start, thumb.x), style.primary, TRACK / 2.0));
    painter.quad(Quad::filled(
        Rect::new(
            Point::new(thumb.x - THUMB / 2.0, thumb.y - THUMB / 2.0),
            Size::new(THUMB, THUMB),
        ),
        style.primary,
        THUMB / 2.0,
    ));
    // The ghost mark, over the thumb: a hollow ring in the text colour,
    // as Iced's inspector drew it.
    if let Some(ghost) = slider.ghost_position() {
        let at = thumb_centre(bounds, ghost);
        painter.quad(Quad {
            rect: Rect::new(
                Point::new(at.x - GHOST / 2.0, at.y - GHOST / 2.0),
                Size::new(GHOST, GHOST),
            ),
            fill: Color::TRANSPARENT,
            border: style.text,
            border_width: GHOST_BORDER,
            radius: GHOST / 2.0,
        });
    }
}

/// The size of a slider's ghost mark, in logical pixels (section 5, rule
/// 16), as Iced's inspector drew it.
const GHOST: f32 = 10.0;

/// The width of a ghost mark's ring, in logical pixels.
const GHOST_BORDER: f32 = 1.5;

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
    // The placeholder, while nothing is typed or being composed. The caret
    // still shows over it, at the start, where typing will go.
    if let Some((hint, colour)) = text
        .placeholder
        .filter(|_| state.text().is_empty() && !state.composing())
    {
        painter.text(&TextRun {
            text: hint,
            anchor: start,
            bounds: Size::new(content.width, content.height),
            align: Align::Left,
            vertical: Vertical::Top,
            style: text.style,
            wrap: false,
            colour,
            clip: shows,
        });
    }
    // A multi-line field's text wraps at the width its editor wraps at, so
    // the letters sit on the lines its caret and selection do (section 8,
    // rule 7); a single-line field's runs on, to scroll inside its box.
    let many = state.lines() == crate::field::Lines::Many;
    painter.text(&TextRun {
        text: state.text(),
        anchor: start,
        bounds: if many {
            Size::new(state.shown_width(), content.height)
        } else {
            Size::new(state.text_width().max(content.width), content.height)
        },
        align: Align::Left,
        vertical: Vertical::Top,
        style: text.style,
        wrap: many,
        colour: style.text,
        clip: shows,
    });
    let scale = if scale > 0.0 { scale } else { 1.0 };
    let width = scale.round().max(1.0) / scale;
    // Text the input method is composing is underlined, as the platforms
    // show it (section 8, rule 4): one line of a physical pixel at the
    // foot of each box it covers, in the text colour.
    for rect in state.preedit_boxes() {
        painter.quad(Quad {
            rect: Rect::new(
                Point::new(start.x + rect.x, snap(start.y + rect.y + rect.height - width, scale)),
                Size::new(rect.width, width),
            ),
            fill: style.text,
            border: Color::TRANSPARENT,
            border_width: 0.0,
            radius: 0.0,
        });
    }
    if focused && let Some(caret) = state.caret() {
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
