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
use iced::{Event, Point, Rectangle, Size, Task, keyboard, mouse, window};
use iced_runtime::Action;
use iced_runtime::futures::futures::{FutureExt, StreamExt};
use iced_runtime::user_interface::{Cache, UserInterface};
use loom_canvas::{CanvasView, PortRef};
use loom_core::ModuleId;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Instant;

/// The window size the app opens with.
pub const WINDOW: Size = crate::WINDOW;

/// The app view, driven by events the way the Iced runtime drives it.
pub struct Ui {
    pub app: App,
    renderer: iced::Renderer,
    cache: Option<Cache>,
    cursor: mouse::Cursor,
    /// The clipboard, shared by widgets and tasks. One text for both the
    /// standard and the primary clipboard.
    pub clipboard: Memory,
    /// True once a task asked the app to exit.
    pub exited: bool,
}

/// An in-memory clipboard.
#[derive(Debug, Default)]
pub struct Memory(pub Option<String>);

impl Clipboard for Memory {
    fn read(&self, _kind: Kind) -> Option<String> {
        self.0.clone()
    }

    fn write(&mut self, _kind: Kind, contents: String) {
        self.0 = Some(contents);
    }
}

impl Ui {
    /// Opens a patch from `examples/patches` with no audio, as the app does
    /// in a window of [`WINDOW`] size, and draws the first frame.
    pub fn open(patch: &str) -> Self {
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
    pub fn with_app(app: App) -> Self {
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
        };
        // The canvas frames the patch on its first redraw.
        ui.redraw();
        ui
    }

    /// One frame: the events go to the view at the current cursor, the
    /// messages they produce are applied, their tasks run, and the new view
    /// is drawn. Returns every message, including those tasks produced.
    pub fn step(&mut self, events: &[Event]) -> Vec<Message> {
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
        let messages = self.apply(messages);
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
            let task = self.app.update(message);
            self.run(task, &mut queue, log);
        }
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
    pub fn redraw(&mut self) -> Vec<Message> {
        self.step(&[Event::Window(window::Event::RedrawRequested(Instant::now()))])
    }

    /// Sends one message and runs its widget operations as the runtime does.
    pub(crate) fn send(&mut self, message: Message) {
        let task = self.app.update(message);
        self.run(task, &mut VecDeque::new(), &mut Vec::new());
    }

    /// Gives keyboard focus to the widget with this ID, as a click on it
    /// would, and draws the frame.
    pub fn focus(&mut self, id: &'static str) {
        let mut operation =
            iced::advanced::widget::operation::focusable::focus::<()>(iced::advanced::widget::Id::new(id));
        self.with_ui(|ui, renderer, _| ui.operate(renderer, &mut operation));
        self.redraw();
    }

    /// True when a named text field has keyboard focus, read from the
    /// widgets. `ui.focus` moves focus with no message, so `App::text_focus`
    /// would not know.
    pub fn text_field_focused(&mut self) -> bool {
        self.focused_field().is_some()
    }

    /// The ID of the focused field, if it has one, read as a focus step
    /// reads it.
    pub fn focused_field(&mut self) -> Option<iced::advanced::widget::Id> {
        use iced::advanced::widget::operation::{self, Operation, Outcome};
        let mut find = operation::focusable::find_focused();
        {
            let mut operation = operation::black_box(&mut find);
            self.with_ui(|ui, renderer, _| ui.operate(renderer, &mut operation));
        }
        match Operation::finish(&find) {
            Outcome::Some(id) => Some(id),
            _ => None,
        }
    }

    /// Where the inspector's value field for this parameter is in the window.
    pub fn value_field(&mut self, module: u32, index: usize) -> Point {
        self.field_by_id(crate::controls::value_field_id(module, index))
    }

    /// Where the text field with this ID is in the window, to click on it.
    pub fn field(&mut self, id: &'static str) -> Point {
        self.field_by_id(iced::advanced::widget::Id::new(id))
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
        find.1.expect("a field with that ID is on screen").center()
    }

    /// Moves the pointer to a window point.
    pub fn move_to(&mut self, p: Point) -> Vec<Message> {
        self.cursor = mouse::Cursor::Available(p);
        self.step(&[Event::Mouse(mouse::Event::CursorMoved { position: p })])
    }

    /// Presses the left button where the pointer is.
    pub fn press(&mut self) -> Vec<Message> {
        self.step(&[Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))])
    }

    /// Releases the left button where the pointer is.
    pub fn release(&mut self) -> Vec<Message> {
        self.step(&[Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))])
    }

    /// Presses at `from`, moves to `to` in `steps` even moves, and releases
    /// there. Returns every message along the way.
    pub fn drag(&mut self, from: Point, to: Point, steps: usize) -> Vec<Message> {
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
    pub fn tap(&mut self, key: keyboard::Key, modifiers: keyboard::Modifiers) -> Vec<Message> {
        let physical = keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified);
        let mut messages = self.step(&[Event::Keyboard(keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key.clone(),
            physical_key: physical,
            location: keyboard::Location::Standard,
            modifiers,
            text: None,
            repeat: false,
        })]);
        messages.extend(self.step(&[Event::Keyboard(keyboard::Event::KeyReleased {
            key: key.clone(),
            modified_key: key,
            physical_key: physical,
            location: keyboard::Location::Standard,
            modifiers,
        })]));
        messages
    }

    /// Types text, one key press and release per character, as a keyboard
    /// does. Only the widget with focus takes it.
    pub fn typewrite(&mut self, text: &str) -> Vec<Message> {
        let mut messages = Vec::new();
        for c in text.chars() {
            let key = keyboard::Key::Character(c.to_string().into());
            let physical = keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified);
            messages.extend(self.step(&[Event::Keyboard(keyboard::Event::KeyPressed {
                key: key.clone(),
                modified_key: key.clone(),
                physical_key: physical,
                location: keyboard::Location::Standard,
                modifiers: keyboard::Modifiers::default(),
                text: Some(c.to_string().into()),
                repeat: false,
            })]));
            messages.extend(self.step(&[Event::Keyboard(keyboard::Event::KeyReleased {
                key: key.clone(),
                modified_key: key,
                physical_key: physical,
                location: keyboard::Location::Standard,
                modifiers: keyboard::Modifiers::default(),
            })]));
        }
        messages
    }

    /// Where the canvas is in the window, and its camera.
    pub fn canvas(&mut self) -> CanvasView {
        let mut view = None;
        self.with_ui(|ui, renderer, _| view = screenshot::canvas_view(ui, renderer));
        view.expect("the app view has a patch canvas")
    }

    /// The centre of a module's output port, by its stable name.
    pub fn output(&mut self, module: u32, name: &str) -> Point {
        self.port(module, name, true)
    }

    /// The centre of a module's input port, by its stable name.
    pub fn input(&mut self, module: u32, name: &str) -> Point {
        self.port(module, name, false)
    }

    fn port(&mut self, module: u32, name: &str, output: bool) -> Point {
        let node = self
            .app
            .controller
            .graph()
            .node(ModuleId(module))
            .expect("module in the patch");
        let count = if output {
            node.output_count()
        } else {
            node.input_count()
        };
        let index = (0..count)
            .find(|&index| if output { node.output_name(index) } else { node.input_name(index) } == Some(name))
            .unwrap_or_else(|| panic!("module {module} has no port `{name}`"));
        let world = self
            .app
            .scene
            .port_position(PortRef { module, output, index })
            .expect("the scene has every port in the graph");
        self.canvas().to_window(world)
    }

    /// The centre of the panel control bound to a parameter, by the
    /// parameter's stable name.
    pub fn control(&mut self, module: u32, param: &str) -> Point {
        let rect = self.control_bounds(module, param);
        self.canvas().to_window(rect.center())
    }

    /// The centre of a knob's dial and the radius halfway across its range
    /// ring, in window coordinates, by the parameter's stable name. On a
    /// modulated knob a press on that circle drags depth (modulation.md,
    /// Arc drag).
    pub fn range_ring(&mut self, module: u32, param: &str) -> (Point, f32) {
        let rect = self.control_bounds(module, param);
        let (centre, radius) = loom_canvas::knob_range_ring(rect);
        let canvas = self.canvas();
        (canvas.to_window(centre), radius * canvas.camera.zoom)
    }

    /// The cell of the panel control bound to a parameter, in world
    /// coordinates.
    pub fn control_bounds(&mut self, module: u32, param: &str) -> Rectangle {
        let target = screenshot::Named {
            module,
            name: param.to_string(),
        };
        let (_, widget) = screenshot::find_control(&self.app.controller, &target).unwrap_or_else(|e| panic!("{e}"));
        self.app
            .scene
            .module(module)
            .and_then(|m| m.widget_bounds(widget))
            .expect("the scene has every panel widget")
    }

    /// Holds these modifier keys, or none, for the steps that follow, as
    /// the runtime reports a change in the held modifiers.
    pub fn hold(&mut self, modifiers: keyboard::Modifiers) -> Vec<Message> {
        self.step(&[Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers))])
    }

    /// The current translation of a named scrollable widget.
    pub fn scroll_translation(&mut self, name: &'static str) -> Option<iced::Vector> {
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
    pub fn text(&mut self, content: &str) -> Option<Rectangle> {
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
        find.1
    }
}

mod tests {
    use super::*;
    use loom_canvas::CanvasEvent;
    use loom_core::{InputRef, OutputRef};

    /// Selects one module and finds a parameter label in its inspector.
    fn inspector_row(ui: &mut Ui, module: u32, label: &str) -> Point {
        let _ = ui.app.update(Message::SelectModule(module));
        ui.redraw();
        ui.text(label).expect("inspector parameter row").center()
    }

    /// A drag into the inspector uses the normal assignment defaults and
    /// records one undo step; the same output cannot add a duplicate.
    #[test]
    fn an_output_dropped_on_an_inspector_row_assigns_once() {
        let mut ui = Ui::open("subtractive.ron");
        let row = inspector_row(&mut ui, 4, "Cutoff");
        let from = ui.output(7, "out");
        let before = ui.app.controller.graph().assignments().count();
        ui.drag(from, row, 12);
        let assignments: Vec<_> = ui.app.controller.graph().assignments().map(|(_, a)| *a).collect();
        assert_eq!(assignments.len(), before + 1);
        let added = assignments.last().expect("new assignment");
        assert_eq!(added.depth, 0.25);
        assert_eq!(added.polarity, None);
        assert_eq!(
            added.source,
            OutputRef {
                module: ModuleId(7),
                port: 0
            }
        );
        assert!(added.enabled);
        ui.drag(from, row, 12);
        assert_eq!(ui.app.controller.graph().assignments().count(), before + 1);
        assert!(ui.app.alerts.status().contains("already modulates"));
        let _ = ui.app.update(Message::Undo);
        assert_eq!(ui.app.controller.graph().assignments().count(), before);
    }

    /// Moving between parameter rows changes the one highlighted target.
    #[test]
    fn only_the_inspector_row_under_an_output_drag_is_highlighted() {
        let mut ui = Ui::open("subtractive.ron");
        let cutoff = inspector_row(&mut ui, 4, "Cutoff");
        let resonance = ui.text("Resonance").expect("Resonance row").center();
        let from = ui.output(7, "out");
        ui.move_to(from);
        ui.press();
        ui.move_to(cutoff);
        assert_eq!(
            ui.app.gestures.assign_drag().expect("window drag").zoom,
            ui.canvas().camera.zoom
        );
        assert_eq!(ui.app.gestures.drop_row(), Some((4, 0)));
        ui.move_to(resonance);
        assert_eq!(ui.app.gestures.drop_row(), Some((4, 1)));
        ui.release();
    }

    /// A whole-number toggle starts at one step of depth, as on its tile.
    #[test]
    fn an_output_dropped_on_a_toggle_row_uses_one_step() {
        let mut ui = Ui::open("subtractive.ron");
        let row = inspector_row(&mut ui, 10, "Run");
        let from = ui.output(7, "out");
        ui.drag(from, row, 12);
        let (_, assignment) = ui.app.controller.graph().assignments().last().expect("assignment");
        assert_eq!(assignment.target, ModuleId(10));
        assert_eq!(assignment.depth, 1.0);
    }

    /// The Button's latch is button-only on the tile, so its inspector row
    /// does not highlight or take an output drop.
    #[test]
    fn a_button_only_parameter_row_rejects_a_drop() {
        let mut ui = Ui::open("subtractive.ron");
        let button = select_new(&mut ui, "loom.ui.button");
        let row = inspector_row(&mut ui, button.0, "On");
        let from = ui.output(7, "out");
        let before = ui.app.controller.graph().assignments().count();
        ui.move_to(from);
        ui.press();
        ui.move_to(row);
        assert_eq!(ui.app.gestures.drop_row(), None);
        ui.release();
        assert_eq!(ui.app.controller.graph().assignments().count(), before);
    }

