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
//! The messages run through the app's own bridge to our runtime
//! (`iced_bridge.rs`), with a driver of its own (`iced_bridge/headless.rs`):
//! input is routed, each message is one update, and focus, scrolling, and
//! window questions run as Iced runs them, on the harness's view. Effects
//! run in memory: the clipboard is a string, work runs inline, a capture
//! draws the view offscreen on wgpu (and answers that there is no GPU on
//! tiny-skia), and Quit marks [`Ui::exited`]. A dialog is answered from the
//! fake dialogs in the crate's tests; in a test under `tests/`, where they
//! are off, it panics, naming it. A task the harness cannot run, such as a
//! window command or one that waits on a timer, panics too, so a test never
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

use crate::iced_bridge::headless::{Pending, Surface, UiDriver, settle};
use crate::iced_bridge::{Bridge, Bridged};
use crate::{App, Args, Message, screenshot};
use iced::advanced::Clipboard;
use iced::advanced::clipboard::Kind;
use iced::advanced::widget::Id;
use iced::advanced::widget::Operation;
use iced::advanced::widget::operation::Outcome;
use iced::{Event, Rectangle, Size, keyboard, mouse, window};
use iced_runtime::user_interface::UserInterface;
use loom_canvas::CanvasView;
use loom_canvas::iced_convert::{ToCore as _, ToIced as _};
use loom_ui_core::{Point, Rect};
// The harness tests reach these through `use super::*`.
#[cfg(test)]
use loom_core::ModuleId;
use std::path::PathBuf;
use std::time::Instant;

/// The window size the app opens with.
pub(crate) const WINDOW: Size = crate::WINDOW;

/// The app view, driven by events the way the Iced runtime drives it.
pub struct Ui {
    /// The app's own bridge, with the harness's driver.
    bridge: Bridge<UiDriver>,
    /// The renderer, the widget cache, and the cursor.
    surface: Surface,
}

/// The clipboard Iced's widgets use during one frame: a copy of the
/// driver's, written back after it, so a paste in a field and the
/// clipboard effects share one text.
#[derive(Debug, Default)]
struct Memory(Option<String>);

impl Clipboard for Memory {
    fn read(&self, _kind: Kind) -> Option<String> {
        self.0.clone()
    }

    fn write(&mut self, _kind: Kind, contents: String) {
        self.0 = Some(contents);
    }
}

