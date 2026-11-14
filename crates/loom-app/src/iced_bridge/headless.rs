//! The bridge with no window, for the `Ui` harness (`ui_test.rs`).
//!
//! The harness runs the app's own bridge, so routing, the turn, focus and
//! scroll, window questions, and the focus answer run through the same code
//! as in the app (ui-layer.md, section 2). Only two parts differ:
//!
//! - [`UiDriver`] carries out the effects in memory, through `loom-ui`'s
//!   headless driver: the clipboard is a string, work runs inline, and a
//!   dialog with no answer panics, naming it (rules 3 and 4). In the crate's
//!   own tests the fake dialogs answer instead. A capture waits for the
//!   harness to draw.
//! - [`Surface`] holds the renderer and the widget cache, and [`settle`]
//!   runs the Iced tasks the bridge returns on it through `tasks.rs`, as
//!   Iced's runtime runs them on the window, until nothing is left.

use super::tasks::{self, Sink, Stuck};
use super::{Bridge, BridgeDriver, Bridged};
use crate::{App, Message};
use iced::advanced::renderer::Style;
use iced::advanced::widget::Operation;
use iced::advanced::widget::operation::Outcome;
use iced::{Size, Task, mouse};
use iced_runtime::user_interface::{Cache, UserInterface};
use loom_ui::effect::{Choice, FileRequest, Image, Job, NoGpu, Question, Reply};
use loom_ui::headless::Headless;
use loom_ui::{Driver, Waker, WindowSpec};
use std::collections::VecDeque;
use std::path::PathBuf;

/// The harness's driver: `loom-ui`'s headless driver, with the fake dialogs
/// in the crate's tests, and captures left for the harness to draw.
#[derive(Default)]
pub(crate) struct UiDriver {
    /// The clipboard, work, exit, and dialogs with no fake answer.
    pub(crate) headless: Headless,
    /// Capture replies waiting for the harness to draw the view.
    captures: Vec<Reply<Result<Image, NoGpu>, Message>>,
}

impl Driver<Message> for UiDriver {
    fn window(&mut self, spec: &WindowSpec) {
        Driver::<Message>::window(&mut self.headless, spec);
    }

    fn clipboard_write(&mut self, text: String) {
        Driver::<Message>::clipboard_write(&mut self.headless, text);
    }

    fn clipboard_read(&mut self, reply: Reply<Option<String>, Message>, waker: &Waker<Message>) {
        self.headless.clipboard_read(reply, waker);
    }

    /// Answered from `files::fake` in the crate's tests. In a test under
    /// `tests/` the fakes are off, so the headless driver panics, naming it.
    fn file_dialog(&mut self, request: FileRequest, reply: Reply<Option<PathBuf>, Message>, waker: &Waker<Message>) {
        #[cfg(test)]
        {
            waker.send(reply.give(crate::dialogs::fake::file_answer(&request)));
        }
        #[cfg(not(test))]
        self.headless.file_dialog(request, reply, waker);
    }

    /// As [`UiDriver::file_dialog`], from `dialogs::fake`.
    fn ask(&mut self, question: Question, reply: Reply<Choice, Message>, waker: &Waker<Message>) {
        #[cfg(test)]
        {
            waker.send(reply.give(crate::dialogs::fake::question_answer(&question)));
        }
        #[cfg(not(test))]
        self.headless.ask(question, reply, waker);
    }

    /// Waits for [`settle`] to draw the view and answer.
    fn capture(&mut self, reply: Reply<Result<Image, NoGpu>, Message>, _waker: &Waker<Message>) {
        self.captures.push(reply);
    }

    fn work(&mut self, job: Job<Message>, waker: &Waker<Message>) {
        self.headless.work(job, waker);
    }

    fn exit(&mut self) {
        Driver::<Message>::exit(&mut self.headless);
    }
}

impl BridgeDriver for UiDriver {
    /// Everything ran in memory: no Iced task is needed.
    fn take_tasks(&mut self) -> Vec<Task<Bridged>> {
        Vec::new()
    }
}

