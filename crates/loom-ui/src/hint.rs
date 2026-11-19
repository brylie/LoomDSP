//! Hover text (ui-layer.md, section 5, rule 11): the text an element sets
//! with [`Element::hover_text`](crate::Element::hover_text), shown by the
//! runtime itself, since hover runs no update and no panel of the app's
//! hears it.
//!
//! - **Which shows.** The hovered element's, or the focused element's,
//!   whichever input moved last (Brylie's decision, 2026-11-17), at once.
//! - **Escape** hides it until what it belongs to changes.
//! - **It stays while the pointer is on it**, or between it and its
//!   element, so it can be read.
//! - **Where.** Above its element, its left edge at the element's, moved in
//!   to stay in the window; below the element when there is no room above.
//!
//! It is laid out with the window, after the panels, with its own
//! [`Layout`], and painted last in the Overlays layer. It has no node: it
//! is its element's description (section 7, rule 23), set from the same
//! string when the element's [`Access`](crate::access::Access) has none.

use crate::element::{Element, Length};
use crate::layout::Layout;
use loom_ui_core::{Color, Point, Rect, Size, TextMeasure, TextStyle, UiId, Weight};

/// How hover text looks: the app fills it from its style tokens
/// ([`App::hint_style`](crate::App::hint_style)), as it fills
/// [`PanelStyle`](crate::paint::PanelStyle).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HintStyle {
    /// The text's size and weight.
    pub text: TextStyle,
    /// The text's colour.
    pub colour: Color,
    /// The box's fill.
    pub background: Color,
    /// The box's border colour.
    pub border: Color,
    /// The border's width, in logical pixels.
    pub border_width: f32,
    /// The box's corner radius.
    pub radius: f32,
    /// The room between the border and the text, on every side.
    pub padding: f32,
    /// The widest the box grows before its text wraps.
    pub max_width: f32,
    /// The room between the box and its element.
    pub gap: f32,
}

impl Default for HintStyle {
    /// Dark grey on light grey, 12 px text, 6 px padding, at most 480 wide:
    /// readable, so an app that sets nothing still shows its hover text.
    fn default() -> Self {
        Self {
            text: TextStyle {
                size: 12.0,
                line_height: 1.3,
                weight: Weight::Regular,
            },
            colour: Color::rgb(0.1, 0.1, 0.1),
            background: Color::rgb(0.95, 0.95, 0.95),
            border: Color::rgb(0.6, 0.6, 0.6),
            border_width: 1.0,
            radius: 4.0,
            padding: 6.0,
            max_width: 480.0,
            gap: 2.0,
        }
    }
}

/// What the runtime keeps between frames to choose the hover text. Which
/// input moved last is told by the focus's move count ([`Focus`]): the
/// pointer records the count when it moves over an element of ours, so a
/// focus move after that, even in the same frame, raises the count past
/// it, and a pointer move after a focus move records the new count.
///
/// [`Focus`]: crate::focus::Focus
#[derive(Debug, Default)]
pub(crate) struct Chooser {
    /// The focus's move count when the pointer last moved over an element.
    /// `None` until it has: the focus then decides alone.
    pointer_at: Option<u64>,
    /// The element whose text Escape hid, with the focus's move count and
    /// the hover's move count then: it shows again once either changes.
    hidden: Option<(UiId, u64, u64)>,
}

impl Chooser {
    /// The pointer moved over one of the runtime's elements, with the
    /// focus moved `moves` times so far: the pointer is the latest input.
    pub(crate) fn pointer_moved(&mut self, moves: u64) {
        self.pointer_at = Some(moves);
    }

    /// Whether the pointer moved at least as recently as focus.
    pub(crate) fn pointer_last(&self, moves: u64) -> bool {
        self.pointer_at.is_some_and(|at| at >= moves)
    }

