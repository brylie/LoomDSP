//! Iced inside our own runtime, until phase 3 (docs/dev/leaving-iced-plan.md,
//! steps 2.1 and 2.2).
//!
//! The app runs inside `loom_ui::Runtime` on the `winit` shell
//! (`loom_ui::shell`). Iced draws every view and holds the widgets, so it is
//! the runtime's routing target here ([`IcedTarget`]): focus and scrolling
//! become Iced's widget operations, and what only Iced's widgets know (focus,
//! where a part is) the app asks as window questions (`window_question.rs`).
//!
//! - `window.rs` is the shell's layers: it turns window events into Iced's,
//!   runs Iced's updates and draws, posts their messages routed before
//!   `update` (`input_owner::route`), and runs the turn's widget operations
//!   through `tasks.rs`.
//! - `services.rs` gives the shell the dialogs and the clipboard.
//! - `preedit.rs` draws the text an input method is composing.
//! - `Bridge` is the runtime with Iced as its driver, for the `Ui` harness
//!   only (`headless.rs`), until step 2.3.
//!
//! Only this module and the files under it may name Iced's `Task` or `rfd`;
//! `tests/purity.rs` checks that, and its list of files not yet ported is
//! empty.

use crate::window_question::WindowQuestion;
use crate::{App, Message};
use iced::Task;
use loom_ui::effect::{FocusTarget, Selection};
use loom_ui::{Context, Deadline, Effects, WindowSpec};
use loom_ui_core::UiId;
use std::time::Instant;
// The bridge the `Ui` harness runs on, with Iced's runtime gone from the app.
#[cfg(any(test, feature = "test-ui"))]
use {
    iced::futures::channel::mpsc,
    loom_ui::{DriverEvent, Runtime, Waker, WindowState},
    sleeper::Sleeper,
    std::hash::{Hash, Hasher},
    std::sync::atomic::{AtomicBool, Ordering},
    std::sync::{Arc, Mutex},
};

/// What Iced sends the bridge. Nearly every value is an app message, so
/// boxing it to shrink the two empty wake-ups would allocate for every
/// message instead.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum Bridged {
    /// A message from a view or a subscription.
    App(Message),
    /// A waker sent a message from another thread. The `Ui` harness's
    /// bridge only, as are the two below: the shell sends its own.
    #[cfg(any(test, feature = "test-ui"))]
    Woken,
    /// The app's deadline has come.
    #[cfg(any(test, feature = "test-ui"))]
    Deadline,
    /// A frame, which the crate's tests and the `Ui` harness send as one
    /// shown.
    #[cfg(any(test, feature = "test-ui"))]
    Presented,
    /// Iced's answer to the focus question (`focus_query`).
    Focus(FocusAnswer),
}

/// Which widget has keyboard focus, as Iced's widgets know it.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct FocusAnswer {
    /// A focusable widget has focus.
    pub any: bool,
    /// Its widget ID, if it has one.
    pub widget: Option<iced::advanced::widget::Id>,
}

impl FocusAnswer {
    /// The part with focus: the field's part when its widget ID is one
    /// `places::field_widget_id` made. A focused widget with no such ID still
    /// has focus, so it reads as [`unnamed_field`], and the canvas still
    /// leaves the keys to it.
    pub(crate) fn part(&self) -> Option<UiId> {
        let named = self.widget.as_ref().and_then(crate::places::field_of);
        named.or_else(|| self.any.then(unnamed_field))
    }
}

/// The part reported for a focused field whose widget has no field ID.
pub(crate) fn unnamed_field() -> UiId {
    UiId::panel("unnamed-field")
}

/// Asks Iced which widget has focus. Iced's text fields do not say when
/// focus moves, and a press can move it, so the bridge asks after a turn
/// that may have moved it (`App::take_focus_question`) and after every
/// focus effect.
pub(crate) fn focus_query<T: Send + 'static>(reply: impl FnMut(FocusAnswer) -> T + Send + 'static) -> Task<T> {
    iced::advanced::widget::operate(FindFocus::default()).map(reply)
}

/// The widget operation that finds the focused widget. The `Ui` harness
/// runs it on its own view, as the bridge does through Iced.
#[derive(Default)]
pub(crate) struct FindFocus(pub FocusAnswer);

