//! Where the window drew its named parts.
//!
//! The canvas knows where its own tiles are. The toolbar, the bars, and
//! the panels are drawn by the toolkit, so their places are read back from
//! the laid-out window: each part that has a UI ID is wrapped in a
//! container whose widget ID is the UI ID's text, and one widget operation
//! collects every such container's bounds, and the canvas's view, in one
//! pass. The accessibility tree and the control channel's `locate` both
//! read the result, so they agree with the screen and with each other
//! (`docs/design/ui-layer.md`, section 4, rule 1, and section 7, rule 1).
//!
//! This file is on the toolkit's side: it goes when our own widgets give
//! their own bounds (phase 3).

use iced::Rectangle;
use iced::advanced::widget::operation::{Outcome, Scrollable};
use iced::advanced::widget::{Id, Operation};
use loom_canvas::CanvasView;
use loom_canvas::iced_convert::ToCore;
use loom_ui_core::{Rect, UiId, Vector};
use std::any::Any;
use std::collections::HashMap;

/// The widget ID of the container that wraps the part named `id`. The
/// view gives its containers the same text, so the two find each other.
pub(crate) fn widget_id(id: &UiId) -> Id {
    Id::from(id.to_string())
}

/// Where one named part is in the window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Place {
    /// Its bounds in the window, in logical pixels, after scrolling. They
    /// may lie outside `clip`, when the part is scrolled out of view.
    pub rect: Rect,
    /// The bounds of the nearest scrolling area around it, which cuts off
    /// what lies outside. `None` for a part in no scrolling area.
    pub clip: Option<Rect>,
}

impl Place {
    /// True when some of the part can be seen: it is in no scrolling area,
    /// or it overlaps the one it is in.
    pub(crate) fn is_shown(&self) -> bool {
        self.clip.is_none_or(|clip| self.rect.intersects(clip))
    }
}

/// What one pass over the window found: the canvas, and every named part.
#[derive(Debug, Clone, Default)]
pub(crate) struct Places {
    /// The patch canvas and its camera.
    pub canvas: Option<CanvasView>,
    /// Each named container, by its widget ID.
    parts: HashMap<Id, Place>,
}

impl Places {
    /// Places with only the canvas known, as tests of the canvas tree use.
    #[cfg(test)]
    pub(crate) fn of_canvas(canvas: Option<CanvasView>) -> Self {
        Self {
            canvas,
            parts: HashMap::new(),
        }
    }

    /// Where the part named `id` is, if the window drew it.
    pub(crate) fn of(&self, id: &UiId) -> Option<Place> {
        self.parts.get(&widget_id(id)).copied()
    }

    /// How many named parts were found.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.parts.len()
    }
}

/// The scrolling area a part is inside: how far its content is moved, and
/// what it cuts off.
#[derive(Debug, Clone, Copy, Default)]
struct Scroll {
    /// The sum of the offsets of every scrolling area around the part.
    offset: Vector,
    /// The bounds of the nearest one, in the window.
    clip: Option<Rect>,
}

/// The widget operation that collects [`Places`].
///
/// The toolkit lays out the content of a scrolling area as if it were not
/// scrolled, so a part's place in the window is its laid-out bounds less
/// the offsets of the areas around it. A scrolling area reports itself and
/// then walks its content with `traverse` on the same operation, so the
/// operation keeps a stack: the report sets what the next `traverse`
/// enters, and leaving it restores what was there.
#[derive(Default)]
pub(crate) struct Collect {
    found: Places,
    /// The scrolling the walk is inside now.
    inside: Scroll,
    /// A scrolling area just reported, which the next `traverse` enters.
    entering: Option<Scroll>,
}

/// What [`Collect::enter`] gives back, for [`Collect::leave`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct Outside(Scroll);

impl Collect {
    /// Goes into a widget's content. Another operation that carries a
    /// `Collect` calls this before it walks on, and [`Collect::leave`]
    /// after, as this one's own `traverse` does.
    pub(crate) fn enter(&mut self) -> Outside {
        let outside = self.inside;
        if let Some(scroll) = self.entering.take() {
            self.inside = scroll;
        }
        Outside(outside)
    }

    /// Comes back out of a widget's content.
    pub(crate) fn leave(&mut self, outside: Outside) {
        self.inside = outside.0;
        // A widget that reported a scrolling area but walked nothing.
        self.entering = None;
    }

    /// What has been found so far.
    pub(crate) fn places(&self) -> Places {
        self.found.clone()
    }
}

impl Operation<Places> for Collect {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<Places>)) {
        let outside = self.enter();
        operate(self);
        self.leave(outside);
    }

    fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
        // Only this file's caller can have set `entering`: a container
        // reports itself before it walks, and it is not a scrolling area.
        self.entering = None;
        let Some(id) = id else {
            return;
        };
        let rect = moved(bounds.to_core(), self.inside.offset);
        self.found.parts.insert(
            id.clone(),
            Place {
                rect,
                clip: self.inside.clip,
            },
        );
    }

    fn scrollable(
        &mut self,
        _id: Option<&Id>,
        bounds: Rectangle,
        _content_bounds: Rectangle,
        translation: iced::Vector,
        _state: &mut dyn Scrollable,
    ) {
        // The area's own bounds are in its parent's content, so they move
        // by the parent's offset. Its content moves by both.
        let own = moved(bounds.to_core(), self.inside.offset);
        self.entering = Some(Scroll {
            offset: Vector::new(
                self.inside.offset.x + translation.x,
                self.inside.offset.y + translation.y,
            ),
            // Inside two areas, only what both show can be seen. The
            // nearest one's bounds are what the part's rectangle is cut
            // by first; the tree marks each area as clipping.
            clip: Some(own),
        });
    }

    fn custom(&mut self, _id: Option<&Id>, _bounds: Rectangle, state: &mut dyn Any) {
        if let Some(view) = state.downcast_ref::<CanvasView>() {
            self.found.canvas = Some(*view);
        }
    }

    fn finish(&self) -> Outcome<Places> {
        Outcome::Some(self.found.clone())
    }
}

/// `rect` moved back by a scroll offset.
fn moved(rect: Rect, offset: Vector) -> Rect {
    Rect::new(
        loom_ui_core::Point::new(rect.x - offset.x, rect.y - offset.y),
        loom_ui_core::Size::new(rect.width, rect.height),
    )
}

#[cfg(test)]
mod tests;
