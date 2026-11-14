//! The window shell: a `winit` window, its GPU surface, and the loop that
//! drives the [`Runtime`].
//!
//! This is the one module of `loom-ui` that names `winit` and `wgpu`
//! (ui-layer.md, section 1, rule 6, and `tests/purity.rs`), behind the
//! `shell` feature. It draws nothing of its own and knows no toolkit for
//! the app's views: the app's side gives two traits.
//!
//! - [`Layers`] hears every window event as winit gives it, draws every
//!   frame into the [`Frame`] it is handed, and [reports](Report) the
//!   cursor, the input method's area, and when it wants a frame.
//! - [`Services`] gives what the runtime's driver needs that the shell
//!   does not own: the clipboard and the system's dialogs.
//!
//! The shell does the rest:
//!
//! - It makes the window hidden, sets up the GPU, draws the first frame,
//!   and only then shows the window.
//! - It turns winit's events into the runtime's (`events.rs`).
//! - It runs one turn for each, and draws only when a turn, the layers, or
//!   the system asks (`pacing.rs`).
//! - It wakes for wakers through winit's event loop proxy, and at the
//!   runtime's deadline.
//! - It runs background work on threads, captures frames offscreen, and
//!   ends the loop when the app quits.
//!
//! Plan step 2.2 (`docs/dev/leaving-iced-plan.md`) says what each part is
//! for. The shell is a driver, so it reads the clock and passes the moment
//! in; the runtime never does.

mod events;
mod gpu;
mod pacing;

pub use gpu::GpuError;
pub use pacing::Redraw;

use crate::app::{App, WindowSpec};
use crate::driver::Driver;
use crate::effect::{Choice, FileRequest, Image, Job, NoGpu, Question, Reply};
use crate::event::{DriverEvent, WindowState};
use crate::routing::Target;
use crate::runtime::{Next, Runtime};
use crate::waker::Waker;
use events::Held;
use gpu::Gpu;
use loom_ui_core::{Rect, Size};
use pacing::{Drawn, FrameKind, Pacing, Wait};
use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{StartCause, WindowEvent as WinitEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use winit::window::{CursorIcon, Window, WindowId};

/// Where one frame goes: a texture view and the device that draws to it.
/// The window's frame and a capture both come as one.
pub struct Frame<'a> {
    /// The GPU device.
    pub device: &'a wgpu::Device,
    /// Its queue. Each layer submits its own work, in order.
    pub queue: &'a wgpu::Queue,
    /// The texture to draw into.
    pub view: &'a wgpu::TextureView,
    /// The texture's format.
    pub format: wgpu::TextureFormat,
    /// Its size in logical pixels.
    pub size: Size,
    /// Physical pixels per logical pixel.
    pub scale: f32,
    /// True for a capture, drawn offscreen for the app; false for the
    /// window's own frame. A capture shows what the window last showed, so
    /// layers do no frame work for it: they may draw their last frame again.
    pub capture: bool,
}

/// What the layers lend each call: the app to read, the routing target to
/// use, a waker to post messages with, and the window as the runtime
/// knows it. A message posted here runs on a turn of its own.
pub struct Parts<'a, A: App, T> {
    /// The app, to read.
    pub app: &'a A,
    /// The routing target.
    pub target: &'a mut T,
    /// Posts messages for the next turn.
    pub waker: &'a Waker<A::Message>,
    /// The window as the runtime knows it.
    pub window: WindowState,
}

/// The input method, as the layers want it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ime {
    /// No text field takes text: the input method is off.
    Off,
    /// A field takes text, and its caret is in `area`, in logical pixels,
    /// so the system's candidate window opens beside it.
    On {
        /// The caret's area.
        area: Rect,
    },
}

/// The pointer, as the layers want it shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pointer {
    /// Shown, with this cursor.
    Icon(CursorIcon),
    /// Hidden, as while typing in some fields.
    Hidden,
}

/// What the layers ask of the window. `None` keeps what it was.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Report {
    /// The pointer.
    pub cursor: Option<Pointer>,
    /// The input method.
    pub ime: Option<Ime>,
    /// When they want a frame.
    pub redraw: Redraw,
    /// The layers hold a message that must run in a turn of its own, after
    /// the messages already queued. The shell runs those, then asks
    /// [`Layers::alone`] for it.
    pub alone: bool,
}

