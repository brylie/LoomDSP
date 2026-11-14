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
//! - **No clipping yet.** A child that runs past its parent is hit where it
//!   is, outside the parent's bounds. That is right while nothing clips;
//!   when scrolling and clipping come with drawing, a clipped part must
//!   stop being hit outside its clip. The same holds for panels: a panel
//!   wider than its slot is hit outside the slot until panels are clipped
//!   to their slots, and then it must be hit-tested the same way.

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
        // Tree order, backwards, visits a later sibling before an earlier
        // one and a child before its parent: the order they are on top.
        let index = records
            .iter()
            .rposition(|record| (record.id.is_some() || record.opaque) && record.bounds.contains(point))?;
        // The element itself, or the nearest keyed element around an
        // opaque one with no key.
        let mut at = Some(index);
        while let Some(index) = at {
            if let Some(id) = &records[index].id {
                return Some(Hit::Element(id));
            }
            at = records[index].parent;
        }
        Some(Hit::Blocked)
    }
}

#[cfg(test)]
mod tests;
