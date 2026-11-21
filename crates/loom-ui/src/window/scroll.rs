//! Scroll containers across the window's panels (ui-layer.md, section 5,
//! rule 12).
//!
//! A container lives in one panel's layout, and a panel in one of its slots
//! moves with it. So after a scroll the panels in that panel's slots are
//! placed again, with no new layout: their sizes did not change, only where
//! they are and what of them shows. A wheel, or a reveal, that a panel's
//! own containers cannot take passes to the containers around its slot in
//! the panel that holds it.

use super::Window;
use crate::layout::{BarPart, cut};
use crate::view::Layer;
use loom_ui_core::{Point, Rect, UiId, Vector};

/// What a press on a container's bar started: a drag of its thumb.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ThumbDrag {
    /// The panel the container is in.
    pub(crate) panel: UiId,
    /// The container's UI ID.
    pub(crate) container: UiId,
    /// How far below the thumb's top the press went down.
    pub(crate) grab: f32,
}

impl Window {
    /// Scrolls the content under `point` down by `by(line)` logical pixels
    /// (up when negative), where `line` is the innermost container's line,
    /// in the topmost panel of the layers `keep` allows that takes the
    /// point: its innermost container first, then the ones out from it,
    /// then those around its slot in the panel that holds it. Returns
    /// whether any moved, or `None` when no container is under the point.
    pub(crate) fn scroll_wheel(
        &mut self,
        point: Point,
        by: impl Fn(f32) -> f32,
        keep: impl Fn(Layer) -> bool,
    ) -> Option<bool> {
        let mut key = self.panel_at(point, keep)?;
        let record = &self.records[&key];
        let mut from = record.layout.record_at(point - origin_of(record));
        let line = self.line_around(&key, from)?;
        let mut left = by(line);
        let mut moved = false;
        while let Some(index) = from {
            let record = self.records.get_mut(&key).expect("a panel being scrolled");
            let (rest, any) = record.layout.scroll_from(index, left);
            if any {
                moved = true;
                self.slots_moved(&key);
            }
            left = rest;
            if left == 0.0 {
                break;
            }
            // Out to the containers around this panel's slot.
            let Some((parent, slot)) = self.records[&key].parent.clone() else {
                break;
            };
            from = self.records[&parent].layout.slot_record(slot);
            key = parent;
        }
        Some(moved)
    }

    /// The topmost panel of the layers `keep` allows that shows something
    /// at `point`, tried in the reverse of draw order, as a hit is.
    fn panel_at(&self, point: Point, keep: impl Fn(Layer) -> bool) -> Option<UiId> {
        self.drawn
            .iter()
            .rev()
            .find(|key| {
                let record = &self.records[*key];
                keep(record.layer)
                    && record.clip.contains(point)
                    && record.layout.record_at(point - origin_of(record)).is_some()
            })
            .cloned()
    }

    /// The line of the innermost container around the record `from` of the
    /// panel keyed `key`, or around that panel's slot further out.
    fn line_around(&self, key: &UiId, from: Option<usize>) -> Option<f32> {
        let (mut key, mut from) = (key.clone(), from);
        while let Some(index) = from {
            let layout = &self.records[&key].layout;
            let mut around = layout.containers_around(index);
            if layout.records()[index].scroll.is_some() {
                around.insert(0, index);
            }
            if let Some(first) = around.first() {
                return layout.records()[*first].scroll.map(|scroll| scroll.style.line);
            }
            let (parent, slot) = self.records[&key].parent.clone()?;
            from = self.records[&parent].layout.slot_record(slot);
            key = parent;
        }
        None
    }