/// What the window needs to do to show `pointer`, when it was `hidden`
/// before: the cursor to set, and whether to show or hide the pointer.
/// A hidden pointer is shown again only when it leaves `Hidden`, since
/// setting a cursor does not show it.
fn pointer_calls(hidden: &mut bool, pointer: Pointer) -> (Option<CursorIcon>, Option<bool>) {
    match pointer {
        Pointer::Icon(icon) => {
            let show = std::mem::replace(hidden, false).then_some(true);
            (Some(icon), show)
        }
        Pointer::Hidden if *hidden => (None, None),
        Pointer::Hidden => {
            *hidden = true;
            (None, Some(false))
        }
    }
}

/// The GPU and window, lent once when the window opens, for layers that
/// build their renderer or connect to the window's services.
pub struct Opened<'a> {
    /// The window.
    pub window: &'a Arc<Window>,
    /// The adapter the device came from.
    pub adapter: &'a wgpu::Adapter,
    /// The GPU device.
    pub device: &'a wgpu::Device,
    /// Its queue.
    pub queue: &'a wgpu::Queue,
    /// The format the window's frames are drawn in.
    pub format: wgpu::TextureFormat,
}

/// What draws the window: everything the app shows, in order.
pub trait Layers<A: App, T> {
    /// The window and its GPU are ready. Called once, before the first
    /// turn, the first frame, and the window is shown.
    fn open(&mut self, _parts: Parts<'_, A, T>, _opened: &Opened<'_>) {}

    /// A window event, as winit gave it. Called before the shell turns it
    /// into the runtime's events, so the app's side can keep its own copy.
    fn window_event(&mut self, _parts: Parts<'_, A, T>, _event: &WinitEvent) {}

    /// Draws a whole frame into `frame.view`, the clear included. Called
    /// for the window and for a capture, so it never assumes the screen.
    fn draw(&mut self, parts: Parts<'_, A, T>, frame: &Frame<'_>);

    /// What the layers want of the window now. Asked after each turn and
    /// each frame, with the parts, so the layers can post what the window's
    /// events gave them since.
    fn report(&mut self, _parts: Parts<'_, A, T>) -> Report {
        Report::default()
    }

    /// The message the last [`Report::alone`] promised, once the messages
    /// queued before it have run. The layers may change the target first.
    fn alone(&mut self, _parts: Parts<'_, A, T>) -> Option<A::Message> {
        None
    }
}

/// The desktop's services that the runtime's driver needs and the shell
/// does not own. Each answer goes back through the waker, for a later turn.
pub trait Services<M> {
    /// Puts `text` on the clipboard.
    fn clipboard_write(&mut self, text: String);
    /// Reads the clipboard.
    fn clipboard_read(&mut self, reply: Reply<Option<String>, M>, waker: &Waker<M>);
    /// Shows a file dialog.
    fn file_dialog(&mut self, request: FileRequest, reply: Reply<Option<PathBuf>, M>, waker: &Waker<M>);
    /// Asks a question.
    fn ask(&mut self, question: Question, reply: Reply<Choice, M>, waker: &Waker<M>);
}

/// Services for an app that uses none: the clipboard keeps nothing and
/// reads empty, and every dialog is dismissed. For examples and tests.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoServices;

impl<M> Services<M> for NoServices {
    fn clipboard_write(&mut self, _text: String) {}

    fn clipboard_read(&mut self, reply: Reply<Option<String>, M>, waker: &Waker<M>) {
        waker.send(reply.give(None));
    }

    fn file_dialog(&mut self, _request: FileRequest, reply: Reply<Option<PathBuf>, M>, waker: &Waker<M>) {
        waker.send(reply.give(None));
    }