/// The harness's window: the renderers, the widget cache, and the cursor.
pub(crate) struct Surface {
    /// The measure renderer, which events, layout, operations, and frames
    /// run on, so they need no GPU.
    renderer: crate::Renderer,
    /// The wgpu renderer for captures, made on the first one: `Ok` once
    /// made, `Err` when no adapter was found.
    gpu: Option<Result<crate::Renderer, NoGpu>>,
    cache: Option<Cache>,
    /// Where the pointer is, for events and drawing.
    pub(crate) cursor: mouse::Cursor,
    size: Size,
}

impl Surface {
    /// A window of `size` that lays out on the measure renderer, with the
    /// window's defaults (Iced's default font at 16). No GPU is asked for
    /// until a capture.
    pub(crate) fn new(size: Size) -> Self {
        Self {
            renderer: crate::Renderer::Secondary(super::Measure::new(iced::Font::DEFAULT, iced::Pixels(16.0))),
            gpu: None,
            cache: Some(Cache::default()),
            cursor: mouse::Cursor::Unavailable,
            size,
        }
    }

    /// True when a GPU adapter was found, so a capture draws the view, the
    /// canvas's shader included. The first call looks for one.
    pub(crate) fn draws_the_canvas(&mut self) -> bool {
        self.gpu().is_ok()
    }

    /// Where every named part and the canvas are when `app`'s view is laid
    /// out from a fresh cache on the measure renderer, and on the wgpu
    /// renderer; `None` with no GPU adapter. The two must agree, or event
    /// tests would aim at places the window does not draw.
    #[cfg(test)]
    pub(crate) fn places_on_both(&mut self, app: &App) -> Option<(crate::places::Places, crate::places::Places)> {
        fn places(app: &App, size: Size, renderer: &mut crate::Renderer) -> crate::places::Places {
            let mut ui = UserInterface::build(app.view(), size, Cache::default(), renderer);
            let mut collect = crate::places::Collect::default();
            ui.operate(
                renderer,
                &mut iced::advanced::widget::operation::black_box(&mut collect),
            );
            collect.places()
        }
        let size = self.size;
        let measured = places(app, size, &mut self.renderer);
        let drawn = places(app, size, self.gpu().ok()?);
        Some((measured, drawn))
    }

    /// The wgpu renderer for captures, made the first time it is asked for.
    fn gpu(&mut self) -> Result<&mut crate::Renderer, NoGpu> {
        self.gpu
            .get_or_insert_with(|| super::offscreen::renderer().ok_or(NoGpu))
            .as_mut()
            .map_err(|NoGpu| NoGpu)
    }

