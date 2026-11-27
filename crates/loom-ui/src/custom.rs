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
use loom_ui_core::{Cursor, IdKey, KeyEvent, Modifiers, Point, PointerEvent, Rect, UiId};
use std::any::Any;
use std::collections::BTreeMap;
use std::fmt;
use std::time::Instant;

mod keys;
mod nodes;
mod pointer;
mod ticks;

pub use nodes::{AccessRequest, AccessValue, CustomNode, CustomNodes};

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

    /// Focuses the part inside it at `rest`, the path under its UI ID, for
    /// a focus effect that named that part. Returns false when it has no
    /// such part, and the effect is dropped (section 9, rule 6). By default
    /// it has none.
    fn focus_inner(&mut self, app: &A, rest: &[IdKey]) -> bool {
        let _ = (app, rest);
        false
    }

    /// The path under its UI ID of the part inside it with the focus, while
    /// the runtime's focus is on it. The focused part's UI ID is the
    /// element's followed by this path, and that is what the app hears
    /// ([`App::focus_moved`]) and reads in its context. `None` when the
    /// element itself is focused, as by default.
    fn focused_inner(&self, app: &A) -> Option<Vec<IdKey>> {
        let _ = app;
        None
    }

    /// When it next needs a tick ([`CustomEvent::Deadline`]), worked out
    /// from the moments it was given, as a caret's next blink or a held
    /// key's next repeat. The runtime asks after each turn and wakes at the
    /// soonest deadline any element or the app gives (section 9, rule 4).
    /// After its tick it must give a later instant, or `None`: a deadline
    /// still due would wake the loop at once, every turn. A debug build
    /// panics on one; a release build leaves that element's deadline out
    /// of the schedule until it gives a later one, and still ticks it in
    /// a turn some other cause brings. `None`, the default, when it needs
    /// none.
    fn next_deadline(&self, app: &A) -> Option<Instant> {
        let _ = app;
        None
    }

    /// True while it moves and needs a tick each display refresh
    /// ([`CustomEvent::Frame`]), as a meter does while its level falls
    /// (section 9, rule 4). The runtime asks after each turn. By default it
    /// never animates.
    fn animating(&self, app: &A) -> bool {
        let _ = app;
        false
    }

    /// Adds a node for each part inside it a user can reach, in its own
    /// focus order, with `bounds` where it is in the window (section 9,
    /// rule 3). Its own node comes from the [`Access`](crate::Access) the
    /// view gave it; these go under that one. The runtime asks after a
    /// layout that made it or moved it, after an event or tick that said
    /// its nodes changed ([`Out::nodes_changed`]), and after a turn in
    /// which [`Custom::access_version`] changed, and keeps them until it
    /// next asks. By default it has no inner parts.
    fn access(&self, app: &A, bounds: Rect, nodes: &mut CustomNodes) {
        let _ = (app, bounds, nodes);
    }

    /// A number that changes when what its nodes show changed with no
    /// event of its own, as when the app's model moves a meter's level:
    /// the runtime asks after each turn and builds its nodes again when it
    /// differs from the last build's. It must be cheap, since it is asked
    /// every turn, and change no faster than a screen reader should hear
    /// (section 7, rule 7). By default 0, so only events and layouts
    /// rebuild them.
    fn access_version(&self, app: &A) -> u64 {
        let _ = app;
        0
    }

    /// Carries out a screen reader's `request` on its own node (`rest`
    /// empty) or on the part at `rest`, sending its messages through
    /// `out`: they run in this turn. The runtime asks only while the
    /// element takes input and the node offers the action and is not off.
    /// Returns false when it does not carry it out, and the request goes
    /// on to [`App::accessibility_event`]. Focus never comes here: it
    /// reaches the app, whose focus effect reaches [`Custom::focus_inner`].
    fn access_action(
        &mut self,
        app: &A,
        rest: &[IdKey],
        request: &AccessRequest,
        out: &mut Out<'_, A::Message>,
    ) -> bool {
        let _ = (app, rest, request, out);
        false
    }
}

/// What a custom element hears.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum CustomEvent {
    /// A key, while the runtime's focus is on it or inside it, or while
    /// nothing has focus and it is the window's home element
    /// ([`App::home`]). It hears presses and releases.
    Key(KeyEvent),
    /// Tab (`back` false) or Shift+Tab (`back` true) moved the focus onto
    /// it, so it can start at its first or its last part.
    Entered {
        /// True for Shift+Tab.
        back: bool,
    },
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
    /// Its deadline came ([`Custom::next_deadline`]). It may send messages,
    /// which run in this turn, and must move its deadline on.
    Deadline,
    /// An animation frame, while it animates ([`Custom::animating`]). It
    /// may change its own state and ask for nothing else: a frame draws
    /// and runs no update, so it sends no message. A debug build panics on
    /// one. An element that needs a message from its animation asks for a
    /// deadline instead and sends it from the turn that deadline wakes. A
    /// release build runs what it sent on the next turn.
    Frame,
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

