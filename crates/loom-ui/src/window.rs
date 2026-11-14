//! How panels compose into a window (ui-layer.md, section 5, rule 8).
//!
//! The window is laid out first, then each panel inside the place it gets.
//! A panel's elements may hold a slot ([`Element::slot`]) naming another
//! panel of the same layer, its child. A panel no slot names is a root of
//! its layer, laid out at the window's size.
//!
//! Each panel keeps its own [`Layout`] across rebuilds, with bounds relative
//! to its own origin; the slot's place is added when a bound is read. So:
//!
//! - when only a parent rebuilds and the slot keeps its size, the child is
//!   not laid out again;
//! - when a slot's size changes, the child is laid out again, with no
//!   rebuild;
//! - when only a child rebuilds, its parent is laid out again only if the
//!   child is in a shrink slot and its measured size changed.
//!
//! A slot that fills or is fixed is sized by its parent alone, so the
//! child is never asked. A shrink slot asks its child: the parent's record
//! is taken out of the table while it is laid out, and the child is laid
//! out in its own tree, so no tree is ever laid out from inside itself.
//!
//! Panels are drawn layer by layer: each root in view order, then the
//! panels in its slots, in slot order, each after all of its parent's
//! elements. Hit-testing tries them in the reverse of that order.
//!
//! Nothing clips yet: a panel wider than its slot is hit outside the slot.
//! When drawing clips each panel to its slot, hit-testing must clip it the
//! same way.
//!
//! [`Element::slot`]: crate::Element::slot

use crate::element::Element;
use crate::hit::Hit;
use crate::layout::{Constraint, Layout, Measure};
use crate::view::Layer;
use loom_ui_core::{Point, Rect, Size, TextMeasure, TextStyle, UiId, Vector};
use std::collections::HashMap;

/// How many sizes a panel in a shrink slot keeps, by the space offered.
/// Laying a slot out asks for up to nine: as narrow as it can be, as wide
/// as it wants, or a set size, along each axis. Keeping all nine means a
/// parent's rebuild asks the child nothing new (`taffy` keeps nine for the
/// same reason).
const MEASURED_SIZES: usize = 9;

/// The window's panels and their layouts.
pub(crate) struct Window {
    /// Each panel that has elements, by its key.
    records: HashMap<UiId, Record>,
    /// Their keys, in view order.
    order: Vec<UiId>,
    /// Their keys in draw order, after the last [`Window::lay_out`].
    drawn: Vec<UiId>,
    /// The window's size at the last [`Window::lay_out`].
    laid_out_at: Option<Size>,
    /// Panic on a slot mistake. On in a debug build; a test turns it off to
    /// check what a release build does.
    panic_on_mistake: bool,
}

/// One panel, as the window keeps it.
struct Record {
    layout: Layout,
    layer: Layer,
    /// The panel whose slot holds it.
    parent: Option<UiId>,
    /// Where its slot is in the window, or the window's corner for a root.
    origin: Point,
    /// The size it was last laid out in.
    slot: Option<Size>,
    /// Rebuilt since it was last laid out.
    dirty: bool,
    /// Laid out since then to measure it, so not at its slot's size.
    stale: bool,
    /// Rebuilt with sizes measured before: measure it again, and tell the
    /// parent if they changed.
    remeasure: bool,
    /// Its sizes in a shrink slot, by the space offered, oldest first.
    measured: [Option<(Constraint, Size)>; MEASURED_SIZES],
    /// How many times it was laid out, for tests.
    computes: u64,
}

/// What one [`Window::lay_out`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Laid {
    /// Panels laid out.
    pub(crate) panels: u64,
    /// Elements in them.
    pub(crate) elements: u64,
}

/// The measure a panel is laid out with: text from the driver, and the
/// panels in its shrink slots from the window's table.
struct Panels<'a> {
    records: &'a mut HashMap<UiId, Record>,
    text: &'a mut dyn TextMeasure,
    laid: &'a mut Laid,
}

impl Measure for Panels<'_> {
    fn text(&mut self, text: &str, style: TextStyle, wrap_width: Option<f32>) -> Size {
        self.text.measure(text, style, wrap_width)
    }

    fn slot(&mut self, owner: &UiId, panel: &UiId, constraint: Constraint) -> Size {
        // Only the panel's own child is asked; a panel another slot holds,
        // or one taken out of the table because it is being laid out (an
        // ancestor), has no size here.
        let Some(mut record) = self.records.remove(panel) else {
            return Size::ZERO;
        };
        let size = if record.parent.as_ref() != Some(owner) {
            Size::ZERO
        } else if let Some((_, size)) = record.measured.iter().flatten().find(|(kept, _)| *kept == constraint) {
            *size
        } else {
            let size = measure(&mut record, constraint, self);
            record.measured.rotate_left(1);
            record.measured[MEASURED_SIZES - 1] = Some((constraint, size));
            size
        };
        self.records.insert(panel.clone(), record);
        size
    }
}

