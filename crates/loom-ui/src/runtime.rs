//! The loop's one turn: take an event, run the updates, carry out their
//! effects, and decide whether to draw.
//!
//! One call to [`Runtime::handle`] is one turn (ui-layer.md, section 2,
//! Behaviour):
//!
//! 1. The messages waiting from wakers and from earlier effects, and the
//!    app's deadline if it has come, make the turn's batch.
//! 2. The event is routed: an element event to the target, a window event
//!    to the runtime's window state and to [`App::window_event`].
//! 3. Each message in the batch gets one `update`, and that update's
//!    effects run right after it returns (rule 5). A message an effect
//!    produces waits for the next turn.
//! 4. After a turn that ran an update, the runtime builds the view and
//!    compares each panel's model with the last, reads the window spec, and
//!    asks the app for its next deadline.
//! 5. It says to draw when a panel changed, the target asks for it, an
//!    effect asked for it, or the event was a redraw request or a frame.
//!    Otherwise nothing is drawn (section 3, rule 6).
//! 6. It returns [`Next`]: whether to draw, and then whether to run again
//!    at once, send frames, wake at a time, or sleep.
//!
//! The runtime never reads the clock: the driver passes the moment in.
//!
//! # Layout
//!
//! A turn builds the elements of each panel whose model changed, and no
//! more. Laying them out waits until the driver asks, with
//! [`Runtime::lay_out`], before it draws or reads a place: the driver holds
//! the text that measures. [`Runtime::bounds`] and [`Runtime::hit`] then
//! read the composed window (ui-layer.md, section 5, rule 8).

use crate::app::{App, Context, Deadline, TabStep, WindowSpec};
use crate::draw::Panels;
use crate::driver::Driver;
use crate::driver::PasteAnswer;
use crate::effect::{Effect, FocusTarget, Selection};
use crate::event::{AccessEvent, DriverEvent, WindowEvent, WindowState};
use crate::field::{FieldAction, FieldEvent};
use crate::focus::{Focus, FocusStep};
use crate::hit::Hit;
use crate::pointer::Pointer;
use crate::routing::{Routed, Target};
use crate::view::{Layer, View};
use crate::waker::{Wake, Waker};
use crate::window::Window;
use accesskit::{Action as KitAction, ActionData, ScrollUnit};
use loom_ui_core::{
    Cursor, Event, Key, KeyEvent, Modifiers, Platform, Point, PointerEvent, Rect, Size, TextMeasure, UiId,
};
use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

/// What the driver should do after a turn.
///
/// # Drawing
///
/// When `draw` is true, the driver draws one frame before it next waits.
/// It draws at once when the turn was for its own
/// [`DriverEvent::RedrawRequested`] or [`DriverEvent::Frame`]. Otherwise it
/// may ask its window system for a frame, and when that frame comes it
/// draws it without sending `RedrawRequested` for it: `RedrawRequested` is
/// only for a frame the system wants that the runtime did not ask for.
/// Several turns that say `draw` before the frame is drawn make one frame.
/// The runtime returns this rather than calling the driver, so a shell
/// cannot turn its own redraw request into a loop of draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Next {
    /// Draw one frame before waiting.
    pub draw: bool,
    /// Messages wait: run another turn at once, with [`DriverEvent::Woken`].
    pub again: bool,
    /// Send a [`DriverEvent::Frame`] each display refresh.
    pub frames: bool,
    /// Wake at this moment with [`DriverEvent::DeadlineReached`].
    pub deadline: Option<Instant>,
}

impl Next {
    /// True when nothing is due after any draw: the driver may sleep until an
    /// event comes.
    pub fn sleeps(&self) -> bool {
        !self.again && !self.frames && self.deadline.is_none()
    }
}

/// What the runtime has done since it started. The tests read these, and
/// so will `--measure-idle` (ui-layer.md, section 3, Measuring it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counts {
    /// Calls to [`Runtime::handle`].
    pub turns: u64,
    /// Calls to `update`.
    pub updates: u64,
    /// Panels compared with their last model.
    pub panels_compared: u64,
    /// Panels whose model, layer, or presence changed.
    pub panels_changed: u64,
    /// Animation frames received.
    pub frames: u64,
    /// Turns that said to draw.
    pub redraws: u64,
    /// Panels laid out, by [`Runtime::lay_out`].
    pub panels_laid_out: u64,
    /// Elements in the panels laid out.
    pub elements_laid_out: u64,
    /// Custom elements' node builds ([`Custom::access`](crate::Custom::access)),
    /// not counting a debug build's check that a skipped build would have
    /// given the same nodes.
    pub custom_node_builds: u64,
    /// Keys dropped from rebuilt panels because an earlier element had the
    /// same UI ID (ui-layer.md, section 4, rule 7).
    pub duplicate_keys: u64,
    /// Panels painted, by [`Runtime::paint`]: each frame paints every
    /// panel of the layers it draws.
    pub panels_painted: u64,
    /// Elements in the panels painted.
    pub elements_painted: u64,
}

/// The runtime for one app and its routing target.
pub struct Runtime<A: App, T> {
    app: A,
    target: T,
    waker: Waker<A::Message>,
    queue: Receiver<A::Message>,
    pending: VecDeque<A::Message>,
    /// The focus move the last turn ended with, for the app to hear first
    /// in the next turn, unless that turn moves the focus again and
    /// reports the newer move instead: the app never hears an older focus
    /// after a newer one.
    pending_focus: Option<A::Message>,
    window: WindowState,
    view: Option<View>,
    /// The app changed outside a turn ([`Runtime::mark_view_stale`], for
    /// tests), so the next turn builds its view again even with no update.
    view_stale: bool,
    /// The text typed into a select's type-ahead (section 6, rule 12).
    typed: crate::select::Typed,
    /// The layouts of the panels that have elements.
    panels: Window,
    spec: Option<WindowSpec>,
    deadline: Option<Deadline<A::Message>>,
    after_draw: Vec<A::Message>,
    waiting_for_draw: Vec<A::Message>,
    exited: bool,
    counts: Counts,
    /// Which of the runtime's own elements has keyboard focus.
    focus: Focus,
    /// The focus each overlay opened over, to give back when it closes
    /// (section 6, rule 4): the input layer it was saved from, the element,
    /// and whether it showed a ring. Last opened last.
    saved_focus: Vec<(Layer, UiId, bool)>,
    /// What is hovered and pressed among the runtime's own elements.
    pointer: Pointer,
    /// The text field that had focus when the runtime last looked, so it
    /// can tell the app when focus leaves it ([`FieldEvent::Left`]).
    field_focus: Option<UiId>,
    /// The target's focus as last seen, to tell when one of its own
    /// elements takes the keyboard ([`Runtime::follow_target`]).
    target_seen: Option<UiId>,
    /// Where a driver answers a field's paste ([`crate::Paste`]), and
    /// those answers.
    paste_answers: (Sender<PasteAnswer>, Receiver<PasteAnswer>),
    /// Paste answers taken from the channel, put in on the next turn.
    pasted: VecDeque<PasteAnswer>,
    /// The driver's wake function, for a paste's answer.
    wake: Wake,
    /// The custom elements' state, by UI ID (ui-layer.md, section 9).
    customs: crate::custom::Customs<A>,
}

impl<A: App, T: Target<A::Message>> Runtime<A, T> {
    /// A runtime for `app`, routing to `target`, in a window that starts as
    /// `window`. `wake` is the driver's wake function, called whenever a
    /// waker sends a message.
    pub fn new(app: A, target: T, window: WindowState, wake: Wake) -> Self {
        let (sender, queue) = mpsc::channel();
        Self {
            app,
            target,
            waker: Waker::new(sender, wake.clone()),
            queue,
            pending: VecDeque::new(),
            pending_focus: None,
            window,
            view: None,
            view_stale: false,
            typed: crate::select::Typed::default(),
            panels: Window::default(),
            spec: None,
            deadline: None,
            after_draw: Vec::new(),
            waiting_for_draw: Vec::new(),
            exited: false,
            counts: Counts::default(),
            focus: Focus::default(),
            saved_focus: Vec::new(),
            pointer: Pointer::default(),
            field_focus: None,
            target_seen: None,
            paste_answers: mpsc::channel(),
            pasted: VecDeque::new(),
            wake,
            customs: crate::custom::Customs::default(),
        }
    }

    /// The input method for the runtime's own fields: on, with the caret's
    /// area in the window, while one of them has focus and takes input;
    /// `None` otherwise, which leaves it to the layers (ui-layer.md,
    /// section 8, rule 4). A driver applies this before what the layers
    /// report.
    pub fn ime(&self) -> Option<crate::Ime> {
        let id = self.focus.id().filter(|id| self.field_takes_input(id))?;
        let content = self.panels.content_of(id)?;
        let area = self.panels.fields.get(id)?.ime_area();
        Some(crate::Ime::On {
            area: Rect::new(
                Point::new(content.x + area.x, content.y + area.y),
                Size::new(area.width.max(1.0), area.height),
            ),
        })
    }

    /// A waker that sends messages into this runtime's queue.
    pub fn waker(&self) -> Waker<A::Message> {
        self.waker.clone()
    }

    /// The app, to read.
    pub fn app(&self) -> &A {
        &self.app
    }

    /// The app, to change, for a test driver that sets up state between
    /// turns. The queue, the deadline, and the messages waiting for a draw
    /// stay as they are. Only with the `test-driver` feature: a real driver
    /// changes the app only through `update` (section 2, rule 1). A driver
    /// that changed what the view shows calls [`Runtime::mark_view_stale`]
    /// too.
    #[cfg(feature = "test-driver")]
    pub fn app_mut(&mut self) -> &mut A {
        &mut self.app
    }

    /// Tells the runtime the app changed outside a turn, through
    /// [`Runtime::app_mut`], in a way the view shows, so the next turn
    /// builds the view again although it runs no update. Only with the
    /// `test-driver` feature.
    #[cfg(feature = "test-driver")]
    pub fn mark_view_stale(&mut self) {
        self.view_stale = true;
    }

    /// True while the view waits to be built again after
    /// [`Runtime::mark_view_stale`]. Only with the `test-driver` feature.
    #[cfg(feature = "test-driver")]
    pub fn view_is_stale(&self) -> bool {
        self.view_stale
    }