impl iced::advanced::widget::Operation<FocusAnswer> for FindFocus {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn iced::advanced::widget::Operation<FocusAnswer>)) {
        operate(self);
    }

    fn focusable(
        &mut self,
        id: Option<&iced::advanced::widget::Id>,
        _bounds: iced::Rectangle,
        state: &mut dyn iced::advanced::widget::operation::Focusable,
    ) {
        if state.is_focused() {
            self.0 = FocusAnswer {
                any: true,
                widget: id.cloned(),
            };
        }
    }

    fn finish(&self) -> iced::advanced::widget::operation::Outcome<FocusAnswer> {
        iced::advanced::widget::operation::Outcome::Some(self.0.clone())
    }
}

/// The Iced task that asks a window question and hands its answer to the
/// app on a later turn, in the order asked. Shared by the bridge and the
/// `Ui` harness.
pub(crate) fn question_task(question: WindowQuestion) -> Task<Message> {
    // Each operation gives one answer; the reply runs once.
    fn once<T, M>(reply: Box<dyn FnOnce(T) -> M + Send>) -> impl FnMut(T) -> M + Send + 'static
    where
        T: 'static,
        M: 'static,
    {
        let mut reply = Some(reply);
        move |answer| (reply.take().expect("one answer per question"))(answer)
    }
    match question {
        WindowQuestion::Focus(reply) => {
            let mut reply = once(reply);
            focus_query(move |answer| reply(answer.part()))
        }
        WindowQuestion::Find { text, reply } => {
            iced_runtime::task::widget(crate::control::Find::new(text)).map(once(reply))
        }
        WindowQuestion::Places(reply) => iced_runtime::task::widget(crate::places::Collect::default()).map(once(reply)),
    }
}

/// The Iced task that carries out a focus effect: the field's widget for
/// an element, chained with select-all when asked, and then the focus
/// question, so the app hears where focus went. Shared by the bridge and
/// the `Ui` harness.
pub(crate) fn focus_task<T: Send + 'static>(
    target: &FocusTarget,
    select: Selection,
    answer: impl Fn(FocusAnswer) -> T + Send + 'static,
) -> Task<T> {
    focus_moves(target, select).chain(focus_query(answer))
}

/// The widget operations that move focus for a focus effect, with no
/// question after them. In the bridge era only text fields take focus, so
/// an element is its text field's widget.
pub(crate) fn focus_moves<T: Send + 'static>(target: &FocusTarget, select: Selection) -> Task<T> {
    match target {
        FocusTarget::Element(id) => {
            let widget = crate::places::field_widget_id(id);
            let focus = iced::widget::operation::focus(widget.clone());
            match select {
                Selection::Keep => focus,
                Selection::All => focus.chain(iced::advanced::widget::operate(
                    iced::advanced::widget::operation::text_input::select_all(widget),
                )),
            }
        }
        FocusTarget::Next => iced::widget::operation::focus_next(),
        FocusTarget::Previous => iced::widget::operation::focus_previous(),
        FocusTarget::Nothing => iced::advanced::widget::operate(iced::advanced::widget::operation::focusable::focus(
            iced::advanced::widget::Id::unique(),
        )),
    }
}

/// The app as `loom_ui::Runtime` runs it. A wrapper, so the app's own
/// `view` (the Iced views) and the trait's `view` (panel models, phase 3)
/// never share a name on one type. It goes at 2.2, when the app
/// implements the trait itself.
// The second field logs each message `update` gets, for the `Ui` harness,
// whose tests read what ran. `None` in the app, so nothing is copied. The
// third holds panels a test adds; it is empty outside tests.
pub(crate) struct Hosted(pub(crate) App, pub(crate) Option<Vec<Message>>, pub(crate) Extra);

/// `loom-ui` panels a test adds to the window, to check routing and drawing
/// between Iced and the runtime's layers before any real view moves there
/// (plan step 3.1). Empty outside tests, so the app shows no panel.
#[derive(Default)]
pub(crate) struct Extra {
    /// The test's panels: what they show and what a click on them means.
    #[cfg(any(test, feature = "test-ui"))]
    pub(crate) panels: Option<TestPanels>,
}