    /// The element whose hover text shows, of the `hovered` element and
    /// the `focused` one, each with its text if it has any: the one whose
    /// input moved last, or the other when it has none (Brylie's decision,
    /// 2026-11-17). `hover` is the hovered element, with text or not, and
    /// `moves` the focus's move count. An Escape is forgotten once the
    /// focus moved or the hover changed since it.
    pub(crate) fn choose(
        &mut self,
        hovered: Option<(UiId, String)>,
        focused: Option<(UiId, String)>,
        hover: u64,
        moves: u64,
    ) -> Option<(UiId, String)> {
        let pick = if self.pointer_last(moves) {
            hovered.or(focused)
        } else {
            focused.or(hovered)
        };
        if let Some((_, at, over)) = &self.hidden
            && (*at != moves || *over != hover)
        {
            self.hidden = None;
        }
        pick.filter(|(id, _)| self.hidden.as_ref().map(|(hid, _, _)| hid) != Some(id))
    }

    /// Hides the hover text of `shown` until the focus moves past `moves`
    /// or the hover leaves `hover`. Returns true when something was hidden.
    pub(crate) fn hide(&mut self, shown: Option<&UiId>, hover: u64, moves: u64) -> bool {
        let Some(id) = shown else {
            return false;
        };
        if self.hidden.as_ref().is_some_and(|(hid, _, _)| hid == id) {
            return false;
        }
        self.hidden = Some((id.clone(), moves, hover));
        true
    }
}

/// The hover text the window shows, laid out.
pub(crate) struct Shown {
    /// The element it belongs to.
    pub(crate) source: UiId,
    /// The layer it is painted in: the overlays, or the question when its
    /// element is in one.
    pub(crate) layer: crate::view::Layer,
    /// Its text.
    text: String,
    /// Its box and text, laid out alone.
    pub(crate) layout: Layout,
    /// Where its box is in the window.
    pub(crate) rect: Rect,
}

impl std::fmt::Debug for Shown {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shown")
            .field("source", &self.source)
            .field("text", &self.text)
            .field("rect", &self.rect)
            .finish_non_exhaustive()
    }
}

/// Lays out `text`, the hover text of the element at `anchor`, in a window
/// of `window` size, with `style`, and places it above the element, or
/// below it when there is no room above.
pub(crate) fn place(
    source: UiId,
    text: String,
    anchor: Rect,
    window: Size,
    style: &HintStyle,
    measure: &mut dyn TextMeasure,
    layer: crate::view::Layer,
) -> Shown {
    let mut layout = Layout::new(UiId::panel("loom-ui-hover-text"));
    layout.rebuild(
        Element::text(text.clone(), style.text)
            .wrap()
            .colour(style.colour)
            .padding(style.padding)
            .max_width(style.max_width.min(window.width))
            .width(Length::Shrink)
            .background(style.background)
            .border(style.border_width, style.border)
            .radius(style.radius),
    );
    layout.compute(window, measure);
    let size = layout.size();
    let x = anchor.x.min(window.width - size.width).max(0.0);
    let above = anchor.y - style.gap - size.height;
    let y = if above >= 0.0 {
        above
    } else {
        (anchor.y + anchor.height + style.gap)
            .min(window.height - size.height)
            .max(0.0)
    };
    let rect = Rect::new(Point::new(x, y), size);
    Shown {
        source,
        layer,
        text,
        layout,
        rect,
    }
}

impl Shown {
    /// The box, the current anchor, and the narrow strip linking them keep the hint.
    pub(crate) fn keeps(&self, point: Point, anchor: Rect) -> bool {
        self.rect.contains(point) || anchor.contains(point) || strip(self.rect, anchor).contains(point)
    }
}

/// The strip between the box `hint` and its element `anchor`, one above
/// the other, across the width they share: the way from one to the other.
/// Empty when they share no width.
fn strip(hint: Rect, anchor: Rect) -> Rect {
    let left = hint.x.max(anchor.x);
    let right = (hint.x + hint.width).min(anchor.x + anchor.width);
    let (top, bottom) = if hint.y + hint.height <= anchor.y {
        (hint.y + hint.height, anchor.y)
    } else {
        (anchor.y + anchor.height, hint.y)
    };
    if right <= left || bottom < top {
        return Rect::new(Point::new(left, top), Size::ZERO);
    }
    Rect::new(Point::new(left, top), Size::new(right - left, bottom - top))
}

#[cfg(test)]
mod tests;
