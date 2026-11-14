//! The window's layers on `loom-ui`'s shell: Iced draws the app's views
//! into the shell's frames until phase 3 (plan step 2.2b-ii).
//!
//! The shell owns the window, the GPU, the runtime, and the redraw rule.
//! These layers do what Iced's own loop did around them:
//!
//! - [`Layers::window_event`] turns each window event into Iced's, and
//!   keeps it for the next report, with the cursor and the modifiers;
//! - [`Layers::report`], after each turn, runs one Iced update over the
//!   events kept, and posts the widgets' messages, then the listeners'
//!   (`subscriptions.rs`), each routed (`input_owner::route`). Then it runs
//!   the turn's Iced tasks through `tasks.rs`: focus and scrolls, the window
//!   questions, and the focus query, whose answer runs alone
//!   ([`Layers::alone`]);
//! - [`Layers::draw`] runs Iced's redraw update, draws, and presents.
//!
//! Iced's loop ran a redraw update's messages inside the frame. Here they
//! go to the next turn, since the layers only read the app while they
//! draw.

use super::tasks::{self, Sink, Stuck};
use super::{Bridged, FocusAnswer, Hosted, IcedTarget, focus_query, question_task};
use crate::{App, Message};
use iced::advanced::renderer::Style;
use iced::advanced::widget::Operation;
use iced::advanced::widget::operation::Outcome;
use iced::{Task, mouse};
use iced_runtime::user_interface::{Cache, State, UserInterface};
use iced_wgpu::graphics::{Shell, Viewport};
use loom_ui::shell::{Frame, Ime, Layers, Opened, Parts, Pointer, Redraw, Report};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::Instant;
use winit::event::WindowEvent as WinitEvent;
use winit::keyboard::ModifiersState;

/// The window's clipboard, shared by the layers (for Iced's widgets) and
/// the services (for the app's effects), once the window has opened.
pub(crate) type SharedClipboard = Rc<RefCell<Option<iced_winit::Clipboard>>>;

/// Iced's layers in the shell's window.
pub(crate) struct WindowLayers {
    /// Iced's renderer, built once the GPU is ready. Always the wgpu one:
    /// tiny-skia cannot draw into the shell's frames. Step B2 drops the
    /// fallback wrapper.
    renderer: Option<crate::Renderer>,
    /// The widget tree's state between builds.
    cache: Option<Cache>,
    /// Window events since the last report, as Iced's.
    events: Vec<iced::Event>,
    /// Where the pointer is, in logical pixels.
    cursor: mouse::Cursor,
    /// The modifiers held, for converting events.
    modifiers: ModifiersState,
    /// The window's scale, for converting events.
    scale: f32,
    /// The clipboard, for Iced's widgets.
    clipboard: SharedClipboard,
    /// Focus answers waiting to run alone, in the order asked.
    answers: VecDeque<FocusAnswer>,
    /// The pointer Iced last asked for, and whether it was reported.
    pointer: (mouse::Interaction, bool),
    /// The input method Iced last asked for, and whether it was reported.
    ime: (Option<Ime>, bool),
    /// When Iced last asked for a frame.
    redraw: Redraw,
    /// The kinds of stuck task logged already: waits, spins, an action.
    logged: [bool; 3],
}

impl WindowLayers {
    /// The layers, with the clipboard the services share.
    pub(crate) fn new(clipboard: SharedClipboard) -> Self {
        Self {
            renderer: None,
            cache: Some(Cache::default()),
            events: Vec::new(),
            cursor: mouse::Cursor::Unavailable,
            modifiers: ModifiersState::default(),
            scale: 1.0,
            clipboard,
            answers: VecDeque::new(),
            pointer: (mouse::Interaction::None, true),
            ime: (None, true),
            redraw: Redraw::No,
            logged: [false; 3],
        }
    }