/// A test's panels for [`Extra`].
#[cfg(any(test, feature = "test-ui"))]
pub(crate) struct TestPanels {
    /// Adds the panels to the view.
    pub(crate) view: fn(&mut loom_ui::View),
    /// The message for a click on one of their elements.
    pub(crate) clicked: fn(&UiId) -> Option<Message>,
    /// The element a message moves the focus to, by a focus effect after
    /// the app's own, as one of the app's panels would ask.
    pub(crate) focuses: fn(&Message) -> Option<UiId>,
}

#[cfg(any(test, feature = "test-ui"))]
impl Extra {
    /// The test's panels, if it added any.
    fn view(&self, view: &mut loom_ui::View) {
        if let Some(panels) = &self.panels {
            (panels.view)(view);
        }
    }

    /// The message for a click on a test panel's element.
    fn clicked(&self, id: &UiId) -> Option<Message> {
        self.panels.as_ref().and_then(|panels| (panels.clicked)(id))
    }

    /// The test's focus effect for `message`, after the app's own.
    fn answer(&self, message: &Message) -> Effects<Message> {
        self.panels
            .as_ref()
            .and_then(|panels| (panels.focuses)(message))
            .map_or_else(Effects::none, Effects::focus)
    }
}

#[cfg(not(any(test, feature = "test-ui")))]
impl Extra {
    /// No panels outside tests.
    fn view(&self, _view: &mut loom_ui::View) {}

    /// Nothing to click outside tests.
    fn clicked(&self, _id: &UiId) -> Option<Message> {
        None
    }

    /// No effects of its own outside tests.
    fn answer(&self, _message: &Message) -> Effects<Message> {
        Effects::none()
    }
}

impl loom_ui::App for Hosted {
    type Message = Message;

    fn update(&mut self, message: Message, cx: &Context<'_, Message>) -> Effects<Message> {
        if let Some(log) = &mut self.1 {
            log.push(message.clone());
        }
        let extra = self.2.answer(&message);
        // The app keeps one copy of what has the keyboard in either toolkit:
        // the runtime's own element first, as `cx.focused()` gives it, then
        // Iced's field. Iced's answer names only its own field, so the copy
        // is taken from the context. With no element of the runtime's
        // focused, as in the app today, it is Iced's answer unchanged.
        let message = match message {
            Message::FocusMoved(_) => Message::FocusMoved(cx.focused().cloned()),
            message => message,
        };
        self.0.update(message, cx).and(extra)
    }

    /// No panels yet: Iced draws every view until phase 3. A test may add
    /// some ([`Extra`]).
    fn view(&self, view: &mut loom_ui::View) {
        self.0.chrome_panels(view);
        self.2.view(view);
    }

    /// What a click on one of the runtime's elements means: a toolbar
    /// button's press, a breadcrumb entry's level, or the bar's.
    fn clicked(&self, id: &UiId) -> Option<Message> {
        self.0
            .toolbar_click(id)
            .or_else(|| self.0.breadcrumb_click(id))
            .or_else(|| self.0.bar_click(id))
            .or_else(|| self.0.question_click(id))
            .or_else(|| self.0.menu_click(id))
            .or_else(|| self.2.clicked(id))
    }

    /// Where Tab goes from one of the runtime's stops: the toolbar and the
    /// breadcrumb at the top, the bar at the bottom, with Iced's canvas and
    /// fields between them (`chrome.rs`).
    fn tab_from(&self, from: &UiId, next: Option<&UiId>, back: bool) -> loom_ui::TabStep<Message> {
        self.0.tab_from(from, next, back)
    }

    /// Hover text in the look of Iced's tooltips: the theme's weak
    /// background, a 2 px radius, 6 px padding, at most 480 wide.
    fn hint_style(&self) -> loom_ui::HintStyle {
        use loom_canvas::iced_convert::ToCore as _;
        let theme = self.0.theme();
        let palette = theme.extended_palette();
        loom_ui::HintStyle {
            text: crate::style::BODY_TEXT,
            colour: palette.background.weak.text.to_core(),
            background: palette.background.weak.color.to_core(),
            border: palette.background.weak.color.to_core(),
            border_width: 0.0,
            radius: 2.0,
            padding: 6.0,
            max_width: 480.0,
            gap: 0.0,
        }
    }

