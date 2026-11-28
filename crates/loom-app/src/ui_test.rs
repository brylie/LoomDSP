//! Headless UI tests: real pointer and key events sent to the app's window.
//!
//! The other app tests call `App::update` with messages. These go one step
//! earlier: the [`Ui`] harness runs the app in `loom_ui::Runtime`, as the
//! window does, with a driver of its own (`ui_test/driver.rs`), and hands
//! the runtime each event, so routing, focus, the custom canvas, and the
//! turn run through the same code as in the window (ui-layer.md, section
//! 10, rule 1). Each message is one update, and its effects run in memory:
//! the clipboard is a string, work runs inline, a capture draws the window
//! offscreen with the window's own painter (`drawing.rs`) and answers that
//! there is no GPU where no adapter is found, and Quit marks
//! [`Ui::exited`]. A dialog is answered from the fake dialogs in the
//! crate's tests; in a test under `tests/`, where they are off, it panics,
//! naming it.
//!
//! The window is laid out with the same `Typesetter` the window draws with
//! (section 10, rule 3), so every part lands where the window draws it, and
//! event tests need no GPU and never start one.
//!
//! Tests find things by name, not by pixel: [`Ui::output`], [`Ui::input`],
//! [`Ui::control`], and [`Ui::text`] return window coordinates from the
//! runtime's layout, the canvas's camera, and the scene. So a test keeps
//! working when a tile moves or the inspector gets wider.
//!
//! Time is the harness's own clock (`clock.rs`, #868), not the system's:
//! each step moves it on by [`STEP`], [`Ui::wait`] moves it on by a chosen
//! amount, and [`Ui::redraw_at`] moves it to the redraw's time. Sleeping
//! does not move it. So the outcome of a test that depends on time, such as
//! a double-click, does not depend on how busy the machine is. Two clicks
//! on the same control or on empty canvas in one test count as a
//! double-click (a control resets, the browser opens). Wait more than the
//! double-click time in between, or click something else, when a test
//! needs two single clicks.
//!
//! Not covered: what the operating system and winit deliver, for example
//! events while the pointer is outside the window.

mod a11y;
mod driver;

use crate::canvas::{CanvasElement, canvas_id};
use crate::offscreen::Offscreen;
use crate::{App, Args, Message, screenshot};
pub use a11y::{A11yNode, A11yTarget, A11yTree, A11yValue};
use driver::UiDriver;
use loom_canvas::CanvasView;
use loom_ui::paint::Typesetter;
use loom_ui::shell::Gate;
use loom_ui::{DriverEvent, Runtime, WindowState};
use loom_ui_core::{
    Code, Event as UiEvent, Key, KeyEvent, Modifiers, Point, PointerButton, PointerEvent, Rect, Size as UiSize,
};
// The tests reach Iced's values through `use super::*` until their values
// change to ours, alone, in commit 6 of plan step P4 (ui-layer.md, section
// 10, rule 2).
#[cfg(test)]
use iced::{mouse, window};
#[cfg(test)]
use loom_canvas::iced_convert::ToIced as _;
// Iced's event, which the harness's methods still take from tests.
use iced::Event;
// The harness tests reach these through `use super::*`.
#[cfg(test)]
use loom_core::ModuleId;
use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// The window size the app opens with.
pub(crate) const WINDOW: UiSize = crate::WINDOW;

/// How far the harness's clock moves on each step: about the time between
/// two events a person makes in quick succession, and well inside a
/// double-click.
pub(crate) const STEP: Duration = Duration::from_millis(1);

/// The most turns the harness runs for one event before it gives up: a
/// loop of turns that never ends, such as a sync that keeps sending,
/// fails the test instead of hanging it.
const MOST_TURNS: usize = 1000;

/// The app's window, driven by events the way the shell drives it.
pub struct Ui {
    /// The runtime the window runs, around the app.
    runtime: Runtime<App, crate::layers::NoToolkit>,
    /// Carries out the effects in memory.
    driver: UiDriver,
    /// How many wake-ups the runtime's waker gave since the last turn.
    wakes: Arc<AtomicUsize>,
    /// Set while the harness itself puts a message in the queue: that turn
    /// runs at once, so it needs no wake-up.
    quiet: Arc<AtomicBool>,
    /// Lays out and draws the window's text, as the window's layers do.
    text: RefCell<Typesetter>,
    /// The GPU for captures, made at the first one: `Some(None)` when no
    /// adapter was found.
    offscreen: Option<Option<Offscreen>>,
    /// Where the pointer is, in window logical pixels.
    cursor: Option<Point>,
    /// The copy folder [`Ui::open`] made for this `Ui`, removed when it
    /// drops. Kept from the start, because the open file can change after
    /// New, Open, or Save As. `None` for an app the test built.
    copy: Option<PathBuf>,
    /// How many turns said to draw a frame, as the window would.
    draws_asked: usize,
}

/// One input a test gives the harness: ours, or Iced's until the tests'
/// values change to ours (plan for leaving Iced, step P4, commit 6).
pub(crate) enum Input {
    /// An event, with where the pointer is when it moves it.
    Event(UiEvent, Option<Point>),
    /// A redraw, as the window draws a frame.
    Redraw,
    /// A window event, as the shell hands the runtime one.
    Window(loom_ui::WindowEvent),
}

