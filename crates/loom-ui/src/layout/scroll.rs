//! Scroll containers inside one panel's layout (ui-layer.md, section 5,
//! rule 12).
//!
//! A container's offset is how far its content is moved up. It is kept by
//! the container's UI ID in [`Layout`], so a rebuilt view keeps it, and it
//! stays between 0 and the most the content allows, which taffy gives as
//! the container's scroll height. A scroll needs no new layout: the
//! elements are only placed again ([`Layout::place`]).
//!
//! Each container has three areas on screen:
//!
//! - the **viewport**, its content box less the gutter at the right, where
//!   the content shows;
//! - the **gutter**, as wide as the bar and its inset on both sides, where
//!   a press pages toward it;
//! - the **thumb**, inside the gutter, as long as the viewport is in
//!   proportion to the content, and only when the content overflows.

use super::{Layout, Record};
use crate::element::ScrollStyle;
use loom_ui_core::{Point, Rect, Size, UiId};

/// The shortest a thumb gets, in logical pixels, so a long content still
/// has a thumb a pointer can grab.
const MIN_THUMB: f32 = 24.0;

/// How a scroll container is set up and how far it is scrolled.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ScrollState {
    /// Its look and its steps.
    pub(crate) style: ScrollStyle,
    /// How far its content is moved up, in logical pixels.
    pub(crate) offset: f32,
    /// The most it can be moved: how much taller the content is than the
    /// viewport. 0 when it fits, and then no bar is drawn.
    pub(crate) most: f32,
}

/// What a press in a container's gutter is on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum BarPart {
    /// The thumb, this far below the thumb's top.
    Thumb(f32),
    /// The gutter above the thumb: a page up.
    Above,
    /// The gutter below the thumb: a page down.
    Below,
}

impl Record {
    /// Where its content shows, for a scroll container: its content box
    /// less the gutter at the right.
    pub(crate) fn viewport(&self) -> Option<Rect> {
        let scroll = self.scroll?;
        let content = self.content;
        Some(Rect::new(
            content.position(),
            Size::new((content.width - scroll.style.gutter()).max(0.0), content.height),
        ))
    }

    /// Its gutter, for a scroll container: the strip at the right that the
    /// bar runs in.
    pub(crate) fn gutter(&self) -> Option<Rect> {
        let view = self.viewport()?;
        Some(Rect::new(
            Point::new(view.x + view.width, view.y),
            Size::new((self.content.width - view.width).max(0.0), view.height),
        ))
    }

    /// Where its thumb is, for a scroll container whose content overflows.
    pub(crate) fn thumb(&self) -> Option<Rect> {
        let scroll = self.scroll.filter(|scroll| scroll.most > 0.0)?;
        let (track, length) = self.track()?;
        let travel = (track.height - length).max(0.0);
        let top = track.y + travel * scroll.offset / scroll.most;
        Some(Rect::new(
            Point::new(track.x, top),
            Size::new(scroll.style.width, length),
        ))
    }

    /// The column the thumb moves in, inside the gutter's inset, and the
    /// thumb's length: the viewport's share of the content, at least
    /// [`MIN_THUMB`] where the track has room for it.
    fn track(&self) -> Option<(Rect, f32)> {
        let scroll = self.scroll?;
        let gutter = self.gutter()?;
        let inset = scroll.style.inset;
        let track = Rect::new(
            Point::new(gutter.x + inset, gutter.y + inset),
            Size::new(scroll.style.width, (gutter.height - 2.0 * inset).max(0.0)),
        );
        let view = gutter.height;
        let share = if view + scroll.most > 0.0 {
            view / (view + scroll.most)
        } else {
            1.0
        };
        let length = (track.height * share).max(MIN_THUMB.min(track.height));
        Some((track, length))
    }

    /// The offset that puts the thumb's top at `top`, for a drag.
    pub(crate) fn offset_for_thumb(&self, top: f32) -> Option<f32> {
        let scroll = self.scroll?;
        let (track, length) = self.track()?;
        let travel = track.height - length;
        if travel <= 0.0 {
            return Some(0.0);
        }
        Some(((top - track.y) / travel).clamp(0.0, 1.0) * scroll.most)
    }
}

impl Layout {
    /// The record of the element with UI ID `id`.
    pub(crate) fn index_of(&self, id: &UiId) -> Option<usize> {
        self.by_id.get(id).copied()
    }

    /// The record of the scroll container with UI ID `id`.
    pub(crate) fn scroll_index(&self, id: &UiId) -> Option<usize> {
        self.by_id
            .get(id)
            .copied()
            .filter(|index| self.records[*index].scroll.is_some())
    }