    /// A key press under an open question, which the runtime keeps from
    /// Iced: the question's keys and Panic (`question::key_of`).
    fn question_key(&self, key: &loom_ui_core::KeyEvent) -> Option<Message> {
        crate::question::key_of(key)
    }

    /// A press outside the probe menu or Recent closes it (row 3b).
    fn pressed_outside(&self, root: &UiId) -> Option<Message> {
        self.0.menu_pressed_outside(root)
    }

    /// The runtime's focus moved: the app keeps one copy of what has the
    /// keyboard, whichever toolkit holds it, so it hears this as it hears
    /// Iced's word on focus (`FocusMoved`).
    fn focus_moved(&self, focus: Option<&UiId>) -> Option<Message> {
        Some(Message::FocusMoved(focus.cloned()))
    }

    fn window(&self) -> WindowSpec {
        WindowSpec {
            title: self.0.title(),
            min_size: None,
        }
    }

    /// Wakes the app after the runtime updates its window scale.
    fn window_event(&self, event: &loom_ui::WindowEvent) -> Option<Message> {
        matches!(event, loom_ui::WindowEvent::ScaleChanged(_)).then_some(Message::WindowRescaled)
    }

    fn next_deadline(&self) -> Option<Deadline<Message>> {
        self.0.next_deadline()
    }

    /// The input owner as a layer: an open question holds the window, and
    /// every other panel or menu Iced draws over the canvas is an overlay
    /// (ui-layer.md, section 5, rule 2). Whether the canvas has the keys is
    /// [`App::canvas_has_keys`].
    fn input(&self) -> loom_ui::InputState {
        use crate::input_owner::InputOwner;
        loom_ui::InputState {
            layer: match self.0.input_owner() {
                InputOwner::Question => loom_ui::Layer::Question,
                InputOwner::Canvas => loom_ui::Layer::Canvas,
                _ => loom_ui::Layer::Overlays,
            },
            canvas_focused: self.0.canvas_has_keys(),
            // With no overlay open, Tab from nothing is the canvas's, as it
            // was before the bar moved: the canvas is above the bar
            // (section 6, rule 2; Brylie's decision, 2026-11-17).
            target_first: matches!(self.0.input_owner(), InputOwner::Canvas),
        }
    }

    /// A screen reader's start, request, or departure, as the accessibility
    /// layer's event. Its requests then go through the same messages as the
    /// pointer (`a11y/actions.rs`, section 7, rule 6).
    fn accessibility_event(&self, event: &loom_ui::AccessEvent) -> Option<Message> {
        Some(Message::A11y(match event {
            loom_ui::AccessEvent::Activated => crate::a11y::Event::Activated,
            loom_ui::AccessEvent::Action(request) => crate::a11y::Event::Action(request.clone()),
            loom_ui::AccessEvent::Deactivated => crate::a11y::Event::Deactivated,
        }))
    }
}

#[cfg(any(test, feature = "test-ui"))]
/// A driver for the bridge: it carries out effects, and gives the Iced
/// tasks they need after each turn. [`IcedDriver`] in the app; the `Ui`
/// harness's own in tests (`iced_bridge/headless.rs`).
pub(crate) trait BridgeDriver: loom_ui::Driver<Message> + Default {
    /// The Iced tasks the effects of the turn just run need.
    fn take_tasks(&mut self) -> Vec<Task<Bridged>>;
}

#[cfg(any(test, feature = "test-ui"))]
/// Iced's state: our runtime, holding the app, and the driver that
/// carries out its effects.
pub(crate) struct Bridge<D: BridgeDriver> {
    runtime: Runtime<Hosted, IcedTarget>,
    driver: D,
    /// Set while the bridge itself puts a message from Iced into the
    /// queue: that turn runs at once, so it needs no wake-up.
    quiet: Arc<AtomicBool>,
    wakes: WakeSource,
    /// Wakes the loop at the runtime's next deadline.
    sleeper: Sleeper,
}

