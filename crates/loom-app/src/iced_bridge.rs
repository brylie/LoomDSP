//! The bridge to Iced, until the `winit` shell replaces it at plan step
//! 2.2 (docs/dev/leaving-iced-plan.md, step 2.1, item 2).
//!
//! Iced still owns the event loop and draws every view, but the app no
//! longer runs inside Iced's own runtime. It runs inside ours,
//! `loom_ui::Runtime`, and Iced is only its driver and its routing target:
//!
//! - a message from an Iced view goes into the runtime's queue, and the
//!   runtime runs one turn: one `update` per message, each with its own
//!   effects right after it (ui-layer.md, section 2);
//! - the effects that reach outside the app come back here, to [`loom_ui::Driver`]
//!   and [`loom_ui::Target`], which turn them into Iced tasks;
//! - a message sent from another thread through the runtime's waker wakes
//!   Iced through one subscription, and the next turn takes it.
//!
//! This is the only module that may name Iced's `Task` once a file is
//! ported (`tests/purity.rs`). Files not yet ported push theirs into the
//! [`Outbox`], which the bridge drains after each turn; the outbox goes
//! when the last of them is ported.

use crate::{App, Message};
use iced::futures::channel::mpsc;
use iced::futures::{Stream, StreamExt};
use iced::{Element, Subscription, Task, Theme};
use loom_ui::effect::{
    Choice, FileAction, FileRequest, FocusTarget, Image, Job, Level, NoGpu, Question, Reply, Selection,
};
use loom_ui::{Context, Deadline, DriverEvent, Effects, Runtime, Waker, WindowSpec, WindowState};
use loom_ui_core::UiId;
use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// What Iced sends the bridge. Nearly every value is an app message, so
/// boxing it to shrink the two empty wake-ups would allocate for every
/// message instead.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum Bridged {
    /// A message from a view or a subscription.
    App(Message),
    /// A waker sent a message from another thread.
    Woken,
    /// The app's deadline has come.
    Deadline,
}

/// Tasks from code not yet ported, drained by the bridge after each turn.
/// Interior, so a view-time read of the app never needs `&mut`.
#[derive(Default)]
pub(crate) struct Outbox(RefCell<Vec<Task<Message>>>);

impl Outbox {
    /// Queues `task` for Iced to run after this turn.
    pub(crate) fn push(&self, task: Task<Message>) {
        if task.units() > 0 {
            self.0.borrow_mut().push(task);
        }
    }

    /// Takes every task queued so far.
    pub(crate) fn take(&self) -> Vec<Task<Message>> {
        std::mem::take(&mut *self.0.borrow_mut())
    }

    /// How many tasks wait.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.0.borrow().len()
    }
}

impl std::fmt::Debug for Outbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Outbox({} tasks)", self.0.borrow().len())
    }
}

/// The app as `loom_ui::Runtime` runs it. A wrapper, so the app's own
/// `view` (the Iced views) and the trait's `view` (panel models, phase 3)
/// never share a name on one type. It goes at 2.2, when the app
/// implements the trait itself.
pub(crate) struct Hosted(pub(crate) App);

impl loom_ui::App for Hosted {
    type Message = Message;

    fn update(&mut self, message: Message, cx: &Context<'_, Message>) -> Effects<Message> {
        self.0.update(message, cx)
    }

    /// No panels yet: Iced draws every view until phase 3.
    fn view(&self, _view: &mut loom_ui::View) {}

    fn window(&self) -> WindowSpec {
        WindowSpec {
            title: self.0.title(),
            min_size: None,
        }
    }

    fn next_deadline(&self) -> Option<Deadline<Message>> {
        self.0.next_deadline()
    }
}

/// Iced's state: our runtime, holding the app.
pub(crate) struct Bridge {
    runtime: Runtime<Hosted, IcedTarget>,
    /// Set while the bridge itself puts a message from Iced into the
    /// queue: that turn runs at once, so it needs no wake-up.
    quiet: Arc<AtomicBool>,
    wakes: WakeSource,
}

impl Bridge {
    /// The bridge around `app`, and the tasks to run first.
    pub(crate) fn start(app: App, first: Task<Message>) -> (Self, Task<Bridged>) {
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
        let mut bridge = Self {
            runtime: Runtime::new(Hosted(app), IcedTarget::default(), window, wake),
            quiet,
            wakes: WakeSource(Arc::new(Mutex::new(Some(receiver)))),
        };
        // The first turn builds the view and reads the first deadline, such
        // as the crash recovery question (autosave.md, Crash recovery).
        let task = bridge.turn(DriverEvent::Woken);
        (bridge, Task::batch([first.map(Bridged::App), task]))
    }