    /// The state of the custom element with UI ID `id`, as its own type
    /// `S`, as [`Context::custom`] reads it: for a driver that draws it,
    /// and for tests.
    pub fn custom<S: 'static>(&self, id: &UiId) -> Option<&S> {
        let state: &dyn std::any::Any = self.customs.state(id)?;
        state.downcast_ref()
    }

    /// Where the custom element with UI ID `id` was at the last layout, in
    /// the window, in logical pixels.
    pub fn custom_bounds(&self, id: &UiId) -> Option<Rect> {
        self.customs.bounds(id)
    }

    /// The pointer shape the runtime asks for (section 9, rule 1, "the
    /// cursor"): the shape of the custom element that holds the capture,
    /// wherever the pointer is; else that of the custom element under the
    /// pointer; else the text beam over one of the runtime's fields.
    /// `None` when it asks for none, as over a toolkit's part or a plain
    /// part of a panel: a driver then shows what the toolkit asked for, or
    /// the plain arrow. A driver reads it after each turn and frame.
    pub fn cursor(&self) -> Option<Cursor> {
        let ask = |id: &UiId| {
            let moment = crate::Moment {
                pointer: self.pointer.at,
                bounds: self.customs.bounds(id)?,
                now: self.pointer.now?,
                modifiers: self.pointer.modifiers,
            };
            Some(self.customs.cursor(&self.app, id, &moment).unwrap_or_default())
        };
        if let Some(id) = self.customs.captured() {
            return ask(id);
        }
        let (_, id) = self.pointer.hovered.as_ref()?;
        if self.customs.is_custom(id) {
            return ask(id);
        }
        self.panels.fields.has(id).then_some(Cursor::Text)
    }

    /// Turns off the panics on mistakes, as in a release build, so a test
    /// sees what a release build reports.
    #[cfg(test)]
    pub(crate) fn allow_mistakes(&mut self) {
        self.panels.allow_mistakes();
    }

    /// The custom elements' state, to change for a test.
    #[cfg(test)]
    pub(crate) fn customs_mut(&mut self) -> &mut crate::custom::Customs<A> {
        &mut self.customs
    }

    /// The routing target, to read.
    pub fn target(&self) -> &T {
        &self.target
    }

    /// The routing target, to change. A driver may need this to draw it.
    pub fn target_mut(&mut self) -> &mut T {
        &mut self.target
    }

    /// The app to read, the target to use, and the panels to lay out and
    /// paint, at once: a driver that draws the window needs all three.
    pub fn parts(&mut self) -> (&A, &mut T, Panels<'_, A>) {
        (
            &self.app,
            &mut self.target,
            Panels {
                window: &mut self.panels,
                counts: &mut self.counts,
                size: self.window.size,
                focus: &mut self.focus,
                pointer: &mut self.pointer,
                app: &self.app,
                waker: &self.waker,
                scale: self.window.scale,
                customs: &mut self.customs,
            },
        )
    }

    /// The panels alone, to lay out and paint.
    fn panels(&mut self) -> Panels<'_, A> {
        let (_, _, panels) = self.parts();
        panels
    }

    /// The window as the runtime knows it.
    pub fn window(&self) -> WindowState {
        self.window
    }

    /// Sets how long one frame of the window's display lasts, from the
    /// next update on. The driver calls it when it learns the display's
    /// rate: at start, and when the window moves to another display. It
    /// runs no turn, since a new rate changes only when the next frame is
    /// due.
    pub fn set_frame(&mut self, frame: Duration) {
        self.window.frame = frame;
    }

    /// What the runtime has done so far.
    pub fn counts(&self) -> Counts {
        self.counts
    }

    /// True once the app has asked to quit.
    pub fn exited(&self) -> bool {
        self.exited
    }

    /// True while a message waits for a frame to be shown
    /// ([`crate::Effect::AfterDraw`]). A driver that cannot tell when a
    /// frame is on screen reads this to know when to look for one.
    pub fn waits_for_draw(&self) -> bool {
        !self.after_draw.is_empty() || !self.waiting_for_draw.is_empty()
    }

    /// The view built after the last turn that ran an update.
    pub fn view(&self) -> Option<&View> {
        self.view.as_ref()
    }

    /// Lays out what changed since the last call, in the window's size,
    /// with `text` measuring text: the panels rebuilt, those whose slot
    /// changed size, and those whose shrink slot's child changed size. A
    /// driver calls it before it draws, and before it reads bounds or hits.
    pub fn lay_out(&mut self, text: &mut dyn TextMeasure) {
        self.panels().lay_out(text);
        // A select that left the layout, scrolled out of view, or changed
        // its options closes its list at once, and one that moved has its
        // list built again where it is now, so no stale list is drawn.
        self.check_select();
        if std::mem::take(&mut self.panels.select_changed) {
            self.build_view();
            self.panels().lay_out(text);
        }
        // A list this layout closed is told to the app in the next turn.
        if self.panels.select_reported {
            self.waker().nudge();
        }
        self.release_if_owed();
        // An element the app was told is hovered may have left the layout.
        self.panels().report_unhover();
    }

    /// The runtime's own element with keyboard focus, if any. Elements a
    /// toolkit still draws keep theirs in the routing target.
    pub fn focused(&self) -> Option<&UiId> {
        self.focus.id()
    }

    /// The focused element when it is one of the runtime's text fields, as
    /// last laid out: what [`Context::field_has_focus`](crate::Context::field_has_focus)
    /// asks, for a driver outside a turn. It follows the focus, so a field
    /// whose panel closed, or that a layout took away, no longer counts.
    pub fn focused_field(&self) -> Option<&UiId> {
        self.focus.id().filter(|id| self.panels.fields.has(id))
    }

    /// Every focus stop of the runtime's elements, in focus order, as last
    /// laid out (section 6, rule 2). A router reads it to tell the
    /// runtime's elements from a toolkit's.
    pub fn focus_stops(&self) -> &[UiId] {
        self.panels.stops()
    }

    /// Where a step from `from` lands in the focus order: the next stop,
    /// or the previous one when `back` is set. With no `from`, or a `from`
    /// that is not a stop, focus comes in from outside: the first stop, or
    /// the last when `back` is set. [`FocusStep::PastEnd`] past either end.
    /// It moves nothing.
    ///
    /// A radio group is one stop on the way (section 6, rule 9).
    pub fn focus_step(&self, from: Option<&UiId>, back: bool) -> FocusStep {
        self.step_among(self.panels.tab_stops(from), from, back)
    }

    /// [`Runtime::focus_step`] among `stops` only, in focus order.
    fn step_among(&self, stops: Vec<&UiId>, from: Option<&UiId>, back: bool) -> FocusStep {
        let at = from.and_then(|from| stops.iter().position(|stop| *stop == from));
        let next = match (at, back) {
            (None, false) => stops.first(),
            (None, true) => stops.last(),
            (Some(at), false) => stops.get(at + 1),
            (Some(at), true) => at.checked_sub(1).and_then(|at| stops.get(at)),
        };
        next.map_or(FocusStep::PastEnd, |stop| FocusStep::To((*stop).clone()))
    }

    /// The focus stops Tab moves among now: those in the layers that take
    /// input (section 5, rule 10), so Tab never reaches a stop under an
    /// open overlay or question. A focus below them steps in from outside.
    fn tab_step(&self, input: Layer, back: bool) -> FocusStep {
        let stops = self
            .panels
            .tab_stops(self.focus.id())
            .into_iter()
            .filter(|stop| self.panels.layer_of(stop).is_some_and(|layer| layer >= input))
            .collect();
        self.step_among(stops, self.focus.id(), back)
    }

    /// Focuses what a press at `point` reaches, for a router to call when
    /// the runtime's layer takes the press: the focus stop under it, or
    /// around what is under it. Focus from a press shows no ring (section
    /// 6, rule 3), and the routing target lets go of its focus, so one
    /// element has it (rule 1). `None` when no stop is there: the runtime's
    /// focus clears, and the target's is not touched.
    ///
    /// # Panics
    ///
    /// As [`Runtime::bounds`].
    pub fn focus_at(&mut self, point: Point) -> Option<UiId> {
        self.check_laid_out();
        let mut panels = self.panels();
        panels.focus_at(point);
        panels.report_focus();
        self.release_if_owed();
        self.focus.id().cloned()
    }

    /// Lets go of the runtime's focus, as [`Effects::unfocus`](crate::Effects::unfocus)
    /// does, for a driver or a test harness outside a turn. The app hears
    /// of it as any focus move, through [`App::focus_moved`](crate::App::focus_moved)
    /// on the next turn, so its copy of the focus stays in step. For tests:
    /// a real driver's app lets go of the focus through its effects.
    #[cfg(any(test, feature = "testing"))]
    pub fn unfocus(&mut self) {
        if self.focus_to(None) {
            self.panels().report_focus();
        }
    }

    /// The highlighted option of a select's open list, or `None` while no
    /// list is open, for tests.
    #[cfg(test)]
    pub(crate) fn select_highlight(&self) -> Option<usize> {
        self.panels.select.as_ref().map(|open| open.highlight)
    }

    /// How far the runtime's scroll container with UI ID `id` is scrolled,
    /// and the most it can be, in logical pixels, as last placed (section
    /// 5, rule 12). `None` for an ID that is no such container.
    pub fn scroll_offset(&self, id: &UiId) -> Option<(f32, f32)> {
        self.panels.scroll_of(id)
    }

    /// Where the first of the runtime's text elements that reads `text` is
    /// in the window, as last laid out.
    pub fn text_bounds(&self, text: &str) -> Option<Rect> {
        self.panels.text_bounds(text)
    }

    /// The nodes of the runtime's own elements, as last laid out, with IDs
    /// from `ids`, as [`Context::access_nodes`](crate::Context::access_nodes)
    /// gives them inside a turn.
    pub fn access_nodes(&self, ids: &mut loom_ui_core::IdTable) -> crate::AccessNodes {
        let focused = self.focus.id().map(|id| {
            if self.customs.is_custom(id) {
                self.customs.focused_part(&self.app, id)
            } else {
                id.clone()
            }
        });
        self.panels.access_nodes(ids, self.window.scale, focused.as_ref())
    }

    /// The nodes of custom elements that break section 7, rule 2, as a
    /// release build reports them: each node's UI ID and its problem. A
    /// debug build panics on the first instead.
    pub fn custom_access_problems(&self) -> &[(UiId, crate::access::Problem)] {
        &self.panels.custom_problems
    }

    /// The hover text showing, as last laid out: the element it belongs to
    /// and where its box is, in logical pixels (section 5, rule 11).
    pub fn hover_text(&self) -> Option<(&UiId, Rect)> {
        self.panels.hint().map(|hint| (&hint.source, hint.rect))
    }

    /// The runtime's element under the pointer, if one of its layers holds
    /// the hover: what hover text reads (section 5, rule 10).
    pub fn hovered(&self) -> Option<&UiId> {
        self.pointer.hovered.as_ref().map(|(_, id)| id)
    }

    /// The element with focus and its bounds, in logical pixels, when its
    /// ring shows: the keyboard put the focus there, and the element is in
    /// a layer that takes input. `None` after a press, with no focus, or
    /// under an open question or panel (section 6, rule 3). What paints the
    /// ring reads the same answer.
    pub fn focus_ring(&self) -> Option<(UiId, Rect)> {
        shown_ring(&self.panels, &self.app, &self.focus)
    }

    /// Tells the routing target to let go of its focus, because one of the
    /// runtime's elements takes it: one element has focus across the window
    /// (section 6, rule 1). Every path that gives the runtime's elements the
    /// focus goes through here: a press, a focus effect, and a focus that
    /// waited for its layout. A target with no focus is told nothing.
    fn release_target(&mut self) {
        if self.target.focused().is_some() {
            self.target.release_focus();
        }
    }

    /// Carries out a release a layout owes the target ([`Focus::settle`]).
    /// A layout through the driver's own parts cannot reach the target, so
    /// it is carried out here: at the end of [`Runtime::lay_out`], and at
    /// the start of each turn, before the turn routes anything.
    fn release_if_owed(&mut self) {
        if std::mem::take(&mut self.focus.release_owed) {
            self.release_target();
        }
    }

    /// Lets go of the runtime's focus when one of the target's own elements
    /// took the keyboard since the last turn, as a click on a toolkit's
    /// field or the toolkit's own Tab does: one element has focus across
    /// the window (section 6, rule 1). The target's focus only changing to
    /// nothing leaves the runtime's as it is. Returns true when the focus
    /// changed.
    fn follow_target(&mut self) -> bool {
        let now = self.target.focused();
        if now == self.target_seen {
            return false;
        }
        self.target_seen = now;
        if self.target_seen.is_none() || self.focus.id().is_none() {
            return false;
        }
        self.focus.wanted = None;
        self.focus.set(None);
        self.focus.ring = false;
        true
    }

    /// Saves and gives back the runtime's focus around an overlay (section
    /// 6, rule 4), from `before`, the input layer and the focus the turn
    /// started with. When the input layer rose above the element that had
    /// the focus, that element is saved. When the layer dropped, every
    /// overlay above it is gone: the focus saved from the layer it is now
    /// takes the focus again, with the ring it had, while the focus is
    /// still on a closing overlay, on an element that went, or nowhere. A
    /// focus an effect of this turn put on an element below is kept. An element that has gone since is moved to its panel's
    /// first stop by the next layout (`Focus::settle`). Returns true when
    /// the focus changed.
    fn keep_focus_across_overlays(&mut self, before: (Layer, Option<UiId>, bool)) -> bool {
        let (was, focus, ring) = before;
        let now = self.app.input().layer;
        if now > was {
            if let Some(id) = focus
                && self.panels.layer_of(&id).is_some_and(|layer| layer < now)
            {
                self.saved_focus.push((was, id, ring));
            }
            return false;
        }
        if now >= was {
            return false;
        }
        let mut back = None;
        while let Some((layer, ..)) = self.saved_focus.last()
            && *layer >= now
        {
            let saved = self.saved_focus.pop().expect("there is a last");
            if saved.0 == now {
                back = Some(saved);
            }
        }
        let Some((_, id, ring)) = back else {
            return false;
        };
        // The layers that closed are the overlays above the input layer
        // now: the canvas and the panels stay open under every overlay.
        let closed_above = now.max(Layer::Panels);
        // The focus is on the closing overlay, on an element that went,
        // or nowhere. Anywhere else, an effect of this turn put it there.
        let on_closing = self
            .focus
            .id()
            .is_none_or(|focused| self.panels.layer_of(focused).is_none_or(|layer| layer > closed_above));
        if !on_closing || !self.panels.stops().contains(&id) && self.panels.panel_of(&id).is_none() {
            return false;
        }
        self.release_target();
        let changed = self.focus.id() != Some(&id);
        self.focus.wanted = None;
        self.focus.set(Some(id));
        self.focus.ring = ring;
        changed
    }

    /// After a focus effect: each one may move focus off a field, even one
    /// that a later effect of the same update moves back, so the field
    /// leaves at each, and the app hears of it on the next turn.
    fn field_left_after_focus(&mut self, now: Instant) {
        let mut later = Vec::new();
        self.field_left(&mut later, now);
        self.pending.extend(later);
    }

    /// Moves the runtime's focus to `to`, from the keyboard or an effect,
    /// so a ring shows, and brings it into view (section 5, rule 12).
    /// Returns true when it changed.
    fn focus_to(&mut self, to: Option<UiId>) -> bool {
        self.focus.wanted = None;
        if to.is_some() {
            self.panels.reveal.clone_from(&to);
        }
        let changed = self.focus.id() != to.as_ref() || (to.is_some() && !self.focus.ring);
        self.focus.set(to);
        self.focus.ring = self.focus.id().is_some();
        changed
    }

    /// Carries out a focus effect. One of the runtime's own stops takes the
    /// focus from the target (section 6, rule 1). A step goes through the
    /// runtime's order while one of its elements has focus, and past either
    /// end leaves it for the target. Everything else is the target's, as
    /// before. Returns true when the runtime's focus changed.
    fn focus_effect(&mut self, target: &FocusTarget, select: Selection) -> bool {
        // A part inside a custom element is the element's to focus: the
        // runtime's focus goes to the element when it found the part, and
        // the effect is dropped when it did not (section 9, rule 6).
        if let FocusTarget::Element(id) = target
            && let Some(found) = self.panels().focus_inner(id)
        {
            if !found {
                return false;
            }
            let owner = self.customs.owner_of(id).cloned();
            self.release_target();
            self.focus_to(owner);
            return true;
        }
        match target {
            FocusTarget::Element(id) if self.panels.stops().contains(id) => {
                self.release_target();
                self.focus_to(Some(id.clone()))
            }
            FocusTarget::Next | FocusTarget::Previous if self.focus.id().is_some() => {
                match self.focus_step(self.focus.id(), *target == FocusTarget::Previous) {
                    FocusStep::To(stop) => self.focus_to(Some(stop)),
                    FocusStep::PastEnd => {
                        self.target.focus(target, select);
                        self.focus_to(None)
                    }
                }
            }
            FocusTarget::Element(id) if self.panels.panel_of(id).is_some() => {
                // In one of the runtime's panels but not a stop yet: the
                // focus stays where it is, and the next layout moves it if a
                // stop has the ID by then (`Focus::settle`).
                self.focus.wanted = Some(id.clone());
                false
            }
            FocusTarget::Element(id) => {
                // The target's, as before. It still waits, in case a panel
                // opened in this turn has it.
                let changed = self.focus_to(None);
                self.target.focus(target, select);
                self.focus.wanted = Some(id.clone());
                changed
            }
            FocusTarget::Next | FocusTarget::Previous | FocusTarget::Nothing => {
                self.target.focus(target, select);
                self.focus_to(None)
            }
        }
    }

    /// Tab and Shift+Tab while one of the runtime's elements has focus:
    /// one step through its order, where the app's [`App::tab_from`]
    /// sends it: the next stop, out to the target with the key going on,
    /// as past either end by default, or out with a message for the next
    /// turn and the key taken (section 6, rule 3). With nothing
    /// focused in either toolkit and the canvas not focused, Tab enters
    /// the first stop and Shift+Tab the last, unless the input layer is
    /// above the panels (section 6, rule 2). Tab with Ctrl held is the
    /// app's region shortcut, never a step (rule 7). Returns whether the
    /// runtime took the key, and whether its focus changed, which needs a
    /// redraw even when the key goes on.
    fn tab(&mut self, event: &Event) -> (bool, bool) {
        let before = self.focus.id().cloned();
        let result = self.tab_step_from(event);
        // Tab or Shift+Tab that lands on a custom element tells it which
        // way it came in (section 9, rule 2).
        if let Event::Key(KeyEvent::Pressed { modifiers, .. }) = event
            && self.focus.id() != before.as_ref()
        {
            self.panels().tell_entered(modifiers.shift);
        }
        result
    }

    /// [`Runtime::tab`], before it tells a custom element it was entered.
    fn tab_step_from(&mut self, event: &Event) -> (bool, bool) {
        let Event::Key(KeyEvent::Pressed {
            key: Key::Tab,
            modifiers,
            ..
        }) = event
        else {
            return (false, false);
        };
        if modifiers.control {
            return (false, false);
        }
        let input = self.app.input();
        if self.focus.id().is_none() {
            // Forward from nothing goes to the half that reads first: the
            // target's, when it says so.
            let ours_first = !input.target_first || modifiers.shift;
            let enters =
                input.layer <= Layer::Panels && !input.canvas_focused && self.target.focused().is_none() && ours_first;
            return match self.tab_step(input.layer, modifiers.shift) {
                FocusStep::To(stop) if enters => (true, self.focus_to(Some(stop))),
                _ => (false, false),
            };
        }
        // A focus in a layer that takes no input now is left where it is
        // when Tab finds no stop above it: the key goes on to the target.
        let below = self
            .focus
            .id()
            .and_then(|id| self.panels.layer_of(id))
            .is_some_and(|layer| layer < input.layer);
        let next = match self.tab_step(input.layer, modifiers.shift) {
            FocusStep::To(stop) => Some(stop),
            FocusStep::PastEnd if below => return (false, false),
            FocusStep::PastEnd => None,
        };
        let from = self.focus.id().cloned().expect("a focus, checked above");
        match self.app.tab_from(&from, next.as_ref(), modifiers.shift) {
            TabStep::To(stop) => (true, self.focus_to(Some(stop))),
            TabStep::Leave => (false, self.focus_to(None)),
            TabStep::Send(message) => {
                self.waker().send(message);
                (true, self.focus_to(None))
            }
        }
    }

    /// A key while one of the runtime's text fields has focus: the field
    /// gets it first (section 6, rule 6), after Tab has had its turn. Copy
    /// and cut write the clipboard, and an edit or Enter becomes the app's
    /// message, in this turn's batch. Returns whether the field used the
    /// key, so it goes no further, and whether it needs a redraw.
    ///
    /// A key the field used does not reach the routing target. When the
    /// runtime has a panel step (rule 6, step 4), it goes between the field
    /// and the target, and hears every key, with word of what the field
    /// used, so a panel that moves by Up and Down still hears them.
    fn field_key(
        &mut self,
        event: &Event,
        now: Instant,
        driver: &mut impl Driver<A::Message>,
        batch: &mut Vec<A::Message>,
    ) -> (bool, bool) {
        if !matches!(event, Event::Key(_) | Event::Ime(_)) {
            return (false, false);
        }
        // A field in a layer below the one taking input, such as a panel
        // under a question, edits nothing: the key, or the input method's
        // event, goes on.
        let Some(id) = self.focus.id().cloned().filter(|id| self.field_takes_input(id)) else {
            return (false, false);
        };
        let used = match event {
            Event::Key(key) => self.panels.fields.key(&id, key, now, Platform::current()),
            Event::Ime(ime) => self.panels.fields.ime(&id, ime, now),
            _ => return (false, false),
        };
        if let Some(text) = used.copy {
            driver.clipboard_write(text);
        }
        // The field's own paste: the driver reads the clipboard, and the
        // text goes in on a later turn (section 8, rule 2).
        if used.paste {
            let answers = self.paste_answers.0.clone();
            driver.clipboard_paste(crate::Paste::new(
                id.clone(),
                self.focus.moves,
                answers,
                self.wake.clone(),
            ));
        }
        let text = || {
            self.panels
                .fields
                .get(&id)
                .map(crate::field::FieldState::committed)
                .unwrap_or_default()
        };
        if used.edited {
            batch.extend(self.app.field_event(&id, &FieldEvent::Edited(text())));
        }
        if used.committed {
            batch.extend(self.app.field_event(&id, &FieldEvent::Committed(text())));
        }
        // Home or End in a multi-line field on a Mac shows its start or end,
        // and the caret stays; any other edit or caret move shows the caret.
        // Either waits for a current layout, and the last asked for wins.
        // Home and End move nothing, so they ask for a redraw, whose layout
        // then scrolls.
        if let Some(edge) = used.reveal {
            self.panels.caret_reveal = Some((id.clone(), crate::window::FieldShow::Edge(edge)));
        } else if used.edited || used.redraw {
            self.follow_caret(&id);
        }
        (used.taken, used.redraw || used.reveal.is_some())
    }

    /// Asks for the caret of the field `id` to be kept in view after an
    /// edit or a caret move in it, when it shows many lines: the containers
    /// around it follow at the next layout, which has the field's new
    /// height (section 8, rule 7). A single-line field scrolls inside
    /// itself instead.
    fn follow_caret(&mut self, id: &UiId) {
        let many = self
            .panels
            .fields
            .get(id)
            .is_some_and(|field| field.lines() == crate::field::Lines::Many);
        if many {
            self.panels.caret_reveal = Some((id.clone(), crate::window::FieldShow::Caret));
        }
    }

    /// Whether the input method is composing in the focused field, while
    /// that field takes input.
    fn composing(&self) -> bool {
        self.focus
            .id()
            .filter(|id| self.field_takes_input(id))
            .and_then(|id| self.panels.fields.get(id))
            .is_some_and(crate::field::FieldState::composing)
    }

    /// Whether the field with UI ID `id` takes input now: it is one of the
    /// runtime's fields, not turned off (section 7, rule 22), and not
    /// below [`App::input`]'s layer (section 5, rule 3). A key and a screen
    /// reader's text action both ask this, so the two follow one rule.
    fn field_takes_input(&self, id: &UiId) -> bool {
        // Its element must still be in its panel: a rebuild may have
        // removed it before the layout that drops its state.
        self.panels.fields.has(id)
            && self.panels.has_node(id)
            && !self
                .panels
                .access_of(id)
                .is_some_and(crate::access::Access::is_disabled)
            && self.in_input_layer(id)
    }

    /// Whether the element with UI ID `id` is in a layer that takes input
    /// now: at or above [`App::input`]'s layer (section 5, rule 3). A
    /// field's keys and actions and a keyboard click all ask this, so they
    /// follow one rule.
    fn in_input_layer(&self, id: &UiId) -> bool {
        in_input_layer(&self.panels, &self.app, id)
    }

    /// A screen reader's text action on one of the runtime's fields
    /// (section 7, rule 10): SetValue, ReplaceSelectedText, or
    /// SetTextSelection, on a field's node or one of its runs. The runtime
    /// carries it out, and an edit becomes the app's
    /// [`FieldEvent::Edited`], in `batch`. Returns whether what the field
    /// reports changed, or `None` for an event that is not such an action,
    /// which goes to the app. A request with the wrong data, or naming a
    /// place past the text, does nothing. It never moves focus: only Focus
    /// does (rule 9).
    /// Gives a screen reader's action on a custom element's node to that
    /// element (section 9, rule 3). `None` when it is not one, or the
    /// element leaves it, so it goes on; Focus always goes on, to the app.
    fn custom_action(&mut self, event: &AccessEvent) -> Option<bool> {
        let AccessEvent::Action(request) = event else {
            return None;
        };
        let request_of = crate::custom::AccessRequest::from_kit(request)?;
        let target = self.app.access_target(request.target_node)?;
        self.panels().custom_action(&target, &request_of)
    }

    fn field_action(&mut self, event: &AccessEvent, now: Instant, batch: &mut Vec<A::Message>) -> Option<bool> {
        let AccessEvent::Action(request) = event else {
            return None;
        };
        if !matches!(
            request.action,
            KitAction::SetValue | KitAction::ReplaceSelectedText | KitAction::SetTextSelection
        ) {
            return None;
        }
        let target = self.app.access_target(request.target_node)?;
        let field = self.panels.fields.owner_of(&target)?.clone();
        // A SetValue the app commits whole is the app's; the field's
        // selection and partial replacement stay the runtime's.
        if request.action == KitAction::SetValue && self.app.commits_set_value(&field) {
            return None;
        }
        // A field below the layer taking input, or turned off, takes no
        // screen reader action (section 5, rule 3; rule 22): the action is
        // used up and changes nothing, so nothing is reported.
        if !self.field_takes_input(&field) {
            return Some(false);
        }
        let place = |node: accesskit::NodeId, index: usize| {
            let id = self.app.access_target(node)?;
            Some((crate::field::run_index(&field, &id)?, index))
        };
        let action = match (request.action, &request.data) {
            (KitAction::SetValue, Some(ActionData::Value(text))) => Some(FieldAction::SetValue(text.to_string())),
            (KitAction::ReplaceSelectedText, Some(ActionData::Value(text))) => {
                Some(FieldAction::ReplaceSelection(text.to_string()))
            }
            (KitAction::SetTextSelection, Some(ActionData::SetTextSelection(selection))) => {
                place(selection.anchor.node, selection.anchor.character_index)
                    .zip(place(selection.focus.node, selection.focus.character_index))
                    .map(|(anchor, focus)| FieldAction::Select { anchor, focus })
            }
            _ => None,
        };
        let Some(action) = action else {
            return Some(false);
        };
        let used = self.panels.fields.act(&field, action, now);
        if used.changed {
            self.follow_caret(&field);
        }
        if used.edited {
            let text = self
                .panels
                .fields
                .get(&field)
                .map(crate::field::FieldState::committed)
                .unwrap_or_default();
            batch.extend(self.app.field_event(&field, &FieldEvent::Edited(text)));
        }
        Some(used.changed)
    }

    /// When focus has moved off the text field that had it: the field
    /// forgets its undo steps, and the app hears [`FieldEvent::Left`], in
    /// `batch`. The window losing the keys moves no focus, so it is not
    /// this (section 8, rule 3).
    fn field_left(&mut self, batch: &mut Vec<A::Message>, now: Instant) {
        if let Some(field) = self.field_focus.take() {
            if self.focus.id() == Some(&field) {
                self.field_focus = Some(field);
                return;
            }
            // A composition in progress stays as text, and the app hears
            // that edit before it hears focus leave.
            if self.panels.fields.left(&field, now) {
                let text = self
                    .panels
                    .fields
                    .get(&field)
                    .map(crate::field::FieldState::committed)
                    .unwrap_or_default();
                batch.extend(self.app.field_event(&field, &FieldEvent::Edited(text)));
            }
            batch.extend(self.app.field_event(&field, &FieldEvent::Left));
        }
        self.field_focus = self.focus.id().cloned().filter(|id| self.panels.fields.has(id));
    }

    /// A paste answer for the field `field`, as a driver gets one, for a
    /// test that answers it later.
    #[cfg(test)]
    pub(crate) fn test_paste(&self, field: UiId) -> crate::Paste {
        crate::Paste::new(field, self.focus.moves, self.paste_answers.0.clone(), self.wake.clone())
    }

    /// The text the field with UI ID `id` shows now, for tests.
    #[cfg(test)]
    pub(crate) fn field_text(&self, id: &UiId) -> Option<&str> {
        self.panels.fields.get(id).map(crate::field::FieldState::text)
    }

    /// What the keyboard can activate the element with UI ID `id` as, now
    /// ([`activatable`]).
    fn activatable(&self, id: &UiId) -> Option<Activation> {
        activatable(&self.panels, &self.app, id)
    }

    /// The element a held Space would click now ([`live_arm`]).
    fn live_arm(&self) -> Option<UiId> {
        live_arm(&self.panels, &self.app, &self.focus, &self.pointer)
    }

    /// Keyboard activation of the focused element (section 3, rule 4):
    /// Enter clicks a button or a link when it goes down, and Space clicks
    /// a button or a switch when it comes up on the element it went down
    /// on.
    /// Escape, any move of the focus, or a pointer press while Space is held
    /// cancels it, and so does the element no longer being clickable when
    /// Space comes up. Repeats click nothing. The click reaches the app
    /// through `App::clicked` on the next turn, as a pointer's does.
    /// Returns whether the key was taken, so the target never hears it, and
    /// whether anything that shows changed, so a repeat draws no frame.
    fn activate(&mut self, event: &Event) -> (bool, bool) {
        let Event::Key(key) = event else {
            return (false, false);
        };
        if let KeyEvent::Released { key: Key::Space } = key
            && self.pointer.space.is_some()
        {
            if let Some(armed) = self.live_arm() {
                self.click(&armed);
            }
            self.pointer.space = None;
            return (true, false);
        }
        if let KeyEvent::Pressed { key: Key::Escape, .. } = key {
            // Escape cancels a held Space and hides the hover text, both at
            // once, and goes no further when it did either (section 5,
            // rule 11). A Space that could no longer click (the focus moved,
            // or a layer rose above it) is dropped without taking Escape,
            // which then goes where it goes, such as to a question.
            let cancelled = self.live_arm();
            self.pointer.space = None;
            let input = self.app.input().layer;
            let shown = self
                .panels
                .hint()
                .map(|hint| hint.source.clone())
                .filter(|source| self.panels.layer_of(source).is_some_and(|layer| layer >= input));
            let hid = self
                .pointer
                .hint
                .hide(shown.as_ref(), self.pointer.hover_moves, self.focus.moves);
            // In a menu, Escape closes the menu at once, the one exception
            // (section 5, rule 11): it cancels the Space held on an item and
            // goes on, so one Escape closes the menu and picks nothing.
            let menu_item = cancelled.as_ref().is_some_and(|armed| {
                self.panels
                    .access_of(armed)
                    .is_some_and(|access| access.role() == crate::access::Role::MenuItem)
            });
            if menu_item {
                return (false, true);
            }
            if cancelled.is_some() || hid {
                return (true, hid);
            }
        }
        let Some(id) = self.focus.id().cloned() else {
            return (false, false);
        };
        let Some(activation) = self.activatable(&id) else {
            return (false, false);
        };
        let KeyEvent::Pressed {
            key, modifiers, repeat, ..
        } = key
        else {
            return (false, false);
        };
        let plain = !(modifiers.control || modifiers.alt || modifiers.meta);
        match key {
            Key::Enter if activation.on_enter() && plain => {
                if !repeat {
                    self.click(&id);
                }
                (true, false)
            }
            Key::Space if activation.on_space() && plain => {
                if !repeat {
                    self.pointer.space = Some((id, self.focus.moves));
                }
                (true, false)
            }
            _ => (false, false),
        }
    }

    /// A key pressed while the focus is on or inside an element that takes
    /// it first ([`Element::takes_first`](crate::Element::takes_first)):
    /// that element's app message, through [`App::panel_key`], before the
    /// focused element hears the key (section 6, rule 6, step 2b). Only in
    /// a layer that takes input now. Returns whether the app took the key.
    fn panel_key(&mut self, event: &Event, batch: &mut Vec<A::Message>) -> bool {
        let Event::Key(press @ KeyEvent::Pressed { key, .. }) = event else {
            return false;
        };
        // While the canvas has the keyboard, its keys are its own (section
        // 6, rule 1), as for every other key step of the runtime's.
        if self.app.input().canvas_focused {
            return false;
        }
        // Escape with a Space held on a button cancels that click first
        // (section 3, rule 4), so it is left to the activation step.
        if *key == Key::Escape && self.live_arm().is_some() {
            return false;
        }
        let Some(focused) = self.focus.id() else {
            return false;
        };
        if !self.in_input_layer(focused) {
            return false;
        }
        let Some(owner) = self.panels.takes_first(focused, *key) else {
            return false;
        };
        match self.app.panel_key(owner, press) {
            Some(message) => {
                batch.push(message);
                true
            }
            None => false,
        }
    }

    /// The arrows, Space, and Enter on a focused radio of a group (section
    /// 6, rule 9; the WAI-ARIA radio group pattern). Down and Right move to
    /// the next radio and choose it, Up and Left to the one before, both
    /// wrapping; Space chooses the focused radio if it is not chosen; Enter
    /// is taken and does nothing, so a dialog around the group does not act
    /// on it. Each choice reaches the app as [`App::clicked`] on the next
    /// turn. A key the radio takes shows the ring (rule 3). Returns whether
    /// the key was taken, and whether anything that shows changed.
    fn radio_key(&mut self, event: &Event) -> (bool, bool) {
        let Event::Key(KeyEvent::Pressed {
            key, modifiers, repeat, ..
        }) = event
        else {
            return (false, false);
        };
        let Some(id) = self.focus.id().cloned() else {
            return (false, false);
        };
        let Some(chosen) = self.panels.radio_chosen(&id) else {
            return (false, false);
        };
        if self.app.input().canvas_focused || !self.in_input_layer(&id) || !plain(*modifiers) {
            return (false, false);
        }
        let ring = !self.focus.ring;
        match key {
            Key::Down | Key::Right | Key::Up | Key::Left => {
                let back = matches!(key, Key::Up | Key::Left);
                let Some(next) = self.panels.radio_step(&id, back) else {
                    return (true, false);
                };
                // No other radio to move to, in a group of one or one
                // whose other radios are turned off (no stops, section 6,
                // rule 2): the arrows are not the radio's, and go on, as to
                // a scroll container around it, so they never choose it;
                // Space does (section 6, rule 9).
                if next == id {
                    return (false, false);
                }
                let moved = self.focus_to(Some(next.clone()));
                self.click(&next);
                (true, moved)
            }
            Key::Space => {
                if !chosen && !repeat {
                    self.click(&id);
                }
                self.focus.ring = true;
                (true, ring)
            }
            Key::Enter => {
                self.focus.ring = true;
                (true, ring)
            }
            _ => (false, false),
        }
    }

    /// The WAI-ARIA select-only combobox pattern's keys on a focused select
    /// (section 6, rule 12). Closed, the keys that open it open its list on
    /// the chosen, the first, or the last option, or on what type-ahead
    /// finds. Open, they move the highlight, choose, or close; Tab chooses
    /// and is not taken, so it moves the focus on after. Returns whether
    /// the key was taken, and whether anything that shows changed.
    fn select_key(&mut self, event: &Event, now: Instant) -> (bool, bool) {
        use crate::select::{Closed, OpenKey, cell_key, closed_key, moved, open_key};
        let Event::Key(KeyEvent::Pressed {
            key, modifiers, text, ..
        }) = event
        else {
            return (false, false);
        };
        let Some(id) = self.focus.id().cloned() else {
            return (false, false);
        };
        let Some((select, _, _)) = self.panels.select_of(&id) else {
            return (false, false);
        };
        if select.options.is_empty() || self.app.input().canvas_focused || !self.in_input_layer(&id) {
            return (false, false);
        }
        let ring = !self.focus.ring;
        let count = select.options.len();
        let open = self.panels.select.clone().filter(|open| open.id == id);
        let text = text.as_deref();
        match open {
            None => {
                let action = if select.in_cell {
                    cell_key(*key, *modifiers)
                } else {
                    closed_key(*key, *modifiers, text)
                };
                let Some(action) = action else {
                    return (false, false);
                };
                let at = match action {
                    Closed::OpenOnChosen => select.start().unwrap_or(0),
                    Closed::OpenOnFirst => 0,
                    Closed::OpenOnLast => count - 1,
                    Closed::Type => {
                        let from = select.start().unwrap_or(count - 1);
                        self.typed
                            .find(&id, text.unwrap_or_default(), now, &select.options, from)
                            .unwrap_or(from.min(count - 1))
                    }
                };
                self.panels().open_select(&id, select, at);
            }
            Some(open) => {
                let Some(action) = open_key(*key, *modifiers, text) else {
                    return (false, false);
                };
                match action {
                    OpenKey::Move(by) => self.panels().highlight_option(moved(open.highlight, by, count)),
                    OpenKey::First => self.panels().highlight_option(0),
                    OpenKey::Last => self.panels().highlight_option(count - 1),
                    OpenKey::Choose => self.panels().choose_highlighted(),
                    OpenKey::ChooseAndTab => {
                        self.panels().choose_highlighted();
                        return (false, true);
                    }
                    OpenKey::Close => self.panels().close_select(),
                    OpenKey::Type => {
                        if let Some(found) =
                            self.typed
                                .find(&id, text.unwrap_or_default(), now, &open.select.options, open.highlight)
                        {
                            self.panels().highlight_option(found);
                        }
                    }
                }
            }
        }
        self.focus.ring = true;
        (true, ring)
    }

    /// A screen reader's action on a select or its open list (section 7,
    /// rule 32): Expand focuses the select and opens its list on the chosen
    /// option, Collapse
    /// closes it with no choice, Increment and Decrement step the chosen
    /// option by one, and Click on an option chooses it. `None` for any
    /// other action or element, which goes on. Returns whether anything
    /// that shows changed.
    fn select_action(&mut self, event: &AccessEvent) -> Option<bool> {
        let AccessEvent::Action(request) = event else {
            return None;
        };
        let target = self.app.access_target(request.target_node)?;
        if let Some(index) = crate::select::option_index(&target) {
            if request.action != KitAction::Click {
                return None;
            }
            let open = self.panels.select.clone()?;
            if index >= open.select.options.len() {
                return Some(false);
            }
            self.panels().highlight_option(index);
            self.panels().choose_highlighted();
            return Some(true);
        }
        let (select, _, _) = self.panels.select_of(&target)?;
        if !matches!(
            request.action,
            KitAction::Expand | KitAction::Collapse | KitAction::Increment | KitAction::Decrement
        ) {
            return None;
        }
        if select.options.is_empty() || !self.in_input_layer(&target) {
            return Some(false);
        }
        let last = select.options.len() - 1;
        match request.action {
            KitAction::Expand => {
                // A screen reader's cursor may be on the select while the
                // keyboard focus is elsewhere: Expand focuses it first, as
                // its Focus action would, since the list stays open only
                // while the select has the focus.
                self.release_target();
                self.focus_to(Some(target.clone()));
                let at = select.start().unwrap_or(0);
                self.panels().open_select(&target, select, at);
            }
            KitAction::Collapse => self.panels().close_select(),
            // A step goes on from the step's start, or the chosen option,
            // and a step that cannot move, at an end, chooses nothing.
            KitAction::Increment => {
                let to = select.start().map_or(0, |at| (at + 1).min(last));
                if select.start() != Some(to) {
                    self.panels().choose_option(&target, &select, to);
                }
            }
            _ => {
                let to = select.start().map_or(0, |at| at.saturating_sub(1));
                if select.start() != Some(to) {
                    self.panels().choose_option(&target, &select, to);
                }
            }
        }
        Some(true)
    }

    /// Closes a select's open list, with no choice, when it must close on
    /// its own (section 5, rule 18): the select left the layout, has no
    /// options or other options, or is scrolled out of view; the focus is
    /// not on it; or a layer above it, such as an overlay or a question,
    /// takes the input. A list that stays open takes the app's current
    /// choice, and is built again when its select moved.
    fn check_select(&mut self) {
        let Some(open) = self.panels.select.clone() else {
            return;
        };
        let now = self.panels.select_of(&open.id);
        // Where things are is known only once the window is laid out again
        // after a rebuild; until then only what the select offers is read.
        let settled = self.panels.is_settled(self.window.size);
        let gone = match &now {
            None => true,
            Some((select, _, shows)) => select.options != open.select.options || (settled && !shows),
        };
        let input = self.app.input();
        let covered = self.panels.layer_of(&open.id).is_none_or(|layer| layer < input.layer);
        if gone || covered || input.canvas_focused || self.focus.id() != Some(&open.id) {
            self.panels().close_select();
            return;
        }
        // Still open: the list follows the app's choice, for its tick and
        // for what a choice compares with, and follows its select when the
        // select moves or the window changes height.
        let Some((select, bounds, _)) = now else {
            return;
        };
        let moved = settled
            && open
                .anchor
                .is_some_and(|anchor| anchor != (bounds, self.window.size.height));
        if let Some(kept) = self.panels.select.as_mut()
            && (kept.select.chosen != select.chosen || kept.select.from != select.from || moved)
        {
            kept.select.chosen = select.chosen;
            kept.select.from = select.from;
            self.panels.select_changed = true;
            self.panels.select_reported = true;
        }
    }

    /// The WAI-ARIA slider pattern's keys on a focused slider (section 6,
    /// rule 10): the arrows by its key step, or its fine step with Shift,
    /// Home and End to the ends, and Page Up and Page Down by its page
    /// step. Each reaches the app through [`App::slid`] on the next turn,
    /// in a group of keys that closes 500 ms after its last. A key it
    /// takes at an end moves nothing and is still taken. A key it does not
    /// take, such as Space, goes on to the container around it. Returns
    /// whether the key was taken, and whether the ring came on.
    fn slider_key(&mut self, event: &Event, now: Instant) -> (bool, bool) {
        let Event::Key(KeyEvent::Pressed { key, modifiers, .. }) = event else {
            return (false, false);
        };
        let Some(id) = self.focus.id().cloned() else {
            return (false, false);
        };
        let Some((slider, _)) = self.panels.slider_of(&id) else {
            return (false, false);
        };
        let off = self
            .panels
            .access_of(&id)
            .is_some_and(crate::access::Access::is_disabled);
        if off || self.app.input().canvas_focused || !self.in_input_layer(&id) {
            return (false, false);
        }
        let from = self.pointer.slide.key_base(&id, slider.position());
        let Some(to) = slider.after_key(from, *key, *modifiers) else {
            return (false, false);
        };
        let reports = self.pointer.slide.key(&id, from, to, slider.position(), now);
        self.panels().report_slid(reports);
        let ring = !self.focus.ring;
        self.focus.ring = true;
        (true, ring)
    }

    /// A screen reader's Increment or Decrement on a slider (section 7,
    /// rules 5 and 30): one key step, joining the group of its steps in
    /// quick turn, so a run of them is one Start, its Moves, and one End
    /// (`Sliding::step`). Taken
    /// even on a slider that is turned off or below the input layer, where
    /// it does nothing. `None` for any other action, or another element,
    /// which goes on. Returns whether anything that shows changed.
    fn slider_action(&mut self, event: &AccessEvent, now: Instant) -> Option<bool> {
        let AccessEvent::Action(request) = event else {
            return None;
        };
        let up = match request.action {
            KitAction::Increment => true,
            KitAction::Decrement => false,
            _ => return None,
        };
        let target = self.app.access_target(request.target_node)?;
        let (slider, _) = self.panels.slider_of(&target)?;
        let off = self
            .panels
            .access_of(&target)
            .is_some_and(crate::access::Access::is_disabled);
        if off || !self.in_input_layer(&target) {
            return Some(false);
        }
        // From where an open group of its steps last left it, which the
        // app may not have laid out yet.
        let from = self.pointer.slide.key_base(&target, slider.position());
        let reports = self
            .pointer
            .slide
            .step(&target, from, slider.after_step(from, up), slider.position(), now);
        self.panels().report_slid(reports);
        Some(false)
    }

    /// The scroll keys on a focused scroll container, which is a stop only
    /// when nothing inside is (section 5, rule 12; the browsers' keys, as
    /// the APG has no pattern): Up and Down one line, Page Up and Page Down
    /// a page less one line, Space and Shift+Space a page, and Home and End
    /// the top and the bottom. Returns whether the key was taken, and
    /// whether the content moved.
    fn scroll_key(&mut self, event: &Event) -> (bool, bool) {
        let Event::Key(KeyEvent::Pressed { key, modifiers, .. }) = event else {
            return (false, false);
        };
        let Some(id) = self.focus.id().cloned() else {
            return (false, false);
        };
        let Some(line) = self.panels.scroll_line(&id) else {
            return (false, false);
        };
        if self.app.input().canvas_focused || !self.in_input_layer(&id) {
            return (false, false);
        }
        let Some(change) = scroll_change(*key, *modifiers) else {
            return (false, false);
        };
        let moved = self
            .panels
            .scroll_container(&id, |at, page, most| change(at, line, page, most));
        (true, moved)
    }

    /// A scroll key the focused element did not use goes to the nearest
    /// scroll container around it, or to one that takes its keys from it
    /// (section 6, rule 6, step 3; section 5, rule 12). Returns whether a
    /// container moved, which takes the key.
    fn bubble_scroll_key(&mut self, event: &Event) -> bool {
        let Event::Key(KeyEvent::Pressed { key, modifiers, .. }) = event else {
            return false;
        };
        let Some(id) = self.focus.id().cloned() else {
            return false;
        };
        if self.app.input().canvas_focused || !self.in_input_layer(&id) {
            return false;
        }
        let Some(change) = scroll_change(*key, *modifiers) else {
            return false;
        };
        self.panels.scroll_by_key(&id, change)
    }

    /// A screen reader's scroll action (section 7, rule 28): ScrollUp,
    /// ScrollDown, or SetScrollOffset on one of the runtime's scroll
    /// containers, or ScrollIntoView on a part inside one, which scrolls
    /// as a focus move does. Returns whether a container moved, or `None`
    /// for an event that is not such an action, which goes to the app.
    fn scroll_action(&mut self, event: &AccessEvent) -> Option<bool> {
        let AccessEvent::Action(request) = event else {
            return None;
        };
        if !matches!(
            request.action,
            KitAction::ScrollUp | KitAction::ScrollDown | KitAction::SetScrollOffset | KitAction::ScrollIntoView
        ) {
            return None;
        }
        let target = self.app.access_target(request.target_node)?;
        if request.action == KitAction::ScrollIntoView {
            if !self.panels.in_container(&target) {
                return None;
            }
            return Some(self.panels.scroll_to_show(&target));
        }
        let line = self.panels.scroll_line(&target)?;
        let moved = match (request.action, &request.data) {
            (KitAction::SetScrollOffset, Some(ActionData::SetScrollOffset(point))) => {
                // In the node's own space, which is physical pixels.
                let scale = if self.window.scale > 0.0 {
                    self.window.scale
                } else {
                    1.0
                };
                let to = point.y as f32 / scale;
                self.panels.scroll_container(&target, |_, _, _| to)
            }
            (KitAction::SetScrollOffset, _) => false,
            (action, data) => {
                let down = action == KitAction::ScrollDown;
                let item = matches!(data, Some(ActionData::ScrollUnit(ScrollUnit::Item)));
                self.panels.scroll_container(&target, |at, page, _| {
                    let by = if item { line } else { page };
                    if down { at + by } else { at - by }
                })
            }
        };
        Some(moved)
    }

    /// A screen reader's ShowContextMenu on an element with a context menu
    /// (section 5, rule 19): the app's message for it, as opened by a key,
    /// for the next turn, as a right press sends. Nothing under a layer
    /// that takes the input above it. `None` for any other action or
    /// element, which goes on to the app's own handling.
    fn context_action(&mut self, event: &AccessEvent) -> Option<bool> {
        let AccessEvent::Action(request) = event else {
            return None;
        };
        if request.action != KitAction::ShowContextMenu {
            return None;
        }
        let target = self.app.access_target(request.target_node)?;
        if !self.panels.has_context_menu(&target) {
            return None;
        }
        // What a layer above covers stays inert (section 7, rule 24): the
        // action is used up and opens nothing, as a right press there
        // never reaches it.
        if !self.in_input_layer(&target) {
            return Some(false);
        }
        if let Some(message) = self.app.context_menu(&target, true) {
            self.waker().send(message);
        }
        Some(false)
    }

    /// Sends the app's message for a click on `id`, for the next turn.
    fn click(&self, id: &UiId) {
        if let Some(message) = self.app.clicked(id) {
            self.waker().send(message);
        }
    }

    /// Where the element with UI ID `id` is in the window, in logical
    /// pixels, as last laid out. A panel's own key gives the whole panel.
    ///
    /// # Panics
    ///
    /// In a debug build, if something changed since the last
    /// [`Runtime::lay_out`], so the answer would be out of date.
    pub fn bounds(&self, id: &UiId) -> Option<Rect> {
        self.check_laid_out();
        self.panels.bounds(id)
    }

    /// What is under `point` in the window, and in which layer, as last
    /// laid out: panels are tried in the reverse of draw order.
    ///
    /// # Panics
    ///
    /// As [`Runtime::bounds`].
    pub fn hit(&self, point: Point) -> Option<(Layer, Hit<'_>)> {
        self.check_laid_out();
        self.panels.hit(point)
    }

    /// Paints the panels of `layer` into `painter`, in draw order, with
    /// `style` for what they do not set (ui-layer.md, section 5, rule 9).
    ///
    /// Lay out first with the same typesetter the painter sets text with
    /// ([`paint::ListPainter`](crate::paint::ListPainter) over the
    /// [`Typesetter`](crate::paint::Typesetter) that
    /// [`Runtime::lay_out`] measured with), so what is measured is what is
    /// drawn. A driver calls it only for a frame it draws.
    ///
    /// # Panics
    ///
    /// As [`Runtime::bounds`].
    pub fn paint(
        &mut self,
        layer: Layer,
        painter: &mut dyn crate::paint::TextPainter,
        style: &crate::paint::PanelStyle,
    ) {
        self.panels().paint(layer, painter, style);
    }

    /// Panics in a debug build when a read would be out of date.
    fn check_laid_out(&self) {
        debug_assert!(
            self.panels.is_settled(self.window.size),
            "the window changed since it was last laid out: call Runtime::lay_out first"
        );
    }

    /// Runs one turn for `event` at `now`, with `driver` carrying out the
    /// effects that reach outside the app.
    pub fn handle(&mut self, event: DriverEvent, now: Instant, driver: &mut impl Driver<A::Message>) -> Next {
        if self.exited {
            return Next {
                draw: false,
                again: false,
                frames: false,
                deadline: None,
            };
        }
        // 1. The batch: messages already waiting, then the deadline.
        self.drain_queue();
        let mut batch: Vec<A::Message> = self.pending.drain(..).collect();
        if self.deadline.as_ref().is_some_and(|deadline| deadline.at <= now)
            && let Some(deadline) = self.deadline.take()
        {
            batch.push(deadline.message);
        }
        // Custom elements whose deadline came tick now; what they send runs
        // in this turn, after the batch (section 9, rule 4).
        self.pointer.now = Some(now);
        self.panels().tick_deadlines(now);
        self.turn(event, batch, now, driver)
    }

    /// Runs one turn for `message` alone, at `now`. Messages already in the
    /// queue, and a deadline that is due, wait for the next turn. For a
    /// driver that must hand a message over at an exact point: the bridge
    /// to Iced first runs what is queued, then hands over the window's word
    /// on focus by itself, so no other message sees the new focus before
    /// the app has heard of it.
    pub fn deliver(&mut self, message: A::Message, now: Instant, driver: &mut impl Driver<A::Message>) -> Next {
        if self.exited {
            return Next {
                draw: false,
                again: false,
                frames: false,
                deadline: None,
            };
        }
        // A focus report the last turn left waits too, for the next turn,
        // unless this one ends with a newer report, which replaces it.
        let waiting = self.pending_focus.take();
        let mut next = self.turn(DriverEvent::Woken, vec![message], now, driver);
        if self.pending_focus.is_none() && waiting.is_some() {
            self.pending_focus = waiting;
            next.again |= !self.exited;
        }
        next
    }

    /// One turn: routes `event`, runs `batch` and the messages routing
    /// adds, one update each, then works out what is next.
    fn turn(
        &mut self,
        event: DriverEvent,
        mut batch: Vec<A::Message>,
        now: Instant,
        driver: &mut impl Driver<A::Message>,
    ) -> Next {
        self.counts.turns += 1;
        self.pointer.now = Some(now);
        // A layout since the last turn may have taken a select's list away.
        self.check_select();
        // A group of keys on a slider closes 500 ms after its last key.
        let closed = self.pointer.slide.close_keys_if(now, self.focus.id());
        self.panels().report_slid(closed);
        self.release_if_owed();
        // The target's own element may have taken the keyboard since the
        // last turn; then a layout between turns, or that, may have moved
        // focus off a field, or changed what a field reports.
        // What shows as pressed is painted from the runtime's own state,
        // so a change of it needs a frame, though no panel is built again
        // (visual-system.md, Role tokens): an input event, or an update
        // that raises the input layer or turns the element off. So does
        // whether the focus ring shows, which a raised input layer hides
        // with no panel built again (section 6, rule 3).
        let pressed = self.panels().shown_pressed();
        let ringed = self.focus_ring().map(|(id, _)| id);
        // The input layer and the focus as the turn starts: an overlay that
        // opens in it moves the focus into itself in the same turn, so what
        // to give back when it closes is read now, once the runtime has
        // heard that the target took the focus since the last turn, so an
        // overlay never saves a focus the target already took.
        let mut redraw = self.follow_target();
        let before = (self.app.input().layer, self.focus.id().cloned(), self.focus.ring);
        self.field_left(&mut batch, now);
        // A scroll since the last turn, by the pointer between turns or by
        // a layout's reveal, changed what the tree reports too.
        // So did a custom element's nodes, built again at a layout.
        let mut reported =
            self.panels.fields.take_changed() | self.panels.take_scrolled() | self.customs.take_nodes_reported();
        // Paste answers that came since the last turn.
        let changed = self.take_pastes(now, &mut batch);
        redraw |= changed;
        reported |= changed;

        // 2. Route the event.
        match event {
            DriverEvent::Input { event, pointer } => {
                self.note_modifiers(&event);
                // While the input method composes in a focused field, every
                // key is the input method's, Tab and Space too: the field
                // takes them before Tab or a button can.
                let composing = matches!(event, Event::Key(_)) && self.composing();
                // A focused select's keys come first, so Tab from an open
                // list chooses before it moves the focus (section 6, rule 12).
                let (mut taken, changed) = if composing {
                    (false, false)
                } else {
                    self.select_key(&event, now)
                };
                redraw |= changed;
                // A focused custom element is step 3's focused element: the
                // keys an element around it takes first (step 2b) come
                // before it, and it comes before Tab, so Tab moves inside
                // it until it lets Tab go (section 9, rule 2).
                let custom = !composing && self.panels().focused_custom().is_some();
                if !taken && custom {
                    taken = self.panel_key(&event, &mut batch) || self.panels().key_to_focused(&event);
                }
                if !taken {
                    let (tabbed, moved) = if composing { (false, false) } else { self.tab(&event) };
                    taken = tabbed;
                    redraw |= moved;
                }
                if !taken && !composing && !custom {
                    taken = self.panel_key(&event, &mut batch);
                }
                if !taken && !composing {
                    let (activated, changed) = self.activate(&event);
                    taken = activated;
                    redraw |= changed;
                }
                if !taken && !composing {
                    let (used, changed) = self.radio_key(&event);
                    taken = used;
                    redraw |= changed;
                }
                if !taken && !composing {
                    let (used, changed) = self.slider_key(&event, now);
                    taken = used;
                    redraw |= changed;
                }
                if !taken && !composing {
                    let (used, changed) = self.scroll_key(&event);
                    taken = used;
                    redraw |= changed;
                }
                if taken {
                    self.target.kept(&event);
                } else {
                    let (used, changed) = self.field_key(&event, now, driver, &mut batch);
                    redraw |= changed;
                    reported |= changed;
                    if used {
                        // A key a field took is the field's alone, so a
                        // toolkit that heard it first drops it too.
                        self.target.kept(&event);
                    } else if self.bubble_scroll_key(&event) {
                        // A scroll key the focused element left moved a
                        // container around it, and goes no further.
                        redraw = true;
                        self.target.kept(&event);
                    } else if let Event::Key(key @ KeyEvent::Pressed { .. }) = &event
                        && self.app.input().layer == Layer::Question
                    {
                        // An open question has every key (section 6, rule
                        // 6, step 2): the app's question keys, or Panic,
                        // and none goes on to the target.
                        batch.extend(self.app.question_key(key));
                        self.target.kept(&event);
                    } else {
                        redraw |= self.route(&event, pointer, now, &mut batch);
                    }
                }
                // A focus the event moved is the first thing the app hears
                // this turn, so no update reads a focus it was not told of.
                if let Some(report) = self.panels().focus_report() {
                    self.pending_focus = None;
                    batch.insert(0, report);
                }
            }
            DriverEvent::Window(event) => self.window_event(event, now, &mut batch),
            DriverEvent::Accessibility(event) => match self.field_action(&event, now, &mut batch) {
                Some(changed) => {
                    redraw |= changed;
                    reported |= changed;
                }
                None => match self
                    .custom_action(&event)
                    .or_else(|| self.select_action(&event))
                    .or_else(|| self.slider_action(&event, now))
                    .or_else(|| self.context_action(&event))
                {
                    Some(changed) => redraw |= changed,
                    None => match self.scroll_action(&event) {
                        Some(moved) => redraw |= moved,
                        None => batch.extend(self.app.accessibility_event(&event)),
                    },
                },
            },
            DriverEvent::RedrawRequested => redraw = true,
            DriverEvent::Frame => {
                self.counts.frames += 1;
                redraw = true;
                // A frame runs no update: what an element sent from its
                // frame's tick waits for the next turn (section 9, rule 4).
                let sent = self.panels().tick_frames();
                self.pending.extend(sent);
            }
            DriverEvent::Presented => batch.append(&mut self.waiting_for_draw),
            DriverEvent::Woken | DriverEvent::DeadlineReached => {}
        }
        // The last turn's focus move, when this event did not move the
        // focus again, is the first thing the app hears.
        if let Some(older) = self.pending_focus.take() {
            batch.insert(0, older);
        }

        // Tab, or a press the target handed back, may have moved focus off
        // a field: the app hears of it in this turn.
        self.field_left(&mut batch, now);
        // Focus leaving a slider closes its group of keys.
        let closed = self.pointer.slide.close_keys_if(now, self.focus.id());
        self.panels().report_slid(closed);
        // Or a select its list.
        self.check_select();
        // A keyboard focus move shows its element now, when the layout is
        // current; otherwise the next layout does it.
        redraw |= self.panels().reveal();
        let scrolled = self.panels.take_scrolled();
        redraw |= scrolled;
        reported |= scrolled;

        // 3. One update per message, each followed by its own effects. A
        // focus effect that moves focus off a field is looked at as it
        // runs (`update`), so focus that leaves and comes back in one turn
        // still ends the field's undo.
        let mut updates = 0;
        redraw |= self.run_batch(batch, now, driver, &mut updates);
        // What custom elements sent runs in this turn too, after the batch,
        // before the rebuild (section 9, rule 1): from the event, and from
        // a cancel an update caused, by an effect or by raising the input
        // layer above a held capture, as an overlay that opens mid-drag
        // does. This ends: after one cancel no capture is left to cancel,
        // and an update cannot make an element send.
        loop {
            let layer = self.app.input().layer;
            self.panels().cancel_capture_below(layer);
            let sent = self.customs.take_sent();
            if sent.is_empty() || self.exited {
                break;
            }
            redraw |= self.run_batch(sent, now, driver, &mut updates);
        }
        // An element that asked for a frame gets one, once, whatever else
        // the turn did.
        redraw |= self.customs.take_redraw();
        // A custom element whose nodes changed builds them again, before
        // the app is asked for its tree (section 9, rule 3).
        reported |= self.panels().build_custom_nodes();
        // An overlay an update opened closes a select's list under it, and
        // a list that opened, closed, or moved its highlight changes what
        // the tree reports: its expanded state and active descendant.
        self.check_select();
        reported |= std::mem::take(&mut self.panels.select_reported);
        // What a field reports changed: one notice, asked for after the
        // turn's updates, so the app's answer is built from its model with
        // the field's messages in it.
        if reported
            && !self.exited
            && let Some(notice) = self.app.access_changed()
        {
            updates += 1;
            redraw |= self.update(notice, now, driver);
        }
        self.counts.updates += updates as u64;

        // 4. What changed.
        // Taken first, so any rebuild uses up a stale view. A select's list
        // that opened, closed, or moved its highlight is built again too.
        let listed = std::mem::take(&mut self.panels.select_changed);
        redraw |= listed;
        let stale = std::mem::take(&mut self.view_stale) || listed;
        if updates > 0 || self.view.is_none() || stale {
            redraw |= self.rebuild(driver);
            // A layer the input layer rose above holds no hover or press.
            redraw |= self.pointer.below(self.app.input().layer);
        }

        redraw |= self.target.needs_redraw(updates);
        redraw |= self.panels().shown_pressed() != pressed;
        redraw |= self.keep_focus_across_overlays(before);
        redraw |= self.focus_ring().map(|(id, _)| id) != ringed;

        // 5. Draw only when something visible changed.
        let draw = redraw && !self.exited;
        if draw {
            self.counts.redraws += 1;
            self.waiting_for_draw.append(&mut self.after_draw);
        }

        // 6. What comes next: a moved focus is reported for the next turn,
        // ahead of any message that waited in the queue, so none of them
        // runs while the app's copy of the focus is behind the runtime's:
        // a dialog's answer sent as its question closed and gave a field
        // its focus back, for one (plan 3.2, row 4d).
        self.panels().report_unhover();
        // A custom element a rebuild moved from under the pointer hears it
        // left; what it sends runs on the next turn.
        self.panels().report_custom_left();
        let report = self.panels().focus_report();
        self.drain_queue();
        let sent = self.customs.take_sent();
        self.pending.extend(sent);
        if report.is_some() {
            self.pending_focus = report;
        }
        // The custom elements' deadlines and frames, asked every turn: an
        // element's state changes with no update. A frame it asked for
        // after the draw was decided, by hearing the pointer left, gets
        // the next turn.
        let custom = self.customs.schedule(&self.app);
        self.customs.plan(custom);
        let redraw_asked = self.customs.redraw_asked();
        Next {
            draw,
            again: !self.exited
                && (!self.pending.is_empty()
                    || self.pending_focus.is_some()
                    || !self.pasted.is_empty()
                    || redraw_asked),
            frames: !self.exited && (self.app.animating() || custom.frames),
            deadline: if self.exited {
                None
            } else {
                // The app's deadline, a slider's group of keys closing, or
                // a custom element's deadline, whichever is soonest.
                let app = self.deadline.as_ref().map(|deadline| deadline.at);
                [app, self.pointer.slide.deadline(), custom.deadline]
                    .into_iter()
                    .flatten()
                    .min()
            },
        }
    }

    /// Runs one update for each message of `batch`, in order, counting
    /// them in `updates`. Returns true when one asked for a draw.
    fn run_batch(
        &mut self,
        batch: Vec<A::Message>,
        now: Instant,
        driver: &mut impl Driver<A::Message>,
        updates: &mut usize,
    ) -> bool {
        let mut redraw = false;
        for message in batch {
            if self.exited {
                break;
            }
            *updates += 1;
            redraw |= self.update(message, now, driver);
            // A focus an update's effects moved is the next thing the app
            // hears, before the turn's other messages, as a focus an event
            // moved is the first (`App::focus_moved`): the app's copy of the
            // focus is then never behind while it handles one.
            if !self.exited
                && let Some(report) = self.panels().focus_report()
            {
                *updates += 1;
                redraw |= self.update(report, now, driver);
            }
        }
        redraw
    }

    /// Routes one input event that Tab did not take (section 5, rule 10).
    /// A pointer event goes to the runtime's layers above the target, then
    /// to the target, then to the layers below it; whatever else, to the
    /// target. Returns true when the hover moved.
    fn route(&mut self, event: &Event, pointer: Option<Point>, now: Instant, batch: &mut Vec<A::Message>) -> bool {
        // A press of the pointer cancels a held Space, wherever it is, even
        // with no position to route by (section 3, rule 4).
        if let Event::Pointer(PointerEvent::Pressed(_)) = event {
            self.pointer.space = None;
            // A press closes a slider's group of keys, wherever it is
            // (section 5, rule 16).
            let closed = self.pointer.slide.close_keys();
            self.panels().report_slid(closed);
        }
        // A custom element that holds the capture hears the event first,
        // outside the window too, and the pointer leaving the window does
        // not end it (section 9, rule 1).
        if let Event::Pointer(pointer_event) = event
            && let Some(heard) = self.panels().offer_captured(pointer_event, pointer)
        {
            return heard.redraw;
        }
        // With no capture, a lost pointer has nothing to end: it reaches
        // no element and no target.
        if let Event::Pointer(PointerEvent::Lost) = event {
            return false;
        }
        let point = match (event, pointer) {
            (Event::Pointer(PointerEvent::LeftWindow), _) => {
                let redraw = self.pointer.clear();
                self.target.input(event, pointer, now, batch);
                return redraw;
            }
            (Event::Pointer(_), Some(point)) => point,
            _ => {
                let routed = self.target.input(event, pointer, now, batch);
                // Step 5: a key the target left goes to the window's home
                // element while nothing has focus (section 9, rule 2).
                if routed != Routed::Taken && matches!(event, Event::Key(_)) {
                    let target_focus = self.target.focused().is_some();
                    self.panels().key_to_home(event, target_focus);
                }
                return false;
            }
        };
        let (app, target, mut panels) = self.parts();
        let input = app.input();
        let above = panels.pointer_above(input, event, point);
        let mut redraw = above.redraw;
        if !above.taken {
            redraw |= match target.input(event, pointer, now, batch) {
                Routed::Taken => panels.pointer_taken(event).redraw,
                Routed::Passed => panels.pointer_below_in_turn(event, point).redraw,
                Routed::Later => false,
            };
        }
        // A custom element the pointer moved off hears it now, so what it
        // sends runs in this turn.
        self.panels().report_custom_left();
        self.release_if_owed();
        redraw
    }

    /// Keeps the held modifiers from a key event, for Shift during a
    /// slider's drag (section 5, rule 16).
    fn note_modifiers(&mut self, event: &Event) {
        // What the latest press came from, for a focus effect that follows
        // it (`Effects::focus_as_input`).
        match event {
            Event::Key(KeyEvent::Pressed { .. }) => self.focus.by_key = true,
            Event::Pointer(loom_ui_core::PointerEvent::Pressed(_)) => self.focus.by_key = false,
            _ => {}
        }
        match event {
            Event::Key(KeyEvent::ModifiersChanged(modifiers) | KeyEvent::Pressed { modifiers, .. }) => {
                self.pointer.modifiers = *modifiers;
            }
            Event::WindowUnfocused => self.pointer.modifiers = Modifiers::NONE,
            _ => {}
        }
    }

    /// Moves everything wakers sent into `pending`, in order, and the
    /// paste answers drivers gave into `pasted`.
    fn drain_queue(&mut self) {
        self.pending.extend(self.queue.try_iter());
        self.pasted.extend(self.paste_answers.1.try_iter());
    }

    /// Puts each paste answer in its field, as one undo step, when that
    /// field still has focus, focus has not moved at all since the paste
    /// was asked for (not even away and back, as a held Space counts it),
    /// and the field takes input; drops it otherwise, so a slow clipboard
    /// never types into another place or after the user moved on (section
    /// 8, rule 2). An
    /// edit reaches the app as `Edited`, in `batch`. A paste whose read was
    /// refused reaches the app as `PasteRefused` whatever focus did since,
    /// so a refusal never goes unheard. Returns whether a
    /// field changed what it shows.
    fn take_pastes(&mut self, now: Instant, batch: &mut Vec<A::Message>) -> bool {
        self.pasted.extend(self.paste_answers.1.try_iter());
        let mut changed = false;
        while let Some((field, moves, answer)) = self.pasted.pop_front() {
            let text = match answer {
                crate::ClipboardText::Text(text) => text,
                crate::ClipboardText::Empty => continue,
                crate::ClipboardText::Refused => {
                    batch.extend(self.app.field_event(&field, &FieldEvent::PasteRefused));
                    continue;
                }
            };
            if moves != self.focus.moves || self.focus.id() != Some(&field) || !self.field_takes_input(&field) {
                continue;
            }
            let used = self.panels.fields.paste(&field, &text, now);
            changed |= used.redraw;
            if used.redraw {
                self.follow_caret(&field);
            }
            if used.edited {
                let text = self
                    .panels
                    .fields
                    .get(&field)
                    .map(crate::field::FieldState::committed)
                    .unwrap_or_default();
                batch.extend(self.app.field_event(&field, &FieldEvent::Edited(text)));
            }
        }
        changed
    }

    /// Applies a window event to the window state, tells the target when
    /// the window loses focus, and adds the app's message for it.
    fn window_event(&mut self, event: WindowEvent, now: Instant, batch: &mut Vec<A::Message>) {
        match &event {
            WindowEvent::Resized(size) => self.window.size = *size,
            WindowEvent::ScaleChanged(scale) => self.window.scale = *scale,
            WindowEvent::Focused(focused) => {
                self.window.focused = *focused;
                if !focused {
                    self.target.input(&Event::WindowUnfocused, None, now, batch);
                    // A drag held when the window loses the keys is
                    // cancelled, as a plugin host's DAW may take them at
                    // any time (section 9, rule 1).
                    self.panels().cancel_capture();
                }
            }
            WindowEvent::Hidden(true) => {
                self.panels().cancel_capture();
            }
            WindowEvent::Hidden(false) | WindowEvent::CloseRequested | WindowEvent::FileDropped(_) => {}
        }
        batch.extend(self.app.window_event(&event));
    }

    /// Runs one update and its effects. Returns true when an effect asked
    /// for a draw.
    fn update(&mut self, message: A::Message, now: Instant, driver: &mut impl Driver<A::Message>) -> bool {
        let focused = self.panels().focused_part().or_else(|| self.target.focused());
        let cx = Context {
            now,
            waker: &self.waker,
            focused,
            window: self.window,
            panels: Some(&self.panels),
            customs: Some(&self.customs),
        };
        let effects = self.app.update(message, &cx);
        let mut redraw = false;
        for effect in effects {
            match effect {
                Effect::FocusAsInput(id) => {
                    let quiet = !self.focus.by_key;
                    redraw |= self.focus_effect(&FocusTarget::Element(id.clone()), Selection::Keep);
                    if quiet {
                        // A part inside a custom element puts the runtime's
                        // focus on the element that owns it.
                        let held = self.customs.owner_of(&id).cloned().unwrap_or_else(|| id.clone());
                        if self.focus.id() == Some(&held) {
                            self.focus.ring = false;
                        } else if self.focus.wanted.as_ref() == Some(&id) {
                            self.focus.wanted_quiet = Some(id);
                        }
                    }
                    self.field_left_after_focus(now);
                }
                Effect::Focus { target, select } => {
                    redraw |= self.focus_effect(&target, select);
                    // A field not laid out yet selects all when the layout
                    // gives it the focus it waits for.
                    if select == Selection::All
                        && let FocusTarget::Element(id) = &target
                        && self.focus.wanted.as_ref() == Some(id)
                    {
                        self.focus.wanted_select = Some(id.clone());
                    }
                    // One of the runtime's fields that took the focus
                    // selects all of its text when asked, as the target's do.
                    if select == Selection::All
                        && let FocusTarget::Element(id) = &target
                        && self.focus.id() == Some(id)
                        && self.panels.fields.has(id)
                    {
                        let used = self.panels.fields.act(id, crate::field::FieldAction::SelectAll, now);
                        redraw |= used.changed;
                        // Selecting all ends a composition, whose text it
                        // keeps: an edit the app hears, as from a key.
                        if used.edited {
                            let text = self
                                .panels
                                .fields
                                .get(id)
                                .map(crate::field::FieldState::committed)
                                .unwrap_or_default();
                            self.pending.extend(self.app.field_event(id, &FieldEvent::Edited(text)));
                        }
                    }
                    self.field_left_after_focus(now);
                }
                // The runtime's own element is its own to scroll, once its
                // layout is current (section 5, rule 12); a toolkit's is
                // the target's, as before.
                Effect::ScrollIntoView(id) if self.panels.panel_of(&id).is_some() => {
                    self.panels.reveal = Some(id);
                    redraw |= self.panels().reveal();
                }
                Effect::ScrollIntoView(id) => self.target.scroll_into_view(&id),
                Effect::ClipboardWrite(text) => driver.clipboard_write(text),
                Effect::ClipboardRead(reply) => driver.clipboard_read(reply, &self.waker),
                Effect::FileDialog { request, reply } => driver.file_dialog(request, reply, &self.waker),
                Effect::Ask { question, reply } => driver.ask(question, reply, &self.waker),
                Effect::Work(job) => driver.work(job, &self.waker),
                Effect::Capture(reply) => {
                    driver.capture(reply, &self.waker);
                    redraw = true;
                }
                Effect::AfterDraw(message) => {
                    self.after_draw.push(message);
                    redraw = true;
                }
                // After what wakers have sent so far, and before anything
                // they send later: `pending` is the runtime's own, so the
                // batch stays whole.
                Effect::NextTurn(messages) => {
                    self.drain_queue();
                    self.pending.extend(messages);
                }
                Effect::Redraw => redraw = true,
                Effect::ReleasePointer => {
                    self.panels().cancel_capture();
                }
                Effect::Accessibility { whole, changes } => driver.accessibility(whole, changes),
                Effect::Quit => {
                    self.exited = true;
                    driver.exit();
                    break;
                }
            }
        }
        redraw
    }

    /// Builds the view and compares it with the last, passes a changed
    /// window spec to the driver, and asks for the next deadline. Returns
    /// true when a panel changed.
    fn rebuild(&mut self, driver: &mut impl Driver<A::Message>) -> bool {
        let changed = self.build_view();
        let spec = self.app.window();
        if self.spec.as_ref() != Some(&spec) {
            driver.window(&spec);
            self.spec = Some(spec);
        }
        self.deadline = self.app.next_deadline();
        changed
    }

    /// Builds the view again from the app, with a select's open list as a
    /// panel of the runtime's in the Overlays layer (section 5, rule 18),
    /// and hands it to the window. Returns true when anything drawn may
    /// differ.
    fn build_view(&mut self) -> bool {
        let mut view = View::new();
        self.app.view(&mut view);
        if let Some(mut open) = self.panels.select.clone()
            && let Some(bounds) = self.panels.bounds(&open.id)
        {
            let anchor = Some((bounds, self.window.size.height));
            if let Some(kept) = self.panels.select.as_mut() {
                kept.anchor = anchor;
            }
            open.anchor = anchor;
            let model = crate::select::ListModel {
                open,
                bounds,
                window: self.window.size.height,
            };
            // At least as high as its select's layer, so a select in a
            // question has its list where the question's input goes.
            let layer = self
                .panels
                .layer_of(&model.open.id)
                .map_or(Layer::Overlays, |layer| layer.max(Layer::Overlays));
            view.panel_with(
                UiId::panel(crate::select::LIST_PANEL),
                layer,
                &model,
                crate::select::build_list,
            );
        }
        let changed = match &self.view {
            Some(last) => view.changed_since(last),
            None => view.len(),
        };
        self.counts.panels_compared += view.len() as u64;
        self.counts.panels_changed += changed as u64;
        let synced = self.panels.sync(view.built(self.view.as_ref()));
        self.counts.duplicate_keys += synced.dropped;
        self.view = Some(view);
        // A new order of panels, or a panel in another slot, is drawn
        // differently even when no model changed.
        changed > 0 || synced.composition
    }
}