    /// Builds `app`'s view at `size`, runs `f` on it with the renderer, the
    /// cursor, and the clipboard, and keeps its cache. Nothing before the
    /// renderer exists.
    fn with_ui<R>(
        &mut self,
        app: &App,
        size: iced::Size,
        f: impl FnOnce(&mut Ui<'_>, &mut crate::Renderer, mouse::Cursor, &mut dyn iced::advanced::Clipboard) -> R,
    ) -> Option<R> {
        let renderer = self.renderer.as_mut()?;
        let cache = self.cache.take().unwrap_or_default();
        let mut ui = UserInterface::build(app.view(), size, cache, renderer);
        let mut clipboard = self.clipboard.borrow_mut();
        let result = match clipboard.as_mut() {
            Some(clipboard) => f(&mut ui, renderer, self.cursor, clipboard),
            None => f(&mut ui, renderer, self.cursor, &mut iced::advanced::clipboard::Null),
        };
        self.cache = Some(ui.into_cache());
        Some(result)
    }

    /// Keeps what an update's state asks of the window.
    fn take_state(&mut self, state: &State) {
        if let State::Updated {
            mouse_interaction,
            redraw_request,
            ..
        } = state
        {
            if self.pointer.0 != *mouse_interaction {
                self.pointer = (*mouse_interaction, false);
            }
            self.redraw = sooner(self.redraw, redraw(*redraw_request));
        }
    }

    /// Logs a task the window cannot finish, once for each kind, and drops
    /// it. The `Ui` harness panics on the same tasks instead.
    fn stuck(&mut self, stuck: &Stuck) {
        let (kind, why) = match stuck {
            Stuck::Waits => (0, "waits on something the window does not give".to_string()),
            Stuck::Spins => (1, "keeps waking itself and never goes on".to_string()),
            Stuck::Action(action) => (2, format!("asks for an action the window does not run: {action}")),
        };
        if !std::mem::replace(&mut self.logged[kind], true) {
            eprintln!("loom-app: a window task {why}; dropped");
        }
    }
}

/// The app's view as Iced builds it.
type Ui<'a> = UserInterface<'a, Message, iced::Theme, crate::Renderer>;

/// Iced's redraw request, as the shell's.
fn redraw(request: iced::window::RedrawRequest) -> Redraw {
    match request {
        iced::window::RedrawRequest::NextFrame => Redraw::NextFrame,
        iced::window::RedrawRequest::At(at) => Redraw::At(at),
        iced::window::RedrawRequest::Wait => Redraw::No,
    }
}

/// The sooner of two frame requests.
fn sooner(a: Redraw, b: Redraw) -> Redraw {
    match (a, b) {
        (Redraw::NextFrame, _) | (_, Redraw::NextFrame) => Redraw::NextFrame,
        (Redraw::At(a), Redraw::At(b)) => Redraw::At(a.min(b)),
        (Redraw::At(at), Redraw::No) | (Redraw::No, Redraw::At(at)) => Redraw::At(at),
        (Redraw::No, Redraw::No) => Redraw::No,
    }
}

/// Iced's pointer, as the shell's.
fn pointer(interaction: mouse::Interaction) -> Pointer {
    match iced_winit::conversion::mouse_interaction(interaction) {
        Some(icon) => Pointer::Icon(icon),
        None => Pointer::Hidden,
    }
}

/// Iced's input method, as the shell's.
fn ime(method: &iced::advanced::InputMethod) -> Ime {
    match method {
        iced::advanced::InputMethod::Disabled => Ime::Off,
        iced::advanced::InputMethod::Enabled { cursor, .. } => Ime::On {
            area: loom_ui_core::Rect::new(
                loom_ui_core::Point::new(cursor.x, cursor.y),
                loom_ui_core::Size::new(cursor.width, cursor.height),
            ),
        },
    }
}

/// The messages one Iced update over `events` gives, routed, in Iced's
/// order: the widgets' `messages` first, then what the `listeners` hear of
/// each event with its status, as Iced ran a batch's messages before its
/// subscriptions heard the batch.
fn batch(
    messages: Vec<Message>,
    events: &[iced::Event],
    statuses: &[iced::event::Status],
    listeners: crate::subscriptions::Listeners,
) -> Vec<Message> {
    let heard = events
        .iter()
        .zip(statuses)
        .flat_map(|(event, status)| listeners.messages(event, *status));
    messages
        .into_iter()
        .chain(heard)
        .flat_map(crate::input_owner::route)
        .collect()
}

/// The logical size the runtime knows, as Iced's.
fn size(parts: &Parts<'_, Hosted, IcedTarget>) -> iced::Size {
    iced::Size::new(parts.window.size.width, parts.window.size.height)
}

impl Layers<Hosted, IcedTarget> for WindowLayers {
    /// Builds the renderer, connects the clipboard, and attaches the
    /// screen reader's adapter, before the window is shown.
    fn open(&mut self, parts: Parts<'_, Hosted, IcedTarget>, opened: &Opened<'_>) {
        let engine = iced_wgpu::Engine::new(
            opened.adapter,
            opened.device.clone(),
            opened.queue.clone(),
            opened.format,
            // Antialiasing off, as the app ran on Iced.
            None,
            // We load no images or vector images, so nothing asks for a
            // redraw from outside a turn.
            Shell::headless(),
        );
        // Iced's defaults, as the app ran on Iced: its default font at 16.
        self.renderer = Some(crate::Renderer::Primary(iced_wgpu::Renderer::new(
            engine,
            iced::Font::DEFAULT,
            iced::Pixels(16.0),
        )));
        *self.clipboard.borrow_mut() = Some(iced_winit::Clipboard::connect(opened.window.clone()));
        self.scale = opened.window.scale_factor() as f32;
        #[cfg(target_os = "macos")]
        crate::a11y::attach(opened.window, parts.waker.clone());
        #[cfg(not(target_os = "macos"))]
        let _ = parts;
    }