    fn ask(&mut self, _question: Question, reply: Reply<Choice, M>, waker: &Waker<M>) {
        waker.send(reply.give(None));
    }
}

/// How the window starts.
#[derive(Debug)]
pub struct Options<M> {
    /// Its inside, in logical pixels.
    pub size: Size,
    /// The messages of the first turn, in order: the app's way to start
    /// work before anything else happens, since there is no waker before
    /// the window exists.
    pub first: Vec<M>,
}

/// Why the shell stopped with an error.
#[derive(Debug)]
pub enum Error {
    /// winit's event loop failed.
    EventLoop(String),
    /// The window could not be made.
    Window(String),
    /// The window has no GPU to draw with.
    Gpu(GpuError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EventLoop(why) => write!(f, "the event loop failed: {why}"),
            Self::Window(why) => write!(f, "the window could not be made: {why}"),
            Self::Gpu(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for Error {}

/// Runs `app` in a window until it quits, with `target` routing its
/// elements, `layers` drawing, and `services` for the clipboard and
/// dialogs. Call it from the main thread, as the window systems require.
pub fn run<A, T, L, S>(app: A, target: T, layers: L, services: S, options: Options<A::Message>) -> Result<(), Error>
where
    A: App,
    A::Message: Send + 'static,
    T: Target<A::Message>,
    L: Layers<A, T>,
    S: Services<A::Message>,
{
    let event_loop = EventLoop::<ShellEvent>::with_user_event()
        .build()
        .map_err(|error| Error::EventLoop(error.to_string()))?;
    let mut handler = Handler {
        start: Some((app, target)),
        layers,
        driver: ShellDriver {
            services,
            window: None,
            captures: Vec::new(),
            exit: false,
        },
        proxy: event_loop.create_proxy(),
        options,
        live: None,
        held: Held::default(),
        hidden: false,
        shown: false,
        pacing: Pacing::default(),
        error: None,
    };
    event_loop
        .run_app(&mut handler)
        .map_err(|error| Error::EventLoop(error.to_string()))?;
    handler.error.map_or(Ok(()), Err)
}

/// What the event loop is woken with from outside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShellEvent {
    /// A waker sent the runtime a message.
    Woken,
}

/// The window and what lives with it, once the event loop resumes.
struct Live<A: App, T> {
    /// The window.
    window: Arc<Window>,
    /// Its GPU.
    gpu: Gpu,
    /// The runtime, holding the app and the target.
    runtime: Runtime<A, T>,
}

/// The shell's event loop handler.
struct Handler<A: App, T, L, S> {
    /// The app and target, until the window exists.
    start: Option<(A, T)>,
    /// What draws.
    layers: L,
    /// What carries out the runtime's effects.
    driver: ShellDriver<A::Message, S>,
    /// Wakes the loop from a waker's thread.
    proxy: EventLoopProxy<ShellEvent>,
    /// How the window starts.
    options: Options<A::Message>,
    /// The window, once made.
    live: Option<Live<A, T>>,
    /// The pointer and modifiers, between events.
    held: Held,
    /// The pointer is hidden.
    hidden: bool,
    /// A frame has been shown in the window, so a capture has one to show.
    shown: bool,
    /// The redraw rule.
    pacing: Pacing,
    /// What stopped the loop, if anything failed.
    error: Option<Error>,
}

impl<A, T, L, S> Handler<A, T, L, S>
where
    A: App,
    A::Message: Send + 'static,
    T: Target<A::Message>,
    L: Layers<A, T>,
    S: Services<A::Message>,
{
    /// Makes the window hidden, its GPU, and the runtime; draws the first
    /// frame; then shows the window.
    fn start(&mut self, event_loop: &ActiveEventLoop) -> Result<(), Error> {
        let Some((app, target)) = self.start.take() else {
            return Ok(());
        };
        let spec = app.window();
        let mut attributes = Window::default_attributes()
            .with_title(spec.title.clone())
            .with_inner_size(LogicalSize::new(self.options.size.width, self.options.size.height))
            .with_visible(false);
        if let Some(min) = spec.min_size {
            attributes = attributes.with_min_inner_size(LogicalSize::new(min.width, min.height));
        }
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|error| Error::Window(error.to_string()))?,
        );
        let gpu = Gpu::new(window.clone()).map_err(Error::Gpu)?;
        let scale = window.scale_factor() as f32;
        let state = WindowState {
            size: events::logical(window.inner_size(), scale),
            scale,
            focused: true,
            frame: monitor_frame(&window),
        };
        let proxy = Mutex::new(self.proxy.clone());
        let wake = Arc::new(move || {
            if let Ok(proxy) = proxy.lock() {
                // Fails only once the loop has ended, when nothing waits.
                let _ = proxy.send_event(ShellEvent::Woken);
            }
        });
        self.driver.window = Some(window.clone());
        let runtime = Runtime::new(app, target, state, wake);
        let live = self.live.insert(Live {
            window: window.clone(),
            gpu,
            runtime,
        });
        {
            let waker = live.runtime.waker();
            let state = live.runtime.window();
            let (app, target) = live.runtime.parts();
            let opened = Opened {
                window: &live.window,
                adapter: &live.gpu.adapter,
                device: &live.gpu.device.device,
                queue: &live.gpu.device.queue,
                format: live.gpu.device.format,
            };
            self.layers.open(
                Parts {
                    app,
                    target,
                    waker: &waker,
                    window: state,
                },
                &opened,
            );
            let waker = live.runtime.waker();
            for message in std::mem::take(&mut self.options.first) {
                waker.send(message);
            }
        }
        self.turn(event_loop, DriverEvent::Woken);
        self.draw(event_loop, FrameKind::Asked);
        window.set_visible(true);
        Ok(())
    }