/// A value a test hands [`Ui::step`] as one input.
pub(crate) trait IntoInput {
    /// The input, or `None` for an event that means nothing to the window.
    fn into_input(self) -> Option<Input>;
}

impl IntoInput for UiEvent {
    fn into_input(self) -> Option<Input> {
        Some(Input::Event(self, None))
    }
}

impl IntoInput for &UiEvent {
    fn into_input(self) -> Option<Input> {
        Some(Input::Event(self.clone(), None))
    }
}

impl IntoInput for iced::Event {
    fn into_input(self) -> Option<Input> {
        (&self).into_input()
    }
}

impl IntoInput for &iced::Event {
    fn into_input(self) -> Option<Input> {
        use loom_canvas::iced_convert::ToCore as _;
        match self {
            iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => Some(Input::Event(
                UiEvent::Pointer(PointerEvent::Moved),
                Some(position.to_core()),
            )),
            iced::Event::Window(iced::window::Event::RedrawRequested(_)) => Some(Input::Redraw),
            iced::Event::Window(iced::window::Event::Unfocused) => {
                Some(Input::Window(loom_ui::WindowEvent::Focused(false)))
            }
            iced::Event::Window(iced::window::Event::Focused) => {
                Some(Input::Window(loom_ui::WindowEvent::Focused(true)))
            }
            event => loom_canvas::iced_convert::event_to_core(event).map(|event| Input::Event(event, None)),
        }
    }
}

/// The example patch `patch` from `examples/patches` in the repository.
pub(crate) fn example_path(patch: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/patches")
        .join(patch)
}

/// The name of the one folder, in the system's temporary folder, that
/// holds every folder [`example_copy`] makes. The temporary folder is
/// shared with every program on the machine and can hold a hundred
/// thousand entries, so the sweep reads only this folder of our own,
/// never the shared one (docs/benchmarks/2026-11-26-ui-test-cost.md).
const COPIES: &str = "loom-app-ui";

/// The folder that holds every copy folder: [`COPIES`] in the system's
/// temporary folder.
fn copies_root() -> PathBuf {
    std::env::temp_dir().join(COPIES)
}

/// True when `dir` is one of the folders [`example_copy`] makes: directly
/// inside [`copies_root`]. Nothing else is ever removed.
fn is_copy_folder(dir: &std::path::Path) -> bool {
    let Ok(root) = std::fs::canonicalize(copies_root()) else {
        return false;
    };
    dir.parent()
        .and_then(|parent| std::fs::canonicalize(parent).ok())
        .is_some_and(|parent| parent == root)
}