/// Lays `record` out in `constraint` to learn its size. It is then not at
/// its slot's size, so it is laid out again when placed.
fn measure(record: &mut Record, constraint: Constraint, panels: &mut Panels<'_>) -> Size {
    record.layout.compute_in(constraint, panels);
    record.computes += 1;
    record.stale = true;
    panels.laid.panels += 1;
    panels.laid.elements += record.layout.element_count() as u64;
    record.layout.size()
}

impl Default for Window {
    fn default() -> Self {
        Self {
            records: HashMap::new(),
            order: Vec::new(),
            drawn: Vec::new(),
            laid_out_at: None,
            panic_on_mistake: cfg!(debug_assertions),
        }
    }
}

impl Window {
    /// Takes the panels of a new view, `built` as [`View::built`] gives
    /// them: an element tree for each panel that is new or changed. Drops
    /// the panels the view no longer has, and works out again which panel
    /// is in which slot. Returns the keys the rebuilds dropped
    /// ([`Layout::duplicate_keys`]).
    ///
    /// [`View::built`]: crate::view::View::built
    pub(crate) fn sync(&mut self, built: Vec<(UiId, Layer, Option<Element>)>) -> u64 {
        self.records
            .retain(|key, _| built.iter().any(|(kept, _, _)| kept == key));
        self.order.clear();
        let mut dropped = 0;
        for (key, layer, element) in built {
            let record = self.records.entry(key.clone()).or_insert_with(|| Record {
                layout: Layout::new(key.clone()),
                layer,
                parent: None,
                origin: Point::ORIGIN,
                slot: None,
                dirty: true,
                stale: false,
                remeasure: false,
                measured: [None; MEASURED_SIZES],
                computes: 0,
            });
            if record.layer != layer {
                record.layer = layer;
                record.dirty = true;
            }
            if let Some(element) = element {
                if !self.panic_on_mistake {
                    record.layout.allow_duplicates();
                }
                record.layout.rebuild(element);
                dropped += record.layout.duplicate_keys() as u64;
                record.dirty = true;
                record.remeasure = record.measured.iter().any(Option::is_some);
            }
            self.order.push(key);
        }
        self.link();
        dropped
    }

    /// Works out each panel's parent from the slots, in view order.
    fn link(&mut self) {
        let mut parents: HashMap<UiId, UiId> = HashMap::new();
        for owner in &self.order {
            let record = &self.records[owner];
            for (child, _) in record.layout.slots() {
                let Some(held) = self.records.get(child) else {
                    continue;
                };
                if held.layer != record.layer {
                    self.mistake(&format!(
                        "panel {owner} has a slot for {child}, which is in another layer"
                    ));
                } else if parents.contains_key(child) {
                    self.mistake(&format!("panel {child} is in two slots"));
                } else {
                    parents.insert(child.clone(), owner.clone());
                }
            }
        }
        // A slot that holds its own ancestor makes a loop with no root.
        // The first panel in view order found in one becomes a root.
        for key in &self.order {
            let mut at = parents.get(key);
            for _ in 0..self.order.len() {
                match at {
                    Some(parent) if parent == key => {
                        self.mistake(&format!("the slots of panel {key} hold its own ancestor"));
                        parents.remove(key);
                        break;
                    }
                    Some(parent) => at = parents.get(parent),
                    None => break,
                }
            }
        }
        for key in &self.order {
            let record = self.records.get_mut(key).expect("every key in the order has a record");
            record.parent = parents.get(key).cloned();
        }
    }

    /// Panics on a slot mistake in a debug build; in a release build, the
    /// first slot in view order has already won.
    fn mistake(&self, what: &str) {
        assert!(!self.panic_on_mistake, "{what}");
    }

    /// Lays out what changed in a window of `size`, from the roots down,
    /// with `text` for text, and gives what it did.
    pub(crate) fn lay_out(&mut self, size: Size, text: &mut dyn TextMeasure) -> Laid {
        let mut laid = Laid::default();
        self.remeasure(text, &mut laid);
        let mut roots: Vec<(Layer, UiId)> = self
            .order
            .iter()
            .filter(|key| self.records[*key].parent.is_none())
            .map(|key| (self.records[key].layer, key.clone()))
            .collect();
        // Stable: within a layer, the roots stay in view order.
        roots.sort_by_key(|(layer, _)| *layer);
        self.drawn.clear();
        for (_, root) in roots {
            self.place(&root, Point::ORIGIN, size, text, &mut laid);
        }
        self.laid_out_at = Some(size);
        laid
    }