    /// Runs one turn for `event`, and the turns its waiting messages need;
    /// then carries out what they asked.
    fn turn(&mut self, event_loop: &ActiveEventLoop, event: DriverEvent) {
        self.run(|runtime, driver| runtime.handle(event, Instant::now(), driver));
        self.after(event_loop);
    }

    /// Runs one turn with `first`, and the turns its waiting messages need,
    /// and asks for the frame they want.
    fn run(&mut self, first: impl FnOnce(&mut Runtime<A, T>, &mut ShellDriver<A::Message, S>) -> Next) {
        let Some(live) = self.live.as_mut() else {
            return;
        };
        let next = settle(&mut live.runtime, &mut self.driver, first);
        if self.pacing.turn(&next) {
            live.window.request_redraw();
        }
    }

    /// Captures what was asked, applies the layers' report, and ends the
    /// loop once the app has quit. While the layers hold a message to run
    /// alone, runs it and asks them again, in a loop, so the stack stays
    /// flat however many come.
    fn after(&mut self, event_loop: &ActiveEventLoop) {
        while self.report(event_loop) {
            self.alone();
        }
    }

    /// One pass of [`after`](Self::after): the captures, then the layers'
    /// report applied. True when the layers hold a message to run alone
    /// and the app has not quit.
    fn report(&mut self, event_loop: &ActiveEventLoop) -> bool {
        let Some(live) = self.live.as_mut() else {
            return false;
        };
        for reply in due_captures(self.shown, &mut self.driver.captures) {
            let waker = live.runtime.waker();
            let window = live.runtime.window();
            let (app, target) = live.runtime.parts();
            let layers = &mut self.layers;
            let image = gpu::capture(&live.gpu.device, live.window.inner_size(), window.scale, |frame| {
                layers.draw(
                    Parts {
                        app,
                        target,
                        waker: &waker,
                        window,
                    },
                    frame,
                );
            });
            waker.send(reply.give(image.ok_or(NoGpu)));
        }
        let report = {
            let waker = live.runtime.waker();
            let window = live.runtime.window();
            let (app, target) = live.runtime.parts();
            self.layers.report(Parts {
                app,
                target,
                waker: &waker,
                window,
            })
        };
        if let Some(pointer) = report.cursor {
            let (icon, visible) = pointer_calls(&mut self.hidden, pointer);
            if let Some(icon) = icon {
                live.window.set_cursor(icon);
            }
            if let Some(visible) = visible {
                live.window.set_cursor_visible(visible);
            }
        }
        match report.ime {
            Some(Ime::Off) => live.window.set_ime_allowed(false),
            Some(Ime::On { area }) => {
                live.window.set_ime_allowed(true);
                live.window.set_ime_cursor_area(
                    LogicalPosition::new(area.x, area.y),
                    LogicalSize::new(area.width, area.height),
                );
            }
            None => {}
        }
        if self.pacing.layers(report.redraw) {
            live.window.request_redraw();
        }
        if self.driver.exit || live.runtime.exited() {
            event_loop.exit();
            return false;
        }
        report.alone
    }

    /// Runs what is queued, then the layers' message in a turn of its own,
    /// so no queued message sees what the layers changed for it.
    fn alone(&mut self) {
        let Some(live) = self.live.as_mut() else {
            return;
        };
        let nexts = alone_turns(&mut live.runtime, &mut self.layers, &mut self.driver);
        let mut redraw = false;
        for next in &nexts {
            redraw |= self.pacing.turn(next);
        }
        if redraw {
            live.window.request_redraw();
        }
    }

