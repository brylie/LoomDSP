//! The loop's one turn: take an event, run the updates, carry out their
//! effects, and decide whether to draw.
//!
//! One call to [`Runtime::handle`] is one turn (ui-layer.md, section 2,
//! Behaviour):
//!
//! 1. The messages waiting from wakers and from earlier effects, and the
//!    app's deadline if it has come, make the turn's batch.
//! 2. The event is routed: an element event to the target, a window event
//!    to the runtime's window state and to [`App::window_event`].
//! 3. Each message in the batch gets one `update`, and that update's
//!    effects run right after it returns (rule 5). A message an effect
//!    produces waits for the next turn.
//! 4. After a turn that ran an update, the runtime builds the view and
//!    compares each panel's model with the last, reads the window spec, and
//!    asks the app for its next deadline.
//! 5. It says to draw when a panel changed, the target asks for it, an
//!    effect asked for it, or the event was a redraw request or a frame.
//!    Otherwise nothing is drawn (section 3, rule 6).
//! 6. It returns [`Next`]: whether to draw, and then whether to run again
//!    at once, send frames, wake at a time, or sleep.
//!
//! The runtime never reads the clock: the driver passes the moment in.

use crate::app::{App, Context, Deadline, WindowSpec};
use crate::driver::Driver;
use crate::effect::Effect;
use crate::event::{DriverEvent, WindowEvent, WindowState};
use crate::routing::Target;
use crate::view::View;
use crate::waker::{Wake, Waker};
use loom_ui_core::Event;
use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver};
use std::time::Instant;

/// What the driver should do after a turn.
///
/// # Drawing
///
/// When `draw` is true, the driver draws one frame before it next waits.
/// It draws at once when the turn was for its own
/// [`DriverEvent::RedrawRequested`] or [`DriverEvent::Frame`]. Otherwise it
/// may ask its window system for a frame, and when that frame comes it
/// draws it without sending `RedrawRequested` for it: `RedrawRequested` is
/// only for a frame the system wants that the runtime did not ask for.
/// Several turns that say `draw` before the frame is drawn make one frame.
/// The runtime returns this rather than calling the driver, so a shell
/// cannot turn its own redraw request into a loop of draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Next {
    /// Draw one frame before waiting.
    pub draw: bool,
    /// Messages wait: run another turn at once, with [`DriverEvent::Woken`].
    pub again: bool,
    /// Send a [`DriverEvent::Frame`] each display refresh.
    pub frames: bool,
    /// Wake at this moment with [`DriverEvent::DeadlineReached`].
    pub deadline: Option<Instant>,
}

impl Next {
    /// True when nothing is due after any draw: the driver may sleep until an
    /// event comes.
    pub fn sleeps(&self) -> bool {
        !self.again && !self.frames && self.deadline.is_none()
    }
}

/// What the runtime has done since it started. The tests read these, and
/// so will `--measure-idle` (ui-layer.md, section 3, Measuring it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counts {
    /// Calls to [`Runtime::handle`].
    pub turns: u64,
    /// Calls to `update`.
    pub updates: u64,
    /// Panels compared with their last model.
    pub panels_compared: u64,
    /// Panels whose model, layer, or presence changed.
    pub panels_changed: u64,
    /// Animation frames received.
    pub frames: u64,
    /// Turns that said to draw.
    pub redraws: u64,
}

/// The runtime for one app and its routing target.
pub struct Runtime<A: App, T> {
    app: A,
    target: T,
    waker: Waker<A::Message>,
    queue: Receiver<A::Message>,
    pending: VecDeque<A::Message>,
    window: WindowState,
    view: Option<View>,
    spec: Option<WindowSpec>,
    deadline: Option<Deadline<A::Message>>,
    after_draw: Vec<A::Message>,
    waiting_for_draw: Vec<A::Message>,
    exited: bool,
    counts: Counts,
}

impl<A: App, T: Target<A::Message>> Runtime<A, T> {
    /// A runtime for `app`, routing to `target`, in a window that starts as
    /// `window`. `wake` is the driver's wake function, called whenever a
    /// waker sends a message.
    pub fn new(app: A, target: T, window: WindowState, wake: Wake) -> Self {
        let (sender, queue) = mpsc::channel();
        Self {
            app,
            target,
            waker: Waker::new(sender, wake),
            queue,
            pending: VecDeque::new(),
            window,
            view: None,
            spec: None,
            deadline: None,
            after_draw: Vec::new(),
            waiting_for_draw: Vec::new(),
            exited: false,
            counts: Counts::default(),
        }
    }