    /// The scroll containers around the record `index`, the innermost
    /// first.
    pub(crate) fn containers_around(&self, index: usize) -> Vec<usize> {
        let mut around = Vec::new();
        let mut at = self.records[index].parent;
        while let Some(parent) = at {
            if self.records[parent].scroll.is_some() {
                around.push(parent);
            }
            at = self.records[parent].parent;
        }
        around
    }

    /// Scrolls the container at record `index` to `offset`, held inside
    /// what its content allows, and places the elements again. Returns true
    /// when it moved.
    pub(crate) fn scroll_to(&mut self, index: usize, offset: f32) -> bool {
        let Some(scroll) = self.records[index].scroll else {
            return false;
        };
        let offset = offset.clamp(0.0, scroll.most).round();
        if offset == scroll.offset {
            return false;
        }
        if let Some(id) = self.records[index].id.clone() {
            self.offsets.insert(id, offset);
        }
        self.place();
        true
    }

    /// Moves the content of the containers around record `index` down by
    /// `by` logical pixels (up when negative), the innermost first: what
    /// one cannot take passes to the next out. Returns what none took, and
    /// whether any moved.
    pub(crate) fn scroll_from(&mut self, index: usize, by: f32) -> (f32, bool) {
        let mut left = by;
        let mut moved = false;
        let mut containers = self.containers_around(index);
        if self.records[index].scroll.is_some() {
            containers.insert(0, index);
        }
        for container in containers {
            if left == 0.0 {
                break;
            }
            let scroll = self.records[container].scroll.expect("a container");
            let wanted = scroll.offset + left;
            let reached = wanted.clamp(0.0, scroll.most);
            if self.scroll_to(container, reached) {
                moved = true;
            }
            left = wanted - reached;
        }
        (left, moved)
    }

    /// The topmost record that shows `point`, keyed or not: where a wheel
    /// over the panel starts.
    pub(crate) fn record_at(&self, point: Point) -> Option<usize> {
        self.records().iter().rposition(|record| record.shows(point))
    }

    /// The record of the slot numbered `slot`, in tree order.
    pub(crate) fn slot_record(&self, slot: usize) -> Option<usize> {
        self.slots.get(slot).map(|(_, index)| *index)
    }

    /// What of a container's bar a press at `point` is on: the innermost
    /// container whose gutter shows there, and the part of the bar.
    pub(crate) fn bar_at(&self, point: Point) -> Option<(usize, BarPart)> {
        let index = self.records().iter().rposition(|record| {
            record.scroll.is_some_and(|scroll| scroll.most > 0.0)
                && record.shows(point)
                && record.gutter().is_some_and(|gutter| gutter.contains(point))
        })?;
        let record = &self.records[index];
        let part = match record.thumb() {
            Some(thumb) if point.y < thumb.y => BarPart::Above,
            Some(thumb) if point.y >= thumb.y + thumb.height => BarPart::Below,
            Some(thumb) => BarPart::Thumb(point.y - thumb.y),
            None => return None,
        };
        Some((index, part))
    }

    /// Scrolls each container around the record `index`, the innermost
    /// first, by the least distance that shows `rect`, in the panel, with
    /// each container's ring room and 2 logical pixels more. A `rect`
    /// taller than a viewport shows from its top. Returns where `rect` is
    /// then, and whether any container moved.
    pub(crate) fn show(&mut self, index: usize, rect: Rect) -> (Rect, bool) {
        let mut rect = rect;
        let mut moved = false;
        for container in self.containers_around(index) {
            let record = &self.records[container];
            let (Some(scroll), Some(view)) = (record.scroll, record.viewport()) else {
                continue;
            };
            let margin = scroll.style.ring + SPARE;
            let (top, bottom) = (rect.y - margin, rect.y + rect.height + margin);
            let (view_top, view_bottom) = (view.y, view.y + view.height);
            let by = if top < view_top {
                top - view_top
            } else if bottom > view_bottom {
                (bottom - view_bottom).min(top - view_top)
            } else {
                0.0
            };
            if by != 0.0 && self.scroll_to(container, scroll.offset + by) {
                let now = self.records[container].scroll.map_or(0.0, |scroll| scroll.offset);
                rect = Rect::new(Point::new(rect.x, rect.y - (now - scroll.offset)), rect.size());
                moved = true;
            }
        }
        (rect, moved)
    }
}

/// The room past the ring that a focus move leaves in view (section 5,
/// rule 12).
const SPARE: f32 = 2.0;

#[cfg(test)]
mod tests;
