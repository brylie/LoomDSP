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
use crate::view::Layer;
use loom_ui_core::{Cursor, Modifiers, Point, PointerEvent, Rect, UiId};
use std::any::Any;
use std::collections::BTreeMap;
use std::fmt;
use std::time::Instant;

mod pointer;

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
pub trait Custom<A: App>: Any {
    /// One input event, at `moment`. The element answers whether it used
    /// it ([`Used`]), and sends its messages through `out`: they run in the
    /// turn of this event, one update each, in order (section 9, rule 1,
    /// "its messages run in the same turn"). By default it uses nothing.
    fn event(&mut self, app: &A, event: &CustomEvent, moment: &Moment, out: &mut Out<'_, A::Message>) -> Used {
        let _ = (app, event, moment, out);
        Used::No
    }

    /// The pointer shape it wants with the pointer at `moment.pointer`:
    /// over it, or anywhere while it holds the capture, so a drag that
    /// leaves it keeps its shape (section 9, rule 1, "the cursor"). By
    /// default the plain arrow.
    fn cursor(&self, app: &A, moment: &Moment) -> Cursor {
        let _ = (app, moment);
        Cursor::Default
    }
}

/// What a custom element hears.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum CustomEvent {
    /// A pointer event: inside its bounds, or anywhere while it holds the
    /// capture. Where the pointer is comes in the [`Moment`].
    Pointer(PointerEvent),
    /// The pointer left it, once: it moved off, a layer above took it, or
    /// it left the window (section 9, rule 1, "hover in and out").
    PointerLeft,
    /// Its capture ended with no release: the window lost the keys or was
    /// hidden, the app released it, or a layer above took the input, as
    /// an overlay that opens mid-drag does. The element undoes what the
    /// gesture began, where a release would finish it (section 9, rule 1,
    /// "when a capture ends").
    CaptureLost,
}

/// What a custom element did with an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[must_use]
pub enum Used {
    /// It left the event, which goes on as if nothing of the runtime's
    /// were there. The element is still hovered.
    #[default]
    No,
    /// It took the event: nothing else hears it.
    Yes,
    /// It took the event and captures the pointer: it hears every pointer
    /// event first, wherever the pointer is, until every button is up or
    /// the capture is cancelled ([`CustomEvent::CaptureLost`]). Only a
    /// press can start a capture: to any other event, `Capture` counts as
    /// [`Used::Yes`].
    Capture,
}

/// The moment an event reaches a custom element: where the pointer and the
/// element are, the time, and the modifier keys. The runtime never reads
/// the clock: `now` is the driver's moment for the turn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Moment {
    /// The pointer in window logical pixels, or `None` outside the window.
    pub pointer: Option<Point>,
    /// The element's bounds in the window, in logical pixels, at the last
    /// layout.
    pub bounds: Rect,
    /// The driver's moment for this turn.
    pub now: Instant,
    /// The modifier keys held, as last reported.
    pub modifiers: Modifiers,
}

/// Where a custom element sends its messages from one call.
pub struct Out<'a, M> {
    sent: &'a mut Vec<M>,
}

impl<M> Out<'_, M> {
    /// Sends `message`, to run in this turn after the messages sent before
    /// it.
    pub fn send(&mut self, message: M) {
        self.sent.push(message);
    }
}

/// The custom elements' state, by UI ID, with where each was last laid out.
pub(crate) struct Customs<A: App> {
    elements: BTreeMap<UiId, Hosted<A>>,
    /// The messages elements sent and the turn has not taken yet.
    sent: Vec<A::Message>,
    /// The element that holds the pointer's capture, if one does.
    captured: Option<Capture>,
    /// The custom element last told of the pointer over it, until it is
    /// told the pointer left ([`CustomEvent::PointerLeft`]).
    hovered: Option<UiId>,
}

/// A held capture of the pointer.
#[derive(Debug, Clone, PartialEq)]
struct Capture {
    /// The element that holds it.
    id: UiId,
    /// Its layer, so a layer rising above it cancels it.
    layer: Layer,
    /// How many presses of each button are down: left, right, middle, and
    /// the rest together, since `Other` folds several buttons.
    held: [u8; 4],
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
            sent: Vec::new(),
            captured: None,
            hovered: None,
        }
    }
}

impl<A: App> Customs<A> {
    /// Keeps, makes, or drops each element's state after a layout that
    /// `found` these custom elements, each with its kind and its bounds in
    /// the window. An element found again with another kind is made again,
    /// since its old state is of the wrong type. It runs at layout,
    /// outside a turn, so [`App::make_custom`] gets `&A` only and sends
    /// nothing.
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
        // An element that left the layout, or came back as another kind,
        // is dropped mid-gesture and hears nothing (section 9, rule 1).
        let live = |id: &UiId| self.elements.contains_key(id);
        if self.captured.as_ref().is_some_and(|capture| !live(&capture.id)) {
            self.captured = None;
        }
        if self.hovered.as_ref().is_some_and(|id| !live(id)) {
            self.hovered = None;
        }
    }

    /// True when `id` is a custom element's UI ID.
    pub(crate) fn is_custom(&self, id: &UiId) -> bool {
        self.elements.contains_key(id)
    }

    /// Gives `event` to the element `id` at `moment`. Its messages wait in
    /// [`Customs::take_sent`]. `Used::No` when no element has that UI ID or
    /// it has no state.
    pub(crate) fn tell(&mut self, app: &A, id: &UiId, event: &CustomEvent, moment: &Moment) -> Used {
        let Some(state) = self.elements.get_mut(id).and_then(|hosted| hosted.state.as_deref_mut()) else {
            return Used::No;
        };
        let mut out = Out { sent: &mut self.sent };
        state.event(app, event, moment, &mut out)
    }

    /// The pointer shape the element `id` wants at `moment`, or `None`
    /// when it has no state to ask.
    pub(crate) fn cursor(&self, app: &A, id: &UiId, moment: &Moment) -> Option<Cursor> {
        Some(self.state(id)?.cursor(app, moment))
    }

    /// The messages elements sent since the last call, in order.
    pub(crate) fn take_sent(&mut self) -> Vec<A::Message> {
        std::mem::take(&mut self.sent)
    }

    /// Takes away the state of the element `id`, as a release build leaves
    /// an element whose kind the app did not know.
    #[cfg(test)]
    pub(crate) fn empty(&mut self, id: &UiId) {
        if let Some(hosted) = self.elements.get_mut(id) {
            hosted.state = None;
        }
    }

    /// The element that holds the capture.
    pub(crate) fn captured(&self) -> Option<&UiId> {
        self.captured.as_ref().map(|capture| &capture.id)
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