    /// A waker that sends messages into this runtime's queue.
    pub fn waker(&self) -> Waker<A::Message> {
        self.waker.clone()
    }

    /// The app, to read.
    pub fn app(&self) -> &A {
        &self.app
    }

    /// The app, to change, for a test driver that sets up state between
    /// turns. The queue, the deadline, and the messages waiting for a draw
    /// stay as they are. Only with the `test-driver` feature: a real driver
    /// changes the app only through `update` (section 2, rule 1).
    #[cfg(feature = "test-driver")]
    pub fn app_mut(&mut self) -> &mut A {
        &mut self.app
    }

    /// The routing target, to read.
    pub fn target(&self) -> &T {
        &self.target
    }

    /// The routing target, to change. A driver may need this to draw it.
    pub fn target_mut(&mut self) -> &mut T {
        &mut self.target
    }

    /// The app to read and the target to use, at once: a driver that draws
    /// the target's views from the app needs both.
    pub fn parts(&mut self) -> (&A, &mut T) {
        (&self.app, &mut self.target)
    }

    /// The window as the runtime knows it.
    pub fn window(&self) -> WindowState {
        self.window
    }

    /// What the runtime has done so far.
    pub fn counts(&self) -> Counts {
        self.counts
    }

    /// True once the app has asked to quit.
    pub fn exited(&self) -> bool {
        self.exited
    }

    /// True while a message waits for a frame to be shown
    /// ([`crate::Effect::AfterDraw`]). A driver that cannot tell when a
    /// frame is on screen reads this to know when to look for one.
    pub fn waits_for_draw(&self) -> bool {
        !self.after_draw.is_empty() || !self.waiting_for_draw.is_empty()
    }

    /// The view built after the last turn that ran an update.
    pub fn view(&self) -> Option<&View> {
        self.view.as_ref()
    }

    /// Runs one turn for `event` at `now`, with `driver` carrying out the
    /// effects that reach outside the app.
    pub fn handle(&mut self, event: DriverEvent, now: Instant, driver: &mut impl Driver<A::Message>) -> Next {
        if self.exited {
            return Next {
                draw: false,
                again: false,
                frames: false,
                deadline: None,
            };
        }
        // 1. The batch: messages already waiting, then the deadline.
        self.drain_queue();
        let mut batch: Vec<A::Message> = self.pending.drain(..).collect();
        if self.deadline.as_ref().is_some_and(|deadline| deadline.at <= now)
            && let Some(deadline) = self.deadline.take()
        {
            batch.push(deadline.message);
        }
        self.turn(event, batch, now, driver)
    }

    /// Runs one turn for `message` alone, at `now`. Messages already in the
    /// queue, and a deadline that is due, wait for the next turn. For a
    /// driver that must hand a message over at an exact point: the bridge
    /// to Iced first runs what is queued, then hands over the window's word
    /// on focus by itself, so no other message sees the new focus before
    /// the app has heard of it.
    pub fn deliver(&mut self, message: A::Message, now: Instant, driver: &mut impl Driver<A::Message>) -> Next {
        if self.exited {
            return Next {
                draw: false,
                again: false,
                frames: false,
                deadline: None,
            };
        }
        self.turn(DriverEvent::Woken, vec![message], now, driver)
    }

