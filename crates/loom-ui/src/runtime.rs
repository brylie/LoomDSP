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
use loom_ui_core::{Event, Key, KeyEvent, Modifiers, Platform, Point, PointerEvent, Rect, Size, TextMeasure, UiId};
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
    window: WindowState,
    view: Option<View>,
    /// The app changed outside a turn ([`Runtime::mark_view_stale`], for
    /// tests), so the next turn builds its view again even with no update.
    view_stale: bool,
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
            window,
            view: None,
            view_stale: false,
            panels: Window::default(),
            spec: None,
            deadline: None,
            after_draw: Vec::new(),
            waiting_for_draw: Vec::new(),
            exited: false,
            counts: Counts::default(),
            focus: Focus::default(),
            pointer: Pointer::default(),
            field_focus: None,
            target_seen: None,
            paste_answers: mpsc::channel(),
            pasted: VecDeque::new(),
            wake,
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
        self.release_if_owed();
    }

    /// The runtime's own element with keyboard focus, if any. Elements a
    /// toolkit still draws keep theirs in the routing target.
    pub fn focused(&self) -> Option<&UiId> {
        self.focus.id()
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
        self.panels.access_nodes(ids, self.window.scale, self.focus.id())
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
        (used.taken, used.redraw)
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

    /// What the keyboard can activate the element with UI ID `id` as, now:
    /// `Some(true)` for a button, `Some(false)` for a switch, `None` when it
    /// is neither, offers no Click, is turned off, or is in a layer below
    /// the one taking input. The press and the release of Space both ask,
    /// so an element that stopped being clickable while Space was held
    /// clicks nothing.
    fn activatable(&self, id: &UiId) -> Option<bool> {
        activatable(&self.panels, &self.app, id)
    }

    /// The element a held Space would click now ([`live_arm`]).
    fn live_arm(&self) -> Option<UiId> {
        live_arm(&self.panels, &self.app, &self.focus, &self.pointer)
    }

    /// Keyboard activation of the focused element (section 3, rule 4):
    /// Enter clicks a button when it goes down, and Space clicks a button
    /// or a switch when it comes up on the element it went down on.
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
            let cancelled = self.live_arm().is_some();
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
            if cancelled || hid {
                return (true, hid);
            }
        }
        let Some(id) = self.focus.id().cloned() else {
            return (false, false);
        };
        let Some(button) = self.activatable(&id) else {
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
            Key::Enter if button && plain => {
                if !repeat {
                    self.click(&id);
                }
                (true, false)
            }
            Key::Space if plain => {
                if !repeat {
                    self.pointer.space = Some((id, self.focus.moves));
                }
                (true, false)
            }
            _ => (false, false),
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
        self.turn(DriverEvent::Woken, vec![message], now, driver)
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
        self.release_if_owed();
        // The target's own element may have taken the keyboard since the
        // last turn; then a layout between turns, or that, may have moved
        // focus off a field, or changed what a field reports.
        // What shows as pressed is painted from the runtime's own state,
        // so a change of it needs a frame, though no panel is built again
        // (visual-system.md, Role tokens): an input event, or an update
        // that raises the input layer or turns the element off.
        let pressed = self.panels().shown_pressed();
        let mut redraw = self.follow_target();
        self.field_left(&mut batch, now);
        // A scroll since the last turn, by the pointer between turns or by
        // a layout's reveal, changed what the tree reports too.
        let mut reported = self.panels.fields.take_changed() | self.panels.take_scrolled();
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
                let (mut taken, moved) = if composing { (false, false) } else { self.tab(&event) };
                redraw |= moved;
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
                    } else {
                        redraw |= self.route(&event, pointer, now, &mut batch);
                    }
                }
                // A focus the event moved is the first thing the app hears
                // this turn, so no update reads a focus it was not told of.
                if let Some(report) = self.panels().focus_report() {
                    batch.insert(0, report);
                }
            }
            DriverEvent::Window(event) => self.window_event(event, now, &mut batch),
            DriverEvent::Accessibility(event) => match self.field_action(&event, now, &mut batch) {
                Some(changed) => {
                    redraw |= changed;
                    reported |= changed;
                }
                None => match self.scroll_action(&event) {
                    Some(moved) => redraw |= moved,
                    None => batch.extend(self.app.accessibility_event(&event)),
                },
            },
            DriverEvent::RedrawRequested => redraw = true,
            DriverEvent::Frame => {
                self.counts.frames += 1;
                redraw = true;
            }
            DriverEvent::Presented => batch.append(&mut self.waiting_for_draw),
            DriverEvent::Woken | DriverEvent::DeadlineReached => {}
        }

        // Tab, or a press the target handed back, may have moved focus off
        // a field: the app hears of it in this turn.
        self.field_left(&mut batch, now);
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
        for message in batch {
            if self.exited {
                break;
            }
            updates += 1;
            redraw |= self.update(message, now, driver);
        }
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
        // Taken first, so any rebuild uses up a stale view.
        let stale = std::mem::take(&mut self.view_stale);
        if updates > 0 || self.view.is_none() || stale {
            redraw |= self.rebuild(driver);
            // A layer the input layer rose above holds no hover or press.
            redraw |= self.pointer.below(self.app.input().layer);
        }

        redraw |= self.target.needs_redraw(updates);
        redraw |= self.panels().shown_pressed() != pressed;

        // 5. Draw only when something visible changed.
        let draw = redraw && !self.exited;
        if draw {
            self.counts.redraws += 1;
            self.waiting_for_draw.append(&mut self.after_draw);
        }

        // 6. What comes next: a moved focus is reported for the next turn.
        self.panels().report_focus();
        self.drain_queue();
        Next {
            draw,
            again: !self.exited && (!self.pending.is_empty() || !self.pasted.is_empty()),
            frames: !self.exited && self.app.animating(),
            deadline: if self.exited {
                None
            } else {
                self.deadline.as_ref().map(|deadline| deadline.at)
            },
        }
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
        }
        let point = match (event, pointer) {
            (Event::Pointer(PointerEvent::LeftWindow), _) => {
                let redraw = self.pointer.clear();
                self.target.input(event, pointer, now, batch);
                return redraw;
            }
            (Event::Pointer(_), Some(point)) => point,
            _ => {
                self.target.input(event, pointer, now, batch);
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
                Routed::Passed => panels.pointer_below(event, point).redraw,
                Routed::Later => false,
            };
        }
        self.release_if_owed();
        redraw
    }

    /// Keeps the held modifiers from a key event, for Ctrl with the wheel
    /// over a slider (section 5, rule 12).
    fn note_modifiers(&mut self, event: &Event) {
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
                }
            }
            WindowEvent::CloseRequested | WindowEvent::FileDropped(_) => {}
        }
        batch.extend(self.app.window_event(&event));
    }

    /// Runs one update and its effects. Returns true when an effect asked
    /// for a draw.
    fn update(&mut self, message: A::Message, now: Instant, driver: &mut impl Driver<A::Message>) -> bool {
        let cx = Context {
            now,
            waker: &self.waker,
            focused: self.focus.id().cloned().or_else(|| self.target.focused()),
            window: self.window,
            panels: Some(&self.panels),
        };
        let effects = self.app.update(message, &cx);
        let mut redraw = false;
        for effect in effects {
            match effect {
                Effect::Focus { target, select } => {
                    redraw |= self.focus_effect(&target, select);
                    // Each focus effect may move focus off a field, even
                    // one that a later effect of the same update moves
                    // back: the field leaves at each, and the app hears of
                    // it on the next turn.
                    let mut later = Vec::new();
                    self.field_left(&mut later, now);
                    self.pending.extend(later);
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
        let mut view = View::new();
        self.app.view(&mut view);
        let changed = match &self.view {
            Some(last) => view.changed_since(last),
            None => view.len(),
        };
        self.counts.panels_compared += view.len() as u64;
        self.counts.panels_changed += changed as u64;
        let synced = self.panels.sync(view.built(self.view.as_ref()));
        self.counts.duplicate_keys += synced.dropped;
        self.view = Some(view);

        let spec = self.app.window();
        if self.spec.as_ref() != Some(&spec) {
            driver.window(&spec);
            self.spec = Some(spec);
        }
        self.deadline = self.app.next_deadline();
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

/// What the keyboard can activate the element with UI ID `id` in `window`
/// as, now, with `app`'s input state: `Some(true)` for a button,
/// `Some(false)` for a switch, `None` when it is neither, offers no Click,
/// is turned off, or is in a layer below the one taking input. The press
/// and the release of Space both ask, so an element that stopped being
/// clickable while Space was held clicks nothing.
pub(crate) fn activatable<A: App>(window: &Window, app: &A, id: &UiId) -> Option<bool> {
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
        crate::access::Role::Button => Some(true),
        crate::access::Role::Switch => Some(false),
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
    (unmoved && activatable(window, app, armed).is_some()).then(|| armed.clone())
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
