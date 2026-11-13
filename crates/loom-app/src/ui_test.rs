//! Headless UI tests: real pointer and key events sent to the app view.
//!
//! The other app tests call `App::update` with messages. These go one step
//! earlier and check that Iced routes each event to the right widget. The
//! [`Ui`] harness does what the Iced runtime does for each frame: build a
//! `UserInterface` from `App::view`, send it events with a cursor position,
//! apply the messages it returns with `App::update`, and keep the widget
//! cache for the next frame, so the canvas remembers a drag in progress.
//!
//! The renderer is Iced's headless renderer. It uses wgpu when a GPU
//! adapter is found and falls back to tiny-skia otherwise, so these tests
//! run on CI machines without a GPU. Events do not depend on which one runs.
//! Set `ICED_TEST_BACKEND=tiny-skia` to force the fallback.
//!
//! Tests find things by name, not by pixel: [`Ui::output`], [`Ui::input`],
//! [`Ui::control`], and [`Ui::text`] return window coordinates from the
//! canvas bounds, its camera, and the scene. So a test keeps working when a
//! tile moves or the inspector gets wider.
//!
//! Tasks that `App::update` returns run as the runtime runs them: widget
//! operations (such as focusing the browser's search field) on the new
//! view, clipboard reads and writes on an in-memory clipboard, and exit as
//! [`Ui::exited`]. A message a task produces is applied like any other. A
//! task the harness cannot run, such as a window command or one that waits
//! on a timer or a file dialog, panics with a message, so a test never
//! passes because something was quietly skipped.
//!
//! Subscriptions do not run, except the two key listeners the app uses:
//! `keyboard::listen` and, while the browser is open, `browser_key`.
//!
//! Time is the wall clock, and steps take microseconds. So two clicks on
//! the same control or on empty canvas in one test count as a double-click
//! (a control resets, the browser opens). Click something else in between
//! when a test needs two single clicks.
//!
//! Not covered: what the operating system and winit deliver, for example
//! events while the pointer is outside the window.

use crate::{App, Args, Message, screenshot};
use iced::advanced::Clipboard;
use iced::advanced::clipboard::Kind;
use iced::advanced::renderer::{Headless, Style};
use iced::advanced::widget::Id;
use iced::advanced::widget::Operation;
use iced::advanced::widget::operation::Outcome;
use iced::{Event, Rectangle, Size, Task, keyboard, mouse, window};
use iced_runtime::Action;
use iced_runtime::futures::futures::{FutureExt, StreamExt};
use iced_runtime::user_interface::{Cache, UserInterface};
use loom_canvas::CanvasView;
use loom_canvas::iced_convert::{ToCore as _, ToIced as _};
use loom_ui_core::{Point, Rect};
// The harness tests reach these through `use super::*`.
#[cfg(test)]
use loom_core::ModuleId;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Instant;

/// The window size the app opens with.
pub(crate) const WINDOW: Size = crate::WINDOW;

/// The app view, driven by events the way the Iced runtime drives it.
pub struct Ui {
    pub(crate) app: App,
    renderer: iced::Renderer,
    cache: Option<Cache>,
    cursor: mouse::Cursor,
    /// The clipboard, shared by widgets and tasks. One text for both the
    /// standard and the primary clipboard.
    pub(crate) clipboard: Memory,
    /// True once a task asked the app to exit.
    pub(crate) exited: bool,
    /// The part with focus, as the harness last read it from the widgets:
    /// what the bridge's target holds in the app.
    focused: Option<loom_ui_core::UiId>,
}

/// An in-memory clipboard.
#[derive(Debug, Default)]
pub(crate) struct Memory(pub(crate) Option<String>);

impl Clipboard for Memory {
    fn read(&self, _kind: Kind) -> Option<String> {
        self.0.clone()
    }

    fn write(&mut self, _kind: Kind, contents: String) {
        self.0 = Some(contents);
    }
}

