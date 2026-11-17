//! Hit-testing: which element is under a point (ui-layer.md, section 5).
//!
//! Hit-testing reads only the layout, so it needs no GPU and runs the same
//! in the window and in a test.
//!
//! - **Later is on top.** A later sibling is over an earlier one, and a
//!   child is over its parent, as they are drawn.
//! - **Only keyed elements are hit.** A point over an element with no key
//!   hits the nearest keyed element around it.
//! - **An opaque element takes the point,** so nothing under it is hit,
//!   even where it holds no keyed element: a panel over the layer below.
//! - **Across panels,** the window tries each panel in the reverse of
//!   draw order (`window.rs`).
//! - **Rectangles are half-open,** so a point on the edge two elements
//!   share hits exactly one of them, the right or the lower one.
//! - **Clipping cuts hits as it cuts drawing.** A part of an element
//!   outside the clip of a clipping element around it is not hit, as it is
//!   not drawn. A clip is a rectangle, even on round corners, so a point
//!   past a corner's curve and inside the bounds is still hit, as it is
//!   still drawn there. A child that runs past a parent that does not clip
//!   is hit where it is. Across panels, a panel is cut to its slot
//!   (`window.rs`).

use crate::layout::Layout;
use loom_ui_core::{Point, UiId};

/// What a point hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit<'a> {
    /// The keyed element with this UI ID.
    Element(&'a UiId),
    /// An opaque element with no keyed element around it: the point is
    /// taken, and nothing is hit.
    Blocked,
}

impl Layout {
    /// What is under `point`, in the panel's own coordinates. `None` when
    /// the point is over nothing that takes it, so it goes to what is
    /// below the panel.
    pub fn hit(&self, point: Point) -> Option<Hit<'_>> {
        let records = self.records();
        // The element itself, or the nearest keyed element around an
        // opaque one with no key.
        let mut at = Some(self.hit_record(point)?);
        while let Some(index) = at {
            if let Some(id) = &records[index].id {
                return Some(Hit::Element(id));
            }
            at = records[index].parent;
        }
        Some(Hit::Blocked)
    }

    /// The focus stop a press at `point` focuses: the hit element, or the
    /// nearest focus stop around it. `None` when the panel does not take
    /// the point; `Some(None)` when it takes it and no stop is there.
    pub(crate) fn focus_at(&self, point: Point) -> Option<Option<&UiId>> {
        let records = self.records();
        let mut at = Some(self.hit_record(point)?);
        while let Some(index) = at {
            if records[index].focusable {
                return Some(records[index].id.as_ref());
            }
            at = records[index].parent;
        }
        Some(None)
    }

    /// The topmost record that takes `point`: a keyed or an opaque one,
    /// where it shows, so a part a clip cuts away is not hit.
    fn hit_record(&self, point: Point) -> Option<usize> {
        // Tree order, backwards, visits a later sibling before an earlier
        // one and a child before its parent: the order they are on top.
        self.records()
            .iter()
            .rposition(|record| (record.id.is_some() || record.opaque) && record.shows(point))
    }
}

#[cfg(test)]
mod tests;
