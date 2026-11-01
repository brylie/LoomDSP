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
use iced::{Event, Point, Rectangle, Size, Task, event, keyboard, mouse, window};
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
        let browser_open = self.app.browser.is_some() && self.app.help.is_none();
        let matrix_keys = self.app.matrix.tab == crate::matrix::Tab::Matrix && self.app.matrix.focused;
        let mut clipboard = std::mem::take(&mut self.clipboard);
        self.with_ui(|ui, renderer, cursor| {
            let (_, statuses) = ui.update(events, cursor, renderer, &mut clipboard, &mut messages);
            for (event, status) in events.iter().zip(statuses) {
                // `keyboard::listen` passes on the key events no widget
                // captured.
                if let (Event::Keyboard(key), event::Status::Ignored) = (event, status) {
                    messages.push(Message::Key(key.clone()));
                }
                if matrix_keys
                    && let Event::Keyboard(key) = event
                    && let Some(message) = crate::matrix::key_message(key.clone(), status)
                {
                    messages.push(message);
                }
                // The open browser listens to keys whether or not a widget
                // used them.
                if browser_open && let Some(message) = crate::browser_key(event.clone(), status, window::Id::unique()) {
                    messages.push(message);
                }
                if let Some(message) = crate::help_key(event.clone(), status, window::Id::unique()) {
                    messages.push(message);
                }
                // The panic key listens to every key event (panic.md).
                if let Some(message) = crate::panic::key(event.clone(), status, window::Id::unique()) {
                    messages.push(message);
                }
            }
        });
        self.clipboard = clipboard;
        let mut queue: VecDeque<Message> = messages.iter().cloned().collect();
        while let Some(message) = queue.pop_front() {
            let task = self.app.update(message);
            self.run(task, &mut queue, &mut messages);
        }
        let theme = self.app.theme();
        let style = Style {
            text_color: theme.palette().text,
        };
        self.with_ui(|ui, renderer, cursor| ui.draw(renderer, &theme, &style, cursor));
        messages
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
        assert!(ui.app.status.contains("already modulates"));
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
        assert_eq!(ui.app.assign_drag.expect("window drag").zoom, ui.canvas().camera.zoom);
        assert_eq!(ui.app.drop_row, Some((4, 0)));
        ui.move_to(resonance);
        assert_eq!(ui.app.drop_row, Some((4, 1)));
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
        assert_eq!(ui.app.drop_row, None);
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
            assert!(ui.app.assign_drag.is_none());
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
        assert!(ui.app.assign_drag.is_some_and(|drag| drag.at.is_none()));
        ui.release();
        assert_eq!(ui.app.controller.graph().assignments().count(), before);
        let _ = ui.app.update(Message::ToggleLearn);
        ui.drag(from, row, 8);
        assert_eq!(ui.app.controller.graph().assignments().count(), before);
        assert_eq!(ui.app.drop_row, None);
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
        assert_eq!(ui.app.assign_drag.expect("output drag").at, Some(input));
        let empty = Point::new(1000.0, 800.0);
        ui.move_to(empty);
        assert_eq!(ui.app.assign_drag.expect("output drag").at, Some(empty));
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
        assert_eq!(ui.app.status, "Cannot connect to an unknown module");
        assert_eq!(ui.app.controller.graph().cables().count(), 0);
        let from = ui.output(1, "out");
        let to = ui.input(2, "pitch");
        ui.drag(from, to, 8);
        assert_eq!(ui.app.status, "Cannot connect to an unknown module");
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
        assert!(ui.text("0.100").is_some(), "clamped into -0.1 to 0.1");
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
}