    /// Iced's update: one turn of our runtime.
    pub(crate) fn update(&mut self, message: Bridged) -> Task<Bridged> {
        match message {
            Bridged::App(message) => {
                self.quiet.store(true, Ordering::SeqCst);
                self.runtime.waker().send(message);
                self.quiet.store(false, Ordering::SeqCst);
                self.turn(DriverEvent::Woken)
            }
            Bridged::Woken => self.turn(DriverEvent::Woken),
            Bridged::Deadline => self.turn(DriverEvent::DeadlineReached),
        }
    }

    /// Runs one turn and returns what Iced must do for it.
    fn turn(&mut self, event: DriverEvent) -> Task<Bridged> {
        let mut driver = IcedDriver::default();
        let now = Instant::now();
        let next = self.runtime.handle(event, now, &mut driver);
        let mut tasks = driver.tasks;
        tasks.extend(self.runtime.target_mut().take_tasks());
        tasks.extend(
            self.runtime
                .app()
                .0
                .outbox
                .take()
                .into_iter()
                .map(|t| t.map(Bridged::App)),
        );
        // A deadline already due runs next. Later deadlines wait for item 3,
        // when timers become deadlines.
        if next.deadline.is_some_and(|at| at <= now) {
            tasks.push(Task::done(Bridged::Deadline));
        }
        Task::batch(tasks)
    }

    /// The app, to read.
    pub(crate) fn app(&self) -> &App {
        &self.runtime.app().0
    }

    /// Iced's view: the app's views, with their messages wrapped.
    pub(crate) fn view(&self) -> Element<'_, Bridged> {
        self.app().view().map(Bridged::App)
    }

    /// The window title.
    pub(crate) fn title(&self) -> String {
        self.app().title()
    }

    /// The theme.
    pub(crate) fn theme(&self) -> Theme {
        self.app().theme()
    }

    /// The app's subscriptions, and the wake-ups from wakers.
    pub(crate) fn subscription(&self) -> Subscription<Bridged> {
        Subscription::batch([
            self.app().subscription().map(Bridged::App),
            Subscription::run_with(self.wakes.clone(), wake_stream).map(|()| Bridged::Woken),
        ])
    }
}

/// The receiving end of the wake-ups, handed to Iced's subscription once.
#[derive(Clone)]
struct WakeSource(Arc<Mutex<Option<mpsc::UnboundedReceiver<()>>>>);

impl Hash for WakeSource {
    /// One wake source per window, so one subscription.
    fn hash<H: Hasher>(&self, state: &mut H) {
        "loom-bridge-wake".hash(state);
    }
}

/// The wake-ups as a stream. Iced builds it once while its key stays the
/// same; a second build finds the receiver taken and gives nothing.
fn wake_stream(source: &WakeSource) -> impl Stream<Item = ()> + use<> {
    let receiver = source.0.lock().ok().and_then(|mut slot| slot.take());
    iced::futures::stream::iter(receiver).flatten()
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
}

impl IcedTarget {
    /// This turn's tasks for Iced, the scrolls last and in order.
    fn take_tasks(&mut self) -> Vec<Task<Bridged>> {
        let mut tasks = std::mem::take(&mut self.tasks);
        tasks.extend(self.scrolls.take());
        tasks
    }
}

impl loom_ui::Target<Message> for IcedTarget {
    /// Iced routes its own events to its widgets.
    fn input(&mut self, _: &loom_ui_core::Event, _: Option<loom_ui_core::Point>, _: Instant, _: &mut Vec<Message>) {}