    /// Draws one frame: first the turn the runtime needs for it, then the
    /// layers into the window, then the `Presented` turn.
    fn draw(&mut self, event_loop: &ActiveEventLoop, kind: FrameKind) {
        let Some(live) = self.live.as_mut() else {
            return;
        };
        let event = match kind {
            FrameKind::Asked => None,
            FrameKind::Animation => Some(DriverEvent::Frame),
            FrameKind::System => Some(DriverEvent::RedrawRequested),
        };
        if let Some(event) = event {
            // This frame is the one the turn says to draw, so the turn asks
            // for no other, and an animation's next frame is asked for
            // after this one.
            let next = live.runtime.handle(event, Instant::now(), &mut self.driver);
            self.pacing.frame_turn(&next);
        }
        let waker = live.runtime.waker();
        let window = live.runtime.window();
        let (app, target) = live.runtime.parts();
        let layers = &mut self.layers;
        let drawn = live.gpu.frame(window.size, window.scale, &live.window, |frame| {
            layers.draw(
                Parts {
                    app,
                    target,
                    waker: &waker,
                    window,
                },
                frame,
            );
        });
        if drawn == Drawn::Shown {
            self.shown = true;
            self.turn(event_loop, DriverEvent::Presented);
        }
        if self.pacing.frame_ended(drawn)
            && let Some(live) = self.live.as_ref()
        {
            live.window.request_redraw();
        }
    }

