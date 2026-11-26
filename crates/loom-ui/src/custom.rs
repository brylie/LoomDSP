//! Custom elements: an element whose state, drawing, and input the app
//! gives, hosted by the runtime (ui-layer.md, section 9).
//!
//! A view places one with [`Element::custom`](crate::Element::custom) and
//! a key. The first layout that finds its UI ID asks the app to make its
//! state ([`App::make_custom`]), and the runtime keeps that state by the UI
//! ID across rebuilds and layouts. The first layout that does not find the
//! UI ID drops it. So the app decides how long the state lives by its
//! choice of key: one key for every level keeps it across level changes,
//! and a key that holds a session's number resets it on New or Open
//! (section 9, rule 1, "its state lasts").
//!
//! The state gets `&A`, the app, on each call, and builds whatever model it
//! needs from it there. Nothing is copied into the view (section 9, "the
//! model path"). `update` reads the state in the same turn through
//! [`Context::custom`](crate::Context::custom).

use crate::app::App;
use loom_ui_core::{Rect, UiId};
use std::any::Any;
use std::collections::BTreeMap;
use std::fmt;

/// Which kind of custom element a view placed, so the app knows which
/// state to make for it ([`App::make_custom`]). An app with a meter and a
/// piano roll has two kinds. The name is the app's; the runtime only
/// compares it.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct CustomKind(&'static str);

impl CustomKind {
    /// A kind named `name`.
    pub const fn new(name: &'static str) -> Self {
        Self(name)
    }

    /// Its name.
    pub fn name(self) -> &'static str {
        self.0
    }
}

impl fmt::Debug for CustomKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CustomKind({:?})", self.0)
    }
}

/// The state of one custom element, which the runtime keeps by its UI ID.
///
/// It is `Any`, so `update` can read it back as its own type
/// ([`Context::custom`](crate::Context::custom)). Every method gets the app
/// to read: the element builds the model it needs there, inside the call,
/// borrowed and not copied (section 9, the model path). A part an element
/// does not use costs it nothing: each method has a default.
pub trait Custom<A: App>: Any {}

/// The custom elements' state, by UI ID, with where each was last laid out.
pub(crate) struct Customs<A: App> {
    elements: BTreeMap<UiId, Hosted<A>>,
}

/// One custom element, as the runtime keeps it.
struct Hosted<A: App> {
    kind: CustomKind,
    /// Its bounds in the window, in logical pixels, at the last layout.
    bounds: Rect,
    /// Its state, or `None` when the app made none for its kind.
    state: Option<Box<dyn Custom<A>>>,
}

impl<A: App> Default for Customs<A> {
    fn default() -> Self {
        Self {
            elements: BTreeMap::new(),
        }
    }
}

impl<A: App> Customs<A> {
    /// Keeps, makes, or drops each element's state after a layout that
    /// `found` these custom elements, each with its kind and its bounds in
    /// the window. An element found again with another kind is made again,
    /// since its old state is of the wrong type.
    pub(crate) fn settle(&mut self, found: Vec<(UiId, CustomKind, Rect)>, app: &A) {
        let mut kept = BTreeMap::new();
        for (id, kind, bounds) in found {
            let hosted = match self.elements.remove(&id) {
                Some(hosted) if hosted.kind == kind => Hosted { bounds, ..hosted },
                _ => {
                    let state = app.make_custom(&id, kind);
                    debug_assert!(
                        state.is_some(),
                        "the app made no state for the custom element {id:?} of {kind:?}"
                    );
                    Hosted { kind, bounds, state }
                }
            };
            kept.insert(id, hosted);
        }
        self.elements = kept;
    }

    /// The state of the element with UI ID `id`.
    pub(crate) fn state(&self, id: &UiId) -> Option<&dyn Custom<A>> {
        self.elements.get(id)?.state.as_deref()
    }

    /// Where the element with UI ID `id` was at the last layout, in the
    /// window.
    pub(crate) fn bounds(&self, id: &UiId) -> Option<Rect> {
        self.elements.get(id).map(|hosted| hosted.bounds)
    }
}

/// The custom elements' state as `update` reads it: by UI ID, as `Any`.
/// [`Context`](crate::Context) is generic over the message, not the app,
/// so it holds this in place of [`Customs`].
pub(crate) trait CustomLookup {
    /// The state of the element with UI ID `id`.
    fn state(&self, id: &UiId) -> Option<&dyn Any>;
}

impl<A: App> CustomLookup for Customs<A> {
    fn state(&self, id: &UiId) -> Option<&dyn Any> {
        Customs::state(self, id).map(|state| state as &dyn Any)
    }
}

#[cfg(test)]
mod tests;
