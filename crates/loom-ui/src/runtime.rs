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
use crate::draw::Panels;
use crate::driver::Driver;
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
use accesskit::{Action as KitAction, ActionData};
use loom_ui_core::{Event, Key, KeyEvent, Platform, Point, PointerEvent, Rect, TextMeasure, UiId};
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
            focus: Focus::default(),
            pointer: Pointer::default(),
            field_focus: None,
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
        self.focus.id.as_ref()
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
        self.step_among(self.panels.stops().iter().collect(), from, back)
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
            .stops()
            .iter()
            .filter(|stop| self.panels.layer_of(stop).is_some_and(|layer| layer >= input))
            .collect();
        self.step_among(stops, self.focus.id.as_ref(), back)
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
        self.focus.id.clone()
    }

    /// The runtime's element under the pointer, if one of its layers holds
    /// the hover: what hover text reads (section 5, rule 10).
    pub fn hovered(&self) -> Option<&UiId> {
        self.pointer.hovered.as_ref().map(|(_, id)| id)
    }

    /// The element with focus and where its ring goes, in logical pixels,
    /// when the keyboard put the focus there. `None` after a press, or with
    /// no focus.
    pub fn focus_ring(&self) -> Option<(UiId, Rect)> {
        let focus = self.focus.id.as_ref().filter(|_| self.focus.ring)?;
        Some((focus.clone(), self.panels.bounds(focus)?))
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

    /// Moves the runtime's focus to `to`, from the keyboard or an effect,
    /// so a ring shows. Returns true when it changed.
    fn focus_to(&mut self, to: Option<UiId>) -> bool {
        self.focus.wanted = None;
        let changed = self.focus.id != to || (to.is_some() && !self.focus.ring);
        self.focus.id = to;
        self.focus.ring = self.focus.id.is_some();
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
            FocusTarget::Next | FocusTarget::Previous if self.focus.id.is_some() => {
                match self.focus_step(self.focus.id.as_ref(), *target == FocusTarget::Previous) {
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
    /// one step through its order. Past either end, focus leaves the
    /// runtime's elements and the key goes on to the target. With nothing
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
        if self.focus.id.is_none() {
            let enters = input.layer <= Layer::Panels && !input.canvas_focused && self.target.focused().is_none();
            return match self.tab_step(input.layer, modifiers.shift) {
                FocusStep::To(stop) if enters => (true, self.focus_to(Some(stop))),
                _ => (false, false),
            };
        }
        // A focus in a layer that takes no input now is left where it is
        // when Tab finds no stop above it: the key goes on to the target.
        let below = self
            .focus
            .id
            .as_ref()
            .and_then(|id| self.panels.layer_of(id))
            .is_some_and(|layer| layer < input.layer);
        match self.tab_step(input.layer, modifiers.shift) {
            FocusStep::To(stop) => (true, self.focus_to(Some(stop))),
            FocusStep::PastEnd if below => (false, false),
            FocusStep::PastEnd => (false, self.focus_to(None)),
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
        let Event::Key(key) = event else {
            return (false, false);
        };
        // A field in a layer below the one taking input, such as a panel
        // under a question, edits nothing: the key goes on.
        let layer = self.app.input().layer;
        let Some(id) = self
            .focus
            .id
            .clone()
            .filter(|id| self.panels.fields.has(id) && self.panels.layer_of(id).is_some_and(|its| its >= layer))
        else {
            return (false, false);
        };
        let used = self.panels.fields.key(&id, key, now, Platform::current());
        if let Some(text) = used.copy {
            driver.clipboard_write(text);
        }
        let text = || {
            self.panels
                .fields
                .get(&id)
                .map(|field| field.text().to_owned())
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
                .map(|state| state.text().to_owned())
                .unwrap_or_default();
            batch.extend(self.app.field_event(&field, &FieldEvent::Edited(text)));
        }
        Some(used.changed)
    }

    /// When focus has moved off the text field that had it: the field
    /// forgets its undo steps, and the app hears [`FieldEvent::Left`], in
    /// `batch`. The window losing the keys moves no focus, so it is not
    /// this (section 8, rule 3).
    fn field_left(&mut self, batch: &mut Vec<A::Message>) {
        if let Some(field) = self.field_focus.take() {
            if self.focus.id.as_ref() == Some(&field) {
                self.field_focus = Some(field);
                return;
            }
            self.panels.fields.left(&field);
            batch.extend(self.app.field_event(&field, &FieldEvent::Left));
        }
        self.field_focus = self.focus.id.clone().filter(|id| self.panels.fields.has(id));
    }

    /// The text the field with UI ID `id` shows now, for tests.
    #[cfg(test)]
    pub(crate) fn field_text(&self, id: &UiId) -> Option<&str> {
        self.panels.fields.get(id).map(crate::field::FieldState::text)
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
        // A layout between turns may have moved focus off a field, or
        // changed what a field reports.
        self.field_left(&mut batch);
        let mut redraw = false;
        let mut reported = self.panels.fields.take_changed();

        // 2. Route the event.
        match event {
            DriverEvent::Input { event, pointer } => {
                let (taken, moved) = self.tab(&event);
                redraw |= moved;
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
                None => batch.extend(self.app.accessibility_event(&event)),
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
        self.field_left(&mut batch);
        // What a field reports changed: one notice, after the field's own
        // messages, so the app's update for it sees the new text.
        if reported {
            batch.extend(self.app.access_changed());
        }

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
        self.counts.updates += updates as u64;

        // 4. What changed.
        if updates > 0 || self.view.is_none() {
            redraw |= self.rebuild(driver);
            // A layer the input layer rose above holds no hover or press.
            redraw |= self.pointer.below(self.app.input().layer);
        }

        redraw |= self.target.needs_redraw(updates);

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
            again: !self.exited && !self.pending.is_empty(),
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
            focused: self.focus.id.clone().or_else(|| self.target.focused()),
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
                    self.field_left(&mut later);
                    self.pending.extend(later);
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

#[cfg(test)]
mod tests;