#[cfg(any(test, feature = "test-ui"))]
impl<D: BridgeDriver> Bridge<D> {
    /// The bridge around `app` with no window to attach to, and its first
    /// turn's tasks. The `Ui` harness starts here.
    pub(crate) fn new(app: App) -> (Self, Task<Bridged>) {
        let (sender, receiver) = mpsc::unbounded();
        let quiet = Arc::new(AtomicBool::new(false));
        let wake = {
            let quiet = quiet.clone();
            Arc::new(move || {
                if !quiet.load(Ordering::SeqCst) {
                    let _ = sender.unbounded_send(());
                }
            })
        };
        let window = WindowState::new(loom_ui_core::Size::new(crate::WINDOW.width, crate::WINDOW.height));
        // The sleeper wakes the loop as a waker does; the turn it starts
        // finds the deadline due. Its wake is never one the bridge holds
        // back: it holds them back only while it queues a message itself,
        // and the turn that follows runs after the time came.
        let sleeper = {
            let wake = wake.clone();
            Sleeper::start(move || wake())
        };
        let mut bridge = Self {
            runtime: Runtime::new(Hosted(app, None, Extra::default()), IcedTarget::default(), window, wake),
            driver: D::default(),
            quiet,
            wakes: WakeSource(Arc::new(Mutex::new(Some(receiver)))),
            sleeper,
        };
        // The first turn builds the view and reads the first deadline, such
        // as the crash recovery question (autosave.md, Crash recovery).
        let task = bridge.turn(DriverEvent::Woken);
        (bridge, task)
    }

    /// Iced's update: one turn of our runtime.
    pub(crate) fn update(&mut self, message: Bridged) -> Task<Bridged> {
        match message {
            // An input event is routed here, before `update`: each message
            // it becomes is its own update in this turn, after what was
            // already waiting (ui-layer.md, section 2, rule 5).
            Bridged::App(message) => {
                self.quiet.store(true, Ordering::SeqCst);
                // One batch, so no watcher thread's message lands between
                // the messages of one event.
                self.runtime.waker().send_all(crate::input_owner::route(message));
                self.quiet.store(false, Ordering::SeqCst);
                self.turn(DriverEvent::Woken)
            }
            Bridged::Woken => self.turn(DriverEvent::Woken),
            Bridged::Deadline => self.turn(DriverEvent::DeadlineReached),
            #[cfg(any(test, feature = "test-ui"))]
            Bridged::Presented => self.turn(DriverEvent::Presented),
            // Every answer reaches the app, changed or not, as the window's
            // word on focus after a press: a press elsewhere ends a draft
            // whose field never had focus.
            //
            // The order keeps the app's copy of focus with the target's.
            // What is already queued runs first, under the focus it was sent
            // under; then the target takes the answer, and `FocusMoved` runs
            // alone, so no other message sees the new focus before the app
            // has heard of it.
            Bridged::Focus(answer) => {
                let queued = self.turn(DriverEvent::Woken);
                let part = answer.part();
                self.runtime.target_mut().focused = part.clone();
                let moved = self.step(|runtime, now, driver| runtime.deliver(Message::FocusMoved(part), now, driver));
                Task::batch([queued, moved])
            }
        }
    }

    /// Runs one turn and returns what Iced must do for it.
    fn turn(&mut self, event: DriverEvent) -> Task<Bridged> {
        self.step(|runtime, now, driver| runtime.handle(event, now, driver))
    }

    /// Runs one turn by `run`, and returns what Iced must do for it.
    fn step(
        &mut self,
        run: impl FnOnce(&mut Runtime<Hosted, IcedTarget>, Instant, &mut D) -> loom_ui::Next,
    ) -> Task<Bridged> {
        let now = Instant::now();
        let next = run(&mut self.runtime, now, &mut self.driver);
        let mut tasks = self.driver.take_tasks();
        // The window's work, one after another: focus and scroll first,
        // then what only Iced's widgets know, in the order asked, then where
        // focus is after a message that may have moved it. Iced runs a
        // batch's tasks side by side, so a question batched beside a scroll
        // could read the window from before it.
        let mut window = Task::batch(self.runtime.target_mut().take_tasks());
        for question in self.runtime.app().0.take_window_questions() {
            window = window.chain(question_task(question).map(Bridged::App));
        }
        if self.runtime.app().0.take_focus_question() {
            window = window.chain(focus_query(Bridged::Focus));
        }
        tasks.push(window);
        // Messages wait in the runtime, such as those an update queued for
        // the next turn, which no waker announced: run that turn.
        if next.again {
            tasks.push(Task::done(Bridged::Woken));
        }
        // A deadline already due runs next; a later one waits on the
        // sleeper, which then wakes the loop.
        let due = next.deadline.is_some_and(|at| at <= now);
        if due {
            tasks.push(Task::done(Bridged::Deadline));
        }
        self.sleeper.set(next.deadline.filter(|_| !due));
        Task::batch(tasks)
    }