    /// A normal parameter with no tile widget still accepts a row drop.
    #[test]
    fn an_inspector_only_parameter_accepts_a_drop() {
        let mut ui = Ui::open("subtractive.ron");
        let button = select_new(&mut ui, "loom.ui.button");
        let row = inspector_row(&mut ui, button.0, "Mode");
        let from = ui.output(7, "out");
        let before = ui.app.controller.graph().assignments().count();
        ui.drag(from, row, 12);
        assert_eq!(ui.app.controller.graph().assignments().count(), before + 1);
        let (_, assignment) = ui.app.controller.graph().assignments().last().expect("assignment");
        assert_eq!(assignment.target, button);
    }

    /// Escape and focus loss clear the drag before any release can assign.
    #[test]
    fn escape_and_focus_loss_cancel_an_inspector_drop() {
        for focus_loss in [false, true] {
            let mut ui = Ui::open("subtractive.ron");
            let row = inspector_row(&mut ui, 4, "Cutoff");
            let from = ui.output(7, "out");
            let before = ui.app.controller.graph().assignments().count();
            ui.move_to(from);
            ui.press();
            ui.move_to(row);
            if focus_loss {
                ui.step(&[Event::Window(window::Event::Unfocused)]);
            } else {
                ui.tap(
                    keyboard::Key::Named(keyboard::key::Named::Escape),
                    keyboard::Modifiers::default(),
                );
            }
            ui.release();
            assert_eq!(ui.app.controller.graph().assignments().count(), before);
            assert!(ui.app.gestures.assign_drag().is_none());
        }
    }

    /// Empty canvas, an outside-window release, and MIDI learn all discard
    /// the output without making an assignment.
    #[test]
    fn empty_outside_and_learn_mode_make_no_inspector_assignment() {
        let mut ui = Ui::open("subtractive.ron");
        let row = inspector_row(&mut ui, 4, "Cutoff");
        let from = ui.output(7, "out");
        let before = ui.app.controller.graph().assignments().count();
        let canvas = ui.canvas();
        ui.drag(from, Point::new(canvas.bounds.x + 4.0, canvas.bounds.y + 4.0), 8);
        assert_eq!(ui.app.controller.graph().assignments().count(), before);
        ui.move_to(from);
        ui.press();
        ui.move_to(row);
        ui.cursor = mouse::Cursor::Unavailable;
        ui.step(&[Event::Mouse(mouse::Event::CursorLeft)]);
        ui.move_to(Point::new(WINDOW.width + 20.0, WINDOW.height + 20.0));
        assert!(ui.app.gestures.assign_drag().is_some_and(|drag| drag.at.is_none()));
        ui.release();
        assert_eq!(ui.app.controller.graph().assignments().count(), before);
        let _ = ui.app.update(Message::Learn(crate::learn::Message::Toggle));
        ui.drag(from, row, 8);
        assert_eq!(ui.app.controller.graph().assignments().count(), before);
        assert_eq!(ui.app.gestures.drop_row(), None);
    }

    fn connected(ui: &Ui, from: (u32, usize), to: (u32, usize)) -> bool {
        let output = OutputRef {
            module: ModuleId(from.0),
            port: from.1,
        };
        let input = InputRef {
            module: ModuleId(to.0),
            port: to.1,
        };
        ui.app.controller.graph().cables().any(|cable| cable == (output, input))
    }

    /// A press on an output port, moves onto an input port, and a release
    /// there make a cable, through the canvas's own event handling.
    #[test]
    fn dragging_from_an_output_to_an_input_makes_a_cable() {
        let mut ui = Ui::open("subtractive.ron");
        let from = ui.output(8, "out");
        let to = ui.input(6, "b");
        assert!(!connected(&ui, (8, 0), (6, 1)));
        let cables = ui.app.controller.graph().cables().count();

        ui.drag(from, to, 12);

        assert!(
            connected(&ui, (8, 0), (6, 1)),
            "the drag made a cable from VCA out to the second multiplier input"
        );
        assert_eq!(ui.app.controller.graph().cables().count(), cables + 1);
    }

    /// The window-wide drag line snaps to an input under the pointer, then
    /// follows the pointer again when it leaves that input.
    #[test]
    fn an_output_drag_line_snaps_to_a_hovered_input() {
        let mut ui = Ui::open("subtractive.ron");
        let from = ui.output(8, "out");
        let input = ui.input(6, "b");
        ui.move_to(from);
        ui.press();
        ui.move_to(Point::new(input.x + 3.0, input.y + 2.0));
        assert_eq!(ui.app.gestures.assign_drag().expect("output drag").at, Some(input));
        let empty = Point::new(1000.0, 800.0);
        ui.move_to(empty);
        assert_eq!(ui.app.gestures.assign_drag().expect("output drag").at, Some(empty));
        ui.release();
    }

    /// panels.md, acceptance: a transport-following input shows the clock
    /// mark with no cable, loses it when a cable is connected, and has it
    /// back when the cable is removed. Cables are made and pulled out by
    /// pointer, through the canvas.
    #[test]
    fn a_clock_mark_shows_only_while_the_input_follows_the_transport() {
        let mut ui = Ui::open("modulation.ron");
        // The LFO's phase input defaults to the bar phase; its other ports,
        // and the filter's inputs, follow nothing.
        let mark = |ui: &Ui| {
            let snapshot = ui.app.ui_snapshot();
            let lfo = snapshot.tiles.iter().find(|t| t.id == 5).expect("the LFO");
            lfo.inputs[0].follows_transport
        };
        let canvas_mark = |ui: &Ui| ui.app.scene.module(5).and_then(|m| m.clock_mark_centre(0));
        assert_eq!(mark(&ui), Some("bar_phase"));
        assert!(canvas_mark(&ui).is_some());
        let filter = ui.app.ui_snapshot();
        let filter = filter.tiles.iter().find(|t| t.id == 4).expect("the filter");
        assert!(filter.inputs.iter().all(|p| p.follows_transport.is_none()));

        let from = ui.output(2, "phase");
        let to = ui.input(5, "phase");
        ui.drag(from, to, 12);
        assert!(connected(&ui, (2, 0), (5, 0)), "the drag made a cable");
        assert_eq!(mark(&ui), None);
        assert_eq!(canvas_mark(&ui), None);

        // Pull the cable out of the input and drop it on empty canvas.
        let away = Point::new(to.x - 60.0, to.y - 80.0);
        ui.drag(to, away, 12);
        assert!(!connected(&ui, (2, 0), (5, 0)), "the cable is gone");
        assert_eq!(mark(&ui), Some("bar_phase"));
        assert!(canvas_mark(&ui).is_some());
    }

    /// Turning a knob on a tile builds the panel controls again, but never
    /// the tiles, ports, and cables (ADR-0010). The counters are this
    /// thread's, so other tests running at the same time do not count.
    #[test]
    fn turning_a_knob_builds_only_the_controls() {
        use loom_canvas::stats::RenderCounts;
        let mut ui = Ui::open("subtractive.ron");
        let cutoff = |ui: &Ui| ui.app.controller.graph().node(ModuleId(4)).expect("filter").params[0];
        let before_value = cutoff(&ui);
        let before = RenderCounts::this_thread();

        let knob = ui.control(4, "cutoff");
        ui.drag(knob, Point::new(knob.x, knob.y - 40.0), 8);

        let after = RenderCounts::this_thread();
        assert_ne!(cutoff(&ui), before_value, "the drag turned the knob");
        assert!(after.control_builds > before.control_builds, "{before:?} -> {after:?}");
        assert_eq!(after.scene_builds, before.scene_builds, "{before:?} -> {after:?}");
    }

    /// The Mode list opens and draws over the tile in the real view. Arrow
    /// keys change its highlight, and Enter commits the chosen mode.
    #[test]
    fn envelope_mode_popup_draws_and_accepts_keyboard_choice() {
        let mut ui = Ui::open("subtractive.ron");
        let mode = ui.control(5, "mode");
        ui.move_to(mode);
        ui.press();
        ui.release();

        let key = |name| keyboard::Key::Named(name);
        ui.tap(key(keyboard::key::Named::ArrowDown), keyboard::Modifiers::default());
        let current = ui.app.controller.graph().node(ModuleId(5)).expect("filter envelope");
        let index = current.descriptor.param_index("mode").expect("Mode parameter");
        assert_eq!(current.params[index], 0.0, "arrows highlight without committing");
        ui.tap(key(keyboard::key::Named::Enter), keyboard::Modifiers::default());

        let node = ui.app.controller.graph().node(ModuleId(5)).expect("filter envelope");
        assert_eq!(node.params[index], 1.0);
    }

    /// Ctrl+Z (Cmd+Z on macOS) reaches the app through the canvas and
    /// undoes the cable the drag made.
    #[test]
    fn undo_key_removes_a_dragged_cable() {
        let mut ui = Ui::open("subtractive.ron");
        let from = ui.output(8, "out");
        let to = ui.input(6, "b");
        ui.drag(from, to, 12);
        assert!(connected(&ui, (8, 0), (6, 1)));

        ui.tap(keyboard::Key::Character("z".into()), keyboard::Modifiers::COMMAND);

        assert!(!connected(&ui, (8, 0), (6, 1)), "undo removed the cable");
    }

    /// A over empty canvas opens the browser. Its focus task gives the
    /// search field focus, so typed text goes there, and Enter adds the
    /// first match.
    #[test]
    fn the_browser_search_field_takes_typing() {
        let mut ui = Ui::open("subtractive.ron");
        let modules = ui.app.controller.graph().nodes().count();
        let canvas = ui.canvas();
        ui.move_to(Point::new(canvas.bounds.x + 8.0, canvas.bounds.y + 8.0));
        ui.tap(keyboard::Key::Character("a".into()), keyboard::Modifiers::default());
        assert!(ui.app.browser.is_some(), "A opens the browser");

        ui.typewrite("svf");
        ui.tap(
            keyboard::Key::Named(keyboard::key::Named::Enter),
            keyboard::Modifiers::default(),
        );

        assert!(ui.app.browser.is_none(), "Enter adds the match and closes the browser");
        assert_eq!(ui.app.controller.graph().nodes().count(), modules + 1);
    }

    /// Copy writes the selected module to the clipboard through a task, and
    /// paste reads it back through another.
    #[test]
    fn copy_and_paste_go_through_the_clipboard() {
        let mut ui = Ui::open("subtractive.ron");
        let modules = ui.app.controller.graph().nodes().count();
        let _ = ui.app.update(Message::Canvas(CanvasEvent::Select(vec![4])));
        let canvas = ui.canvas();
        ui.move_to(Point::new(canvas.bounds.x + 8.0, canvas.bounds.y + 8.0));

        ui.tap(keyboard::Key::Character("c".into()), keyboard::Modifiers::COMMAND);
        assert!(ui.clipboard.0.is_some(), "copy wrote the clipboard");
        ui.tap(keyboard::Key::Character("v".into()), keyboard::Modifiers::COMMAND);

        assert_eq!(ui.app.controller.graph().nodes().count(), modules + 1);
    }

    /// The same drag released on empty canvas makes no cable.
    #[test]
    fn dragging_onto_empty_canvas_makes_no_cable() {
        let mut ui = Ui::open("subtractive.ron");
        let from = ui.output(8, "out");
        let cables = ui.app.controller.graph().cables().count();
        let canvas = ui.canvas();
        let empty = Point::new(canvas.bounds.x + 4.0, canvas.bounds.y + 4.0);

        ui.drag(from, empty, 12);

        assert_eq!(ui.app.controller.graph().cables().count(), cables);
    }

    #[test]
    fn finds_controls_and_text_by_name() {
        let mut ui = Ui::open("subtractive.ron");
        let canvas = ui.canvas();
        assert!(canvas.bounds.width > 0.0 && canvas.bounds.height > 0.0);
        let knob = ui.control(4, "cutoff");
        assert!(canvas.bounds.contains(knob), "{knob:?} is on the canvas {canvas:?}");
        assert!(ui.text("no widget shows this").is_none());

        // A click on the filter's title selects it, and the inspector then
        // shows its parameter rows.
        let title = {
            let module = ui.app.scene.module(4).expect("filter in the scene");
            let b = module.bounds();
            Point::new(b.x + b.width * 0.5, b.y + 6.0)
        };
        let title = ui.canvas().to_window(title);
        ui.move_to(title);
        ui.press();
        ui.release();
        assert_eq!(ui.app.selection, [4].into());
        let row = ui.text("Cutoff").expect("the inspector shows the Cutoff row");
        assert!(!canvas.bounds.intersects(&row), "the row {row:?} is beside the canvas");
    }