    /// One turn: routes `event`, runs `batch` and the messages routing
    /// adds, one update each, then works out what is next.
    fn turn(
        &mut self,
        event: DriverEvent,
        mut batch: Vec<A::Message>,
        now: Instant,
        driver: &mut impl Driver<A::Message>,
    ) -> Next {
        self.counts.turns += 1;
        let mut redraw = false;

        // 2. Route the event.
        match event {
            DriverEvent::Input { event, pointer } => self.target.input(&event, pointer, now, &mut batch),
            DriverEvent::Window(event) => self.window_event(event, now, &mut batch),
            DriverEvent::RedrawRequested => redraw = true,
            DriverEvent::Frame => {
                self.counts.frames += 1;
                redraw = true;
            }
            DriverEvent::Presented => batch.append(&mut self.waiting_for_draw),
            DriverEvent::Woken | DriverEvent::DeadlineReached => {}
        }

        // 3. One update per message, each followed by its own effects.
        let mut updates = 0;
        for message in batch {
            if self.exited {
                break;
            }
            updates += 1;
            redraw |= self.update(message, now, driver);
        }
        self.counts.updates += updates as u64;

        // 4. What changed.
        if updates > 0 || self.view.is_none() {
            redraw |= self.rebuild(driver);
        }
        redraw |= self.target.needs_redraw(updates);

        // 5. Draw only when something visible changed.
        let draw = redraw && !self.exited;
        if draw {
            self.counts.redraws += 1;
            self.waiting_for_draw.append(&mut self.after_draw);
        }

        // 6. What comes next.
        self.drain_queue();
        Next {
            draw,
            again: !self.exited && !self.pending.is_empty(),
            frames: !self.exited && self.app.animating(),
            deadline: if self.exited {
                None
            } else {
                self.deadline.as_ref().map(|deadline| deadline.at)
            },
        }
    }

    /// Moves everything wakers sent into `pending`, in order.
    fn drain_queue(&mut self) {
        self.pending.extend(self.queue.try_iter());
    }

    /// Applies a window event to the window state, tells the target when
    /// the window loses focus, and adds the app's message for it.
    fn window_event(&mut self, event: WindowEvent, now: Instant, batch: &mut Vec<A::Message>) {
        match &event {
            WindowEvent::Resized(size) => self.window.size = *size,
            WindowEvent::ScaleChanged(scale) => self.window.scale = *scale,
            WindowEvent::Focused(focused) => {
                self.window.focused = *focused;
                if !focused {
                    self.target.input(&Event::WindowUnfocused, None, now, batch);
                }
            }
            WindowEvent::CloseRequested | WindowEvent::FileDropped(_) => {}
        }
        batch.extend(self.app.window_event(&event));
    }

    /// Runs one update and its effects. Returns true when an effect asked
    /// for a draw.
    fn update(&mut self, message: A::Message, now: Instant, driver: &mut impl Driver<A::Message>) -> bool {
        let cx = Context {
            now,
            waker: &self.waker,
            focused: self.target.focused(),
            window: self.window,
        };
        let effects = self.app.update(message, &cx);
        let mut redraw = false;
        for effect in effects {
            match effect {
                Effect::Focus { target, select } => self.target.focus(&target, select),
                Effect::ScrollIntoView(id) => self.target.scroll_into_view(&id),
                Effect::ClipboardWrite(text) => driver.clipboard_write(text),
                Effect::ClipboardRead(reply) => driver.clipboard_read(reply, &self.waker),
                Effect::FileDialog { request, reply } => driver.file_dialog(request, reply, &self.waker),
                Effect::Ask { question, reply } => driver.ask(question, reply, &self.waker),
                Effect::Work(job) => driver.work(job, &self.waker),
                Effect::Capture(reply) => {
                    driver.capture(reply, &self.waker);
                    redraw = true;
                }
                Effect::AfterDraw(message) => {
                    self.after_draw.push(message);
                    redraw = true;
                }
                // After what wakers have sent so far, and before anything
                // they send later: `pending` is the runtime's own, so the
                // batch stays whole.
                Effect::NextTurn(messages) => {
                    self.drain_queue();
                    self.pending.extend(messages);
                }
                Effect::Redraw => redraw = true,
                Effect::Quit => {
                    self.exited = true;
                    driver.exit();
                    break;
                }
            }
        }
        redraw
    }

    /// Builds the view and compares it with the last, passes a changed
    /// window spec to the driver, and asks for the next deadline. Returns
    /// true when a panel changed.
    fn rebuild(&mut self, driver: &mut impl Driver<A::Message>) -> bool {
        let mut view = View::new();
        self.app.view(&mut view);
        let changed = match &self.view {
            Some(last) => view.changed_since(last),
            None => view.len(),
        };
        self.counts.panels_compared += view.len() as u64;
        self.counts.panels_changed += changed as u64;
        self.view = Some(view);

        let spec = self.app.window();
        if self.spec.as_ref() != Some(&spec) {
            driver.window(&spec);
            self.spec = Some(spec);
        }
        self.deadline = self.app.next_deadline();
        changed > 0
    }
}

#[cfg(test)]
mod tests;