/// Removes the copy folders left by earlier runs: those more than an hour
/// old, so a folder another test process is using now stays. Once a
/// process, on its first copy. It reads [`copies_root`] alone, so its cost
/// follows the number of copies left behind, not what else is in the
/// system's temporary folder.
fn sweep_old_copies() {
    let Ok(entries) = std::fs::read_dir(copies_root()) else {
        return;
    };
    let hour = std::time::Duration::from_secs(3600);
    for entry in entries.flatten() {
        let dir = entry.path();
        let old = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > hour);
        if old && is_copy_folder(&dir) {
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

/// A copy of the example patch `patch`, in a new folder of
/// [`copies_root`] for this test alone: the process ID and a count keep
/// tests that run at once apart. A save in a test then writes the copy.
/// A [`Ui`] removes the folder when it drops; any other is cleared by a
/// later run's sweep ([`sweep_old_copies`]).
///
/// Anything that is not an example in the repository is opened as it was
/// before: a test's own file by its absolute path (joined to the examples
/// folder, it stays itself), or a name that is no file, which opens the
/// app with no patch.
pub(crate) fn example_copy(patch: &str) -> PathBuf {
    example_copy_owned(patch).0
}

/// [`example_copy`], with the folder it made for the copy. The folder is
/// `None` when the patch was opened as it was, so a caller owns only a
/// folder made for it, never one a path it was given happens to sit in.
fn example_copy_owned(patch: &str) -> (PathBuf, Option<PathBuf>) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    static SWEPT: std::sync::Once = std::sync::Once::new();
    let source = example_path(patch);
    if std::path::Path::new(patch).is_absolute() || !source.is_file() {
        return (source, None);
    }
    SWEPT.call_once(sweep_old_copies);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = copies_root().join(format!("{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a folder for the copy");
    // Named by the source file alone, so a relative name with `..` in it
    // still puts the copy inside the folder made for it.
    let copy = dir.join(source.file_name().expect("an example is a file"));
    std::fs::copy(&source, &copy).unwrap_or_else(|why| panic!("copy {patch}: {why}"));
    (copy, Some(dir))
}

/// The app [`Ui::open`] runs, for a test that applies messages to it
/// directly: a patch from `examples/patches`, with no audio. It opens a
/// copy of the patch in a folder of its own ([`example_copy`]), so a test
/// that saves writes the copy, never the example in the repository. A
/// [`Ui`] removes its copy when it drops; a test that uses this app with
/// no `Ui` leaves its copy for a later run's sweep, an hour on.
pub(crate) fn open_app(patch: &str) -> App {
    app_at(example_copy(patch))
}

/// The app [`open_app`] runs, on the patch file at `path`.
fn app_at(path: PathBuf) -> App {
    App::new(Args {
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
    })
}

/// A `Ui` removes the copy folder it made, so copies do not pile up
/// across runs. Only a copy folder is removed ([`is_copy_folder`]).
impl Drop for Ui {
    fn drop(&mut self) {
        if let Some(dir) = self.copy.take().filter(|dir| is_copy_folder(dir)) {
            let _ = std::fs::remove_dir_all(dir);
        }
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
    pub fn tap_key(&mut self, key: Key, modifiers: Modifiers) {
        assert!(
            !modifiers.shortcut(),
            "command shortcuts reach real files and dialogs from tests/: use Ui::open in a unit test"
        );
        self.tap(key, modifiers);
    }

    /// The app, to read.
    pub(crate) fn app(&self) -> &App {
        self.runtime.app()
    }

    /// The app, for a test to set up state between turns. The runtime's
    /// queue, deadline, and focus stay as they are, so a test moves focus
    /// with a focus effect or a click, never by setting the app's copy.
    pub(crate) fn app_mut(&mut self) -> &mut App {
        // What a test changes here, outside a turn, shows on the next frame,
        // and the canvas syncs with it as after an update (`canvas.rs`).
        self.runtime.mark_view_stale();
        let app = self.runtime.app_mut();
        app.updates += 1;
        app
    }

    /// Takes every whole accessibility tree the app sent since the last
    /// call, oldest first: what a screen reader was handed.
    pub(crate) fn take_sent_trees(&mut self) -> Vec<accesskit::TreeUpdate> {
        std::mem::take(&mut self.driver.trees)
    }

    /// Takes the incremental accessibility updates the screen reader received.
    pub(crate) fn take_sent_changes(&mut self) -> Vec<accesskit::TreeUpdate> {
        std::mem::take(&mut self.driver.changes)
    }

    /// The clipboard's text.
    pub(crate) fn clipboard(&self) -> Option<&str> {
        self.driver.headless.clipboard()
    }

    /// Puts `text` on the clipboard, or empties it.
    pub(crate) fn set_clipboard(&mut self, text: Option<String>) {
        self.driver.headless.set_clipboard(text);
    }

    /// True once the app has quit.
    pub(crate) fn exited(&self) -> bool {
        self.driver.headless.exited()
    }

    /// Puts the pointer at `cursor`, or takes it out of the window, with
    /// no event: as when the window stops hearing the pointer.
    pub(crate) fn set_cursor(&mut self, cursor: impl Into<Cursor>) {
        self.cursor = cursor.into().0;
    }

    /// True when a GPU adapter was found, so a capture draws the window,
    /// the canvas included. Events never need one.
    pub(crate) fn draws_the_canvas(&mut self) -> bool {
        self.offscreen().is_some()
    }

    /// The GPU for captures, made the first time one is asked for.
    fn offscreen(&mut self) -> Option<&mut Offscreen> {
        self.offscreen.get_or_insert_with(Offscreen::new).as_mut()
    }

    /// A waker into the app's runtime, as a watcher thread holds one.
    pub(crate) fn waker(&self) -> loom_ui::Waker<Message> {
        self.runtime.waker()
    }

    /// Presses Open on the toolbar. From `tests/`, where the fake dialogs are
    /// off, the dialog it asks for has no answer, so the harness panics,
    /// naming it. Save is not offered here: it would overwrite the example.
    pub fn press_open(&mut self) {
        // By its UI ID: a sub-patch's inspector shows an Open of its own.
        self.lay_out_panels();
        let open = crate::toolbar::toolbar_id().with("open");
        let at = self.runtime().bounds(&open).expect("the toolbar's Open");
        let _ = self.move_to(Point::new(at.x + at.width / 2.0, at.y + at.height / 2.0));
        let _ = self.press();
        let _ = self.release();
    }

    /// What the window shows, as `--dump-ui` prints it.
    pub fn snapshot_json(&self) -> serde_json::Value {
        serde_json::to_value(self.app().ui_snapshot()).expect("the snapshot is plain data")
    }

    /// Opens a patch from `examples/patches` with no audio, as the app does
    /// in a window of [`WINDOW`] size, and draws the first frame.
    pub(crate) fn open(patch: &str) -> Self {
        let (path, copy) = example_copy_owned(patch);
        let mut ui = Self::with_app(app_at(path));
        ui.copy = copy;
        ui
    }

    /// Draws the first frame of an app built by the test, for example one
    /// that plays with `--fake-audio`.
    pub(crate) fn with_app(mut app: App) -> Self {
        // The harness's own time, which moves only as the test does (#868).
        app.clock = crate::clock::Clock::stepped(Instant::now());
        app.message_log = Some(Vec::new());
        let wakes = Arc::new(AtomicUsize::new(0));
        let quiet = Arc::new(AtomicBool::new(false));
        let wake = {
            let (wakes, quiet) = (wakes.clone(), quiet.clone());
            Arc::new(move || {
                if !quiet.load(Ordering::SeqCst) {
                    wakes.fetch_add(1, Ordering::SeqCst);
                }
            })
        };
        let window = WindowState::new(WINDOW);
        let mut ui = Self {
            runtime: Runtime::new(app, crate::layers::NoToolkit, window, wake),
            driver: UiDriver::default(),
            wakes,
            quiet,
            text: RefCell::new(Typesetter::new()),
            offscreen: None,
            cursor: None,
            copy: None,
            draws_asked: 0,
        };
        // The first turn builds the view and reads the first deadline, such
        // as the crash recovery question (autosave.md, Crash recovery); the
        // first frame places the canvas, which then frames the patch.
        ui.turn(DriverEvent::Woken);
        ui.redraw();
        ui
    }

    /// One turn of the runtime for `event`, at the harness's time, then
    /// what comes of it until nothing waits: messages for the next turn,
    /// wake-ups, a deadline that has come, and the captures, drawn
    /// offscreen.
    fn turn(&mut self, event: DriverEvent) {
        let now = self.app().clock.now();
        let mut next = self.runtime.handle(event, now, &mut self.driver);
        let mut turns = 0;
        loop {
            self.draws_asked += usize::from(next.draw);
            turns += 1;
            assert!(
                turns < MOST_TURNS,
                "the runtime kept running turns for one event; the last messages: {:?}",
                self.app()
                    .message_log
                    .as_ref()
                    .map(|log| log.iter().rev().take(5).collect::<Vec<_>>())
            );
            self.answer_captures();
            let now = self.app().clock.now();
            let event = if next.again || self.wakes.swap(0, Ordering::SeqCst) > 0 {
                DriverEvent::Woken
            } else if next.deadline.is_some_and(|at| at <= now) {
                DriverEvent::DeadlineReached
            } else {
                // The window lays out and draws only at a frame, which the
                // harness's steps and redraws make (`Ui::frame`).
                break;
            };
            next = self.runtime.handle(event, now, &mut self.driver);
        }
    }

    /// An animation frame at `at`, as the window's display refresh gives
    /// one while something animates, and the messages that came of it.
    pub(crate) fn animation_frame(&mut self, at: Instant) -> Vec<Message> {
        self.app().clock.advance_to(at);
        self.turn(DriverEvent::Frame);
        self.take_log()
    }

    /// Calls `f` with the shell's parts for this window, as the shell hands
    /// them to the layers between turns.
    pub(crate) fn with_parts<R>(
        &mut self,
        f: impl FnOnce(loom_ui::shell::Parts<'_, App, crate::layers::NoToolkit>) -> R,
    ) -> R {
        let waker = self.runtime.waker();
        let window = self.runtime.window();
        let (app, target, panels) = self.runtime.parts();
        f(loom_ui::shell::Parts {
            app,
            target,
            waker: &waker,
            window,
            panels,
        })
    }

    /// How many turns so far said to draw a frame, as the window would
    /// draw one after each.
    pub(crate) fn draws_asked(&self) -> usize {
        self.draws_asked
    }

    /// Draws the window offscreen for each capture that waits, and answers
    /// it, or answers that there is no GPU.
    fn answer_captures(&mut self) {
        let captures = std::mem::take(&mut self.driver.captures);
        if captures.is_empty() {
            return;
        }
        self.lay_out_now();
        let scale = self.runtime.window().scale;
        let size = self.runtime.window().size;
        let waker = self.runtime.waker();
        let image = {
            let Self {
                runtime,
                offscreen,
                text,
                ..
            } = self;
            let offscreen = offscreen.get_or_insert_with(Offscreen::new).as_mut();
            let (_, _, mut panels) = runtime.parts();
            match offscreen {
                Some(offscreen) => offscreen.window(&mut panels, text, size, scale).and_then(|rgba| {
                    let (width, height) = crate::offscreen::physical(size, scale);
                    loom_ui::effect::Image::new(width, height, scale, rgba)
                }),
                None => None,
            }
        };
        for reply in captures {
            let answer = image.clone().ok_or(loom_ui::effect::NoGpu);
            waker.send(reply.give(answer));
        }
    }

    /// One frame of input: each event goes to the runtime as the window
    /// hands it over, at the harness's time, then the frame is drawn and
    /// the control channel's next scripted step goes in, one a frame.
    /// Returns every message `update` got, in order.
    pub(crate) fn step(&mut self, events: &[Event]) -> Vec<Message> {
        self.step_inputs(events.iter().filter_map(IntoInput::into_input).collect())
    }

    /// [`Ui::step`] with our own events, as the harness's methods give them.
    fn step_ui(&mut self, events: &[UiEvent]) -> Vec<Message> {
        self.step_inputs(events.iter().filter_map(IntoInput::into_input).collect())
    }

    /// [`Ui::step`] with the inputs made.
    fn step_inputs(&mut self, inputs: Vec<Input>) -> Vec<Message> {
        // Each step happens a little after the last, as a person's input
        // would (`STEP`).
        self.app().clock.advance(STEP);
        for input in inputs {
            match input {
                Input::Redraw => self.turn(DriverEvent::RedrawRequested),
                Input::Window(event) => self.turn(DriverEvent::Window(event)),
                Input::Event(event, at) => {
                    if let Some(at) = at {
                        self.cursor = Some(at);
                    }
                    self.real(event);
                }
            }
        }
        self.frame();
        let mut log = self.take_log();
        // A scripted step goes as a real one, each in a frame of its own,
        // as the window's layers give them (`Report::input`).
        while self.feed_one() {
            log.extend(self.take_log());
        }
        log
    }

    /// The control channel's next scripted step after a frame, as the
    /// window's layers give it (`Report::input`): its input goes in as real
    /// input, then a frame is drawn. False when no step waits.
    fn feed_one(&mut self) -> bool {
        let (input, jumps) = {
            self.lay_out_now();
            let (app, _, panels) = self.runtime.parts();
            let input = app.next_scripted(&panels);
            let jumps: Vec<Message> = input
                .iter()
                .flat_map(|step| app.press_messages(&panels, &step.event, step.at))
                .collect();
            (input, jumps)
        };
        if input.is_empty() {
            return false;
        }
        for message in jumps {
            self.waker().send(message);
        }
        for loom_ui::shell::ScriptedInput { event, at } in input {
            if let Some(at) = at {
                self.cursor = Some(at);
            }
            let pointer = self.cursor;
            self.turn(DriverEvent::Input { event, pointer });
        }
        self.frame();
        true
    }

    /// A real input event, as the window's shell hands it over: a real
    /// pointer goes through the script's gate first (`Pointer::gate`), the
    /// input the script held runs, the gate is asked again, and a press
    /// that passes then closes the cable jump list if it is outside it, as
    /// the layers do (`layers.rs`).
    fn real(&mut self, event: UiEvent) {
        if matches!(event, UiEvent::Pointer(_)) {
            loop {
                self.lay_out_now();
                let gate = self.app().pointer.gate(&event);
                match gate {
                    Gate::Hold => return,
                    Gate::Pass => break,
                    Gate::After(first) => {
                        for loom_ui::shell::ScriptedInput { event, at } in first {
                            let pointer = at.or(self.cursor);
                            self.turn(DriverEvent::Input { event, pointer });
                        }
                    }
                }
            }
            let jump = {
                let (app, _, panels) = self.runtime.parts();
                app.press_messages(&panels, &event, self.cursor)
            };
            self.waker().send_all(jump);
        }
        let pointer = self.cursor;
        self.turn(DriverEvent::Input { event, pointer });
    }

    /// A frame: the window is laid out and drawn, and what waited for it
    /// runs, as in the window (`Effects::after_draw`). The control channel
    /// keeps what the canvas says the pointer is on.
    fn frame(&mut self) {
        self.lay_out_panels();
        // The canvas builds its frame, as the window's prepare does.
        if let (Some(element), Some(bounds)) = (
            self.runtime.custom::<CanvasElement>(&canvas_id()),
            self.runtime.bounds(&canvas_id()),
        ) {
            element.build_frame(self.runtime.app(), bounds);
        }
        if self.runtime.waits_for_draw() {
            self.turn(DriverEvent::Presented);
        }
        let (app, _, panels) = self.runtime.parts();
        if app.pointer.records() {
            let (canvas, typed) = crate::canvas::panel_pointer(app, &panels);
            app.pointer.record_seen(self.cursor, canvas, typed);
        }
    }

    /// The messages `update` got since the last call, in order.
    fn take_log(&mut self) -> Vec<Message> {
        self.runtime
            .app_mut()
            .message_log
            .as_mut()
            .map(std::mem::take)
            .unwrap_or_default()
    }

    /// Every text the runtime paints in `layer` now, with the box it is
    /// placed in, in logical pixels: the room its element gives it, which
    /// holds its ink. For tests that check what sits next to what.
    pub(crate) fn painted_texts(&mut self, layer: loom_ui::Layer) -> Vec<(String, Rect)> {
        use loom_ui::paint::{Align, Vertical, testing::Painted};
        self.painted(layer)
            .iter()
            .filter_map(|painted| match painted {
                Painted::Text { text, run } => {
                    let (w, h) = (run.bounds.width, run.bounds.height);
                    let x = match run.align {
                        Align::Left => run.anchor.x,
                        Align::Center => run.anchor.x - w / 2.0,
                        Align::Right => run.anchor.x - w,
                    };
                    let y = match run.vertical {
                        Vertical::Center => run.anchor.y - h / 2.0,
                        Vertical::Top => run.anchor.y,
                    };
                    Some((text.clone(), Rect::new(Point::new(x, y), UiSize::new(w, h))))
                }
                _ => None,
            })
            .collect()
    }

    /// Every text the runtime paints in `layer` now, with its colour, in
    /// the order painted. For tests that check a look, such as an item
    /// turned off.
    pub(crate) fn painted_text_colours(&mut self, layer: loom_ui::Layer) -> Vec<(String, loom_ui_core::Color)> {
        use loom_ui::paint::testing::Painted;
        self.painted(layer)
            .iter()
            .filter_map(|painted| match painted {
                Painted::Text { text, run } => Some((text.clone(), run.colour)),
                _ => None,
            })
            .collect()
    }

    /// Every box the runtime paints in `layer` now, in the order painted.
    pub(crate) fn painted_quads(&mut self, layer: loom_ui::Layer) -> Vec<loom_ui::paint::Quad> {
        use loom_ui::paint::testing::Painted;
        self.painted(layer)
            .iter()
            .filter_map(|painted| match painted {
                Painted::Quad(quad) => Some(*quad),
                _ => None,
            })
            .collect()
    }

    /// What the runtime paints in `layer` now, as the window paints it.
    fn painted(&mut self, layer: loom_ui::Layer) -> Vec<loom_ui::paint::testing::Painted> {
        self.lay_out_panels();
        let style = crate::drawing::panel_style();
        let mut recorder = loom_ui::paint::testing::Recorder::default();
        let (_, _, mut panels) = self.runtime.parts();
        panels.paint(layer, &mut recorder, &style);
        recorder.painted
    }

    /// Lays out the window, as the window does before each frame, so hits
    /// and focus read what was drawn, and runs what the layout started: a
    /// canvas it placed hears so on the next turn.
    pub(crate) fn lay_out_panels(&mut self) {
        // A test that set the app's state directly gets the view it set,
        // through one turn with nothing to run.
        if self.runtime.view_is_stale() {
            self.turn(DriverEvent::Woken);
        }
        self.lay_out_now();
        if self.wakes.load(Ordering::SeqCst) > 0 {
            self.turn(DriverEvent::Woken);
            self.lay_out_now();
        }
    }

    /// Lays the window out with the harness's typesetter, and nothing more.
    fn lay_out_now(&mut self) {
        self.runtime.lay_out(&mut *self.text.borrow_mut());
    }

    /// The runtime, to read its own focus, hover, and layout.
    pub(crate) fn runtime(&self) -> &Runtime<App, crate::layers::NoToolkit> {
        &self.runtime
    }

    /// Adds `loom-ui` panels to the window for a routing test, and lays
    /// them out.
    pub(crate) fn with_test_panels(&mut self, panels: crate::test_panels::TestPanels) {
        self.app_mut().test_panels = Some(panels);
        // The runtime builds its view after a turn that ran an update, so
        // one harmless message brings the panels in.
        let _ = self.deliver(Message::Tick);
        self.lay_out_panels();
    }

    /// Applies `message` as the runtime does, then every message it
    /// produces and what those produce in turn, and returns them all.
    pub(crate) fn deliver(&mut self, message: Message) -> Vec<Message> {
        self.quiet.store(true, Ordering::SeqCst);
        self.runtime.waker().send(message);
        self.quiet.store(false, Ordering::SeqCst);
        self.turn(DriverEvent::Woken);
        self.take_log()
    }

    /// Handles an accessibility event, and draws a frame only if its turn
    /// asked for one.
    #[cfg(test)]
    pub(crate) fn accessibility_turn(&mut self, event: loom_ui::AccessEvent) -> Vec<Message> {
        let before = self.runtime.counts().redraws;
        self.turn(DriverEvent::Accessibility(event));
        if self.runtime.counts().redraws != before {
            self.frame();
        }
        self.take_log()
    }

    /// A screen reader's event, as the adapter gives the window one: the
    /// runtime's turn first, then what comes of it, as for a message.
    pub(crate) fn accessibility(&mut self, event: loom_ui::AccessEvent) -> Vec<Message> {
        self.turn(DriverEvent::Accessibility(event));
        self.take_log()
    }

    /// Takes focus from every field, as a focus effect to no element does,
    /// with no message of the test's own.
    pub(crate) fn unfocus_all(&mut self) -> Vec<Message> {
        // The app hears of it on the next turn (plan 3.2, row 4d).
        self.runtime.unfocus();
        self.deliver(Message::Tick)
    }

    /// A frame, as the window draws one: the canvas syncs with the app on
    /// it, frames the patch, and reports which lights are visible.
    pub(crate) fn redraw(&mut self) -> Vec<Message> {
        self.redraw_at(self.app().clock.now())
    }

    /// Lets `by` pass on the harness's clock, as a person would wait
    /// between two gestures. Nothing runs: the next step or redraw sees the
    /// new time. Use it instead of sleeping, which the harness's clock does
    /// not see.
    pub(crate) fn wait(&mut self, by: Duration) {
        self.app().clock.advance(by);
    }

    /// Lets `by` pass and wakes the runtime, as the window's timer does at
    /// a deadline the runtime asked for, with no input: a group of slider
    /// steps that ends 500 ms after its last ends here (ui-layer.md,
    /// section 5, rule 16).
    pub(crate) fn wake_after(&mut self, by: Duration) -> Vec<Message> {
        self.wait(by);
        self.turn(DriverEvent::Woken);
        self.take_log()
    }

    /// The harness's time now.
    pub(crate) fn now(&self) -> Instant {
        self.app().clock.now()
    }

    /// Resizes the harness's window, leaving the new layout for the next frame.
    #[cfg(test)]
    pub(crate) fn resize(&mut self, width: f32, height: f32) {
        self.turn(DriverEvent::Window(loom_ui::WindowEvent::Resized(UiSize::new(
            width, height,
        ))));
    }

    /// A window event, as the shell hands the runtime one: a close request
    /// or a dropped file. The app hears it through `App::window_event`, and
    /// what comes of it runs as for a message.
    #[cfg(test)]
    pub(crate) fn window_event(&mut self, event: loom_ui::WindowEvent) -> Vec<Message> {
        self.turn(DriverEvent::Window(event));
        self.take_log()
    }

    /// A frame at a chosen time, so a test can pass a timer's deadline
    /// without sleeping.
    pub(crate) fn redraw_at(&mut self, at: Instant) -> Vec<Message> {
        // The clock moves to the frame's time, never back.
        self.app().clock.advance_to(at);
        self.lay_out_panels();
        self.turn(DriverEvent::RedrawRequested);
        self.frame();
        // One scripted step a frame, as the window's layers give them.
        self.feed_one();
        self.take_log()
    }

    /// Presses a key and holds it. With `repeat`, it is a repeat the
    /// keyboard sends while the key stays down.
    pub(crate) fn key_down(&mut self, key: Key, repeat: bool) -> Vec<Message> {
        self.step(&[key_pressed(key, repeat)])
    }

    /// Lets go of a key.
    pub(crate) fn key_up(&mut self, key: Key) -> Vec<Message> {
        self.step(&[key_released(key, Modifiers::NONE)])
    }

    /// Clicks the first text the window shows that reads exactly
    /// `content`, such as a toolbar button's label.
    pub(crate) fn click_text(&mut self, content: &str) -> Vec<Message> {
        let at = self
            .text(content)
            .unwrap_or_else(|| panic!("no element shows `{content}`"))
            .center();
        let mut messages = self.move_to(at);
        messages.extend(self.press());
        messages.extend(self.release());
        messages
    }

    /// Sends one message, and everything that comes of it, as the runtime
    /// runs it: [`Ui::deliver`], with no log.
    pub(crate) fn send(&mut self, message: Message) {
        let _ = self.deliver(message);
    }

    /// Gives keyboard focus to the text field of `part`, as a click on it
    /// would, and draws the frame.
    pub(crate) fn focus(&mut self, part: &loom_ui_core::UiId) {
        self.lay_out_panels();
        let at = self
            .runtime()
            .bounds(part)
            .unwrap_or_else(|| panic!("no field `{part}` in the window"));
        let centre = Point::new(at.x + at.width / 2.0, at.y + at.height / 2.0);
        let _ = self.runtime.focus_at(centre);
        self.redraw();
    }

    /// True when a text field has keyboard focus.
    pub(crate) fn text_field_focused(&mut self) -> bool {
        self.runtime().focused_field().is_some()
    }

    /// The part whose text field has focus.
    pub(crate) fn focused_field(&mut self) -> Option<loom_ui_core::UiId> {
        self.runtime().focused_field().cloned()
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
        let draft = self.app().session.value_draft.as_ref().expect("a draft is typed");
        assert_eq!(draft.draft.text, text, "every key stays in the draft");
        self.tap(Key::Enter, Modifiers::NONE);
        assert!(self.app().session.value_draft.is_none(), "Enter commits it");
        assert_eq!(
            self.app().focus,
            self.app().value_field_part(module, index),
            "the field keeps focus"
        );
    }

    /// Where the inspector's value field for this parameter is in the window.
    pub(crate) fn value_field(&mut self, module: u32, index: usize) -> Point {
        let part = self
            .app()
            .value_field_part(module, index)
            .expect("the inspector shows that value field");
        self.field(&part)
    }

    /// Where the text field of `part` is in the window, to click on it.
    pub(crate) fn field(&mut self, part: &loom_ui_core::UiId) -> Point {
        self.lay_out_panels();
        let at = self
            .runtime()
            .bounds(part)
            .unwrap_or_else(|| panic!("no field `{part}` in the window"));
        Point::new(at.x + at.width / 2.0, at.y + at.height / 2.0)
    }

    /// Moves the pointer to a window point.
    pub(crate) fn move_to(&mut self, p: Point) -> Vec<Message> {
        self.cursor = Some(p);
        self.step_ui(&[UiEvent::Pointer(PointerEvent::Moved)])
    }

    /// Presses the left button where the pointer is.
    pub(crate) fn press(&mut self) -> Vec<Message> {
        self.step_ui(&[UiEvent::Pointer(PointerEvent::Pressed(PointerButton::Left))])
    }

    /// Releases the left button where the pointer is.
    pub(crate) fn release(&mut self) -> Vec<Message> {
        self.step_ui(&[UiEvent::Pointer(PointerEvent::Released(PointerButton::Left))])
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
    pub(crate) fn tap(&mut self, key: Key, modifiers: Modifiers) -> Vec<Message> {
        let mut messages = self.step(&[key_press(key, modifiers, None, false)]);
        messages.extend(self.step(&[key_released(key, modifiers)]));
        messages
    }

    /// Types text, one key press and release per character, as a keyboard
    /// does. Only the element with focus takes it.
    pub(crate) fn typewrite(&mut self, text: &str) -> Vec<Message> {
        let mut messages = Vec::new();
        let none = Modifiers::NONE;
        for c in text.chars() {
            let typed = c.to_string();
            let key = Key::Character(c);
            messages.extend(self.step(&[key_press(key, none, Some(&typed), false)]));
            messages.extend(self.step(&[key_released(key, none)]));
        }
        messages
    }

    /// Where the canvas is in the window, and its camera, read from its
    /// element as the window last laid it out.
    pub(crate) fn canvas(&mut self) -> CanvasView {
        self.lay_out_panels();
        let bounds = self
            .runtime()
            .bounds(&canvas_id())
            .expect("the window has a patch canvas");
        self.runtime()
            .custom::<CanvasElement>(&canvas_id())
            .expect("the canvas has its state")
            .input()
            .view(bounds)
    }

    /// Where the canvas is and its open typed field, as the accessibility
    /// layer and the control channel's `locate` are told.
    pub(crate) fn places(&mut self) -> crate::places::Places {
        let canvas = self.canvas();
        let typed = self
            .runtime()
            .custom::<CanvasElement>(&canvas_id())
            .and_then(|element| element.input().typed_field(&self.app().canvas_model(), canvas.bounds));
        crate::places::Places::live(Some(canvas), typed)
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
            self.app().session.controller.graph(),
            &self.app().session.hierarchy,
            module,
            &format!("{prefix}{name}"),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let crate::targets::Part::Port(port) = part else {
            panic!("module {module}: `{name}` is not a port");
        };
        let world = self
            .app()
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
        let (_, widget) =
            screenshot::find_control(&self.app().session.controller, &self.app().session.hierarchy, &target)
                .unwrap_or_else(|e| panic!("{e}"));
        self.app()
            .session
            .scene
            .module(module)
            .and_then(|m| m.widget_bounds(widget))
            .expect("the scene has every panel widget")
    }

    /// Holds these modifier keys, or none, for the steps that follow, as
    /// the runtime reports a change in the held modifiers.
    pub(crate) fn hold(&mut self, modifiers: Modifiers) -> Vec<Message> {
        self.step_ui(&[UiEvent::Key(KeyEvent::ModifiersChanged(modifiers))])
    }

    /// The bounds of the first text the window shows that reads exactly
    /// `content`, for example an inspector row label, as last laid out.
    /// `None` if no element shows it.
    pub(crate) fn text(&mut self, content: &str) -> Option<Rect> {
        self.lay_out_panels();
        self.runtime().text_bounds(content)
    }
}

/// Where a test puts the pointer: a window point, or nowhere. Iced's cursor
/// too, until the tests' values change to ours (plan step P4, commit 6).
pub(crate) struct Cursor(Option<Point>);

impl From<Option<Point>> for Cursor {
    fn from(at: Option<Point>) -> Self {
        Self(at)
    }
}

impl From<iced::mouse::Cursor> for Cursor {
    fn from(cursor: iced::mouse::Cursor) -> Self {
        use loom_canvas::iced_convert::ToCore as _;
        Self(cursor.position().map(|at| at.to_core()))
    }
}

/// A key press of ours with `modifiers` held, typing no text, at a place
/// nothing reads (`Code::Other`): what a unit test hands a key handler.
#[cfg(test)]
pub(crate) fn pressed(key: loom_ui_core::Key, modifiers: loom_ui_core::Modifiers) -> loom_ui_core::KeyEvent {
    loom_ui_core::KeyEvent::Pressed {
        key,
        code: loom_ui_core::Code::Other,
        modifiers,
        repeat: false,
        text: None,
    }
}

/// A key press with no modifiers and no text, as the keyboard sends it.
pub(crate) fn key_pressed(key: Key, repeat: bool) -> Event {
    key_press(key, Modifiers::NONE, None, repeat)
}

/// A key press with `modifiers` held, carrying `text` when the key types
/// a character, at a place nothing reads (`Code::Other`). Iced's event
/// until the tests' values change to ours (plan step P4, commit 6).
pub(crate) fn key_press(key: Key, modifiers: Modifiers, text: Option<&str>, repeat: bool) -> Event {
    let press = KeyEvent::Pressed {
        key,
        code: Code::Other,
        modifiers,
        repeat,
        text: text.map(str::to_string),
    };
    Event::Keyboard(loom_canvas::iced_convert::key_event_to_iced(&press, modifiers))
}

/// A key release with `modifiers` held, as Iced's event.
pub(crate) fn key_released(key: Key, modifiers: Modifiers) -> Event {
    let release = KeyEvent::Released { key, code: Code::Other };
    Event::Keyboard(loom_canvas::iced_convert::key_event_to_iced(&release, modifiers))
}

#[cfg(test)]
mod arrow_tests;
#[cfg(test)]
mod tests;