    /// Adds a module of `kind`, selects it, and draws the inspector.
    fn select_new(ui: &mut Ui, kind: &str) -> ModuleId {
        let id = ui.app.controller.add_module(kind).expect("a built-in module");
        ui.app.commit();
        let _ = ui.app.update(Message::SelectModule(id.0));
        ui.redraw();
        id
    }

    /// Dragging the grip previews without rebuilding and commits one undo step.
    #[test]
    fn resizing_scope_with_pointer_and_keyboard() {
        let mut ui = Ui::open("modulation.ron");
        let scope = select_new(&mut ui, "loom.display.scope");
        let target = ui.app.controller.add_module("loom.math.mul").unwrap();
        ui.app.controller.connect_named(scope, "out", target, "a").unwrap();
        ui.app.commit();
        ui.app.positions.insert(scope.0, Point::new(300.0, 100.0));
        ui.app.refresh();
        ui.app.edits.history.clear();
        ui.redraw();
        let tile = ui.app.scene.module(scope.0).unwrap();
        let bounds = tile.bounds();
        let original = (tile.width(), tile.height());
        let output_before = tile.output_position(0);
        let generation = ui.app.scene.generation();
        let canvas = ui.canvas();
        let from = canvas.to_window(Point::new(
            bounds.x + bounds.width - 3.0,
            bounds.y + bounds.height - 3.0,
        ));
        let to = canvas.to_window(Point::new(
            bounds.x + bounds.width + 2.0 * loom_canvas::GRID - 3.0,
            bounds.y + bounds.height + 3.0 * loom_canvas::GRID - 3.0,
        ));
        ui.move_to(from);
        ui.press();
        ui.move_to(to);
        assert_eq!(
            ui.app.scene.generation(),
            generation,
            "preview does not rebuild the scene"
        );
        ui.release();
        let resized = ui.app.scene.module(scope.0).unwrap();
        assert_eq!(
            (resized.width(), resized.height()),
            (
                original.0 + 2.0 * loom_canvas::GRID,
                original.1 + 3.0 * loom_canvas::GRID
            )
        );
        assert_ne!(ui.app.scene.generation(), generation);
        let output_after = resized.output_position(0);
        assert!(output_after.x > output_before.x && output_after.y > output_before.y);
        assert!(
            matches!(ui.app.scene.hit(output_after, 4.0), loom_canvas::Hit::Port(port) if port.module == scope.0 && port.output)
        );
        assert_eq!(
            ui.app.scene.port_position(loom_canvas::PortRef {
                module: scope.0,
                index: 0,
                output: true
            }),
            Some(output_after)
        );
        let saved = ui.app.current_patch().to_ron().unwrap();
        let loaded = loom_patch::Patch::from_ron(&saved).unwrap();
        assert_eq!(
            loaded.modules.iter().find(|module| module.id == scope.0).unwrap().size,
            ui.app.sizes.get(&scope.0).copied()
        );
        ui.app.undo();
        let undone = ui.app.scene.module(scope.0).unwrap();
        assert_eq!((undone.width(), undone.height()), original);
        assert!(ui.app.edits.history.undo_step().is_none());
        ui.app.redo();
        assert_eq!(
            ui.app.scene.module(scope.0).unwrap().width(),
            original.0 + 2.0 * loom_canvas::GRID
        );
        ui.tap(
            keyboard::Key::Named(keyboard::key::Named::ArrowRight),
            keyboard::Modifiers::ALT,
        );
        assert_eq!(
            ui.app.scene.module(scope.0).unwrap().width(),
            original.0 + 3.0 * loom_canvas::GRID
        );
        ui.app.undo();
        assert_eq!(
            ui.app.scene.module(scope.0).unwrap().width(),
            original.0 + 2.0 * loom_canvas::GRID
        );
    }

    /// The inspector applies a placeholder label as one reversible patch edit.
    #[test]
    fn placeholder_label_is_editable_and_undoable() {
        let mut ui = Ui::open("subtractive.ron");
        let missing = ui
            .app
            .controller
            .add_unknown(loom_core::UnknownModule {
                kind: "future.effect@1".into(),
                label: None,
                params: Default::default(),
                ranges: Default::default(),
                inputs: vec![],
                outputs: vec![],
                parameters: vec![],
            })
            .unwrap();
        ui.app.commit();
        ui.app.edits.history.clear();
        let label = inspector_row(&mut ui, missing.0, "Label");
        ui.move_to(Point::new(label.x + 25.0, label.y + 28.0));
        ui.press();
        ui.release();
        ui.typewrite("My effect");
        assert_eq!(
            ui.app
                .current_patch()
                .modules
                .iter()
                .find(|m| m.id == missing.0)
                .unwrap()
                .label,
            None
        );
        assert!(ui.app.edits.history.undo_step().is_none());
        ui.tap(
            keyboard::Key::Named(keyboard::key::Named::Enter),
            keyboard::Modifiers::default(),
        );
        assert_eq!(
            ui.app
                .current_patch()
                .modules
                .iter()
                .find(|m| m.id == missing.0)
                .unwrap()
                .label
                .as_deref(),
            Some("My effect")
        );
        let copied = ui.app.copy().unwrap();
        assert!(copied.contains("My effect"));
        ui.app.undo();
        assert_eq!(
            ui.app
                .current_patch()
                .modules
                .iter()
                .find(|m| m.id == missing.0)
                .unwrap()
                .label,
            None
        );
        assert!(ui.app.edits.history.undo_step().is_none());
        ui.app.redo();
        assert_eq!(
            ui.app
                .current_patch()
                .modules
                .iter()
                .find(|m| m.id == missing.0)
                .unwrap()
                .label
                .as_deref(),
            Some("My effect")
        );
        ui.redraw();
        let label = ui.text("Label").unwrap().center();
        ui.move_to(Point::new(label.x + 25.0, label.y + 28.0));
        ui.press();
        ui.release();
        ui.tap(
            keyboard::Key::Named(keyboard::key::Named::End),
            keyboard::Modifiers::default(),
        );
        for _ in 0..9 {
            ui.tap(
                keyboard::Key::Named(keyboard::key::Named::Backspace),
                keyboard::Modifiers::default(),
            );
        }
        ui.tap(
            keyboard::Key::Named(keyboard::key::Named::Enter),
            keyboard::Modifiers::default(),
        );
        assert_eq!(
            ui.app
                .current_patch()
                .modules
                .iter()
                .find(|m| m.id == missing.0)
                .unwrap()
                .label,
            None
        );
        ui.app.undo();
        assert_eq!(
            ui.app
                .current_patch()
                .modules
                .iter()
                .find(|m| m.id == missing.0)
                .unwrap()
                .label
                .as_deref(),
            Some("My effect")
        );
    }