    /// Routes one input event through the runtime, as the window's shell
    /// hands it over before Iced hears it, and returns what Iced must do
    /// for that turn.
    pub(crate) fn input(&mut self, event: loom_ui_core::Event, pointer: Option<loom_ui_core::Point>) -> Task<Bridged> {
        self.turn(DriverEvent::Input { event, pointer })
    }

    /// The runtime, to read: its focus, hover, and panels.
    pub(crate) fn runtime(&self) -> &Runtime<Hosted, IcedTarget> {
        &self.runtime
    }

    /// The runtime, for the harness to reach the panels and the target as
    /// the window's layers do.
    pub(crate) fn runtime_mut(&mut self) -> &mut Runtime<Hosted, IcedTarget> {
        &mut self.runtime
    }

    /// The app, to read.
    pub(crate) fn app(&self) -> &App {
        &self.runtime.app().0
    }

    /// The app, for the `Ui` harness to set up state between turns
    /// (loom-ui's `test-driver` feature).
    #[cfg(any(test, feature = "test-ui"))]
    pub(crate) fn app_mut(&mut self) -> &mut App {
        // What a test changes here, outside a turn, shows on the next frame.
        self.runtime.mark_view_stale();
        &mut self.runtime.app_mut().0
    }

    /// Starts logging each message `update` gets, for [`Bridge::take_log`].
    #[cfg(any(test, feature = "test-ui"))]
    pub(crate) fn log_messages(&mut self) {
        self.runtime.app_mut().1 = Some(Vec::new());
    }

    /// The messages `update` got since the last call, in order.
    #[cfg(any(test, feature = "test-ui"))]
    pub(crate) fn take_log(&mut self) -> Vec<Message> {
        self.runtime
            .app_mut()
            .1
            .as_mut()
            .map(std::mem::take)
            .unwrap_or_default()
    }

    /// A waker into the runtime's queue.
    #[cfg(any(test, feature = "test-ui"))]
    pub(crate) fn waker(&self) -> Waker<Message> {
        self.runtime.waker()
    }

    /// The driver, to read.
    #[cfg(any(test, feature = "test-ui"))]
    pub(crate) fn driver(&self) -> &D {
        &self.driver
    }

    /// The driver, for the `Ui` harness to answer what waits on it.
    #[cfg(any(test, feature = "test-ui"))]
    pub(crate) fn driver_mut(&mut self) -> &mut D {
        &mut self.driver
    }

    /// The wake-ups waiting, for a harness that has no Iced subscription
    /// to hear them. Each is one `Bridged::Woken`.
    #[cfg(any(test, feature = "test-ui"))]
    pub(crate) fn take_wakes(&self) -> usize {
        let mut slot = self.wakes.0.lock().expect("the wake receiver");
        let Some(receiver) = slot.as_mut() else {
            return 0;
        };
        std::iter::from_fn(|| receiver.try_recv().ok()).count()
    }
}

#[cfg(any(test, feature = "test-ui"))]
/// The receiving end of the wake-ups, handed to Iced's subscription once.
#[derive(Clone)]
struct WakeSource(Arc<Mutex<Option<mpsc::UnboundedReceiver<()>>>>);

#[cfg(any(test, feature = "test-ui"))]
impl Hash for WakeSource {
    /// One wake source per window, so one subscription.
    fn hash<H: Hasher>(&self, state: &mut H) {
        "loom-bridge-wake".hash(state);
    }
}

/// The routing target: Iced holds the elements, so input never comes
/// here, and focus and scrolling become Iced's widget operations.
#[derive(Default)]
pub(crate) struct IcedTarget {
    tasks: Vec<Task<Bridged>>,
    /// This turn's scrolls, one after another. Each finds its part after
    /// the scroll before it has moved the window, so two scrolls in a turn
    /// (a matrix card, then its cell) never both work from the old offsets
    /// and add up; Iced would run batched tasks side by side.
    scrolls: Option<Task<Bridged>>,
    /// The part with focus, from Iced's last answer.
    focused: Option<UiId>,
    /// One of the runtime's elements took the focus, so Iced's field lets
    /// go of it before Iced hears the turn's events (`release_focus`).
    release_first: bool,
    /// The runtime took this turn's key, so Iced must not hear it (`kept`).
    drop_keys: bool,
    /// Lets a test isolate an app's redraw effect from Iced's update policy.
    #[cfg(test)]
    suppress_update_redraw: bool,
}