/// The focused element and its bounds when its focus ring shows (section
/// 6, rule 3): the keyboard put the focus there, and the element is in a
/// layer that takes input, so no ring shows under an open question or
/// panel. One answer for [`Runtime::focus_ring`] and for painting.
pub(crate) fn shown_ring<A: App>(window: &Window, app: &A, focus: &crate::focus::Focus) -> Option<(UiId, Rect)> {
    let id = focus.id().filter(|_| focus.ring)?;
    if !in_input_layer(window, app, id) {
        return None;
    }
    Some((id.clone(), window.bounds(id)?))
}

/// True when no modifier but Shift is held.
fn plain(modifiers: Modifiers) -> bool {
    !(modifiers.control || modifiers.alt || modifiers.meta)
}

/// True when the element with UI ID `id` in `window` is in a layer that
/// takes input now, by `app`'s input state: the one taking input or one
/// above it.
pub(crate) fn in_input_layer<A: App>(window: &Window, app: &A, id: &UiId) -> bool {
    let layer = app.input().layer;
    window.layer_of(id).is_some_and(|its| its >= layer)
}

/// Which keys click an element the keyboard can activate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Activation {
    /// A button: Enter when it goes down, Space when it comes up.
    Button,
    /// A switch or a check box: Space only, as the WAI-ARIA switch and
    /// checkbox patterns say (section 6, rule 11).
    Switch,
    /// A link: Enter only, as WAI-ARIA's link pattern has it. Space
    /// stays the page's, to scroll.
    Link,
}

