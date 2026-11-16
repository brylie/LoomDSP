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

use crate::app::{App, Context, Deadline, WindowSpec};
use crate::driver::Driver;
use crate::effect::{Effect, FocusTarget, Selection};
use crate::event::{DriverEvent, WindowEvent, WindowState};
use crate::focus::FocusStep;
use crate::hit::Hit;
use crate::routing::Target;
use crate::view::{Layer, View};
use crate::waker::{Wake, Waker};
use crate::window::Window;
use loom_ui_core::{Event, Key, KeyEvent, Point, Rect, TextMeasure, UiId};
use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver};
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
    /// The layouts of the panels that have elements.
    panels: Window,
    spec: Option<WindowSpec>,
    deadline: Option<Deadline<A::Message>>,
    after_draw: Vec<A::Message>,
    waiting_for_draw: Vec<A::Message>,
    exited: bool,
    counts: Counts,
    /// Which of the runtime's own elements has keyboard focus.
    focus: Option<UiId>,
    /// The keyboard, or a focus effect, put it there, so a ring shows
    /// (ui-layer.md, section 6, rule 3). A press does not.
    ring: bool,
    /// A focus effect named a UI ID that no laid-out stop has yet, such as
    /// a field in a panel opened in the same turn. The next layout takes it
    /// if a focus stop has it by then, and drops it otherwise; a focus that
    /// moves first drops it too.
    wanted: Option<UiId>,
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
            waker: Waker::new(sender, wake),
            queue,
            pending: VecDeque::new(),
            window,
            view: None,
            panels: Window::default(),
            spec: None,
            deadline: None,
            after_draw: Vec::new(),
            waiting_for_draw: Vec::new(),
            exited: false,
            counts: Counts::default(),
            focus: None,
            ring: false,
            wanted: None,
        }
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
    /// changes the app only through `update` (section 2, rule 1).
    #[cfg(feature = "test-driver")]
    pub fn app_mut(&mut self) -> &mut A {
        &mut self.app
    }

    /// The routing target, to read.
    pub fn target(&self) -> &T {
        &self.target
    }

    /// The routing target, to change. A driver may need this to draw it.
    pub fn target_mut(&mut self) -> &mut T {
        &mut self.target
    }

    /// The app to read and the target to use, at once: a driver that draws
    /// the target's views from the app needs both.
    pub fn parts(&mut self) -> (&A, &mut T) {
        (&self.app, &mut self.target)
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
        let laid = self.panels.lay_out(self.window.size, text);
        self.counts.panels_laid_out += laid.panels;
        self.counts.elements_laid_out += laid.elements;
        self.settle_focus();
    }

    /// After a layout: takes a focus that a focus effect asked for before
    /// its element was laid out, taking it from the target, and moves a
    /// focus whose element is gone to the first stop of the panel that held
    /// it, or to none when that panel is gone too or has no stop (section 6,
    /// rule 4).
    fn settle_focus(&mut self) {
        if let Some(wanted) = self.wanted.take()
            && self.panels.stops().contains(&wanted)
        {
            self.release_target();
            self.focus = Some(wanted);
            self.ring = true;
            return;
        }
        let Some(focus) = &self.focus else {
            return;
        };
        if !self.panels.stops().contains(focus) {
            self.focus = self
                .panels
                .panel_of(focus)
                .cloned()
                .and_then(|panel| self.panels.first_stop_in(&panel));
        }
    }

    /// The runtime's own element with keyboard focus, if any. Elements a
    /// toolkit still draws keep theirs in the routing target.
    pub fn focused(&self) -> Option<&UiId> {
        self.focus.as_ref()
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
    pub fn focus_step(&self, from: Option<&UiId>, back: bool) -> FocusStep {
        let stops = self.panels.stops();
        let at = from.and_then(|from| stops.iter().position(|stop| stop == from));
        let next = match (at, back) {
            (None, false) => stops.first(),
            (None, true) => stops.last(),
            (Some(at), false) => stops.get(at + 1),
            (Some(at), true) => at.checked_sub(1).and_then(|at| stops.get(at)),
        };
        next.map_or(FocusStep::PastEnd, |stop| FocusStep::To(stop.clone()))
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
        self.wanted = None;
        self.focus = self.panels.focus_at(point).flatten().cloned();
        self.ring = false;
        if self.focus.is_some() {
            self.release_target();
        }
        self.focus.clone()
    }

    /// The element with focus and where its ring goes, in logical pixels,
    /// when the keyboard put the focus there. `None` after a press, or with
    /// no focus.
    pub fn focus_ring(&self) -> Option<(UiId, Rect)> {
        let focus = self.focus.as_ref().filter(|_| self.ring)?;
        Some((focus.clone(), self.panels.bounds(focus)?))
    }

    /// Tells the routing target to let go of its focus, because one of the
    /// runtime's elements takes it: one element has focus across the window
    /// (section 6, rule 1). Every path that gives the runtime's elements the
    /// focus goes through here: a press, a focus effect, and a focus that
    /// waited for its layout. A target with no focus is told nothing. When a
    /// layout runs through the driver's own parts, the release it owes the
    /// target is carried out by the router there.
    fn release_target(&mut self) {
        if self.target.focused().is_some() {
            self.target.focus(&FocusTarget::Nothing, Selection::Keep);
        }
    }

    /// Moves the runtime's focus to `to`, from the keyboard or an effect,
    /// so a ring shows. Returns true when it changed.
    fn focus_to(&mut self, to: Option<UiId>) -> bool {
        self.wanted = None;
        let changed = self.focus != to || (to.is_some() && !self.ring);
        self.focus = to;
        self.ring = self.focus.is_some();
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
            FocusTarget::Next | FocusTarget::Previous if self.focus.is_some() => {
                match self.focus_step(self.focus.as_ref(), *target == FocusTarget::Previous) {
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
                // stop has the ID by then (`settle_focus`).
                self.wanted = Some(id.clone());
                false
            }
            FocusTarget::Element(id) => {
                // The target's, as before. It still waits, in case a panel
                // opened in this turn has it.
                let changed = self.focus_to(None);
                self.target.focus(target, select);
                self.wanted = Some(id.clone());
                changed
            }
            FocusTarget::Next | FocusTarget::Previous | FocusTarget::Nothing => {
                self.target.focus(target, select);
                self.focus_to(None)
            }
        }
    }

    /// Tab and Shift+Tab while one of the runtime's elements has focus:
    /// one step through its order. Past either end, focus leaves the
    /// runtime's elements and the key goes on to the target. Tab with Ctrl
    /// held is the app's region shortcut, never a step (section 6, rule 7).
    /// Returns whether the runtime took the key, and whether its focus
    /// changed, which needs a redraw even when the key goes on.
    fn tab(&mut self, event: &Event) -> (bool, bool) {
        let Event::Key(KeyEvent::Pressed {
            key: Key::Tab,
            modifiers,
            ..
        }) = event
        else {
            return (false, false);
        };
        if self.focus.is_none() || modifiers.control {
            return (false, false);
        }
        match self.focus_step(self.focus.as_ref(), modifiers.shift) {
            FocusStep::To(stop) => (true, self.focus_to(Some(stop))),
            FocusStep::PastEnd => (false, self.focus_to(None)),
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
        let mut redraw = false;

        // 2. Route the event.
        match event {
            DriverEvent::Input { event, pointer } => {
                let (taken, moved) = self.tab(&event);
                redraw |= moved;
                if !taken {
                    self.target.input(&event, pointer, now, &mut batch);
                }
            }
            DriverEvent::Window(event) => self.window_event(event, now, &mut batch),
            DriverEvent::Accessibility(event) => batch.extend(self.app.accessibility_event(&event)),
            DriverEvent::RedrawRequested => redraw = true,
            DriverEvent::Frame => {
                self.counts.frames += 1;
                redraw = true;
            }
            DriverEvent::Presented => batch.append(&mut self.waiting_for_draw),
            DriverEvent::Woken | DriverEvent::DeadlineReached => {}
        }

        // 3. One update per message, each followed by its own effects.
        let mut updates = 0;
        for message in batch {
            if self.exited {
                break;
            }
            updates += 1;
            redraw |= self.update(message, now, driver);
        }
        self.counts.updates += updates as u64;

        // 4. What changed.
        if updates > 0 || self.view.is_none() {
            redraw |= self.rebuild(driver);
        }

        redraw |= self.target.needs_redraw(updates);

        // 5. Draw only when something visible changed.
        let draw = redraw && !self.exited;
        if draw {
            self.counts.redraws += 1;
            self.waiting_for_draw.append(&mut self.after_draw);
        }

        // 6. What comes next.
        self.drain_queue();
        Next {
            draw,
            again: !self.exited && !self.pending.is_empty(),
            frames: !self.exited && self.app.animating(),
            deadline: if self.exited {
                None
            } else {
                self.deadline.as_ref().map(|deadline| deadline.at)
            },
        }
    }

    /// Moves everything wakers sent into `pending`, in order.
    fn drain_queue(&mut self) {
        self.pending.extend(self.queue.try_iter());
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
            focused: self.focus.clone().or_else(|| self.target.focused()),
            window: self.window,
            panels: Some(&self.panels),
        };
        let effects = self.app.update(message, &cx);
        let mut redraw = false;
        for effect in effects {
            match effect {
                Effect::Focus { target, select } => redraw |= self.focus_effect(&target, select),
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

#[cfg(test)]
mod tests;