    fn window_event(&mut self, _parts: Parts<'_, Hosted, IcedTarget>, event: &WinitEvent) {
        match event {
            WinitEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WinitEvent::ScaleFactorChanged { scale_factor, .. } => self.scale = *scale_factor as f32,
            WinitEvent::CursorMoved { position, .. } => {
                self.cursor = mouse::Cursor::Available(iced::Point::new(
                    position.x as f32 / self.scale,
                    position.y as f32 / self.scale,
                ));
            }
            WinitEvent::CursorLeft { .. } => self.cursor = mouse::Cursor::Unavailable,
            // The frame's own update runs in `draw`.
            WinitEvent::RedrawRequested => return,
            _ => {}
        }
        if let Some(event) = iced_winit::conversion::window_event(event.clone(), self.scale, self.modifiers) {
            self.events.push(event);
        }
    }

    fn report(&mut self, parts: Parts<'_, Hosted, IcedTarget>) -> Report {
        let app = &parts.app.0;
        // Iced's subscriptions hear a batch as the app stood before it.
        let listeners = app.listeners();
        let events = std::mem::take(&mut self.events);
        if !events.is_empty() {
            let mut messages = Vec::new();
            let updated = self.with_ui(app, size(&parts), |ui, renderer, cursor, clipboard| {
                ui.update(&events, cursor, renderer, clipboard, &mut messages)
            });
            if let Some((state, statuses)) = updated {
                self.take_state(&state);
                parts.waker.send_all(batch(messages, &events, &statuses, listeners));
            }
        }
        // The turn's window work, one after another, as the bridge chained
        // it: focus and scrolls, the window questions, then the focus query.
        let mut window = Task::batch(parts.target.take_tasks());
        for question in app.take_window_questions() {
            window = window.chain(question_task(question).map(Bridged::App));
        }
        if app.take_focus_question() {
            window = window.chain(focus_query(Bridged::Focus));
        }
        let size = size(&parts);
        let ran = tasks::run(
            window,
            &mut WindowSink {
                layers: self,
                app,
                size,
            },
        );
        for output in ran.outputs {
            match output {
                Bridged::App(message) => {
                    parts.waker.send_all(crate::input_owner::route(message));
                }
                Bridged::Focus(answer) => self.answers.push_back(answer),
                // Only the harness's bridge makes these.
                #[cfg(any(test, feature = "test-ui"))]
                Bridged::Woken | Bridged::Deadline => {}
                #[cfg(test)]
                Bridged::Presented => {}
            }
        }
        if let Some(stuck) = &ran.stuck {
            self.stuck(stuck);
        }
        let cursor = (!std::mem::replace(&mut self.pointer.1, true)).then(|| pointer(self.pointer.0));
        let ime = (!std::mem::replace(&mut self.ime.1, true))
            .then_some(self.ime.0)
            .flatten();
        Report {
            cursor,
            ime,
            redraw: std::mem::take(&mut self.redraw),
            alone: !self.answers.is_empty(),
        }
    }

    /// The oldest focus answer: the target takes it, then `FocusMoved`
    /// tells the app, alone, as the bridge did.
    fn alone(&mut self, parts: Parts<'_, Hosted, IcedTarget>) -> Option<Message> {
        let part = self.answers.pop_front()?.part();
        parts.target.focused = part.clone();
        Some(Message::FocusMoved(part))
    }