/// Where a custom element sends its messages from one call, and asks for
/// a frame.
pub struct Out<'a, M> {
    sent: &'a mut Vec<M>,
    redraw: &'a mut bool,
    nodes_changed: &'a mut bool,
}

impl<M> Out<'_, M> {
    /// Sends `message`, to run in this turn after the messages sent before
    /// it.
    pub fn send(&mut self, message: M) {
        self.sent.push(message);
    }

    /// Asks for a frame, because what it draws changed and no message
    /// says so: a hover that lights it, or a drag that moves its knob.
    /// The turn draws once, however many times it was asked (section 9,
    /// rule 4).
    pub fn redraw(&mut self) {
        *self.redraw = true;
    }

    /// Says what its nodes show changed, so the runtime builds them again
    /// after this turn ([`Custom::access`]). A frame's redraw does not, so
    /// an element that animates rebuilds nothing for a screen reader until
    /// it says so.
    pub fn nodes_changed(&mut self) {
        *self.nodes_changed = true;
    }
}

/// When the custom elements next need a tick: their soonest deadline, and
/// whether one animates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Schedule {
    /// The soonest deadline any element gives.
    pub(crate) deadline: Option<Instant>,
    /// True when an element animates.
    pub(crate) frames: bool,
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
    /// True when an element asked for a frame ([`Out::redraw`]) and no
    /// turn has taken it yet.
    redraw: bool,
    /// The schedule the last turn gave the driver, so a call outside a
    /// turn that changes it wakes the loop for a turn that gives it again.
    planned: Schedule,
    /// True when an element said its nodes changed ([`Out::nodes_changed`])
    /// and they were not built again yet.
    nodes_changed: bool,
    /// True when a build outside a turn changed the nodes, so the next
    /// turn tells the app ([`App::access_changed`]).
    nodes_reported: bool,
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
    /// The access version and the bounds of its last node build, so the
    /// next is skipped while neither moved. `None` before the first.
    built: Option<(u64, Rect)>,
    /// A deadline it gave that did not move past its tick, which the
    /// schedule leaves out until it gives a later one.
    stuck: Option<Instant>,
}

impl<A: App> Default for Customs<A> {
    fn default() -> Self {
        Self {
            elements: BTreeMap::new(),
            sent: Vec::new(),
            captured: None,
            hovered: None,
            redraw: false,
            planned: Schedule::default(),
            nodes_changed: false,
            nodes_reported: false,
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
                    Hosted {
                        kind,
                        bounds,
                        state,
                        stuck: None,
                        built: None,
                    }
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
        let mut out = Out {
            sent: &mut self.sent,
            redraw: &mut self.redraw,
            nodes_changed: &mut self.nodes_changed,
        };
        state.event(app, event, moment, &mut out)
    }

    /// The custom element `id` is inside, other than itself: the one whose
    /// part it names.
    pub(crate) fn owner_of(&self, id: &UiId) -> Option<&UiId> {
        self.elements
            .keys()
            .find(|element| id.is_under(element) && id.keys().len() > element.keys().len())
    }

    /// Asks the element `id` to focus its part at `rest`. False when it has
    /// no state or no such part.
    pub(crate) fn focus_inner(&mut self, app: &A, id: &UiId, rest: &[IdKey]) -> bool {
        self.elements
            .get_mut(id)
            .and_then(|hosted| hosted.state.as_deref_mut())
            .is_some_and(|state| state.focus_inner(app, rest))
    }

    /// The focused part's UI ID, when the runtime's focus is on the element
    /// `id`: the element's own, followed by the path it gives.
    pub(crate) fn focused_part(&self, app: &A, id: &UiId) -> UiId {
        let rest = self.state(id).and_then(|state| state.focused_inner(app));
        rest.into_iter().flatten().fold(id.clone(), UiId::with)
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

    /// True when an element asked for a frame since the last call, which
    /// forgets it.
    pub(crate) fn take_redraw(&mut self) -> bool {
        std::mem::take(&mut self.redraw)
    }

    /// True when an element asked for a frame that no turn has taken yet.
    pub(crate) fn redraw_asked(&self) -> bool {
        self.redraw
    }

    /// When the elements next need a tick, asked of each now.
    pub(crate) fn schedule(&self, app: &A) -> Schedule {
        let mut schedule = Schedule::default();
        for hosted in self.elements.values() {
            let Some(state) = hosted.state.as_deref() else {
                continue;
            };
            // A deadline that did not move past its tick is left out, so
            // the loop does not spin on it.
            if let Some(at) = state
                .next_deadline(app)
                .filter(|at| hosted.stuck.is_none_or(|stuck| *at > stuck))
            {
                schedule.deadline = Some(schedule.deadline.map_or(at, |soonest| soonest.min(at)));
            }
            schedule.frames |= state.animating(app);
        }
        schedule
    }

    /// Notes `schedule` as what the turn gave the driver.
    pub(crate) fn plan(&mut self, schedule: Schedule) {
        self.planned = schedule;
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