    /// A key press or release, as the runtime's event.
    fn key(&mut self, event_loop: &ActiveEventLoop, event: &winit::event::KeyEvent, synthetic: bool) {
        let key = event.key_without_modifiers();
        let converted = events::key(
            &key,
            event.state,
            event.repeat,
            event.text.as_deref(),
            synthetic,
            self.held.modifiers,
        );
        if let Some(converted) = converted {
            let pointer = self.held.pointer;
            self.turn(
                event_loop,
                DriverEvent::Input {
                    event: converted,
                    pointer,
                },
            );
        } else {
            // No turn ran, but the layers heard the event, so what they
            // report still applies now.
            self.after(event_loop);
        }
    }
}

impl<A, T, L, S> ApplicationHandler<ShellEvent> for Handler<A, T, L, S>
where
    A: App,
    A::Message: Send + 'static,
    T: Target<A::Message>,
    L: Layers<A, T>,
    S: Services<A::Message>,
{
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(error) = self.start(event_loop) {
            self.error = Some(error);
            event_loop.exit();
        }
    }

    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        if let StartCause::ResumeTimeReached { .. } = cause {
            let due = self.pacing.due(Instant::now());
            if due.redraw
                && let Some(live) = self.live.as_ref()
            {
                live.window.request_redraw();
            }
            if due.deadline {
                self.turn(event_loop, DriverEvent::DeadlineReached);
            }
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: ShellEvent) {
        match event {
            ShellEvent::Woken => self.turn(event_loop, DriverEvent::Woken),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WinitEvent) {
        let Some(live) = self.live.as_mut() else {
            return;
        };
        {
            let waker = live.runtime.waker();
            let window = live.runtime.window();
            let (app, target) = live.runtime.parts();
            self.layers.window_event(
                Parts {
                    app,
                    target,
                    waker: &waker,
                    window,
                },
                &event,
            );
        }
        match &event {
            WinitEvent::RedrawRequested => {
                let kind = self.pacing.frame_begins();
                self.draw(event_loop, kind);
                return;
            }
            WinitEvent::Resized(size) => live.gpu.resize(*size),
            // The window may be on another display now, with another rate.
            WinitEvent::Moved(_) | WinitEvent::ScaleFactorChanged { .. } => {
                let frame = monitor_frame(&live.window);
                if frame != live.runtime.window().frame {
                    live.runtime.set_frame(frame);
                }
            }
            WinitEvent::KeyboardInput {
                event, is_synthetic, ..
            } => {
                let (event, synthetic) = (event.clone(), *is_synthetic);
                self.key(event_loop, &event, synthetic);
                return;
            }
            _ => {}
        }
        let scale = live.window.scale_factor() as f32;
        let converted = events::driver_events(&event, scale, &mut self.held);
        if converted.is_empty() {
            // No turn runs, but the layers heard the event, so what they
            // report (a cursor, the input method's area, a redraw) still
            // applies now.
            self.after(event_loop);
        }
        for converted in converted {
            self.turn(event_loop, converted);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(match self.pacing.wait() {
            Wait::Sleep => ControlFlow::Wait,
            Wait::Until(at) => ControlFlow::WaitUntil(at),
            Wait::Poll => ControlFlow::Poll,
        });
    }
}

/// The captures to draw now: all of them once a frame has been `shown`,
/// none before. A capture is the window as it was last drawn, and before
/// the first frame there is none, so it waits for the first frame rather
/// than give a blank image.
fn due_captures<R>(shown: bool, captures: &mut Vec<R>) -> Vec<R> {
    if shown { std::mem::take(captures) } else { Vec::new() }
}

/// Runs one turn with `first`, then a turn for each batch of messages
/// still waiting, and gives what is next once they have all run. A turn
/// that drew keeps its draw.
fn settle<A: App, T: Target<A::Message>, D: Driver<A::Message>>(
    runtime: &mut Runtime<A, T>,
    driver: &mut D,
    first: impl FnOnce(&mut Runtime<A, T>, &mut D) -> Next,
) -> Next {
    let mut next = first(runtime, driver);
    while next.again && !runtime.exited() {
        let more = runtime.handle(DriverEvent::Woken, Instant::now(), driver);
        next = Next {
            draw: next.draw || more.draw,
            ..more
        };
    }
    next
}

/// The turns of a [`Report::alone`]: what is queued, then the layers'
/// message by itself. The layers are asked for it only after the queued
/// messages have run, so what they change for it (such as the target's
/// focus) is not seen by those. Gives what each turn said is next.
fn alone_turns<A, T, L, D>(runtime: &mut Runtime<A, T>, layers: &mut L, driver: &mut D) -> Vec<Next>
where
    A: App,
    T: Target<A::Message>,
    L: Layers<A, T>,
    D: Driver<A::Message>,
{
    let mut nexts = vec![settle(runtime, driver, |runtime, driver| {
        runtime.handle(DriverEvent::Woken, Instant::now(), driver)
    })];
    if runtime.exited() {
        return nexts;
    }
    let message = {
        let waker = runtime.waker();
        let window = runtime.window();
        let (app, target) = runtime.parts();
        layers.alone(Parts {
            app,
            target,
            waker: &waker,
            window,
        })
    };
    if let Some(message) = message {
        nexts.push(settle(runtime, driver, |runtime, driver| {
            runtime.deliver(message, Instant::now(), driver)
        }));
    }
    nexts
}

/// How long one frame lasts on the display `window` is on now.
fn monitor_frame(window: &Window) -> std::time::Duration {
    events::frame_period(
        window
            .current_monitor()
            .and_then(|monitor| monitor.refresh_rate_millihertz()),
    )
}

/// The runtime's driver in the window: the shell's own effects, and the
/// app's [`Services`] for the rest.
struct ShellDriver<M, S> {
    /// The clipboard and dialogs.
    services: S,
    /// The window, for its title and limits.
    window: Option<Arc<Window>>,
    /// Captures asked for in this turn, drawn after it.
    captures: Vec<Reply<Result<Image, NoGpu>, M>>,
    /// The app asked to quit.
    exit: bool,
}

impl<M: Send + 'static, S: Services<M>> Driver<M> for ShellDriver<M, S> {
    fn window(&mut self, spec: &WindowSpec) {
        if let Some(window) = &self.window {
            window.set_title(&spec.title);
            window.set_min_inner_size(spec.min_size.map(|min| LogicalSize::new(min.width, min.height)));
        }
    }

    fn clipboard_write(&mut self, text: String) {
        self.services.clipboard_write(text);
    }

    fn clipboard_read(&mut self, reply: Reply<Option<String>, M>, waker: &Waker<M>) {
        self.services.clipboard_read(reply, waker);
    }

    fn file_dialog(&mut self, request: FileRequest, reply: Reply<Option<PathBuf>, M>, waker: &Waker<M>) {
        self.services.file_dialog(request, reply, waker);
    }

    fn ask(&mut self, question: Question, reply: Reply<Choice, M>, waker: &Waker<M>) {
        self.services.ask(question, reply, waker);
    }

    /// Kept for after the turn, when the shell has the layers to draw with.
    fn capture(&mut self, reply: Reply<Result<Image, NoGpu>, M>, _waker: &Waker<M>) {
        self.captures.push(reply);
    }

    fn work(&mut self, job: Job<M>, waker: &Waker<M>) {
        let waker = waker.clone();
        thread::spawn(move || job.run(&waker));
    }

    fn exit(&mut self) {
        self.exit = true;
    }
}

#[cfg(test)]
mod tests;