    fn focus(&mut self, target: &FocusTarget, select: Selection) {
        let task: Task<Bridged> = match target {
            FocusTarget::Element(id) => {
                let widget = widget_id(id);
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
            FocusTarget::Nothing => iced::advanced::widget::operate(
                iced::advanced::widget::operation::focusable::focus(iced::advanced::widget::Id::unique()),
            ),
        };
        self.tasks.push(task);
    }

    fn scroll_into_view(&mut self, id: &UiId) {
        let next = scroll_into_view(id);
        self.scrolls = Some(match self.scrolls.take() {
            Some(before) => before.chain(next),
            None => next,
        });
    }

    /// Iced keeps focus inside its widgets. The focus pull request reads
    /// it back here; until then the app asks Iced itself.
    fn focused(&self) -> Option<UiId> {
        None
    }

    /// Iced draws after every update, as it always has (section 2, rule
    /// 10: the bridge's habit, deleted with it).
    fn needs_redraw(&mut self, updates: usize) -> bool {
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

/// The Iced widget ID for an element's UI ID: its text, as the parts the
/// toolkit draws are named (ui-layer.md, section 7).
pub(crate) fn widget_id(id: &UiId) -> iced::advanced::widget::Id {
    iced::advanced::widget::Id::from(id.to_string())
}

/// The driver: the effects that reach outside the app, as Iced tasks or as
/// calls Iced cannot make for us.
#[derive(Default)]
struct IcedDriver {
    tasks: Vec<Task<Bridged>>,
}

impl loom_ui::Driver<Message> for IcedDriver {
    /// Iced reads the title from the app (`Bridge::title`).
    fn window(&mut self, _spec: &WindowSpec) {}

    fn clipboard_write(&mut self, text: String) {
        self.tasks.push(iced::clipboard::write(text));
    }

    fn clipboard_read(&mut self, reply: Reply<Option<String>, Message>, _waker: &Waker<Message>) {
        let mut reply = Some(reply);
        self.tasks
            .push(iced::clipboard::read().then(move |text| match reply.take() {
                Some(reply) => Task::done(Bridged::App(reply.give(text))),
                None => Task::none(),
            }));
    }

    /// Shows the dialog now. It still blocks, as before, but outside
    /// `update`; the answer comes on the next turn. The `winit` shell makes
    /// it stop blocking at 2.2.
    fn file_dialog(&mut self, request: FileRequest, reply: Reply<Option<PathBuf>, Message>, waker: &Waker<Message>) {
        waker.send(reply.give(show_file_dialog(&request)));
    }

    fn ask(&mut self, question: Question, reply: Reply<Choice, Message>, waker: &Waker<Message>) {
        waker.send(reply.give(show_question(&question)));
    }

    /// Captures the window as Iced last drew it. With no window open, the
    /// answer says there is nothing to draw with.
    fn capture(&mut self, reply: Reply<Result<Image, NoGpu>, Message>, _waker: &Waker<Message>) {
        let mut reply = Some(reply);
        self.tasks.push(
            iced::window::latest()
                .then(|id| match id {
                    Some(id) => iced::window::screenshot(id).map(Some),
                    None => Task::done(None),
                })
                .then(move |shot| {
                    let image = shot
                        .and_then(|shot| {
                            Image::new(shot.size.width, shot.size.height, shot.scale_factor, shot.rgba.to_vec())
                        })
                        .ok_or(NoGpu);
                    match reply.take() {
                        Some(reply) => Task::done(Bridged::App(reply.give(image))),
                        None => Task::none(),
                    }
                }),
        );
    }

    fn work(&mut self, job: Job<Message>, waker: &Waker<Message>) {
        let waker = waker.clone();
        std::thread::spawn(move || job.run(&waker));
    }

    fn exit(&mut self) {
        self.tasks.push(iced::exit());
    }
}

/// Shows a native file dialog for `request` and waits for it.
fn show_file_dialog(request: &FileRequest) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().set_title(&request.title);
    for filter in &request.filters {
        dialog = dialog.add_filter(&filter.name, &filter.extensions);
    }
    if let Some(folder) = &request.folder {
        dialog = dialog.set_directory(folder);
    }
    if let Some(name) = &request.file_name {
        dialog = dialog.set_file_name(name);
    }
    match request.action {
        FileAction::Open => dialog.pick_file(),
        FileAction::Save => dialog.save_file(),
    }
}

/// Shows a native question and waits for it. The answer is the index of
/// the button pressed, or `None` when it closed with none.
fn show_question(question: &Question) -> Choice {
    use rfd::{MessageButtons, MessageDialog, MessageLevel};
    debug_assert!(
        (1..=3).contains(&question.buttons.len()),
        "a native question has one to three buttons: {:?}",
        question.buttons
    );
    let buttons = match question.buttons.as_slice() {
        [] => MessageButtons::Ok,
        [ok] => MessageButtons::OkCustom(ok.clone()),
        [ok, cancel] => MessageButtons::OkCancelCustom(ok.clone(), cancel.clone()),
        [yes, no, cancel, ..] => MessageButtons::YesNoCancelCustom(yes.clone(), no.clone(), cancel.clone()),
    };
    let result = MessageDialog::new()
        .set_level(match question.level {
            Level::Info => MessageLevel::Info,
            Level::Warning => MessageLevel::Warning,
            Level::Error => MessageLevel::Error,
        })
        .set_title(&question.title)
        .set_description(&question.text)
        .set_buttons(buttons)
        .show();
    choice(&question.buttons, result)
}

/// The button a dialog result names, by its label, or by its place where
/// the platform reports a standard button instead.
fn choice(buttons: &[String], result: rfd::MessageDialogResult) -> Choice {
    use rfd::MessageDialogResult;
    match result {
        MessageDialogResult::Custom(label) => buttons.iter().position(|b| *b == label),
        MessageDialogResult::Ok | MessageDialogResult::Yes => (!buttons.is_empty()).then_some(0),
        MessageDialogResult::No => (buttons.len() > 2).then_some(1),
        MessageDialogResult::Cancel => None,
    }
}

#[cfg(test)]
mod tests;