    fn draw(&mut self, parts: Parts<'_, Hosted, IcedTarget>, frame: &Frame<'_>) {
        let app = &parts.app.0;
        let theme = app.theme();
        let background = theme.palette().background;
        let style = Style {
            text_color: theme.palette().text,
        };
        let physical = iced::Size::new(
            (frame.size.width * frame.scale).round() as u32,
            (frame.size.height * frame.scale).round() as u32,
        );
        let viewport = Viewport::with_physical_size(physical, frame.scale);
        if frame.capture {
            // A capture is the window's last frame drawn again, as Iced's
            // screenshot was: the renderer still holds what that frame
            // recorded. Building the view anew would draw each widget before
            // its redraw update has set its status, so buttons would show as
            // disabled, and running that update here would be frame work a
            // capture must not do. A change the turn before is not in it
            // yet: the control channel's `screenshot` asks only after two
            // frames have been shown (`control::FRAMES_BEFORE_ANSWER`), and
            // the shell holds a capture until the first frame is shown. The
            // last frame is at the current size too: after a resize the
            // shell draws before any capture is due.
            if let Some(crate::Renderer::Primary(renderer)) = self.renderer.as_mut() {
                renderer.present(Some(background), frame.format, frame.view, &viewport);
            }
            return;
        }
        let mut messages = Vec::new();
        let window = size(&parts);
        let drawn = self.with_ui(app, window, |ui, renderer, cursor, clipboard| {
            let redrawn = [iced::Event::Window(
                iced::window::Event::RedrawRequested(Instant::now()),
            )];
            // Again while the layout changes, at most three times, as Iced's
            // loop does; a message ends it, since it runs on the next turn
            // here.
            let mut passes = 0;
            let state = loop {
                let (state, _) = ui.update(&redrawn, cursor, renderer, clipboard, &mut messages);
                passes += 1;
                if !messages.is_empty() || !state.has_layout_changed() || passes == 3 {
                    break state;
                }
            };
            ui.draw(renderer, &theme, &style, cursor);
            if let State::Updated { input_method, .. } = &state
                && let Some(composed) = super::preedit::of(input_method)
                && let iced::advanced::InputMethod::Enabled { cursor: caret, .. } = input_method
            {
                super::preedit::draw(renderer, composed, *caret, window, &theme, crate::style::BODY);
            }
            if let crate::Renderer::Primary(renderer) = renderer {
                renderer.present(Some(background), frame.format, frame.view, &viewport);
            }
            state
        });
        if let Some(state) = drawn {
            self.take_state(&state);
            if let State::Updated { input_method, .. } = &state {
                let method = Some(ime(input_method));
                if self.ime.0 != method {
                    self.ime = (method, false);
                }
            }
            // Only the widgets' messages: Iced never gave its listeners the
            // redraw event (`listen_with` drops it), so none hears it here.
            let routed: Vec<Message> = messages.into_iter().flat_map(crate::input_owner::route).collect();
            parts.waker.send_all(routed);
        }
    }
}

/// Where the window's tasks act: widget operations on the view, and the
/// window's clipboard.
struct WindowSink<'a> {
    layers: &'a mut WindowLayers,
    app: &'a App,
    size: iced::Size,
}

impl Sink for WindowSink<'_> {
    fn operate(&mut self, operation: Box<dyn Operation>) {
        let mut next = Some(operation);
        while let Some(mut operation) = next.take() {
            self.layers.with_ui(self.app, self.size, |ui, renderer, _, _| {
                ui.operate(renderer, operation.as_mut());
            });
            if let Outcome::Chain(chained) = operation.finish() {
                next = Some(chained);
            }
        }
    }

    fn clipboard_read(&mut self) -> Option<String> {
        let clipboard = self.layers.clipboard.borrow();
        clipboard.as_ref()?.read(iced::advanced::clipboard::Kind::Standard)
    }

    fn clipboard_write(&mut self, contents: String) {
        if let Some(clipboard) = self.layers.clipboard.borrow_mut().as_mut() {
            clipboard.write(iced::advanced::clipboard::Kind::Standard, contents);
        }
    }
}

#[cfg(test)]
mod tests;