    /// Opening and saving placeholders preserves absent and fractional positions.
    #[test]
    fn unknown_positions_round_trip_until_moved() {
        let path = std::env::temp_dir().join(format!("loom-unknown-positions-{}.ron", std::process::id()));
        std::fs::write(
            &path,
            r#"Patch(version: 1, name: "Missing", modules: [
            (id: 1, kind: "future.one@1"),
            (id: 2, kind: "future.two@1", position: Some((17.25, 45.5))),
            (id: 3, kind: "loom.clock.transport@1")
        ])"#,
        )
        .unwrap();
        let mut ui = Ui::open(path.to_str().unwrap());
        let patch = ui.app.current_patch();
        assert_eq!(patch.modules.iter().find(|m| m.id == 1).unwrap().position, None);
        assert_eq!(
            patch.modules.iter().find(|m| m.id == 2).unwrap().position,
            Some((17.25, 45.5))
        );
        ui.app.canvas_event(loom_canvas::CanvasEvent::Move {
            ids: vec![1],
            delta: iced::Vector::new(12.0, 0.0),
        });
        assert!(
            ui.app
                .current_patch()
                .modules
                .iter()
                .find(|m| m.id == 1)
                .unwrap()
                .position
                .is_some()
        );
        ui.app.undo();
        assert_eq!(
            ui.app
                .current_patch()
                .modules
                .iter()
                .find(|m| m.id == 1)
                .unwrap()
                .position,
            None
        );
        let _ = std::fs::remove_file(path);
    }

    /// Real cable drags onto and from a placeholder are refused without an undo step.
    #[test]
    fn dragging_cables_to_unknown_ports_is_refused() {
        let path = std::env::temp_dir().join(format!("loom-unknown-drag-{}.ron", std::process::id()));
        std::fs::write(
            &path,
            r#"Patch(version: 1, name: "Missing", modules: [
            (id: 1, kind: "future.effect@1", position: Some((480.0, 100.0))),
            (id: 2, kind: "loom.phase.generator@1", position: Some((100.0, 100.0))),
            (id: 3, kind: "loom.clock.transport@1", position: Some((100.0, 300.0)))
        ], cables: [(from: (2, "phase"), to: (1, "in"))],
        modulation: [(source: (1, "out"), target: (2, "tune"), depth: 0.5)])"#,
        )
        .unwrap();
        let mut ui = Ui::open(path.to_str().unwrap());
        ui.app
            .controller
            .disconnect(loom_core::InputRef {
                module: ModuleId(1),
                port: 0,
            })
            .unwrap();
        ui.app.commit();
        ui.app.edits.history.clear();
        ui.redraw();
        let from = ui.output(2, "phase");
        let to = ui.input(1, "in");
        ui.drag(from, to, 8);
        assert_eq!(ui.app.alerts.status(), "Cannot connect to an unknown module");
        assert_eq!(ui.app.controller.graph().cables().count(), 0);
        let from = ui.output(1, "out");
        let to = ui.input(2, "pitch");
        ui.drag(from, to, 8);
        assert_eq!(ui.app.alerts.status(), "Cannot connect to an unknown module");
        assert_eq!(ui.app.controller.graph().cables().count(), 0);
        assert!(ui.app.edits.history.undo_step().is_none());
        let _ = std::fs::remove_file(path);
    }

    /// panels.md, Choice names: the inspector shows Button's mode by name,
    /// and a click on the second radio button below the row picks
    /// "Momentary". The row's value text follows.
    #[test]
    fn a_click_on_a_choice_name_in_the_inspector_picks_it() {
        let mut ui = Ui::open("subtractive.ron");
        let button = select_new(&mut ui, "loom.ui.button");
        let mode = ui
            .app
            .controller
            .graph()
            .node(button)
            .unwrap()
            .descriptor
            .param_index("mode")
            .unwrap();
        assert!(ui.text("Toggle").is_some(), "the value is the name");
        let label = ui.text("Mode").expect("the Mode row");
        // The radio buttons sit under the row. Walk down until a click
        // lands on the second one.
        let mut picked = false;
        for step in 0..30 {
            let at = Point::new(label.x + 6.0, label.y + label.height + 2.0 + step as f32 * 2.0);
            ui.move_to(at);
            ui.press();
            ui.release();
            if ui.app.controller.graph().node(button).unwrap().params[mode] == 1.0 {
                picked = true;
                break;
            }
        }
        assert!(picked, "a radio button under the Mode row picks Momentary");
        assert!(ui.text("Momentary").is_some(), "the row's value follows");
    }

    /// More than four names get a pick list, and a parameter with ranges a
    /// Range selector. Both draw, and the rows keep their labels and values.
    #[test]
    fn the_inspector_draws_pick_lists_and_range_selectors() {
        let mut ui = Ui::open("subtractive.ron");
        select_new(&mut ui, "loom.mod.lfo");
        assert!(ui.text("Shape").is_some());
        assert!(ui.text("Sine").is_some(), "the shape's value is its name");
        let mul = select_new(&mut ui, "loom.math.mul");
        assert!(ui.text("Gain").is_some());
        let _ = ui.app.update(Message::Range {
            module: mul.0,
            index: 0,
            range: 0,
        });
        ui.redraw();
        let crate::inspector::Inspector::Module(rows) = ui.app.inspector_model() else {
            panic!("a module row");
        };
        assert_eq!(rows.params[0].value, "0.100", "clamped into -0.1 to 0.1");
        assert_eq!(
            rows.params[0].field.as_ref().map(|field| field.text.as_str()),
            Some("0.100"),
            "the field shows the value"
        );
    }

    /// A typed value outside the selected control range selects the smallest
    /// range that holds it, and one commit is still one undo step
    /// (panel-widgets.md, Typing a value).
    #[test]
    fn a_typed_value_above_the_control_range_selects_a_wider_one() {
        let mut ui = Ui::open("subtractive.ron");
        let mul = select_new(&mut ui, "loom.math.mul");
        let _ = ui.app.update(Message::Range {
            module: mul.0,
            index: 0,
            range: 0,
        });
        let module = mul.0;
        let selected = |ui: &Ui| ui.app.controller.graph().node(mul).unwrap().spec(0).unwrap();
        assert_eq!(selected(&ui).max, 0.1);
        let steps = ui.app.edits.history.len();
        let _ = ui.app.update(Message::ValueText {
            module,
            index: 0,
            text: "0.5".to_string(),
        });
        let _ = ui.app.update(Message::ValueSubmit { module, index: 0 });
        assert_eq!(ui.app.controller.graph().node(mul).unwrap().params[0], 0.5);
        assert!(selected(&ui).max >= 0.5, "the range widened");
        assert_eq!(ui.app.edits.history.len(), steps + 1, "one undo step");
    }

    /// Subtractive with the filter envelope (module 5) modulating the
    /// filter cutoff (module 4) at the default depth. Returns that depth.
    fn modulated_cutoff() -> (Ui, f32) {
        let mut ui = Ui::open("subtractive.ron");
        let cutoff = ui
            .app
            .controller
            .graph()
            .node(ModuleId(4))
            .expect("filter")
            .descriptor
            .param_index("cutoff")
            .expect("cutoff");
        let _ = ui.app.update(Message::Assign {
            source: OutputRef {
                module: ModuleId(5),
                port: 0,
            },
            module: 4,
            index: cutoff,
        });
        ui.redraw();
        let depth = env_depth(&ui);
        (ui, depth)
    }

    /// The depth of the one assignment from the filter envelope.
    fn env_depth(ui: &Ui) -> f32 {
        let source = OutputRef {
            module: ModuleId(5),
            port: 0,
        };
        ui.app
            .controller
            .graph()
            .assignments()
            .find(|(_, a)| a.source == source)
            .expect("the envelope modulates the cutoff")
            .1
            .depth
    }

    /// A press on the ring of a modulated knob, at its side and at its top
    /// edge above the knob's cell, drags depth: up raises it. A press on
    /// the knob's body turns the knob and leaves the depth (#217).
    #[test]
    fn the_range_ring_takes_a_depth_drag_at_its_side_and_top() {
        let (mut ui, start) = modulated_cutoff();
        let cutoff_value = |ui: &Ui| ui.app.controller.graph().node(ModuleId(4)).expect("filter").params[0];
        let (centre, radius) = ui.range_ring(4, "cutoff");
        let cell = {
            let rect = ui.control_bounds(4, "cutoff");
            ui.canvas().to_window(Point::new(rect.x, rect.y))
        };

        // 20 pixels up is a tenth of the range, as on the knob.
        let side = Point::new(centre.x + radius, centre.y);
        ui.drag(side, Point::new(side.x, side.y - 20.0), 4);
        let after_side = env_depth(&ui);
        assert!((after_side - (start + 0.1)).abs() < 1e-4, "{start} -> {after_side}");

        let top = Point::new(centre.x, centre.y - radius);
        assert!(
            top.y < cell.y,
            "the top of the ring {top:?} is above the cell at {cell:?}"
        );
        ui.drag(top, Point::new(top.x, top.y - 20.0), 4);
        let after_top = env_depth(&ui);
        assert!(
            (after_top - (after_side + 0.1)).abs() < 1e-4,
            "{after_side} -> {after_top}"
        );

        let before = cutoff_value(&ui);
        ui.drag(centre, Point::new(centre.x, centre.y - 20.0), 4);
        assert_ne!(cutoff_value(&ui), before, "the body press turned the knob");
        assert_eq!(env_depth(&ui), after_top, "the body press left the depth");
    }

    /// With Shift held, the same drag changes the depth by the knob's fine
    /// ratio of a plain drag: a tenth (#217).
    #[test]
    fn shift_gives_fine_depth_steps() {
        let (mut ui, start) = modulated_cutoff();
        let (centre, radius) = ui.range_ring(4, "cutoff");
        let side = Point::new(centre.x + radius, centre.y);
        let up = Point::new(side.x, side.y - 20.0);

        ui.drag(side, up, 4);
        let plain = env_depth(&ui) - start;
        ui.hold(keyboard::Modifiers::SHIFT);
        ui.drag(side, up, 4);
        ui.hold(keyboard::Modifiers::default());
        let fine = env_depth(&ui) - start - plain;

        assert!((plain - 0.1).abs() < 1e-4, "{plain}");
        assert!((fine - plain * 0.1).abs() < 1e-5, "plain {plain}, fine {fine}");
    }

    /// A long drag stops at depth 1 going up and at -1 going down (#217).
    #[test]
    fn a_long_depth_drag_clamps_at_one() {
        let (mut ui, _) = modulated_cutoff();
        let (centre, radius) = ui.range_ring(4, "cutoff");
        let side = Point::new(centre.x + radius, centre.y);

        // 200 pixels is the whole range of the knob, so 600 crosses it.
        ui.drag(side, Point::new(side.x, side.y - 600.0), 30);
        assert_eq!(env_depth(&ui), 1.0);
        ui.drag(side, Point::new(side.x, side.y + 600.0), 30);
        assert_eq!(env_depth(&ui), -1.0);
    }

    /// One arc drag is one undo step: Ctrl or Cmd+Z brings back the depth
    /// from before it (#217).
    #[test]
    fn one_depth_drag_is_one_undo_step() {
        let (mut ui, start) = modulated_cutoff();
        let steps = ui.app.edits.history.len();
        let (centre, radius) = ui.range_ring(4, "cutoff");
        let side = Point::new(centre.x + radius, centre.y);

        ui.drag(side, Point::new(side.x, side.y - 40.0), 8);
        assert!((env_depth(&ui) - (start + 0.2)).abs() < 1e-4, "{}", env_depth(&ui));
        assert_eq!(ui.app.edits.history.len(), steps + 1, "the drag is one step");

        ui.tap(keyboard::Key::Character("z".into()), keyboard::Modifiers::COMMAND);
        assert_eq!(env_depth(&ui), start);
    }

    /// Opening the module browser and closing it keeps the canvas's camera.
    /// Iced keeps widget state by place in the tree, and `App::view` always
    /// wraps the window in a `stack!` so the canvas keeps its place when the
    /// browser layer is added (#226, ADR-0021).
    #[test]
    fn the_camera_survives_opening_the_browser() {
        let mut ui = Ui::open("subtractive.ron");
        let canvas = ui.canvas();
        let empty = Point::new(canvas.bounds.x + 8.0, canvas.bounds.y + 8.0);
        ui.move_to(empty);
        // A trackpad pans, and a mouse wheel zooms.
        ui.step(&[Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Pixels { x: 37.0, y: -23.0 },
        })]);
        ui.step(&[Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 },
        })]);
        let moved = ui.canvas().camera;
        assert_ne!(moved, canvas.camera, "the scroll moved the camera");

        ui.tap(keyboard::Key::Character("a".into()), keyboard::Modifiers::default());
        assert!(ui.app.browser.is_some(), "A opens the browser");
        assert_eq!(ui.canvas().camera, moved, "with the browser open");
        ui.tap(
            keyboard::Key::Named(keyboard::key::Named::Escape),
            keyboard::Modifiers::default(),
        );
        assert!(ui.app.browser.is_none(), "Escape closes the browser");

        assert_eq!(ui.canvas().camera, moved, "after the browser closed");
    }

    /// The example patches open at a zoom where port labels and captions
    /// are drawn, and F brings that zoom back after zooming out (#455).
    /// Labels draw from 7.5 / 9 of natural size (panels.md, Level of
    /// detail).
    #[test]
    fn examples_open_and_frame_with_labels_shown() {
        let readable = 7.5 / 9.0 - 1e-4;
        for patch in ["subtractive.ron", "modulation.ron"] {
            let mut ui = Ui::open(patch);
            assert!(
                ui.canvas().camera.zoom >= readable,
                "{patch} opens at {}",
                ui.canvas().camera.zoom
            );

            let view = ui.canvas();
            let centre = Point::new(view.bounds.center_x(), view.bounds.center_y());
            ui.move_to(centre);
            ui.step(&[Event::Mouse(iced::mouse::Event::WheelScrolled {
                delta: iced::mouse::ScrollDelta::Lines { x: 0.0, y: -10.0 },
            })]);
            assert!(ui.canvas().camera.zoom < readable, "the wheel zooms out");

            ui.tap(keyboard::Key::Character("f".into()), keyboard::Modifiers::default());
            assert!(
                ui.canvas().camera.zoom >= readable,
                "{patch} frames at {}",
                ui.canvas().camera.zoom
            );
        }
    }

    /// A click on Envelope's AD segment picks AD, and the tile hides
    /// Sustain and Release at once (panel-widgets.md, Segmented control;
    /// ADR-0030).
    #[test]
    fn a_segment_click_changes_the_mode_and_hides_its_controls() {
        let mut ui = Ui::open("sine.ron");
        let env = ui.app.controller.add_module("loom.env.envelope").unwrap();
        ui.app.refresh();
        ui.step(&[]);
        let tile = ui.app.scene.module(env.0).unwrap().clone();
        let rect = tile.widget_bounds(1).expect("the mode segments");
        let ad = loom_canvas::segment_centre(rect, 1, 2);
        let at = ui.canvas().to_window(ad);
        ui.move_to(at);
        ui.press();
        ui.release();
        let node = ui.app.controller.graph().node(env).unwrap();
        let mode = node.params[node.descriptor.param_index("mode").unwrap()];
        assert_eq!(mode, 1.0, "AD picked");
        let hidden: Vec<_> = ui
            .app
            .scene
            .module(env.0)
            .unwrap()
            .panel
            .widgets
            .iter()
            .filter(|w| w.hidden)
            .map(|w| w.label.clone())
            .collect();
        assert_eq!(hidden, ["Sustain", "Release"]);
    }

    /// An open select list, through the real view: a click opens it, Down and
    /// Enter pick the next choice once, and Escape closes it with no edit. The
    /// keys never reach the canvas or the tile behind it (panel-widgets.md,
    /// Select list).
    #[test]
    fn a_select_list_takes_its_keys_through_the_real_view() {
        use keyboard::key::Named;
        let mut ui = Ui::open("sine.ron");
        let lfo = select_new(&mut ui, "loom.mod.lfo");
        let shape = |ui: &Ui| {
            let node = ui.app.controller.graph().node(lfo).unwrap();
            node.params[node.descriptor.param_index("shape").unwrap()]
        };
        let before = shape(&ui);
        let position = ui.app.scene.module(lfo.0).unwrap().position;
        let at = ui.control(lfo.0, "shape");
        let none = keyboard::Modifiers::default();

        ui.move_to(at);
        ui.press();
        ui.release();
        ui.tap(keyboard::Key::Named(Named::Escape), none);
        assert_eq!(shape(&ui), before, "Escape edits nothing");

        ui.press();
        ui.release();
        ui.tap(keyboard::Key::Named(Named::ArrowDown), none);
        assert_eq!(shape(&ui), before, "Down moves only the highlight");
        ui.tap(keyboard::Key::Named(Named::Enter), none);
        assert_eq!(shape(&ui), before + 1.0, "Enter commits one choice");
        assert_eq!(ui.app.scene.module(lfo.0).unwrap().position, position);
    }

    /// A fader on a float parameter keeps every value in its range: a drag to
    /// mid-range does not snap through a whole number, and a parameter bound
    /// only to a fader takes a modulation assignment, in the inspector and the
    /// matrix (panel-widgets.md, Fader).
    #[test]
    fn a_fader_sets_a_float_value_and_takes_an_assignment() {
        let mut ui = Ui::open("sine.ron");
        let id = select_new(&mut ui, "test.panel");
        let param = |ui: &Ui, name: &str| {
            let node = ui.app.controller.graph().node(id).unwrap();
            node.params[node.descriptor.param_index(name).unwrap()]
        };
        let at = ui.control(id.0, "wide");
        let zoom = ui.canvas().camera.zoom;
        let before = param(&ui, "wide");
        let to = Point::new(at.x + 20.0, at.y);
        ui.drag(at, to, 4);
        let moved = param(&ui, "wide") - before;
        let throw = (loom_canvas::GRID * 6.0 - 8.0) * zoom;
        assert!(
            (moved - 20.0 / throw.max(200.0)).abs() < 1e-3,
            "{moved}: a small drag makes a small change, with no snap"
        );

        let (_, kind, spec) =
            crate::scene::control_param(&ui.app.controller, id.0, widget_of(&ui, id, "tall")).unwrap();
        assert_eq!(kind, loom_core::WidgetKind::Fader { vertical: true });
        let position = crate::scene::control_position(kind, &spec, 1_000.0);
        assert!(position > 0.0 && position < 1.0, "through the logarithmic mapping");
        let value = crate::scene::control_value(kind, &spec, position);
        assert!((value - 1_000.0).abs() < 0.5, "{value}");
        assert!(kind.takes_modulation());
        assert!(kind.has_range());
        assert!(kind.is_one_number());
    }

    /// A parameter whose only widget is a select list takes a modulation
    /// assignment everywhere: its inspector row accepts a drop, the matrix
    /// offers it, a cable dragged onto the box on the canvas assigns it, and
    /// the box shows a dot, or a hollow ring when the assignment is off
    /// (panel-widgets.md, rule 3; #567).
    #[test]
    fn a_select_only_parameter_is_a_modulation_target() {
        use crate::snapshot::Mark;
        let mut ui = Ui::open("sine.ron");
        let id = select_new(&mut ui, "test.panel");
        let node = ui.app.controller.graph().node(id).unwrap();
        let long = node.descriptor.param_index("long").unwrap();
        assert!(
            node.descriptor
                .panel
                .widgets
                .iter()
                .filter(|w| w.binds("long"))
                .all(|w| matches!(w.kind, loom_core::WidgetKind::Select)),
            "the test needs a parameter shown only as a select list"
        );

        let crate::inspector::Inspector::Module(rows) = ui.app.inspector_model() else {
            panic!("the inspector shows the selected module");
        };
        assert!(rows.params[long].accepts_drop, "its row accepts a drop");
        assert!(
            crate::matrix::targets(ui.app.controller.graph())
                .iter()
                .any(|t| t.module == id && t.param == long),
            "the matrix offers it as a target"
        );

        let widget = widget_of(&ui, id, "long");
        let mark = |ui: &Ui| {
            let snapshot = ui.app.ui_snapshot();
            let tile = snapshot.tiles.iter().find(|t| t.id == id.0).expect("the tile");
            assert_eq!(tile.widgets[widget].kind, "select");
            tile.widgets[widget].modulation
        };
        assert_eq!(mark(&ui), Mark::None);

        let from = ui.output(id.0, "uni");
        let to = ui.control(id.0, "long");
        ui.drag(from, to, 12);
        let assignments: Vec<_> = ui
            .app
            .controller
            .graph()
            .assignments()
            .filter(|(_, a)| a.target == id && a.param == long)
            .map(|(assignment, _)| assignment)
            .collect();
        assert_eq!(assignments.len(), 1, "the drop on the box made an assignment");
        ui.redraw();
        assert_eq!(mark(&ui), Mark::Dot);

        let _ = ui.app.update(Message::Enabled {
            id: assignments[0],
            enabled: false,
        });
        ui.redraw();
        assert_eq!(mark(&ui), Mark::Ring, "a hollow ring while the assignment is off");
    }

    /// The square of the XY pad on a new `test.panel`, in world units.
    fn pad_area(ui: &mut Ui, id: ModuleId) -> Rectangle {
        loom_canvas::xy_area(ui.control_bounds(id.0, "pad_x"))
    }

    /// A parameter's position on its control.
    fn position_of(ui: &Ui, module: ModuleId, name: &str) -> f32 {
        let node = ui.app.controller.graph().node(module).unwrap();
        let spec = node.spec(node.descriptor.param_index(name).unwrap()).unwrap();
        spec.normalize(param_value(ui, module, name))
    }

    /// A press on an XY pad moves nothing, and a drag moves the point by the
    /// pointer's movement, on both axes at once, as one undo step. Shift is
    /// 10 times finer (panel-widgets.md, XY pad, and rule 2).
    #[test]
    fn an_xy_pad_drags_relatively_on_both_axes() {
        let mut ui = Ui::open("sine.ron");
        let id = select_new(&mut ui, "test.panel");
        let before = (position_of(&ui, id, "pad_x"), position_of(&ui, id, "pad_y"));
        let at = ui.control(id.0, "pad_x");
        ui.move_to(at);
        ui.press();
        assert_eq!(position_of(&ui, id, "pad_x"), before.0, "a plain press moves nothing");
        assert_eq!(position_of(&ui, id, "pad_y"), before.1, "a plain press moves nothing");
        let steps = ui.app.edits.history.len();
        ui.move_to(Point::new(at.x + 20.0, at.y - 20.0));
        ui.release();
        assert_eq!(ui.app.edits.history.len(), steps + 1, "one undo step");
        let moved = position_of(&ui, id, "pad_x") - before.0;
        assert!((moved - 0.1).abs() < 1e-3, "{moved}");
        let moved = position_of(&ui, id, "pad_y") - before.1;
        assert!((moved - 0.1).abs() < 1e-3, "up raises y: {moved}");

        // Long enough after the last click not to be a double-click.
        std::thread::sleep(std::time::Duration::from_millis(450));
        let coarse = position_of(&ui, id, "pad_x");
        ui.hold(keyboard::Modifiers::SHIFT);
        ui.drag(at, Point::new(at.x + 20.0, at.y), 4);
        ui.hold(keyboard::Modifiers::default());
        let moved = position_of(&ui, id, "pad_x") - coarse;
        assert!((moved - 0.01).abs() < 1e-3, "Shift is finer: {moved}");
    }

    /// With Alt held, a press jumps the point to the pointer. A double-click
    /// resets both parameters as one undo step (panel-widgets.md, XY pad).
    #[test]
    fn an_xy_pad_jumps_with_alt_and_resets_both() {
        let mut ui = Ui::open("sine.ron");
        let id = select_new(&mut ui, "test.panel");
        let area = pad_area(&mut ui, id);
        let world = Point::new(area.x + area.width * 0.25, area.y + area.height * 0.25);
        let at = ui.canvas().to_window(world);
        ui.hold(keyboard::Modifiers::ALT);
        ui.move_to(at);
        ui.press();
        ui.release();
        ui.hold(keyboard::Modifiers::default());
        let (x, y) = (position_of(&ui, id, "pad_x"), position_of(&ui, id, "pad_y"));
        assert!((x - 0.25).abs() < 1e-3, "x follows the pointer: {x}");
        assert!((y - 0.75).abs() < 1e-3, "y follows the pointer: {y}");

        std::thread::sleep(std::time::Duration::from_millis(450));
        let steps = ui.app.edits.history.len();
        ui.move_to(at);
        ui.press();
        ui.release();
        ui.press();
        ui.release();
        assert_eq!(param_value(&ui, id, "pad_x"), 0.5, "reset to the default");
        assert_eq!(param_value(&ui, id, "pad_y"), 1_000.0, "reset to the default");
        assert_eq!(ui.app.edits.history.len(), steps + 1, "one undo step for both");
    }

    /// Left and Right change `x`, and Up and Down change `y`, by 1%, or 0.1%
    /// with Shift. Enter does nothing, and digits open no field: a pad takes
    /// no typed value (panel-widgets.md, XY pad).
    #[test]
    fn an_xy_pad_takes_its_keys_by_axis() {
        use keyboard::key::Named;
        let mut ui = Ui::open("sine.ron");
        let id = select_new(&mut ui, "test.panel");
        focus_control(&mut ui, id, "pad_x");
        let none = keyboard::Modifiers::default();
        let start = (position_of(&ui, id, "pad_x"), position_of(&ui, id, "pad_y"));

        ui.tap(keyboard::Key::Named(Named::ArrowRight), none);
        assert!((position_of(&ui, id, "pad_x") - start.0 - 0.01).abs() < 1e-4);
        assert!((position_of(&ui, id, "pad_y") - start.1).abs() < 1e-4, "y stays");
        ui.tap(keyboard::Key::Named(Named::ArrowLeft), keyboard::Modifiers::SHIFT);
        assert!(
            (position_of(&ui, id, "pad_x") - start.0 - 0.009).abs() < 1e-4,
            "Shift is finer"
        );
        ui.tap(keyboard::Key::Named(Named::ArrowUp), none);
        assert!((position_of(&ui, id, "pad_y") - start.1 - 0.01).abs() < 1e-4);
        ui.tap(keyboard::Key::Named(Named::ArrowDown), none);
        assert!((position_of(&ui, id, "pad_y") - start.1).abs() < 1e-4);

        let before = (param_value(&ui, id, "pad_x"), param_value(&ui, id, "pad_y"));
        ui.tap(keyboard::Key::Named(Named::Enter), none);
        ui.typewrite("7");
        ui.tap(keyboard::Key::Named(Named::Enter), none);
        assert_eq!((param_value(&ui, id, "pad_x"), param_value(&ui, id, "pad_y")), before);
    }

    /// In learn mode a click, Space, or Enter on a pad arms nothing, and the
    /// status line says to use the inspector. The tile shows no mapping label
    /// (midi-learn.md, Making a mapping).
    #[test]
    fn learn_mode_arms_nothing_on_an_xy_pad() {
        use keyboard::key::Named;
        let mut ui = Ui::open("sine.ron");
        let id = select_new(&mut ui, "test.panel");
        let _ = ui.app.update(Message::Learn(crate::learn::Message::Toggle));
        ui.redraw();
        focus_control(&mut ui, id, "pad_x");
        assert!(ui.app.learn.armed().is_none(), "a click arms nothing");
        assert!(ui.app.ui_snapshot().status.contains("inspector"));
        for key in [Named::Space, Named::Enter] {
            ui.app.alerts.say(String::new());
            ui.tap(keyboard::Key::Named(key), keyboard::Modifiers::default());
            assert!(ui.app.learn.armed().is_none(), "{key:?} arms nothing");
            assert!(ui.app.ui_snapshot().status.contains("inspector"), "{key:?} says why");
        }
        let widget = widget_of(&ui, id, "pad_x");
        let view = crate::learn::view(&ui.app.controller, ui.app.learn.armed());
        assert!(!view.labels.contains_key(&(id.0, widget)));
    }

    /// A cable dropped on a pad goes to the nearest edge, and each parameter
    /// shows its own mark: a bar for one, and a ring for one whose assignment
    /// is off. A press on the bar drags depth, not the value
    /// (panel-widgets.md, XY pad, and rule 3).
    #[test]
    fn an_xy_pad_takes_a_drop_at_its_nearest_edge_and_marks_each_axis() {
        use crate::snapshot::Mark;
        let mut ui = Ui::open("sine.ron");
        let id = select_new(&mut ui, "test.panel");
        let area = pad_area(&mut ui, id);
        let node = ui.app.controller.graph().node(id).unwrap();
        let (x, y) = (
            node.descriptor.param_index("pad_x").unwrap(),
            node.descriptor.param_index("pad_y").unwrap(),
        );
        let target = |ui: &Ui, param: usize| {
            let graph = ui.app.controller.graph();
            let found = graph.assignments().find(|(_, a)| a.target == id && a.param == param);
            found.map(|(assignment, _)| assignment)
        };
        let from = ui.output(id.0, "uni");
        let bottom = ui
            .canvas()
            .to_window(Point::new(area.center_x(), area.y + area.height - 2.0));
        ui.drag(from, bottom, 12);
        assert!(target(&ui, x).is_some(), "the bottom edge is x");
        assert!(target(&ui, y).is_none());
        let from = ui.output(id.0, "bi");
        let left = ui.canvas().to_window(Point::new(area.x + 2.0, area.center_y()));
        ui.drag(from, left, 12);
        let off = target(&ui, y).expect("the left edge is y");

        let _ = ui.app.update(Message::Enabled {
            id: off,
            enabled: false,
        });
        ui.redraw();
        let widget = widget_of(&ui, id, "pad_x");
        let snapshot = ui.app.ui_snapshot();
        let tile = snapshot.tiles.iter().find(|t| t.id == id.0).unwrap();
        let shown = &tile.widgets[widget];
        assert_eq!(shown.kind, "xy_pad");
        assert!(matches!(shown.modulation, Mark::Range { .. }), "x shows a bar");
        let second = shown.second.as_ref().expect("the pad reports y");
        assert_eq!(second.modulation, Mark::Ring, "y shows a ring");
        assert!(shown.text.contains(", "), "the text says both values: {}", shown.text);

        let depth = |ui: &Ui| {
            ui.app
                .controller
                .graph()
                .assignment(target(ui, x).unwrap())
                .unwrap()
                .depth
        };
        let before = (depth(&ui), param_value(&ui, id, "pad_x"));
        let at = ui
            .canvas()
            .to_window(loom_canvas::xy_edge_point(area, loom_canvas::ControlPart::Main, 0.5));
        ui.drag(at, Point::new(at.x + 20.0, at.y), 4);
        assert!(depth(&ui) > before.0, "the bar changes the depth");
        assert_eq!(param_value(&ui, id, "pad_x"), before.1, "and not the value");
    }

    /// Each edge of a pad has its parameter's own label and description
    /// (panel-widgets.md, XY pad).
    #[test]
    fn each_axis_of_an_xy_pad_has_its_own_text() {
        let mut ui = Ui::open("sine.ron");
        let id = select_new(&mut ui, "test.panel");
        let widget = widget_of(&ui, id, "pad_x");
        let tile = ui.app.scene.module(id.0).unwrap();
        let view = &tile.panel.widgets[widget];
        let parts = view.kind.parts().expect("a pad has two parts");
        assert_eq!(view.label, "Position", "the caption names the pair");
        assert_eq!(parts[0].label, "pad_x");
        assert_eq!(parts[1].label, "pad_y");
    }

    /// A handle of the range on a new `test.panel`, in window coordinates.
    fn handle(ui: &mut Ui, id: ModuleId, name: &str) -> Point {
        let part = crate::targets::find(ui.app.controller.graph(), id.0, name).unwrap();
        let world = crate::targets::world_bounds(&ui.app.scene, &ui.app.values, part).unwrap();
        ui.canvas().to_window(world.center())
    }

    /// The point at a position on the range's track, in window coordinates.
    fn on_span(ui: &mut Ui, id: ModuleId, t: f32) -> Point {
        let rect = ui.control_bounds(id.0, "span_low");
        let track = (rect.x + 4.0, rect.width - 8.0);
        let y = handle(ui, id, "span_low").y;
        let window = ui.canvas().to_window(Point::new(track.0 + track.1 * t, 0.0));
        Point::new(window.x, y)
    }

    /// Dragging a handle moves that parameter only, and it stops at the other
    /// handle. A press on the track outside the band moves the tile, not a
    /// handle (panel-widgets.md, Range).
    #[test]
    fn a_range_handle_drags_and_cannot_cross() {
        let mut ui = Ui::open("sine.ron");
        let id = select_new(&mut ui, "test.panel");
        // Below high, by a drag on the low handle.
        let (low, high) = (param_value(&ui, id, "span_low"), param_value(&ui, id, "span_high"));
        assert_eq!((low, high), (0.0, 1.0));
        let at = handle(&mut ui, id, "span_low");
        ui.drag(at, Point::new(at.x + 20.0, at.y), 4);
        let moved = param_value(&ui, id, "span_low");
        assert!(moved > 0.0, "low moved up: {moved}");
        assert_eq!(param_value(&ui, id, "span_high"), 1.0, "high stayed");

        std::thread::sleep(std::time::Duration::from_millis(450));
        let at = handle(&mut ui, id, "span_low");
        ui.drag(at, Point::new(at.x + 1_000.0, at.y), 8);
        assert_eq!(param_value(&ui, id, "span_low"), 1.0, "low stops at high");
        assert_eq!(param_value(&ui, id, "span_high"), 1.0);

        // Apart again, and a press on the track outside the band.
        std::thread::sleep(std::time::Duration::from_millis(450));
        let _ = ui.app.update(Message::Canvas(CanvasEvent::SetControl {
            module: id.0,
            widget: widget_of(&ui, id, "span_low"),
            part: loom_canvas::ControlPart::Main,
            position: 0.75,
        }));
        ui.redraw();
        let before = ui.app.scene.module(id.0).unwrap().bounds();
        let values = (param_value(&ui, id, "span_low"), param_value(&ui, id, "span_high"));
        let outside = on_span(&mut ui, id, 0.2);
        ui.drag(outside, Point::new(outside.x + 48.0, outside.y), 4);
        assert_ne!(ui.app.scene.module(id.0).unwrap().bounds(), before, "the tile moved");
        assert_eq!(
            (param_value(&ui, id, "span_low"), param_value(&ui, id, "span_high")),
            values
        );
    }

    /// One large pointer move goes all the way: a handle does not lag behind a
    /// fast pointer (panel-widgets.md, Range).
    #[test]
    fn a_fast_move_takes_a_range_handle_all_the_way() {
        let mut ui = Ui::open("sine.ron");
        let id = select_new(&mut ui, "test.panel");
        let widget = widget_of(&ui, id, "tall_low");
        for (part, position) in [
            (loom_canvas::ControlPart::Main, 0.0),
            (loom_canvas::ControlPart::Second, 1.0),
        ] {
            let _ = ui.app.update(Message::Canvas(CanvasEvent::SetControl {
                module: id.0,
                widget,
                part,
                position,
            }));
        }
        ui.redraw();
        let at = handle(&mut ui, id, "tall_high");
        ui.drag(at, Point::new(at.x, at.y + 180.0), 1);
        let high = param_value(&ui, id, "tall_high");
        assert!((high - 0.1).abs() < 0.01, "{high}");
    }

    /// Dragging the band moves both as one undo step, and keeps the width. A
    /// double-click on a handle resets it, and on the band resets both as one
    /// undo step. Two quick clicks on different handles reset nothing
    /// (panel-widgets.md, Range).
    #[test]
    fn a_range_band_moves_both_and_a_double_click_resets() {
        let mut ui = Ui::open("sine.ron");
        let id = select_new(&mut ui, "test.panel");
        let middle = on_span(&mut ui, id, 0.75);
        let steps = ui.app.edits.history.len();
        ui.drag(middle, Point::new(middle.x - 10.0, middle.y), 4);
        assert_eq!(ui.app.edits.history.len(), steps + 1, "one undo step");
        let (low, high) = (param_value(&ui, id, "span_low"), param_value(&ui, id, "span_high"));
        assert!(low < 0.0 && high < 1.0, "both moved: {low}, {high}");
        assert!((high - low - 1.0).abs() < 1e-3, "the width stays: {}", high - low);

        std::thread::sleep(std::time::Duration::from_millis(450));
        let (a, b) = (handle(&mut ui, id, "span_low"), handle(&mut ui, id, "span_high"));
        ui.move_to(a);
        ui.press();
        ui.release();
        ui.move_to(b);
        ui.press();
        ui.release();
        assert_eq!(
            param_value(&ui, id, "span_low"),
            low,
            "low then high is no double-click"
        );
        assert_eq!(param_value(&ui, id, "span_high"), high);

        std::thread::sleep(std::time::Duration::from_millis(450));
        ui.move_to(b);
        ui.press();
        ui.release();
        ui.press();
        ui.release();
        assert_eq!(param_value(&ui, id, "span_high"), 1.0, "high resets");
        assert_eq!(param_value(&ui, id, "span_low"), low, "low stays");

        std::thread::sleep(std::time::Duration::from_millis(450));
        let middle = on_span(&mut ui, id, 0.7);
        let steps = ui.app.edits.history.len();
        ui.move_to(middle);
        ui.press();
        ui.release();
        ui.press();
        ui.release();
        assert_eq!(param_value(&ui, id, "span_low"), 0.0, "low resets");
        assert_eq!(ui.app.edits.history.len(), steps + 1, "one undo step");
    }

    /// Selecting a control range on either parameter selects it for both, as
    /// one undo step. A typed value picks the smallest range that holds both
    /// (control-ranges.md, Ranges on a range widget).
    #[test]
    fn a_range_widgets_parameters_share_a_control_range() {
        let mut ui = Ui::open("sine.ron");
        let id = select_new(&mut ui, "test.panel");
        let node = ui.app.controller.graph().node(id).unwrap();
        let low = node.descriptor.param_index("span_low").unwrap();
        let high = node.descriptor.param_index("span_high").unwrap();
        let wide = node.descriptor.params[low].options.ranges.len() - 1;
        let selected = |ui: &Ui| {
            let node = ui.app.controller.graph().node(id).unwrap();
            (node.range(low), node.range(high))
        };
        let (a, b) = selected(&ui);
        assert_eq!(a, b);

        let steps = ui.app.edits.history.len();
        ui.app.set_range(id.0, high, wide);
        let (a, b) = selected(&ui);
        assert_eq!(a, b, "selecting one selects both");
        assert_eq!(ui.app.edits.history.len(), steps + 1, "one undo step");
        let _ = ui.app.update(Message::Undo);
        assert_eq!(selected(&ui).0, selected(&ui).1, "undo keeps them together");

        ui.app.apply_typed_value(id.0, high, "5").unwrap();
        let (a, b) = selected(&ui);
        assert_eq!(a, b);
        assert!(a.unwrap().contains(5.0) && a.unwrap().contains(param_value(&ui, id, "span_low")));
    }

    /// The keyboard goes inside a range and never traps: Enter goes in, arrows
    /// pick a part and stop at the ends, Space or Enter grabs, the arrows then
    /// move it, a handle stops at the other, Enter or Escape lets go and keeps
    /// the change, Escape leaves, and Tab leaves from any state
    /// (panel-widgets.md, rule 4, and Range).
    #[test]
    fn the_keyboard_goes_inside_a_range() {
        use keyboard::key::Named;
        let mut ui = Ui::open("sine.ron");
        let id = select_new(&mut ui, "test.panel");
        focus_control(&mut ui, id, "span_high");
        let none = keyboard::Modifiers::default();
        let key = |k| keyboard::Key::Named(k);
        let (low, high) = (position_of(&ui, id, "span_low"), position_of(&ui, id, "span_high"));

        ui.tap(key(Named::ArrowRight), none);
        assert_eq!(
            position_of(&ui, id, "span_high"),
            high,
            "outside, arrows change nothing"
        );

        ui.tap(key(Named::Enter), none);
        ui.tap(key(Named::ArrowLeft), none);
        ui.tap(key(Named::Space), none);
        ui.tap(key(Named::ArrowRight), none);
        assert!(
            (position_of(&ui, id, "span_low") - low - 0.01).abs() < 1e-4,
            "inside starts on low"
        );
        assert_eq!(position_of(&ui, id, "span_high"), high, "high stays");
        ui.tap(key(Named::ArrowRight), keyboard::Modifiers::SHIFT);
        assert!(
            (position_of(&ui, id, "span_low") - low - 0.011).abs() < 1e-4,
            "Shift is finer"
        );
        let kept = position_of(&ui, id, "span_low");
        ui.tap(key(Named::Escape), none);
        assert_eq!(position_of(&ui, id, "span_low"), kept, "Escape lets go and keeps it");

        // Not grabbed, the arrows pick: the band, then high, and stop there.
        ui.tap(key(Named::ArrowRight), none);
        ui.tap(key(Named::ArrowRight), none);
        ui.tap(key(Named::ArrowRight), none);
        assert_eq!(position_of(&ui, id, "span_low"), kept, "picking changes nothing");
        ui.tap(key(Named::Enter), none);
        ui.tap(key(Named::ArrowLeft), none);
        assert!(
            (position_of(&ui, id, "span_high") - high + 0.01).abs() < 1e-4,
            "high is grabbed"
        );
        // A long way down: it stops at low.
        for _ in 0..80 {
            ui.tap(key(Named::ArrowLeft), none);
        }
        assert_eq!(position_of(&ui, id, "span_high"), position_of(&ui, id, "span_low"));
        ui.tap(key(Named::Enter), none);

        // The band moves both.
        ui.tap(key(Named::ArrowLeft), none);
        ui.tap(key(Named::Space), none);
        let before = position_of(&ui, id, "span_low");
        ui.tap(key(Named::ArrowLeft), none);
        assert!((position_of(&ui, id, "span_low") - before + 0.01).abs() < 1e-4);
        assert!((position_of(&ui, id, "span_high") - before + 0.01).abs() < 1e-4);

        // Tab leaves from a grab, and arrows then belong to the next control.
        ui.tap(key(Named::Tab), none);
        let after = position_of(&ui, id, "span_low");
        ui.tap(key(Named::ArrowLeft), none);
        assert_eq!(position_of(&ui, id, "span_low"), after, "the grab ended");
    }

    /// Nothing redraws while nothing a pad or a range shows changes: refreshing
    /// the same values keeps the panel generation (panel-widgets.md, rule 7).
    #[test]
    fn two_value_widgets_do_not_redraw_when_nothing_changes() {
        let mut ui = Ui::open("sine.ron");
        select_new(&mut ui, "test.panel");
        let generation = ui.app.values.generation();
        ui.app.refresh_values();
        crate::scene::update_live_values(&mut ui.app.values, &ui.app.controller);
        assert_eq!(ui.app.values.generation(), generation);
    }

    /// Transport's Tempo is a number that an arrow moves by 1 BPM, or 0.1 BPM
    /// with Shift. A number with no declared step keeps 1% of its range
    /// (panel-widgets.md, Number).
    #[test]
    fn the_tempo_number_steps_by_whole_beats_per_minute() {
        use keyboard::key::Named;
        let mut ui = Ui::open("sine.ron");
        let transport = ui.app.controller.transport_module().expect("a transport");
        let _ = ui.app.update(Message::SelectModule(transport.0));
        ui.redraw();
        focus_control(&mut ui, transport, "tempo");
        let tempo = |ui: &Ui| param_value(ui, transport, "tempo");
        let start = tempo(&ui);
        let up = keyboard::Key::Named(Named::ArrowUp);
        ui.tap(up.clone(), keyboard::Modifiers::default());
        assert!((tempo(&ui) - start - 1.0).abs() < 1e-2, "{}", tempo(&ui));
        ui.tap(up.clone(), keyboard::Modifiers::SHIFT);
        assert!((tempo(&ui) - start - 1.1).abs() < 1e-2, "{}", tempo(&ui));

        // Steps land exactly, however many there are, and walk back to the start.
        let down = keyboard::Key::Named(Named::ArrowDown);
        ui.tap(down.clone(), keyboard::Modifiers::SHIFT);
        assert_eq!(tempo(&ui), 121.0);
        for _ in 0..100 {
            ui.tap(up.clone(), keyboard::Modifiers::default());
        }
        assert_eq!(tempo(&ui), 221.0);
        for _ in 0..101 {
            ui.tap(down.clone(), keyboard::Modifiers::default());
        }
        assert_eq!(tempo(&ui), 120.0);

        let id = select_new(&mut ui, "test.panel");
        focus_control(&mut ui, id, "tempo");
        let before = position_of(&ui, id, "tempo");
        ui.tap(up, keyboard::Modifiers::default());
        assert!((position_of(&ui, id, "tempo") - before - 0.01).abs() < 1e-4);
    }

    /// A press ends a grab: back on the same range, the arrows change nothing
    /// until the keyboard goes inside again (panel-widgets.md, rule 4).
    #[test]
    fn a_press_ends_the_inside_of_a_range() {
        use keyboard::key::Named;
        let mut ui = Ui::open("sine.ron");
        let id = select_new(&mut ui, "test.panel");
        focus_control(&mut ui, id, "span_low");
        let none = keyboard::Modifiers::default();
        ui.tap(keyboard::Key::Named(Named::Enter), none);
        ui.tap(keyboard::Key::Named(Named::Space), none);
        std::thread::sleep(std::time::Duration::from_millis(450));
        focus_control(&mut ui, id, "span_low");
        let before = position_of(&ui, id, "span_low");
        ui.tap(keyboard::Key::Named(Named::ArrowRight), none);
        assert_eq!(position_of(&ui, id, "span_low"), before);
    }

    /// Escape lets a grab go before it hides pinned lines, and hides them
    /// before it leaves the range (modulation.md, Lines). Losing focus ends
    /// the inside.
    #[test]
    fn escape_lets_a_grab_go_before_it_hides_pinned_lines() {
        use keyboard::key::Named;
        let mut ui = Ui::open("sine.ron");
        let id = select_new(&mut ui, "test.panel");
        let _ = ui.app.update(Message::Assign {
            source: OutputRef { module: id, port: 0 },
            module: id.0,
            index: ui
                .app
                .controller
                .graph()
                .node(id)
                .unwrap()
                .descriptor
                .param_index("level")
                .unwrap(),
        });
        ui.redraw();
        let badge = ui.canvas().to_window(
            ui.app
                .scene
                .badge_bounds(id.0, 0)
                .expect("the output shows a badge")
                .center(),
        );
        ui.move_to(badge);
        ui.press();
        ui.release();

        focus_control(&mut ui, id, "span_low");
        let none = keyboard::Modifiers::default();
        let key = |k| keyboard::Key::Named(k);
        ui.tap(key(Named::Enter), none);
        ui.tap(key(Named::Space), none);
        // The first Escape lets go, and the second hides the pinned lines, so
        // the keyboard is still inside: Space grabs again and the arrows move.
        ui.tap(key(Named::Escape), none);
        ui.tap(key(Named::Escape), none);
        ui.tap(key(Named::Space), none);
        let before = position_of(&ui, id, "span_low");
        ui.tap(key(Named::ArrowRight), none);
        assert!(position_of(&ui, id, "span_low") > before, "still inside");

        // With no pinned lines left, the same two Escapes leave the range.
        ui.tap(key(Named::Escape), none);
        ui.tap(key(Named::Escape), none);
        let before = position_of(&ui, id, "span_low");
        ui.tap(key(Named::ArrowRight), none);
        assert_eq!(position_of(&ui, id, "span_low"), before, "left the range");
    }

    /// Learn mode arms nothing on a range. Space outside its inside does what
    /// it does on a knob: it reaches Run. Its snapshot says both values, and
    /// `MODULE:high` names the high handle (panel-widgets.md, Range;
    /// control-channel.md, rule 10).
    #[test]
    fn a_range_reports_both_values_and_names_each_handle() {
        use keyboard::key::Named;
        let mut ui = Ui::open("sine.ron");
        let id = select_new(&mut ui, "test.panel");
        let widget = widget_of(&ui, id, "span_low");
        let snapshot = ui.app.ui_snapshot();
        let tile = snapshot.tiles.iter().find(|t| t.id == id.0).unwrap();
        assert_eq!(tile.widgets[widget].kind, "range");
        assert_eq!(tile.widgets[widget].label, "Span");
        assert!(
            tile.widgets[widget].text.contains(" to "),
            "{}",
            tile.widgets[widget].text
        );

        let graph = ui.app.controller.graph();
        let low = crate::targets::find(graph, id.0, "span_low").unwrap();
        let high = crate::targets::find(graph, id.0, "span_high").unwrap();
        assert_ne!(low, high);
        assert_eq!(
            crate::targets::name(graph, high),
            Some(format!("{}:span_high", id.0)),
            "the name finds it again"
        );
        let (a, b) = (handle(&mut ui, id, "span_low"), handle(&mut ui, id, "span_high"));
        assert!(b.x > a.x, "each name aims at its own handle");

        focus_control(&mut ui, id, "span_low");
        let running = |ui: &Ui| ui.app.ui_snapshot().tiles.len();
        let before = running(&ui);
        let messages = ui.tap(keyboard::Key::Named(Named::Space), keyboard::Modifiers::default());
        assert!(
            messages
                .iter()
                .any(|m| matches!(m, Message::Canvas(CanvasEvent::ToggleRun))),
            "Space reaches Run, as on a knob"
        );
        assert_eq!(running(&ui), before);

        let _ = ui.app.update(Message::Learn(crate::learn::Message::Toggle));
        ui.redraw();
        ui.tap(keyboard::Key::Named(Named::Enter), keyboard::Modifiers::default());
        assert!(ui.app.learn.armed().is_none(), "learn mode arms nothing");
        assert!(ui.app.ui_snapshot().status.contains("inspector"));
    }

    /// The widget index of the control bound to a parameter.
    fn widget_of(ui: &Ui, module: ModuleId, param: &str) -> usize {
        let node = ui.app.controller.graph().node(module).unwrap();
        node.descriptor
            .panel
            .widgets
            .iter()
            .position(|w| w.binds(param))
            .unwrap()
    }

    /// A number shows the base value as text and steps a whole number by one
    /// per drag step, even over 1000 values. While modulated, its mark is a dot
    /// and its text stays the base value (panel-widgets.md, Number).
    #[test]
    fn a_number_steps_by_one_and_shows_the_base_value() {
        let mut ui = Ui::open("sine.ron");
        let id = select_new(&mut ui, "test.panel");
        let count = |ui: &Ui| {
            let node = ui.app.controller.graph().node(id).unwrap();
            node.params[node.descriptor.param_index("count").unwrap()]
        };
        assert_eq!(count(&ui), 7.0);
        let at = ui.control(id.0, "count");
        let up = Point::new(at.x, at.y - 40.0);
        ui.drag(at, up, 8);
        assert_eq!(count(&ui), 17.0, "40 pixels at 4 pixels per value");

        let widget = widget_of(&ui, id, "tempo");
        let (_, kind, spec) = crate::scene::control_param(&ui.app.controller, id.0, widget).unwrap();
        assert_eq!(kind, loom_core::WidgetKind::Number);
        let value = ui.app.values.get(id.0, widget).unwrap();
        assert_eq!(value.text, crate::param::format(&spec, 120.0), "the base value");
        let reach = crate::modulation::Reach {
            depth: 0.25,
            enabled: true,
            polarity: loom_core::Polarity::Unipolar,
        };
        assert_eq!(
            crate::modulation::mark(kind, &spec, 120.0, &[reach]),
            loom_canvas::ModulationMark::Dot
        );
    }

    /// The value of a parameter by name.
    fn param_value(ui: &Ui, module: ModuleId, name: &str) -> f32 {
        let node = ui.app.controller.graph().node(module).unwrap();
        node.params[node.descriptor.param_index(name).unwrap()]
    }

    /// Focuses a canvas control by clicking it.
    fn focus_control(ui: &mut Ui, module: ModuleId, name: &str) {
        let at = ui.control(module.0, name);
        ui.move_to(at);
        ui.press();
        ui.release();
    }

    /// #579: the snapshot shows the canvas's typed field: which control it is
    /// on, its text, and the reason bad text gives. It opens with real keys, and
    /// goes when Escape closes it.
    #[test]
    fn the_snapshot_shows_the_typed_field() {
        use keyboard::key::Named;
        let mut ui = Ui::open("subtractive.ron");
        ui.app.pointer = crate::pointer::Pointer::new(true);
        let id = select_new(&mut ui, "loom.filter.svf");
        focus_control(&mut ui, id, "cutoff");
        ui.redraw();
        assert_eq!(ui.app.ui_snapshot().typed_field, None, "a click opens no field");

        ui.typewrite("4");
        ui.redraw();
        let typed = ui.app.ui_snapshot().typed_field.expect("the digit opened the field");
        assert_eq!(typed.target, format!("{}:cutoff", id.0));
        assert_eq!(typed.text, "4");
        assert_eq!(typed.reason, None);

        ui.typewrite("x");
        ui.redraw();
        let typed = ui.app.ui_snapshot().typed_field.expect("the field is still open");
        assert_eq!(typed.text, "4x");
        assert!(typed.reason.is_some(), "bad text gives its reason");

        ui.tap(keyboard::Key::Named(Named::Escape), keyboard::Modifiers::empty());
        ui.redraw();
        assert_eq!(ui.app.ui_snapshot().typed_field, None, "Escape closed it");
    }

    /// Without the control channel's pointer state, the field is not reported,
    /// as with `pointer` (control-channel.md).
    #[test]
    fn the_typed_field_is_null_without_the_control_channel() {
        let mut ui = Ui::open("subtractive.ron");
        let id = select_new(&mut ui, "loom.filter.svf");
        focus_control(&mut ui, id, "cutoff");
        ui.typewrite("4");
        ui.redraw();
        assert_eq!(ui.app.ui_snapshot().typed_field, None);
    }

    /// Typing 128 on a focused Tempo knob sets exactly 128 BPM, as one undo
    /// step. A drag or an arrow key rarely lands on it (panel-widgets.md,
    /// Typing a value).
    #[test]
    fn typing_on_a_focused_knob_sets_the_exact_value() {
        use keyboard::key::Named;
        let mut ui = Ui::open("sine.ron");
        let transport = ui.app.controller.transport_module().expect("a transport");
        let _ = ui.app.update(Message::SelectModule(transport.0));
        ui.redraw();
        focus_control(&mut ui, transport, "tempo");
        let steps = ui.app.edits.history.len();
        ui.typewrite("128");
        ui.tap(keyboard::Key::Named(Named::Enter), keyboard::Modifiers::default());
        assert_eq!(param_value(&ui, transport, "tempo"), 128.0);
        assert_eq!(ui.app.edits.history.len(), steps + 1, "one commit is one undo step");
    }

    /// "2k" commits 2000, and "abc" leaves the value and blocks Enter. Then
    /// Backspace corrects it, and the module is still there: Backspace in the
    /// field never reaches the canvas.
    #[test]
    fn typed_text_commits_units_and_bad_text_is_corrected_in_place() {
        use keyboard::key::Named;
        let none = keyboard::Modifiers::default();
        let mut ui = Ui::open("sine.ron");
        let id = select_new(&mut ui, "test.panel");
        focus_control(&mut ui, id, "cutoff");
        ui.typewrite("2k");
        ui.tap(keyboard::Key::Named(Named::Enter), none);
        assert_eq!(param_value(&ui, id, "cutoff"), 2000.0);

        focus_control(&mut ui, id, "tempo");
        ui.typewrite("1");
        ui.typewrite("abc");
        let before = param_value(&ui, id, "tempo");
        ui.tap(keyboard::Key::Named(Named::Enter), none);
        assert_eq!(param_value(&ui, id, "tempo"), before, "bad text commits nothing");
        for _ in 0..3 {
            ui.tap(keyboard::Key::Named(Named::Backspace), none);
        }
        ui.typewrite("50");
        ui.tap(keyboard::Key::Named(Named::Enter), none);
        assert_eq!(param_value(&ui, id, "tempo"), 150.0);
        assert!(ui.app.controller.graph().node(id).is_some(), "the module stays");
    }

    /// Enter then Enter on a value the text rounds keeps the exact value, with
    /// no undo step (panel-widgets.md, Typing a value, Unchanged text).
    #[test]
    fn enter_then_enter_keeps_a_value_the_text_rounds() {
        use keyboard::key::Named;
        let none = keyboard::Modifiers::default();
        let mut ui = Ui::open("sine.ron");
        let id = select_new(&mut ui, "test.panel");
        let index = ui
            .app
            .controller
            .graph()
            .node(id)
            .unwrap()
            .descriptor
            .param_index("tempo")
            .unwrap();
        let _ = ui.app.update(Message::ParamValue {
            module: id.0,
            index,
            value: 127.96,
        });
        let _ = ui.app.update(Message::SliderReleased);
        ui.redraw();
        focus_control(&mut ui, id, "tempo");
        let steps = ui.app.edits.history.len();
        ui.tap(keyboard::Key::Named(Named::Enter), none);
        ui.tap(keyboard::Key::Named(Named::Enter), none);
        assert_eq!(param_value(&ui, id, "tempo"), 127.96);
        assert_eq!(ui.app.edits.history.len(), steps, "no undo step");
    }

    /// The inspector's field: typing is a draft, Enter commits it as one undo
    /// step, and bad text shows its reason and commits nothing. Escape drops a
    /// draft, and in MIDI learn mode the row has no field.
    #[test]
    fn the_inspector_takes_typed_values() {
        use keyboard::key::Named;
        let mut ui = Ui::open("sine.ron");
        let id = select_new(&mut ui, "test.panel");
        let node = ui.app.controller.graph().node(id).unwrap();
        let index = node.descriptor.param_index("cutoff").unwrap();
        let module = id.0;
        let text = |text: &str| Message::ValueText {
            module,
            index,
            text: text.to_string(),
        };
        let field = |ui: &Ui| {
            let crate::inspector::Inspector::Module(rows) = ui.app.inspector_model() else {
                panic!("a module row");
            };
            rows.params.into_iter().find(|p| p.index == index).unwrap().field
        };
        assert_eq!(field(&ui).unwrap().error, None);

        let steps = ui.app.edits.history.len();
        let _ = ui.app.update(text("440 hz"));
        assert_eq!(field(&ui).unwrap().text, "440 hz");
        let _ = ui.app.update(Message::ValueSubmit { module, index });
        assert_eq!(param_value(&ui, id, "cutoff"), 440.0);
        assert_eq!(ui.app.edits.history.len(), steps + 1);
        assert!(ui.app.value_draft.is_none());

        let _ = ui.app.update(text("abc"));
        assert_eq!(field(&ui).unwrap().error.as_deref(), Some("Not a number in Hz"));
        let _ = ui.app.update(Message::ValueSubmit { module, index });
        assert_eq!(param_value(&ui, id, "cutoff"), 440.0, "bad text commits nothing");
        assert!(ui.app.value_draft.is_some(), "and stays to be corrected");

        // Escape drops it, through the same listener the app uses.
        let messages = ui.tap(keyboard::Key::Named(Named::Escape), keyboard::Modifiers::default());
        assert!(messages.iter().any(|m| matches!(m, Message::ValueDraftClear)));
        assert!(ui.app.value_draft.is_none());

        // A press with no field focused drops a draft too.
        let _ = ui.app.update(text("5"));
        ui.step(&[]);
        let at = ui.control(module, "level");
        ui.move_to(at);
        ui.press();
        ui.release();
        assert!(ui.app.value_draft.is_none(), "focus moved away");

        // In learn mode the row is plain text, so a click arms it.
        let _ = ui.app.update(Message::Learn(crate::learn::Message::Toggle));
        assert!(field(&ui).is_none());
    }

    /// Selects a `test.panel` module, and clicks its cutoff value field with a
    /// real press. Returns the module and the parameter index.
    fn focus_cutoff_field(ui: &mut Ui) -> (ModuleId, usize) {
        let id = select_new(ui, "test.panel");
        let index = ui
            .app
            .controller
            .graph()
            .node(id)
            .unwrap()
            .descriptor
            .param_index("cutoff")
            .unwrap();
        let at = ui.value_field(id.0, index);
        ui.move_to(at);
        ui.press();
        ui.release();
        assert!(ui.app.text_focus.any, "the app knows the field has focus");
        (id, index)
    }

    /// The module's position, to see that a key did not move it.
    fn tile_position(ui: &Ui, id: ModuleId) -> Point {
        ui.app.positions[&id.0]
    }

    /// Enter in an inspector field commits only that parameter. It never
    /// focuses a control on the tile, so the next number edits nothing else.
    #[test]
    fn enter_in_an_inspector_field_leaves_the_canvas_alone() {
        use keyboard::key::Named;
        let mut ui = Ui::open("sine.ron");
        let (id, index) = focus_cutoff_field(&mut ui);
        let params = |ui: &Ui| ui.app.controller.graph().node(id).unwrap().params.clone();
        let steps = ui.app.edits.history.len();

        let enter = keyboard::Key::Named(Named::Enter);
        ui.typewrite("440");
        ui.tap(enter.clone(), keyboard::Modifiers::default());
        assert_eq!(param_value(&ui, id, "cutoff"), 440.0);
        let others = params(&ui);

        // The commit selected the new text, so this number replaces it.
        ui.typewrite("880");
        ui.tap(enter, keyboard::Modifiers::default());
        assert_eq!(param_value(&ui, id, "cutoff"), 880.0);
        let now = params(&ui);
        for (i, (before, after)) in others.iter().zip(&now).enumerate() {
            if i != index {
                assert_eq!(before, after, "parameter {i} did not change");
            }
        }
        assert_eq!(ui.app.edits.history.len(), steps + 2, "one undo step per commit");
    }

    /// Keys that edit a text field never act on the canvas behind it: not an
    /// arrow, Space, Backspace, or a letter with the pointer over the canvas.
    #[test]
    fn keys_in_an_inspector_field_do_not_reach_the_canvas() {
        use keyboard::key::Named;
        let mut ui = Ui::open("sine.ron");
        let (id, _) = focus_cutoff_field(&mut ui);
        let place = tile_position(&ui, id);
        let running = ui.app.controller.transport().running;
        let steps = ui.app.edits.history.len();

        let none = keyboard::Modifiers::default();
        ui.tap(keyboard::Key::Named(Named::ArrowLeft), none);
        ui.tap(keyboard::Key::Named(Named::ArrowRight), none);
        assert_eq!(tile_position(&ui, id), place, "an arrow moves the caret, not the tile");
        ui.tap(keyboard::Key::Named(Named::Space), none);
        assert_eq!(ui.app.controller.transport().running, running, "Space is not Run");

        // Over the canvas, with the module selected, Backspace would delete it
        // and `a` would open the browser. The field has the keys all the same.
        let tile = ui.control(id.0, "level");
        ui.cursor = mouse::Cursor::Available(tile);
        ui.tap(keyboard::Key::Named(Named::Backspace), none);
        assert!(ui.app.controller.graph().node(id).is_some(), "Backspace edits the text");
        ui.typewrite("a");
        assert!(ui.app.browser.is_none());
        assert_eq!(ui.app.edits.history.len(), steps);

        // Escape ends the field's focus and keeps the selection. The next one
        // reaches the canvas again.
        ui.tap(keyboard::Key::Named(Named::Escape), none);
        assert!(
            ui.app.selection.contains(&id.0),
            "Escape in a field keeps the selection"
        );
        assert!(!ui.app.text_focus.any, "the field gave up focus");
        ui.tap(keyboard::Key::Named(Named::Escape), none);
        assert!(ui.app.selection.is_empty());
    }

    /// Tab closes a field with no change and moves the canvas's focus as usual.
    #[test]
    fn tab_closes_an_inspector_field() {
        use keyboard::key::Named;
        let mut ui = Ui::open("sine.ron");
        let (id, _) = focus_cutoff_field(&mut ui);
        let before = param_value(&ui, id, "cutoff");
        ui.typewrite("5");
        assert!(ui.app.value_draft.is_some());
        ui.tap(keyboard::Key::Named(Named::Tab), keyboard::Modifiers::default());
        assert!(ui.app.value_draft.is_none());
        assert!(!ui.app.text_focus.any);
        assert_eq!(param_value(&ui, id, "cutoff"), before);
    }

    /// Tab in a value field also moves the canvas's focus, so a keyboard user
    /// keeps their place. Only a field in a view's list keeps Tab for itself.
    #[test]
    fn tab_from_a_value_field_moves_the_canvas() {
        use keyboard::key::Named;
        let mut ui = Ui::open("sine.ron");
        let (id, _) = focus_cutoff_field(&mut ui);
        assert!(!ui.app.tab_moves_between_fields(), "a value field is in no list");
        let messages = ui.tap(keyboard::Key::Named(Named::Tab), keyboard::Modifiers::default());
        assert!(
            messages.iter().any(|m| matches!(m, Message::Canvas(_))),
            "the canvas took Tab: {messages:?}"
        );
        assert_ne!(ui.app.selection.iter().copied().next(), Some(id.0), "focus moved on");
    }

    /// A draft ends when focus moves to another value field, and a new patch
    /// starts with no draft and no field holding the keys.
    #[test]
    fn a_draft_ends_when_focus_moves_or_the_patch_changes() {
        let mut ui = Ui::open("sine.ron");
        let (id, _) = focus_cutoff_field(&mut ui);
        ui.typewrite("abc");
        assert_eq!(
            ui.app.value_draft.as_ref().unwrap().text,
            "abc",
            "bad text keeps its focus"
        );
        assert!(ui.app.text_focus.any);

        let node = ui.app.controller.graph().node(id).unwrap();
        let other = node.descriptor.param_index("level").unwrap();
        let at = ui.value_field(id.0, other);
        ui.move_to(at);
        ui.press();
        ui.release();
        assert!(ui.app.value_draft.is_none(), "the bad text went with the focus");
        assert!(ui.app.text_focus.any, "the other field has focus now");

        ui.typewrite("5");
        assert!(ui.app.value_draft.is_some());
        assert!(ui.app.replace(crate::Source::Empty));
        ui.step(&[]);
        assert!(ui.app.value_draft.is_none());
        assert!(!ui.app.text_focus.any);
    }

    /// A readout shows its value as text in the snapshot, at its own place,
    /// and a press on it selects the module and moves nothing. The meter on a
    /// value around zero starts from zero (panel-widgets.md, Readout, Meter on a
    /// display value).
    #[test]
    fn a_readout_shows_text_and_a_press_selects_its_module() {
        let mut ui = Ui::open("sine.ron");
        let id = ui.app.controller.add_module("test.readout").unwrap();
        ui.app.refresh();
        ui.step(&[]);
        ui.app.run_engine(4800);
        let snapshot = ui.app.ui_snapshot();
        let tile = snapshot.tiles.iter().find(|tile| tile.id == id.0).expect("the tile");
        let shown = |label: &str| {
            let widget = tile.widgets.iter().find(|w| w.label == label).unwrap();
            (widget.kind, widget.text.clone())
        };
        assert_eq!(shown("Count"), ("readout", "7".to_string()));
        assert_eq!(shown("Pitch"), ("readout", "B4 -20¢".to_string()));
        assert_eq!(shown("Input"), ("readout", "Second".to_string()));
        assert_eq!(shown("Cents").1, "12.5 cents");
        let view = ui.app.scene.module(id.0).unwrap();
        assert!(
            view.panel
                .widgets
                .iter()
                .any(|w| matches!(w.kind, loom_canvas::WidgetViewKind::Meter { zero } if zero == 0.5)),
            "the meter's bar starts from zero, in the middle"
        );

        let rect = view.widget_bounds(0).unwrap();
        let at = ui.canvas().to_window(rect.center());
        ui.move_to(at);
        ui.press();
        ui.release();
        assert_eq!(ui.app.selection, [id.0].into());
        let node = ui.app.controller.graph().node(id).unwrap();
        assert_eq!(node.params[0], 7.0, "a press edits nothing");
    }
}