impl IcedTarget {
    /// True once per turn whose key the runtime took: the report then
    /// leaves it out of what Iced hears.
    pub(crate) fn take_drop_keys(&mut self) -> bool {
        std::mem::take(&mut self.drop_keys)
    }

    /// The release owed before Iced hears the turn's events, once.
    fn take_release(&mut self) -> Option<Task<Bridged>> {
        std::mem::take(&mut self.release_first)
            .then(|| focus_task(&FocusTarget::Nothing, Selection::Keep, Bridged::Focus))
    }

    /// This turn's tasks for Iced, the scrolls last and in order.
    fn take_tasks(&mut self) -> Vec<Task<Bridged>> {
        let mut tasks = std::mem::take(&mut self.tasks);
        tasks.extend(self.scrolls.take());
        tasks
    }
}

impl loom_ui::Target<Message> for IcedTarget {
    /// Iced routes its own events to its widgets after the turn, in the
    /// window's report, so a pointer event's answer comes then: the report
    /// hands each one Iced leaves unused to the runtime's layers below it
    /// (`Panels::pointer_below`). Keys go by focus.
    fn input(
        &mut self,
        event: &loom_ui_core::Event,
        _: Option<loom_ui_core::Point>,
        _: Instant,
        _: &mut Vec<Message>,
    ) -> loom_ui::Routed {
        match event {
            loom_ui_core::Event::Pointer(_) => loom_ui::Routed::Later,
            _ => loom_ui::Routed::Passed,
        }
    }

    /// Iced applies focus changes after it has routed the turn's events, so
    /// a release from the runtime waits for the report, which applies it
    /// before those events: the key after it never reaches Iced's field.
    fn release_focus(&mut self) {
        self.release_first = true;
    }

    /// Iced heard the window's key, or input method event, already; the
    /// report leaves it out of Iced's update, so Tab among the runtime's
    /// stops never also moves an Iced field, and text composed in a field
    /// of the runtime's never also reaches one of Iced's.
    fn kept(&mut self, event: &loom_ui_core::Event) {
        // Only a press: a release the runtime takes (Space on its button)
        // follows a press Iced never heard, so Iced may hear it.
        if matches!(
            event,
            loom_ui_core::Event::Key(loom_ui_core::KeyEvent::Pressed { .. }) | loom_ui_core::Event::Ime(_)
        ) {
            self.drop_keys = true;
        }
    }

    fn focus(&mut self, target: &FocusTarget, select: Selection) {
        self.tasks.push(focus_task(target, select, Bridged::Focus));
    }

    fn scroll_into_view(&mut self, id: &UiId) {
        let next = scroll_into_view(id);
        self.scrolls = Some(match self.scrolls.take() {
            Some(before) => before.chain(next),
            None => next,
        });
    }

    /// Iced keeps focus inside its widgets, so this is its last answer to
    /// the focus question.
    fn focused(&self) -> Option<UiId> {
        self.focused.clone()
    }

    /// Iced draws after every update, as it always has (section 2, rule
    /// 10: the bridge's habit, deleted with it).
    fn needs_redraw(&mut self, updates: usize) -> bool {
        #[cfg(test)]
        if self.suppress_update_redraw {
            return false;
        }
        updates > 0
    }
}

/// Scrolls the part named `id` into view: the nearest scrolling area
/// around it moves just enough, on both axes (`Rect::reveal`). A part the
/// window does not show, or that is in no scrolling area, scrolls nothing.
///
/// One Iced action with two walks of the window: [`Reveal`] finds the part
/// and its area with `places::Collect`, the walk the accessibility tree and
/// `locate` read, and its `finish` chains [`ScrollBy`], which counts the
/// scrolling areas to the same one and scrolls it. Iced runs a chained
/// operation at once, on the same window, before any message or rebuild,
/// so both walks meet the same areas in the same order and the move is
/// worked out from the layout it is applied to. Joined by `Task::then`
/// instead, the second walk would be a new action, and the window could
/// change between them.
pub(crate) fn scroll_into_view<T: Send + 'static>(id: &UiId) -> Task<T> {
    iced::advanced::widget::operate(Reveal {
        target: id.clone(),
        parts: crate::places::Collect::default(),
    })
    .discard()
}

