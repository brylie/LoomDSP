//! How the app describes its window: panels, by key.
//!
//! This is the first, small part of `view` (ui-layer.md, section 3). Each
//! panel has a key, a layer, and a model. The runtime keeps a copy of each
//! panel's last model and compares the new one with it, so a turn that
//! changes nothing a panel shows does not redraw it (section 3, rule 2).
//!
//! What is not here yet, and arrives in phase 3: the builder that makes a
//! panel's elements from its model, layout, hit-testing, focus, and the
//! accessibility tree. `View` then gains the app's message type.

use loom_ui_core::UiId;
use std::any::Any;
use std::fmt;

/// A stacking level of the window, from the bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Layer {
    /// The patch canvas, or another custom element under everything.
    Canvas,
    /// The panels around and over the canvas.
    Panels,
    /// Pop-ups and overlays above the panels.
    Overlays,
    /// A question that holds the whole window.
    Question,
}

/// A panel's model, compared with its last one.
trait Model: Any {
    /// True when `other` is a model of the same type with the same value.
    fn same_as(&self, other: &dyn Model) -> bool;
    /// The model, to downcast.
    fn as_any(&self) -> &dyn Any;
}

impl<T: PartialEq + 'static> Model for T {
    fn same_as(&self, other: &dyn Model) -> bool {
        other.as_any().downcast_ref::<T>() == Some(self)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// One panel as `view` described it.
struct Panel {
    key: UiId,
    layer: Layer,
    model: Box<dyn Model>,
}

/// The window, as one call to `view` describes it.
#[derive(Default)]
pub struct View {
    panels: Vec<Panel>,
}

impl View {
    /// An empty description.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a panel. `key` names it across turns, `layer` says where it
    /// stacks, and `model` is everything it shows.
    ///
    /// # Panics
    ///
    /// In a debug build, if another panel in this view already has `key`:
    /// a key names one panel, and comparing with the last view relies on it.
    pub fn panel<T: PartialEq + Clone + 'static>(&mut self, key: UiId, layer: Layer, model: &T) {
        debug_assert!(
            !self.panels.iter().any(|panel| panel.key == key),
            "panel key {key} is used twice in one view"
        );
        self.panels.push(Panel {
            key,
            layer,
            model: Box::new(model.clone()),
        });
    }

    /// The panels' keys and layers, in the order they were added.
    pub fn panels(&self) -> impl Iterator<Item = (&UiId, Layer)> {
        self.panels.iter().map(|panel| (&panel.key, panel.layer))
    }

    /// How many panels of `self` differ from `last`: added, moved to
    /// another layer, or with another model, plus those `last` had and
    /// `self` lacks.
    pub(crate) fn changed_since(&self, last: &Self) -> usize {
        let changed = self
            .panels
            .iter()
            .filter(|panel| {
                !last.panels.iter().any(|old| {
                    old.key == panel.key && old.layer == panel.layer && panel.model.same_as(old.model.as_ref())
                })
            })
            .count();
        let removed = last
            .panels
            .iter()
            .filter(|old| !self.panels.iter().any(|panel| panel.key == old.key))
            .count();
        changed + removed
    }

    /// How many panels there are.
    pub fn len(&self) -> usize {
        self.panels.len()
    }

    /// True when there are no panels.
    pub fn is_empty(&self) -> bool {
        self.panels.is_empty()
    }
}

impl fmt::Debug for View {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.panels()).finish()
    }
}

#[cfg(test)]
mod tests;