impl Activation {
    /// True when Enter clicks it.
    pub(crate) fn on_enter(self) -> bool {
        matches!(self, Self::Button | Self::Link)
    }

    /// True when Space clicks it.
    pub(crate) fn on_space(self) -> bool {
        matches!(self, Self::Button | Self::Switch)
    }
}

/// What the keyboard can activate the element with UI ID `id` in `window`
/// as, now, with `app`'s input state: a button, a switch, or a link, or
/// `None` when it is none of them, offers no Click, is turned off, or is in
/// a layer below the one taking input. The press and the release of Space
/// both ask, so an element that stopped being clickable while Space was
/// held clicks nothing.
pub(crate) fn activatable<A: App>(window: &Window, app: &A, id: &UiId) -> Option<Activation> {
    // While the canvas has the keyboard, its keys are its own (section 6,
    // rule 1): no element of ours is activated by them.
    if app.input().canvas_focused || !in_input_layer(window, app, id) {
        return None;
    }
    let access = window.access_of(id)?;
    if !access.offers(crate::access::Action::Click) || access.is_disabled() {
        return None;
    }
    match access.role() {
        // A menu item is a button in a list (section 3, rule 4).
        crate::access::Role::Button | crate::access::Role::MenuItem => Some(Activation::Button),
        crate::access::Role::Switch | crate::access::Role::CheckBox => Some(Activation::Switch),
        crate::access::Role::Link => Some(Activation::Link),
        _ => None,
    }
}