    /// Scrolls the containers around the element with UI ID `id`, in its
    /// panel and then around that panel's slot, by the least distance that
    /// shows it with its ring (section 5, rule 12). Returns true when any
    /// moved.
    pub(crate) fn scroll_to_show(&mut self, id: &UiId) -> bool {
        let Some(mut key) = self.panel_of(id).cloned() else {
            return false;
        };
        let Some(bounds) = self.records[&key].layout.bounds(id) else {
            return false;
        };
        let Some(mut index) = self.records[&key].layout.index_of(id) else {
            return false;
        };
        let mut rect = bounds;
        let mut moved = false;
        loop {
            let record = self.records.get_mut(&key).expect("a panel being scrolled");
            let (now, any) = record.layout.show(index, rect);
            if any {
                moved = true;
                self.slots_moved(&key);
            }
            let Some((parent, slot)) = self.records[&key].parent.clone() else {
                break;
            };
            let Some(slot_index) = self.records[&parent].layout.slot_record(slot) else {
                break;
            };
            // The rectangle in the parent's own coordinates.
            let shift = origin_of(&self.records[&key]) - origin_of(&self.records[&parent]);
            rect = Rect::new(now.position() + shift, now.size());
            index = slot_index;
            key = parent;
        }
        moved
    }

    /// True when the element with UI ID `id` is inside one of the
    /// runtime's scroll containers, in its panel or around its panel's
    /// slot.
    pub(crate) fn in_container(&self, id: &UiId) -> bool {
        let Some(mut key) = self.panel_of(id).cloned() else {
            return false;
        };
        let Some(mut index) = self.records[&key].layout.index_of(id) else {
            return false;
        };
        loop {
            if !self.records[&key].layout.containers_around(index).is_empty() {
                return true;
            }
            let Some((parent, slot)) = self.records[&key].parent.clone() else {
                return false;
            };
            let Some(slot_index) = self.records[&parent].layout.slot_record(slot) else {
                return false;
            };
            index = slot_index;
            key = parent;
        }
    }

    /// What of the window the scroll containers around the element with UI
    /// ID `id` show: their bounds cut by each other, in its panel and
    /// around its panel's slot. `None` when no container is around it. Its
    /// focus ring is cut to this (section 6, rule 3).
    pub(crate) fn ring_clip(&self, id: &UiId) -> Option<Rect> {
        let mut key = self.panel_of(id)?.clone();
        let mut index = self.records[&key].layout.index_of(id)?;
        let mut shows: Option<Rect> = None;
        loop {
            let record = &self.records[&key];
            let layout = &record.layout;
            for container in layout.containers_around(index) {
                let bounds = layout.records()[container].bounds;
                let bounds = Rect::new(bounds.position() + origin_of(record), bounds.size());
                shows = Some(shows.map_or(bounds, |inside| cut(inside, bounds)));
            }
            let Some((parent, slot)) = record.parent.clone() else {
                return shows;
            };
            let Some(slot_index) = self.records[&parent].layout.slot_record(slot) else {
                return shows;
            };
            index = slot_index;
            key = parent;
        }
    }

    /// Scrolls the container with UI ID `id` by `change`, which gives the
    /// new offset from the old one, the viewport's height, and the most it
    /// can scroll. Returns true when it moved.
    pub(crate) fn scroll_container(&mut self, id: &UiId, change: impl FnOnce(f32, f32, f32) -> f32) -> bool {
        let Some(key) = self.panel_of(id).cloned() else {
            return false;
        };
        let layout = &mut self.records.get_mut(&key).expect("its panel").layout;
        let Some(index) = layout.scroll_index(id) else {
            return false;
        };
        let record = &layout.records()[index];
        let (Some(scroll), Some(view)) = (record.scroll, record.viewport()) else {
            return false;
        };
        let offset = change(scroll.offset, view.height, scroll.most);
        let moved = layout.scroll_to(index, offset);
        if moved {
            self.slots_moved(&key);
        }
        moved
    }

    /// One line of the container with UI ID `id`, for its keys: `None` when
    /// it is no scroll container of the runtime's.
    pub(crate) fn scroll_line(&self, id: &UiId) -> Option<f32> {
        let panel = self.panel_of(id)?;
        let layout = &self.records[panel].layout;
        layout.records()[layout.scroll_index(id)?]
            .scroll
            .map(|scroll| scroll.style.line)
    }