/// The app [`Ui::open`] runs, for a test that applies messages to it
/// directly: a patch from `examples/patches`, with no audio.
pub(crate) fn open_app(patch: &str) -> App {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/patches")
        .join(patch);
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

    /// The app, to read.
    pub(crate) fn app(&self) -> &App {
        self.bridge.app()
    }

    /// The app, for a test to set up state between turns. The runtime's
    /// queue, deadline, and focus stay as they are, so a test moves focus
    /// with a focus effect or a click, never by setting the app's copy.
    pub(crate) fn app_mut(&mut self) -> &mut App {
        self.bridge.app_mut()
    }

    /// The clipboard's text.
    pub(crate) fn clipboard(&self) -> Option<&str> {
        self.bridge.driver().headless.clipboard()
    }

    /// Puts `text` on the clipboard, or empties it.
    pub(crate) fn set_clipboard(&mut self, text: Option<String>) {
        self.bridge.driver_mut().headless.set_clipboard(text);
    }

    /// True once the app has quit.
    pub(crate) fn exited(&self) -> bool {
        self.bridge.driver().headless.exited()
    }

    /// Puts the pointer at `cursor`, or takes it out of the window, with
    /// no event: as when the window stops hearing the pointer.
    pub(crate) fn set_cursor(&mut self, cursor: mouse::Cursor) {
        self.surface.cursor = cursor;
    }

    /// True when the renderer is wgpu, so a capture draws the canvas too.
    pub(crate) fn draws_the_canvas(&self) -> bool {
        self.surface.draws_the_canvas()
    }

    /// A waker into the app's runtime, as a watcher thread holds one.
    pub(crate) fn waker(&self) -> loom_ui::Waker<Message> {
        self.bridge.waker()
    }

    /// Presses Open on the toolbar. From `tests/`, where the fake dialogs are
    /// off, the dialog it asks for has no answer, so the harness panics,
    /// naming it. Save is not offered here: it would overwrite the example.
    pub fn press_open(&mut self) {
        self.click_text("Open");
    }

    /// What the window shows, as `--dump-ui` prints it.
    pub fn snapshot_json(&self) -> serde_json::Value {
        serde_json::to_value(self.app().ui_snapshot()).expect("the snapshot is plain data")
    }

    /// Opens a patch from `examples/patches` with no audio, as the app does
    /// in a window of [`WINDOW`] size, and draws the first frame.
    pub(crate) fn open(patch: &str) -> Self {
        Self::with_app(open_app(patch))
    }

    /// Draws the first frame of an app built by the test, for example one
    /// that plays with `--fake-audio`.
    pub(crate) fn with_app(app: App) -> Self {
        let (mut bridge, first) = Bridge::<UiDriver>::new(app);
        bridge.log_messages();
        let mut ui = Self {
            bridge,
            surface: Surface::new(WINDOW),
        };
        ui.settle(vec![first]);
        // The canvas frames the patch on its first redraw.
        ui.redraw();
        ui
    }

    /// One frame: the events go to the view at the current cursor, the
    /// messages they produce go to the bridge, what the bridge returns
    /// runs, and the new view is drawn. Returns every message `update` got,
    /// in order, with input as the messages it was routed to.
    pub(crate) fn step(&mut self, events: &[Event]) -> Vec<Message> {
        let mut messages = Vec::new();
        let listening = self.app().listening();
        let mut clipboard = Memory(self.clipboard().map(str::to_string));
        self.with_ui(|ui, renderer, cursor| {
            let (_, statuses) = ui.update(events, cursor, renderer, &mut clipboard, &mut messages);
            for (event, status) in events.iter().zip(statuses) {
                messages.extend(crate::input_owner::heard_messages(event, status, listening));
            }
        });
        self.set_clipboard(clipboard.0);
        let tasks = messages
            .into_iter()
            .map(|message| self.bridge.update(Bridged::App(message)))
            .collect();
        self.settle(tasks);
        // Something outside a turn may have asked where focus is, such as a
        // test replacing the patch directly. The bridge asks after its next
        // turn, so the harness runs one.
        if self.app().focus_question_waits() {
            let task = self.bridge.update(Bridged::Woken);
            self.settle(vec![task]);
        }
        let Self { bridge, surface } = self;
        surface.draw(bridge.app());
        self.bridge.take_log()
    }

    /// Runs what the bridge returned until nothing is left.
    fn settle(&mut self, tasks: Vec<Pending>) {
        settle(&mut self.bridge, &mut self.surface, tasks);
    }

    /// Applies `message` as the runtime does, then every message it
    /// produces and what those produce in turn, and returns them all.
    pub(crate) fn deliver(&mut self, message: Message) -> Vec<Message> {
        let task = self.bridge.update(Bridged::App(message));
        self.settle(vec![task]);
        self.bridge.take_log()
    }

    /// Takes focus from every field, as the bridge does for a focus effect
    /// to no element, with no message.
    pub(crate) fn unfocus_all(&mut self) -> Vec<Message> {
        let task =
            crate::iced_bridge::focus_moves(&loom_ui::effect::FocusTarget::Nothing, loom_ui::effect::Selection::Keep);
        self.settle(vec![task]);
        self.bridge.take_log()
    }

    /// Scrolls the part named `id` into view as the bridge's scroll does,
    /// with no message, and returns the messages that came of it.
    pub(crate) fn scroll_to(&mut self, id: &loom_ui_core::UiId) -> Vec<Message> {
        let task = crate::iced_bridge::scroll_into_view(id);
        self.settle(vec![task]);
        self.bridge.take_log()
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

    /// Builds the view, runs `f` on it, and keeps its cache.
    pub(crate) fn with_ui(
        &mut self,
        f: impl FnOnce(&mut UserInterface<'_, Message, iced::Theme, crate::Renderer>, &mut crate::Renderer, mouse::Cursor),
    ) {
        let Self { bridge, surface } = self;
        surface.with_ui(bridge.app(), f);
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

    /// Sends one message, and everything that comes of it, as the runtime
    /// runs it: [`Ui::deliver`], with no log.
    pub(crate) fn send(&mut self, message: Message) {
        let _ = self.deliver(message);
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
        let draft = self.app().session.value_draft.as_ref().expect("a draft is typed");
        assert_eq!(draft.draft.text, text, "every key stays in the draft");
        self.tap(
            keyboard::Key::Named(keyboard::key::Named::Enter),
            keyboard::Modifiers::default(),
        );
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
        self.surface.cursor = mouse::Cursor::Available(position);
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
