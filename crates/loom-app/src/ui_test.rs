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
            frames: None,
            hold_note: false,
            recovery_dir: None,
            no_recovery: false,
            test_modules: false,
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
        let browser_open = self.app.browser.is_some();
        let mut clipboard = std::mem::take(&mut self.clipboard);
        self.with_ui(|ui, renderer, cursor| {
            let (_, statuses) = ui.update(events, cursor, renderer, &mut clipboard, &mut messages);
            for (event, status) in events.iter().zip(statuses) {
                // `keyboard::listen` passes on the key events no widget
                // captured.
                if let (Event::Keyboard(key), event::Status::Ignored) = (event, status) {
                    messages.push(Message::Key(key.clone()));
                }
                // The open browser listens to keys whether or not a widget
                // used them.
                if browser_open && let Some(message) = crate::browser_key(event.clone(), status, window::Id::unique()) {
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
    fn with_ui(
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
        let ports = if output {
            node.descriptor.outputs
        } else {
            node.descriptor.inputs
        };
        let index = ports
            .iter()
            .position(|p| p.name == name)
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
        let to = ui.input(9, "right");
        assert!(!connected(&ui, (8, 0), (9, 1)));
        let cables = ui.app.controller.graph().cables().count();

        ui.drag(from, to, 12);

        assert!(
            connected(&ui, (8, 0), (9, 1)),
            "the drag made a cable from VCA out to right"
        );
        assert_eq!(ui.app.controller.graph().cables().count(), cables + 1);
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

    /// Ctrl+Z (Cmd+Z on macOS) reaches the app through the canvas and
    /// undoes the cable the drag made.
    #[test]
    fn undo_key_removes_a_dragged_cable() {
        let mut ui = Ui::open("subtractive.ron");
        let from = ui.output(8, "out");
        let to = ui.input(9, "right");
        ui.drag(from, to, 12);
        assert!(connected(&ui, (8, 0), (9, 1)));

        ui.tap(keyboard::Key::Character("z".into()), keyboard::Modifiers::COMMAND);

        assert!(!connected(&ui, (8, 0), (9, 1)), "undo removed the cable");
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
}