/// The element a held Space would click now: the one it went down on,
/// when the focus never moved since and it can still be clicked
/// ([`activatable`]). The pressed look reads this too, so a button looks
/// pressed by Space exactly while releasing Space would click it.
pub(crate) fn live_arm<A: App>(window: &Window, app: &A, focus: &Focus, pointer: &Pointer) -> Option<UiId> {
    let (armed, moves) = pointer.space.as_ref()?;
    let unmoved = focus.moves == *moves && focus.id() == Some(armed);
    (unmoved && activatable(window, app, armed).is_some_and(Activation::on_space)).then(|| armed.clone())
}

/// How a scroll key moves a container, from its offset, one line, one page,
/// and the most it can move (section 5, rule 12; the browsers' keys, as the
/// APG has no pattern): Up and Down one line, Page Up and Page Down a page
/// less one line, Space and Shift+Space a page, and Home and End the top and
/// the bottom. `None` for any other key, or with another modifier.
fn scroll_change(key: Key, modifiers: Modifiers) -> Option<crate::window::scroll::ScrollChange> {
    let shift_only = modifiers.shift && plain(modifiers);
    Some(match key {
        Key::Up if modifiers.none() => |at, line, _, _| at - line,
        Key::Down if modifiers.none() => |at, line, _, _| at + line,
        Key::PageUp if modifiers.none() => |at, line, page, _| at - (page - line).max(line),
        Key::PageDown if modifiers.none() => |at, line, page, _| at + (page - line).max(line),
        Key::Space if modifiers.none() => |at, _, page, _| at + page,
        Key::Space if shift_only => |at, _, page, _| at - page,
        Key::Home if modifiers.none() => |_, _, _, _| 0.0,
        Key::End if modifiers.none() => |_, _, _, most| most,
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