    /// The offset of the container with UI ID `id`, and the most it can
    /// scroll, as last placed.
    pub(crate) fn scroll_of(&self, id: &UiId) -> Option<(f32, f32)> {
        let panel = self.panel_of(id)?;
        let layout = &self.records[panel].layout;
        let scroll = layout.records()[layout.scroll_index(id)?].scroll?;
        Some((scroll.offset, scroll.most))
    }

    /// What a press at `point` on a container's bar is on, in the topmost
    /// panel of the layers `keep` allows that takes the point: the panel,
    /// the container, and the part. A press on the gutter above or below
    /// the thumb pages there at once; a press on the thumb starts a drag.
    pub(crate) fn press_bar(&mut self, point: Point, keep: impl Fn(Layer) -> bool) -> Option<ThumbDrag> {
        let key = self.panel_at(point, keep)?;
        let inside = point - origin_of(&self.records[&key]);
        let (index, part) = self.records[&key].layout.bar_at(inside)?;
        let layout = &self.records[&key].layout;
        let record = &layout.records()[index];
        let container = record.id.clone()?;
        let (scroll, view) = (record.scroll?, record.viewport()?);
        let page = view.height;
        match part {
            BarPart::Thumb(grab) => Some(ThumbDrag {
                panel: key,
                container,
                grab,
            }),
            BarPart::Above | BarPart::Below => {
                let by = if part == BarPart::Above { -page } else { page };
                let layout = &mut self.records.get_mut(&key).expect("its panel").layout;
                if layout.scroll_to(index, scroll.offset + by) {
                    self.slots_moved(&key);
                }
                // A press on the track is used, though it starts no drag.
                Some(ThumbDrag {
                    panel: key,
                    container,
                    grab: f32::NAN,
                })
            }
        }
    }

    /// Moves a dragged thumb so its grab point is at `point`'s height.
    /// Returns true when the content moved.
    pub(crate) fn drag_thumb(&mut self, drag: &ThumbDrag, point: Point) -> bool {
        if drag.grab.is_nan() {
            return false;
        }
        let Some(record) = self.records.get(&drag.panel) else {
            return false;
        };
        let inside = point - origin_of(record);
        let layout = &record.layout;
        let Some(index) = layout.scroll_index(&drag.container) else {
            return false;
        };
        let Some(offset) = layout.records()[index].offset_for_thumb(inside.y - drag.grab) else {
            return false;
        };
        let layout = &mut self.records.get_mut(&drag.panel).expect("its panel").layout;
        let moved = layout.scroll_to(index, offset);
        if moved {
            let key = drag.panel.clone();
            self.slots_moved(&key);
        }
        moved
    }

    /// Places the panels in the slots of the panel keyed `key` again, and
    /// theirs, after its content scrolled: where each is and what of it
    /// shows. Their sizes did not change, so none is laid out again. Notes
    /// the scroll, for the screen reader's tree.
    fn slots_moved(&mut self, key: &UiId) {
        self.scrolled = true;
        let Some(record) = self.records.get(key) else {
            return;
        };
        let (origin, clip) = (record.origin, record.clip);
        let offset = Vector::new(origin.x, origin.y);
        let slots: Vec<(UiId, Rect, Rect)> = record
            .layout
            .slots()
            .zip(record.layout.slot_clips())
            .map(|((child, at), shows)| {
                let shows = Rect::new(shows.position() + offset, shows.size());
                (child.clone(), at, cut(clip, shows))
            })
            .collect();
        for (index, (child, at, shows)) in slots.into_iter().enumerate() {
            if let Some(held) = self.records.get_mut(&child)
                && held.held_by(key, index)
            {
                held.origin = Point::new(origin.x + at.x, origin.y + at.y);
                held.clip = shows;
                self.slots_moved(&child);
            }
        }
    }

    /// True once since the last call when a container scrolled: what the
    /// tree reports changed.
    pub(crate) fn take_scrolled(&mut self) -> bool {
        std::mem::take(&mut self.scrolled)
    }
}

/// The corner of a panel in the window, as a vector.
fn origin_of(record: &super::Record) -> Vector {
    Vector::new(record.origin.x, record.origin.y)
}

#[cfg(test)]
pub(crate) mod tests;
