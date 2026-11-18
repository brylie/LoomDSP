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

/// Which input moved last, for which hover text shows when the pointer and
/// the focus are on different elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Latest {
    /// The pointer: the hovered element's text.
    #[default]
    Pointer,
    /// The focus: the focused element's text.
    Focus,
}

/// What the runtime keeps between frames to choose the hover text.
#[derive(Debug, Default)]
pub(crate) struct Chooser {
    /// Which input moved last.
    pub(crate) latest: Latest,
    /// The focus as last seen, to tell when it moved.
    seen_focus: Option<UiId>,
    /// The element whose text Escape hid, until what shows changes.
    hidden: Option<UiId>,
}

impl Chooser {
    /// The element whose hover text shows, of the `hovered` element and
    /// the `focused` one, each with its text if it has any. Notes a focus
    /// move as the latest input, and forgets an Escape once what would show
    /// is another element.
    pub(crate) fn choose(
        &mut self,
        hovered: Option<(UiId, String)>,
        focused: Option<(UiId, String)>,
        focus: Option<&UiId>,
    ) -> Option<(UiId, String)> {
        if self.seen_focus.as_ref() != focus {
            self.seen_focus = focus.cloned();
            if focus.is_some() {
                self.latest = Latest::Focus;
            }
        }
        let pick = match self.latest {
            Latest::Pointer => hovered.or(focused),
            Latest::Focus => focused.or(hovered),
        };
        if self.hidden.is_some() && self.hidden.as_ref() != pick.as_ref().map(|(id, _)| id) {
            self.hidden = None;
        }
        pick.filter(|(id, _)| self.hidden.as_ref() != Some(id))
    }

    /// Hides the hover text of `shown` until what shows changes. Returns
    /// true when something was hidden.
    pub(crate) fn hide(&mut self, shown: Option<&UiId>) -> bool {
        match shown {
            Some(id) if self.hidden.as_ref() != Some(id) => {
                self.hidden = Some(id.clone());
                true
            }
            _ => false,
        }
    }
}

/// The hover text the window shows, laid out.
pub(crate) struct Shown {
    /// The element it belongs to.
    pub(crate) source: UiId,
    /// Its text.
    text: String,
    /// Its box and text, laid out alone.
    pub(crate) layout: Layout,
    /// Where its box is in the window.
    pub(crate) rect: Rect,
    /// Where the pointer keeps it showing: its box, its element, and the
    /// room between them.
    pub(crate) keep: Rect,
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
        text,
        layout,
        rect,
        keep: union(rect, anchor),
    }
}

/// The smallest rectangle around `a` and `b`.
fn union(a: Rect, b: Rect) -> Rect {
    let left = a.x.min(b.x);
    let top = a.y.min(b.y);
    let right = (a.x + a.width).max(b.x + b.width);
    let bottom = (a.y + a.height).max(b.y + b.height);
    Rect::new(Point::new(left, top), Size::new(right - left, bottom - top))
}

#[cfg(test)]
mod tests;