    /// Measures each rebuilt panel again in the spaces its shrink slot
    /// offered before, deepest first, and tells its parent to lay out again
    /// where a size changed.
    fn remeasure(&mut self, text: &mut dyn TextMeasure, laid: &mut Laid) {
        let mut waiting: Vec<(usize, UiId)> = self
            .order
            .iter()
            .filter(|key| self.records[*key].remeasure)
            .map(|key| (self.depth(key), key.clone()))
            .collect();
        waiting.sort_by_key(|(depth, _)| std::cmp::Reverse(*depth));
        for (_, key) in waiting {
            let mut record = self.records.remove(&key).expect("a waiting panel has a record");
            let before = std::mem::replace(&mut record.measured, [None; MEASURED_SIZES]);
            let mut changed = false;
            let mut panels = Panels {
                records: &mut self.records,
                text: &mut *text,
                laid: &mut *laid,
            };
            for (index, entry) in before.iter().enumerate() {
                let Some((constraint, old)) = *entry else {
                    continue;
                };
                let size = measure(&mut record, constraint, &mut panels);
                record.measured[index] = Some((constraint, size));
                changed |= size != old;
            }
            record.remeasure = false;
            let parent = record.parent.clone();
            self.records.insert(key.clone(), record);
            if changed && let Some(parent) = parent.and_then(|parent| self.records.get_mut(&parent)) {
                parent.layout.slot_changed(&key);
                parent.dirty = true;
            }
        }
    }

    /// How many panels are around the one keyed `key`.
    fn depth(&self, key: &UiId) -> usize {
        let mut depth = 0;
        let mut at = self.records[key].parent.as_ref();
        while let Some(parent) = at {
            depth += 1;
            at = self.records[parent].parent.as_ref();
        }
        depth
    }

    /// Lays the panel keyed `key` out at `size`, if it needs it, places it
    /// at `origin`, and then places the panels in its slots.
    fn place(&mut self, key: &UiId, origin: Point, size: Size, text: &mut dyn TextMeasure, laid: &mut Laid) {
        let Some(mut record) = self.records.remove(key) else {
            return;
        };
        if record.dirty || record.stale || record.slot != Some(size) {
            let mut panels = Panels {
                records: &mut self.records,
                text: &mut *text,
                laid: &mut *laid,
            };
            record.layout.compute_in(Constraint::within(size), &mut panels);
            record.computes += 1;
            laid.panels += 1;
            laid.elements += record.layout.element_count() as u64;
            record.dirty = false;
            record.stale = false;
            record.slot = Some(size);
        }
        record.origin = origin;
        let slots: Vec<(UiId, Rect)> = record.layout.slots().map(|(child, at)| (child.clone(), at)).collect();
        self.records.insert(key.clone(), record);
        self.drawn.push(key.clone());
        for (child, at) in slots {
            if self
                .records
                .get(&child)
                .is_some_and(|held| held.parent.as_ref() == Some(key))
            {
                let place = Point::new(origin.x + at.x, origin.y + at.y);
                self.place(&child, place, at.size(), text, laid);
            }
        }
    }

    /// True when nothing changed since the last [`Window::lay_out`] in a
    /// window of `size`, so bounds and hits are current.
    pub(crate) fn is_settled(&self, size: Size) -> bool {
        (self.records.is_empty() || self.laid_out_at == Some(size))
            && self.records.values().all(|record| !record.dirty && !record.remeasure)
    }

    /// Where the element with UI ID `id` is in the window: its panel is the
    /// one whose key starts the ID, the longest such key if several do.
    pub(crate) fn bounds(&self, id: &UiId) -> Option<Rect> {
        let (_, record) = self
            .records
            .iter()
            .filter(|(key, _)| id == *key || id.is_under(key))
            .max_by_key(|(key, _)| key.keys().len())?;
        // A panel's own key gives its place: its slot, or the window.
        let bounds = if id.keys().len() == record.layout.panel().keys().len() {
            Rect::new(Point::ORIGIN, record.slot.unwrap_or_else(|| record.layout.size()))
        } else {
            record.layout.bounds(id)?
        };
        Some(Rect::new(
            Point::new(record.origin.x + bounds.x, record.origin.y + bounds.y),
            bounds.size(),
        ))
    }

    /// What is under `point` in the window: the panels are tried in the
    /// reverse of draw order, and the first that takes the point answers.
    pub(crate) fn hit(&self, point: Point) -> Option<(Layer, Hit<'_>)> {
        self.drawn.iter().rev().find_map(|key| {
            let record = &self.records[key];
            let inside = point - Vector::new(record.origin.x, record.origin.y);
            record.layout.hit(inside).map(|hit| (record.layer, hit))
        })
    }

    /// How many times the panel keyed `key` was laid out.
    #[cfg(test)]
    pub(crate) fn computes(&self, key: &UiId) -> u64 {
        self.records.get(key).map_or(0, |record| record.computes)
    }

    /// Turns off the panics on slot mistakes and duplicate keys, as in a
    /// release build.
    #[cfg(test)]
    pub(crate) fn allow_mistakes(&mut self) {
        self.panic_on_mistake = false;
    }
}

#[cfg(test)]
mod tests;