    /// Builds `app`'s view, runs `f` on it, and keeps its cache.
    pub(crate) fn with_ui(
        &mut self,
        app: &App,
        f: impl FnOnce(&mut UserInterface<'_, Message, iced::Theme, crate::Renderer>, &mut crate::Renderer, mouse::Cursor),
    ) {
        let cache = self.cache.take().expect("the cache is put back after every frame");
        let mut ui = UserInterface::build(app.view(), self.size, cache, &mut self.renderer);
        f(&mut ui, &mut self.renderer, self.cursor);
        self.cache = Some(ui.into_cache());
    }

    /// Draws `app`'s view, as after each frame.
    pub(crate) fn draw(&mut self, app: &App) {
        let theme = app.theme();
        let style = Style {
            text_color: theme.palette().text,
        };
        self.with_ui(app, |ui, renderer, cursor| ui.draw(renderer, &theme, &style, cursor));
    }

    /// Runs a widget operation, and those it chains, on `app`'s view.
    fn operate(&mut self, app: &App, operation: Box<dyn Operation>) {
        let mut next = Some(operation);
        while let Some(mut operation) = next.take() {
            self.with_ui(app, |ui, renderer, _| ui.operate(renderer, operation.as_mut()));
            if let Outcome::Chain(chained) = operation.finish() {
                next = Some(chained);
            }
        }
    }

    /// Draws `app`'s view offscreen with wgpu and gives its pixels, at
    /// scale 1, as the window opens. With no GPU adapter there is nothing to
    /// draw with, so the answer says so (ui-layer.md, section 2, effects
    /// table).
    fn capture(&mut self, app: &App) -> Result<Image, NoGpu> {
        let size = self.size;
        let cursor = self.cursor;
        let cache = self.cache.take().expect("the cache is put back after every frame");
        let Ok(renderer) = self.gpu() else {
            self.cache = Some(cache);
            return Err(NoGpu);
        };
        let theme = app.theme();
        let style = Style {
            text_color: theme.palette().text,
        };
        let physical = Size::new(size.width.round() as u32, size.height.round() as u32);
        let mut ui = UserInterface::build(app.view(), size, cache, renderer);
        // A redraw update first, as before every frame in the window: Iced
        // 0.14 keeps a button's status (and a field's) on the widget, sets
        // it only while handling a redraw, and draws a freshly built one as
        // disabled until then (#460, #776). What the widgets ask for is
        // dropped: a capture is a still frame, not a turn.
        let redraw = iced::Event::Window(iced::window::Event::RedrawRequested(std::time::Instant::now()));
        let mut asked = Vec::new();
        let _ = ui.update(
            &[redraw],
            cursor,
            renderer,
            &mut iced::advanced::clipboard::Null,
            &mut asked,
        );
        ui.draw(renderer, &theme, &style, cursor);
        // The tree owns the text the renderer holds weakly, so it stays
        // until the frame is drawn.
        let pixels = super::offscreen::pixels(renderer, physical, 1.0, theme.palette().background);
        self.cache = Some(ui.into_cache());
        Image::new(physical.width, physical.height, 1.0, pixels).ok_or(NoGpu)
    }
}

/// What the bridge hands Iced to run after a turn.
pub(crate) type Pending = Task<Bridged>;

/// Runs `tasks`, the bridge's answer to what Iced sent it, as Iced's runtime
/// would, until nothing is left: widget operations on `surface`, clipboard
/// actions on the driver's clipboard, each output and each wake-up back
/// into the bridge, and captures drawn on `surface`. Panics on a task that
/// waits on the outside world, or that only the app's own driver makes,
/// such as exit, so no test passes by skipping one.
pub(crate) fn settle(bridge: &mut Bridge<UiDriver>, surface: &mut Surface, tasks: Vec<Pending>) {
    let mut queue: VecDeque<Task<Bridged>> = tasks.into();
    loop {
        if let Some(task) = queue.pop_front() {
            for output in run(task, bridge, surface) {
                queue.push_back(bridge.update(output));
            }
            continue;
        }
        // A capture: the view as it is now, drawn offscreen.
        let captures = std::mem::take(&mut bridge.driver_mut().captures);
        if !captures.is_empty() {
            for reply in captures {
                let image = surface.capture(bridge.app());
                queue.push_back(bridge.update(Bridged::App(reply.give(image))));
            }
            continue;
        }
        // A message from another thread, or one a turn handed the queue.
        let wakes = bridge.take_wakes();
        if wakes == 0 {
            return;
        }
        for _ in 0..wakes {
            queue.push_back(bridge.update(Bridged::Woken));
        }
    }
}

/// The harness's place for a task's actions: operations on its surface,
/// and the clipboard on its driver.
struct HarnessSink<'a> {
    bridge: &'a mut Bridge<UiDriver>,
    surface: &'a mut Surface,
}

impl Sink for HarnessSink<'_> {
    fn operate(&mut self, operation: Box<dyn Operation>) {
        self.surface.operate(self.bridge.app(), operation);
    }

    fn clipboard_read(&mut self) -> Option<String> {
        self.bridge.driver_mut().headless.clipboard().map(str::to_string)
    }

    fn clipboard_write(&mut self, contents: String) {
        self.bridge.driver_mut().headless.set_clipboard(Some(contents));
    }
}

/// Runs one task to its end and gives its outputs. Panics on a task that
/// stops early, naming why, so no test passes by skipping one.
fn run(task: Pending, bridge: &mut Bridge<UiDriver>, surface: &mut Surface) -> Vec<Bridged> {
    let ran = tasks::run(task, &mut HarnessSink { bridge, surface });
    match ran.stuck {
        None => ran.outputs,
        Some(Stuck::Waits) => {
            panic!("Ui: a task waits on something the harness cannot give, such as a timer or a dialog")
        }
        Some(Stuck::Spins) => panic!("Ui: a task keeps waking itself and never goes on"),
        Some(Stuck::Action(action)) => panic!("Ui: the harness cannot run this task: {action}"),
    }
}