/// Finds a part and its nearest scrolling area, then chains the scroll
/// that brings the part into view.
struct Reveal {
    /// The part to show.
    target: UiId,
    /// The walk that finds it.
    parts: crate::places::Collect,
}

impl Reveal {
    /// The scroll that shows the part, or `None` when there is nothing to
    /// move: the window does not show it, it is in no scrolling area, or
    /// it is in view already.
    fn scroll(&self) -> Option<ScrollBy> {
        let place = self.parts.places().of(&self.target)?;
        let area = place.area?;
        let by = area.bounds.reveal(place.rect);
        (by != loom_ui_core::Vector::new(0.0, 0.0)).then_some(ScrollBy {
            area: area.index,
            met: 0,
            by,
        })
    }
}

impl iced::advanced::widget::Operation<()> for Reveal {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn iced::advanced::widget::Operation<()>)) {
        let outside = self.parts.enter();
        operate(self);
        self.parts.leave(outside);
    }

    fn container(&mut self, id: Option<&iced::advanced::widget::Id>, bounds: iced::Rectangle) {
        iced::advanced::widget::Operation::<crate::places::Places>::container(&mut self.parts, id, bounds);
    }

    fn scrollable(
        &mut self,
        id: Option<&iced::advanced::widget::Id>,
        bounds: iced::Rectangle,
        content_bounds: iced::Rectangle,
        translation: iced::Vector,
        state: &mut dyn iced::advanced::widget::operation::Scrollable,
    ) {
        iced::advanced::widget::Operation::<crate::places::Places>::scrollable(
            &mut self.parts,
            id,
            bounds,
            content_bounds,
            translation,
            state,
        );
    }

    fn finish(&self) -> iced::advanced::widget::operation::Outcome<()> {
        use iced::advanced::widget::operation::Outcome;
        match self.scroll() {
            Some(scroll) => Outcome::Chain(Box::new(scroll)),
            None => Outcome::None,
        }
    }
}

/// Scrolls the scrolling area a walk meets in place `area` by `by`.
struct ScrollBy {
    /// Which area, in the order the walk meets them.
    area: usize,
    /// How many the walk has met so far.
    met: usize,
    /// How far, in logical pixels.
    by: loom_ui_core::Vector,
}

impl iced::advanced::widget::Operation<()> for ScrollBy {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn iced::advanced::widget::Operation<()>)) {
        operate(self);
    }

    fn scrollable(
        &mut self,
        _id: Option<&iced::advanced::widget::Id>,
        bounds: iced::Rectangle,
        content_bounds: iced::Rectangle,
        _translation: iced::Vector,
        state: &mut dyn iced::advanced::widget::operation::Scrollable,
    ) {
        if self.met == self.area {
            let by = iced::widget::scrollable::AbsoluteOffset {
                x: self.by.x,
                y: self.by.y,
            };
            state.scroll_by(by, bounds, content_bounds);
        }
        self.met += 1;
    }
}

#[cfg(any(test, feature = "test-ui"))]
pub(crate) mod headless;
mod measure;
pub(crate) mod offscreen;
pub(crate) mod paint;
pub(crate) mod pointers;
pub(crate) use measure::Measure;
#[cfg(any(test, feature = "test-ui"))]
pub(crate) use pointers::Pointers;
mod preedit;
mod services;
#[cfg(any(test, feature = "test-ui"))]
mod sleeper;
mod tasks;
#[cfg(test)]
mod tests;
mod window;

pub(crate) use services::AppServices;
/// The bridge the crate's tests run on: the `Ui` harness's, the only one
/// left since the app runs on the shell (`window.rs`).
#[cfg(test)]
pub(crate) type AppBridge = Bridge<headless::UiDriver>;
#[cfg(test)]
use services::choice;
pub(crate) use window::WindowLayers;