impl Ui {
    /// Opens a patch from `examples/patches` the way `--dump-ui` does: no
    /// preferences file, audio, MIDI, or recovery folder. For the tests in
    /// `tests/`. A test that needs dialogs or fake devices stays in the crate.
    pub fn headless(patch: &str) -> Result<Self, String> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/patches")
            .join(patch);
        let args = <Args as clap::Parser>::parse_from([PathBuf::from("loom-app"), path]);
        Ok(Self::with_app(crate::snapshot::headless(&args)?))
    }

    /// Presses and releases a key.
    ///
    /// Panics on a command shortcut. A test in `tests/` builds the crate without
    /// `cfg(test)`, so the fake dialogs and folders are off: Cmd+S would overwrite
    /// the example patch, Cmd+O would open a real dialog, and Cmd+Shift+O would
    /// make the real library folder. Test those with `Ui::open` in a unit test.
    pub fn tap_key(&mut self, key: keyboard::Key, modifiers: keyboard::Modifiers) {
        assert!(
            !modifiers.command(),
            "command shortcuts reach real files and dialogs from tests/: use Ui::open in a unit test"
        );
        self.tap(key, modifiers);
    }

    /// What the window shows, as `--dump-ui` prints it.
    pub fn snapshot_json(&self) -> serde_json::Value {
        serde_json::to_value(self.app.ui_snapshot()).expect("the snapshot is plain data")
    }

    /// Opens a patch from `examples/patches` with no audio, as the app does
    /// in a window of [`WINDOW`] size, and draws the first frame.
    pub(crate) fn open(patch: &str) -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/patches")
            .join(patch);
        let app = App::new(Args {
            patch: Some(path),
            generate: None,
            no_audio: true,
            fake_audio: false,
            record: None,
            bench: crate::bench::Options::default(),
            control: crate::control::Options::default(),
            output_safety: None,
            devices: crate::devices::Options::default(),
            view: screenshot::Options::default(),
            dump_ui: false,
            select: Vec::new(),
            show_help: false,
            frames: None,
            hold_note: false,
            recovery_dir: None,
            no_recovery: false,
            test_modules: false,
            dev: false,
        });
        Self::with_app(app)
    }

    /// Draws the first frame of an app built by the test, for example one
    /// that plays with `--fake-audio`.
    pub(crate) fn with_app(app: App) -> Self {
        // `ICED_TEST_BACKEND=tiny-skia` forces the renderer CI falls back
        // to without a GPU, as for Iced's own test crate.
        let backend = std::env::var("ICED_TEST_BACKEND").ok();
        let renderer = iced::futures::executor::block_on(<iced::Renderer as Headless>::new(
            iced::Font::DEFAULT,
            iced::Pixels(16.0),
            backend.as_deref(),
        ))
        .expect("Iced's headless renderer falls back to tiny-skia without a GPU");
        let mut ui = Self {
            app,
            renderer,
            cache: Some(Cache::default()),
            cursor: mouse::Cursor::Unavailable,
            clipboard: Memory::default(),
            exited: false,
            focused: None,
        };
        // The canvas frames the patch on its first redraw.
        ui.redraw();
        ui
    }

    /// One frame: the events go to the view at the current cursor, the
    /// messages they produce are applied, their tasks run, and the new view
    /// is drawn. Returns every message, including those tasks produced.
    pub(crate) fn step(&mut self, events: &[Event]) -> Vec<Message> {
        let mut messages = Vec::new();
        let listening = self.app.listening();
        let mut clipboard = std::mem::take(&mut self.clipboard);
        self.with_ui(|ui, renderer, cursor| {
            let (_, statuses) = ui.update(events, cursor, renderer, &mut clipboard, &mut messages);
            for (event, status) in events.iter().zip(statuses) {
                messages.extend(crate::input_owner::heard_messages(event, status, listening));
            }
        });
        self.clipboard = clipboard;
        let mut messages = self.apply(messages);
        // Something outside a turn may have asked where focus is, such as a
        // test replacing the patch directly. The bridge asks after its next
        // turn; the harness asks here.
        if self.app.take_focus_question() {
            let moved = self.focus_answer();
            messages.extend(self.deliver(moved));
        }
        let theme = self.app.theme();
        let style = Style {
            text_color: theme.palette().text,
        };
        self.with_ui(|ui, renderer, cursor| ui.draw(renderer, &theme, &style, cursor));
        messages
    }

    /// Applies messages as the runtime does, with what their tasks produce,
    /// and returns them all. An input message is logged as the messages it
    /// routes to, which are what a test looks for.
    fn apply(&mut self, messages: Vec<Message>) -> Vec<Message> {
        let mut log: Vec<Message> = messages
            .iter()
            .filter(|m| !matches!(m, Message::Input { .. }))
            .cloned()
            .collect();
        self.drain(messages.into(), &mut log);
        log
    }

    /// Runs a task as the runtime does, with the messages it produces and what
    /// their tasks produce in turn, and returns them all.
    pub(crate) fn run_task(&mut self, task: Task<Message>) -> Vec<Message> {
        let (mut queue, mut log) = (VecDeque::new(), Vec::new());
        self.run(task, &mut queue, &mut log);
        self.drain(queue, &mut log);
        log
    }

    /// Applies `message` as the runtime does, then every message it
    /// produces and what those produce in turn, and returns them all.
    pub(crate) fn deliver(&mut self, message: Message) -> Vec<Message> {
        let mut log = Vec::new();
        self.drain(VecDeque::from([message]), &mut log);
        log
    }

    /// Applies queued messages until none is left, through `App::update`, as
    /// the runtime does. Messages that tasks produce go to `log`, and so do the
    /// messages an input message routes to.
    fn drain(&mut self, mut queue: VecDeque<Message>, log: &mut Vec<Message>) {
        while let Some(message) = queue.pop_front() {
            if let Message::Input {
                event,
                status,
                listening,
            } = &message
            {
                // Logged for the test to read. `update` applies them all, and
                // only then do their tasks run, as in the app.
                log.extend(listening.route(event.clone(), *status));
            }
            let replies = self.turn(message, &mut queue, log);
            log.extend(replies.iter().cloned());
            queue.extend(replies);
        }
    }

    /// One message through `update`, as the runtime runs it: tasks from
    /// code not yet ported run as Iced's would, and effects as a driver
    /// carries them out. Messages from tasks go to `queue` and `log`; the
    /// messages effects send back (a dialog's answer, work's results) are
    /// returned, for the caller to apply.
    ///
    /// Work runs inline, the clipboard is the harness's own, and Quit marks
    /// it exited. A dialog is answered from the fake dialogs in a unit test;
    /// in a test under `tests/`, where they are off, it panics, naming it
    /// (ui-layer.md, section 2, rule 3), as an effect it cannot run does.
    fn turn(&mut self, message: Message, queue: &mut VecDeque<Message>, log: &mut Vec<Message>) -> Vec<Message> {
        let (sender, inbox) = std::sync::mpsc::channel();
        let waker = loom_ui::Waker::new(sender, std::sync::Arc::new(|| {}));
        let window = loom_ui::WindowState::new(loom_ui_core::Size::new(WINDOW.width, WINDOW.height));
        // The record takes a focus answer as it is applied, as the bridge's
        // target does just before it hands `FocusMoved` over.
        if let Message::FocusMoved(part) = &message {
            self.focused = part.clone();
        }
        let cx = loom_ui::Context::new(Instant::now(), &waker, self.focused.clone(), window);
        let effects = self.app.update(message, &cx);
        for task in self.app.outbox.take() {
            self.run(task, queue, log);
        }
        let mut moved_focus = false;
        for effect in effects {
            #[cfg(test)]
            let Some(effect) = crate::dialogs::fake::answer(&self.app, effect, &waker) else {
                continue;
            };
            match effect {
                loom_ui::Effect::Work(job) => job.run(&waker),
                loom_ui::Effect::ClipboardWrite(text) => self.clipboard.0 = Some(text),
                loom_ui::Effect::ClipboardRead(reply) => {
                    waker.send(reply.give(self.clipboard.0.clone()));
                }
                loom_ui::Effect::Quit => self.exited = true,
                loom_ui::Effect::Focus { target, select } => {
                    self.run(crate::iced_bridge::focus_moves(&target, select), queue, log);
                    moved_focus = true;
                }
                other => panic!("Ui: the harness cannot run the effect `{}`", other.name()),
            }
        }
        let mut replies: Vec<Message> = inbox.try_iter().collect();
        // As the bridge does: after a message that may have moved focus, or a
        // focus effect, ask the widgets. The answer comes after what is
        // already queued, which runs under the old focus.
        if self.app.take_focus_question() || moved_focus {
            replies.push(self.focus_answer());
        }
        replies
    }

    /// Reads focus from the widgets, as the bridge's focus question does,
    /// and gives the app's message for the answer, changed or not. The
    /// record takes it when the message is applied (`turn`).
    fn focus_answer(&mut self) -> Message {
        Message::FocusMoved(self.read_focus().part())
    }

    /// What the widgets say has focus now.
    fn read_focus(&mut self) -> crate::iced_bridge::FocusAnswer {
        let mut find = crate::iced_bridge::FindFocus::default();
        {
            let mut operation = iced::advanced::widget::operation::black_box(&mut find);
            self.with_ui(|ui, renderer, _| ui.operate(renderer, &mut operation));
        }
        find.0
    }

    /// Runs a task to the end. Messages it produces go to `queue` and to
    /// `log`. Widget operations run on a view built from the app as it is
    /// now, so they see what the update just changed.
    fn run(&mut self, task: Task<Message>, queue: &mut VecDeque<Message>, log: &mut Vec<Message>) {
        let Some(mut stream) = iced_runtime::task::into_stream(task) else {
            return;
        };
        loop {
            // A stream can return Pending once while its parts hand over,
            // for example after a clipboard reply. A few polls tell that
            // apart from a task that waits on the outside world.
            let polled = (0..8).find_map(|_| stream.next().now_or_never());
            let action = match polled {
                Some(Some(action)) => action,
                Some(None) => return,
                None => panic!("Ui: a task waits on something the harness cannot give, such as a timer or a dialog"),
            };
            match action {
                Action::Output(message) => {
                    log.push(message.clone());
                    queue.push_back(message);
                }
                Action::Widget(operation) => {
                    let mut next = Some(operation);
                    while let Some(mut operation) = next.take() {
                        self.with_ui(|ui, renderer, _| ui.operate(renderer, operation.as_mut()));
                        if let Outcome::Chain(chained) = operation.finish() {
                            next = Some(chained);
                        }
                    }
                }
                Action::Clipboard(iced_runtime::clipboard::Action::Read { channel, .. }) => {
                    let _ = channel.send(self.clipboard.0.clone());
                }
                Action::Clipboard(iced_runtime::clipboard::Action::Write { contents, .. }) => {
                    self.clipboard.0 = Some(contents);
                }
                Action::Exit => self.exited = true,
                other => panic!("Ui: the harness cannot run this task: {other:?}"),
            }
        }
    }

    /// Builds the view, runs `f` on it, and keeps its cache.
    pub(crate) fn with_ui(
        &mut self,
        f: impl FnOnce(&mut UserInterface<'_, Message, iced::Theme, iced::Renderer>, &mut iced::Renderer, mouse::Cursor),
    ) {
        let cache = self.cache.take().expect("the cache is put back after every frame");
        let mut ui = UserInterface::build(self.app.view(), WINDOW, cache, &mut self.renderer);
        f(&mut ui, &mut self.renderer, self.cursor);
        self.cache = Some(ui.into_cache());
    }

    /// A redraw, as the runtime sends before each frame. The canvas uses it
    /// to frame the patch and to report which lights are visible.
    pub(crate) fn redraw(&mut self) -> Vec<Message> {
        self.redraw_at(Instant::now())
    }

    /// A redraw at a chosen time, so a test can pass a timer's deadline
    /// without sleeping.
    pub(crate) fn redraw_at(&mut self, at: Instant) -> Vec<Message> {
        self.step(&[Event::Window(window::Event::RedrawRequested(at))])
    }

    /// Presses a key and holds it. With `repeat`, it is a repeat the
    /// keyboard sends while the key stays down.
    pub(crate) fn key_down(&mut self, key: keyboard::Key, repeat: bool) -> Vec<Message> {
        self.step(&[key_pressed(key, repeat)])
    }

    /// Lets go of a key.
    pub(crate) fn key_up(&mut self, key: keyboard::Key) -> Vec<Message> {
        self.step(&[key_released(key, keyboard::Modifiers::default())])
    }

    /// Clicks the first text widget that reads exactly `content`, such as a
    /// toolbar button's label.
    pub(crate) fn click_text(&mut self, content: &str) -> Vec<Message> {
        let at = self
            .text(content)
            .unwrap_or_else(|| panic!("no widget shows `{content}`"))
            .center();
        let mut messages = self.move_to(at);
        messages.extend(self.press());
        messages.extend(self.release());
        messages
    }

    /// Sends one message and runs its widget operations as the runtime does.
    /// A dialog's answer and the results of work are applied too, as the
    /// runtime would on the next turns; messages from tasks are not.
    pub(crate) fn send(&mut self, message: Message) {
        let replies = self.turn(message, &mut VecDeque::new(), &mut Vec::new());
        for reply in replies {
            self.send(reply);
        }
    }

    /// Gives keyboard focus to the text field of `part`, as a click on it
    /// would, and draws the frame. It sends no message, as a focus that a
    /// widget takes itself sends none: the app hears of it on the next turn
    /// that asks.
    pub(crate) fn focus(&mut self, part: &loom_ui_core::UiId) {
        let mut operation =
            iced::advanced::widget::operation::focusable::focus::<()>(crate::places::field_widget_id(part));
        self.with_ui(|ui, renderer, _| ui.operate(renderer, &mut operation));
        self.redraw();
    }

    /// True when a text field has keyboard focus, read from the widgets.
    pub(crate) fn text_field_focused(&mut self) -> bool {
        self.read_focus().any
    }

    /// The part whose text field has focus, read from the widgets.
    pub(crate) fn focused_field(&mut self) -> Option<loom_ui_core::UiId> {
        self.read_focus().part()
    }

    /// Clicks the inspector's value field for `module`'s parameter `index`,
    /// types `text` into it, and presses Enter, checking the draft on the
    /// way: the typed text is the draft until Enter commits it, and the
    /// field keeps focus after, as it does for a module's own parameters.
    pub(crate) fn type_a_value(&mut self, module: u32, index: usize, text: &str) {
        let at = self.value_field(module, index);
        self.move_to(at);
        self.press();
        self.release();
        self.typewrite(text);
        let draft = self.app.session.value_draft.as_ref().expect("a draft is typed");
        assert_eq!(draft.draft.text, text, "every key stays in the draft");
        self.tap(
            keyboard::Key::Named(keyboard::key::Named::Enter),
            keyboard::Modifiers::default(),
        );
        assert!(self.app.session.value_draft.is_none(), "Enter commits it");
        assert_eq!(
            self.app.focus,
            self.app.value_field_part(module, index),
            "the field keeps focus"
        );
    }

    /// Where the inspector's value field for this parameter is in the window.
    pub(crate) fn value_field(&mut self, module: u32, index: usize) -> Point {
        let part = self
            .app
            .value_field_part(module, index)
            .expect("the inspector shows that value field");
        self.field(&part)
    }

    /// Where the text field of `part` is in the window, to click on it.
    pub(crate) fn field(&mut self, part: &loom_ui_core::UiId) -> Point {
        self.field_by_id(crate::places::field_widget_id(part))
    }

    /// The centre of the focusable widget with this ID.
    fn field_by_id(&mut self, id: iced::advanced::widget::Id) -> Point {
        use iced::advanced::widget::operation::{Focusable, Outcome};
        use iced::advanced::widget::{Id, Operation};

        struct Find(Id, Option<Rectangle>);
        impl Operation for Find {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
                operate(self);
            }
            fn focusable(&mut self, id: Option<&Id>, bounds: Rectangle, _state: &mut dyn Focusable) {
                if id == Some(&self.0) {
                    self.1 = Some(bounds);
                }
            }
            fn finish(&self) -> Outcome<()> {
                Outcome::None
            }
        }
        let mut find = Find(id, None);
        self.with_ui(|ui, renderer, _| ui.operate(renderer, &mut find));
        find.1.expect("a field with that ID is on screen").center().to_core()
    }

    /// Moves the pointer to a window point.
    pub(crate) fn move_to(&mut self, p: Point) -> Vec<Message> {
        let position = p.to_iced();
        self.cursor = mouse::Cursor::Available(position);
        self.step(&[Event::Mouse(mouse::Event::CursorMoved { position })])
    }

    /// Presses the left button where the pointer is.
    pub(crate) fn press(&mut self) -> Vec<Message> {
        self.step(&[Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))])
    }

    /// Releases the left button where the pointer is.
    pub(crate) fn release(&mut self) -> Vec<Message> {
        self.step(&[Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))])
    }

    /// Presses at `from`, moves to `to` in `steps` even moves, and releases
    /// there. Returns every message along the way.
    pub(crate) fn drag(&mut self, from: Point, to: Point, steps: usize) -> Vec<Message> {
        let mut messages = self.move_to(from);
        messages.extend(self.press());
        for i in 1..=steps {
            let t = i as f32 / steps as f32;
            messages.extend(self.move_to(Point::new(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t)));
        }
        messages.extend(self.release());
        messages
    }

    /// Presses and releases a key with modifiers held.
    pub(crate) fn tap(&mut self, key: keyboard::Key, modifiers: keyboard::Modifiers) -> Vec<Message> {
        let mut messages = self.step(&[key_press(key.clone(), modifiers, None, false)]);
        messages.extend(self.step(&[key_released(key, modifiers)]));
        messages
    }

    /// Types text, one key press and release per character, as a keyboard
    /// does. Only the widget with focus takes it.
    pub(crate) fn typewrite(&mut self, text: &str) -> Vec<Message> {
        let mut messages = Vec::new();
        let none = keyboard::Modifiers::default();
        for c in text.chars() {
            let typed = c.to_string();
            let key = keyboard::Key::Character(typed.as_str().into());
            messages.extend(self.step(&[key_press(key.clone(), none, Some(&typed), false)]));
            messages.extend(self.step(&[key_released(key, none)]));
        }
        messages
    }

    /// Where the canvas is in the window, and its camera.
    pub(crate) fn canvas(&mut self) -> CanvasView {
        let mut view = None;
        self.with_ui(|ui, renderer, _| view = screenshot::canvas_view(ui, renderer));
        view.expect("the app view has a patch canvas")
    }

    /// Where the window drew the canvas and each named part, as the
    /// accessibility layer and the control channel's `locate` are told.
    pub(crate) fn places(&mut self) -> crate::places::Places {
        let mut collect = crate::places::Collect::default();
        self.with_ui(|ui, renderer, _| {
            ui.operate(
                renderer,
                &mut iced::advanced::widget::operation::black_box(&mut collect),
            );
        });
        collect.places()
    }

    /// The centre of a module's output port, by its stable name.
    pub(crate) fn output(&mut self, module: u32, name: &str) -> Point {
        self.port(module, name, true)
    }

    /// The centre of a module's input port, by its stable name.
    pub(crate) fn input(&mut self, module: u32, name: &str) -> Point {
        self.port(module, name, false)
    }

    /// The centre of a port on any tile: a module's, a sub-patch tile's, or
    /// a port tile's one port, by name and direction.
    fn port(&mut self, module: u32, name: &str, output: bool) -> Point {
        let prefix = if output {
            crate::targets::OUTPUT_PREFIX
        } else {
            crate::targets::INPUT_PREFIX
        };
        let part = crate::targets::find(
            self.app.session.controller.graph(),
            &self.app.session.hierarchy,
            module,
            &format!("{prefix}{name}"),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let crate::targets::Part::Port(port) = part else {
            panic!("module {module}: `{name}` is not a port");
        };
        let world = self
            .app
            .session
            .scene
            .port_position(port)
            .expect("the scene has every port in the graph");
        self.canvas().to_window(world)
    }

    /// The centre of the panel control bound to a parameter, by the
    /// parameter's stable name.
    pub(crate) fn control(&mut self, module: u32, param: &str) -> Point {
        let rect = self.control_bounds(module, param);
        self.canvas().to_window(rect.center())
    }

    /// The centre of a knob's dial and the radius halfway across its range
    /// ring, in window coordinates, by the parameter's stable name. On a
    /// modulated knob a press on that circle drags depth (modulation.md,
    /// Arc drag).
    pub(crate) fn range_ring(&mut self, module: u32, param: &str) -> (Point, f32) {
        let rect = self.control_bounds(module, param);
        let (centre, radius) = loom_canvas::knob_range_ring(rect);
        let canvas = self.canvas();
        (canvas.to_window(centre), radius * canvas.camera.zoom)
    }

    /// The cell of the panel control bound to a parameter, in world
    /// coordinates.
    pub(crate) fn control_bounds(&mut self, module: u32, param: &str) -> Rect {
        let target = screenshot::Named {
            module,
            name: param.to_string(),
        };
        let (_, widget) = screenshot::find_control(&self.app.session.controller, &self.app.session.hierarchy, &target)
            .unwrap_or_else(|e| panic!("{e}"));
        self.app
            .session
            .scene
            .module(module)
            .and_then(|m| m.widget_bounds(widget))
            .expect("the scene has every panel widget")
    }

    /// Holds these modifier keys, or none, for the steps that follow, as
    /// the runtime reports a change in the held modifiers.
    pub(crate) fn hold(&mut self, modifiers: keyboard::Modifiers) -> Vec<Message> {
        self.step(&[Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers))])
    }

    /// The current translation of a named scrollable widget.
    pub(crate) fn scroll_translation(&mut self, name: &'static str) -> Option<iced::Vector> {
        struct Find {
            id: Id,
            translation: Option<iced::Vector>,
        }
        impl Operation for Find {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
                if self.translation.is_none() {
                    operate(self);
                }
            }
            fn scrollable(
                &mut self,
                id: Option<&Id>,
                _bounds: Rectangle,
                _content_bounds: Rectangle,
                translation: iced::Vector,
                _state: &mut dyn iced::advanced::widget::operation::Scrollable,
            ) {
                if id == Some(&self.id) {
                    self.translation = Some(translation);
                }
            }
        }
        let mut find = Find {
            id: Id::new(name),
            translation: None,
        };
        self.with_ui(|ui, renderer, _| ui.operate(renderer, &mut find));
        find.translation
    }

    /// The bounds of the first text widget that reads exactly `content`,
    /// for example an inspector row label. `None` if no widget shows it.
    pub(crate) fn text(&mut self, content: &str) -> Option<Rect> {
        struct Find<'a>(&'a str, Option<Rectangle>);
        impl Operation for Find<'_> {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
                if self.1.is_none() {
                    operate(self);
                }
            }
            fn text(&mut self, _id: Option<&Id>, bounds: Rectangle, text: &str) {
                if self.1.is_none() && text == self.0 {
                    self.1 = Some(bounds);
                }
            }
            fn finish(&self) -> Outcome<()> {
                Outcome::None
            }
        }
        let mut find = Find(content, None);
        self.with_ui(|ui, renderer, _| ui.operate(renderer, &mut find));
        find.1.map(|bounds| bounds.to_core())
    }
}

/// A key press with no modifiers and no text, as the keyboard sends it.
pub(crate) fn key_pressed(key: keyboard::Key, repeat: bool) -> Event {
    key_press(key, keyboard::Modifiers::default(), None, repeat)
}

/// A key press with `modifiers` held, carrying `text` when the key types
/// a character.
fn key_press(key: keyboard::Key, modifiers: keyboard::Modifiers, text: Option<&str>, repeat: bool) -> Event {
    Event::Keyboard(keyboard::Event::KeyPressed {
        key: key.clone(),
        modified_key: key,
        physical_key: keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
        location: keyboard::Location::Standard,
        modifiers,
        text: text.map(Into::into),
        repeat,
    })
}

/// A key release with `modifiers` held.
pub(crate) fn key_released(key: keyboard::Key, modifiers: keyboard::Modifiers) -> Event {
    Event::Keyboard(keyboard::Event::KeyReleased {
        key: key.clone(),
        modified_key: key,
        physical_key: keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
        location: keyboard::Location::Standard,
        modifiers,
    })
}

#[cfg(test)]
mod arrow_tests;
#[cfg(test)]
mod tests;
