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
//! - [`Layers::draw`] runs Iced's redraw update and draws: the canvas in
//!   the app's own pass, then Iced's layers (`paint.rs`).
//!
//! Iced's loop ran a redraw update's messages inside the frame. Here they
//! go to the next turn, since the layers only read the app while they
//! draw.

use super::tasks::{self, Sink, Stuck};
use super::{Bridged, FocusAnswer, Hosted, IcedTarget, answer_task, focus_query};
use crate::{App, Message};
use iced::advanced::renderer::Style;
use iced::advanced::widget::Operation;
use iced::advanced::widget::operation::Outcome;
use iced::{Task, mouse};
use iced_runtime::user_interface::{Cache, State, UserInterface};
use iced_wgpu::graphics::Shell;
use loom_ui::shell::{Frame, Ime, Layers, Opened, Parts, Pointer, Redraw, Report};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::time::Instant;
use winit::event::WindowEvent as WinitEvent;
use winit::keyboard::ModifiersState;

/// Iced's widgets' clipboard: the shell's own (`loom_ui::clipboard`), the
/// one the app's effects and the runtime's fields use, so every path shares
/// one backend (ui-layer.md, section 8, rule 2). A bridge, gone in phase 4.
/// Iced reads synchronously, so a read waits for the clipboard's thread,
/// as Iced's own clipboard read waited on the system. The primary
/// selection is out of scope, so it reads nothing and writes nothing.
pub(crate) struct IcedClipboard(loom_ui::clipboard::Clipboard);

impl iced::advanced::Clipboard for IcedClipboard {
    fn read(&self, kind: iced::advanced::clipboard::Kind) -> Option<String> {
        match kind {
            iced::advanced::clipboard::Kind::Standard => self.0.read_now().text().map(str::to_owned),
            iced::advanced::clipboard::Kind::Primary => None,
        }
    }

    fn write(&mut self, kind: iced::advanced::clipboard::Kind, contents: String) {
        if kind == iced::advanced::clipboard::Kind::Standard {
            self.0.write(contents);
        }
    }
}

/// Iced's layers in the shell's window.
pub(crate) struct WindowLayers {
    /// Iced's renderer, built once the GPU is ready. Always the wgpu half
    /// of the app's renderer type; the other half, the measure renderer,
    /// is the headless harness's and draws nothing.
    renderer: Option<crate::Renderer>,
    /// The canvas's renderer, built with Iced's, for the window's format.
    canvas: Option<super::paint::CanvasPass>,
    /// The renderers for the runtime's own panels, below and above Iced.
    panels: Option<super::paint::PanelPass>,
    /// The text the runtime's panels are measured and drawn with: one
    /// typesetter for both, so what is measured is what is drawn.
    text: RefCell<loom_ui::paint::Typesetter>,
    /// The widget tree's state between builds.
    cache: Option<Cache>,
    /// Window events since the last report, as Iced's.
    events: Vec<iced::Event>,
    /// The pointer events among `events`, for the runtime's layers.
    pointers: super::pointers::Pointers,
    /// Where the pointer is, in logical pixels.
    cursor: mouse::Cursor,
    /// The modifiers held, for converting events.
    modifiers: ModifiersState,
    /// The window's scale, for converting events.
    scale: f32,
    /// The clipboard, for Iced's widgets.
    clipboard: IcedClipboard,
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
    /// The layers, on the system clipboard the shell's driver uses too.
    pub(crate) fn new() -> Self {
        let clipboard = IcedClipboard(loom_ui::clipboard::Clipboard::system());
        Self {
            renderer: None,
            canvas: None,
            panels: None,
            text: RefCell::new(loom_ui::paint::Typesetter::new()),
            cache: Some(Cache::default()),
            events: Vec::new(),
            pointers: super::pointers::Pointers::default(),
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
        let result = f(&mut ui, renderer, self.cursor, &mut self.clipboard);
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

/// Iced's pointer, as the shell's: our own shape, which the shell turns
/// into winit's.
fn pointer(interaction: mouse::Interaction) -> Pointer {
    use loom_ui_core::Cursor;
    use mouse::Interaction;
    Pointer::Icon(match interaction {
        Interaction::Hidden => return Pointer::Hidden,
        Interaction::Grab => Cursor::Grab,
        Interaction::Grabbing => Cursor::Grabbing,
        Interaction::Pointer => Cursor::Pointer,
        Interaction::Crosshair => Cursor::Crosshair,
        Interaction::Text => Cursor::Text,
        Interaction::NotAllowed => Cursor::NotAllowed,
        Interaction::ResizingHorizontally => Cursor::ResizeHorizontal,
        Interaction::ResizingVertically => Cursor::ResizeVertical,
        Interaction::ResizingDiagonallyDown => Cursor::ResizeDiagonal,
        // `None` and `Idle` are the plain arrow. No part of the app shows
        // the other shapes Iced has (help, wait, copy, zoom, and the rest),
        // so they are the arrow too. A shape is added to `Cursor` when a
        // view needs it.
        _ => Cursor::Default,
    })
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
    /// Builds the renderer, before the window is shown. The screen reader's adapter is the shell's own.
    fn open(&mut self, _parts: Parts<'_, Hosted, IcedTarget>, opened: &Opened<'_>) {
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
        self.canvas = Some(super::paint::CanvasPass::new(opened.device, opened.format));
        self.panels = Some(super::paint::PanelPass::new(opened.device, opened.format));
        self.scale = opened.window.scale_factor() as f32;
    }

    fn window_event(&mut self, parts: Parts<'_, Hosted, IcedTarget>, event: &WinitEvent) {
        match event {
            WinitEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WinitEvent::ScaleFactorChanged { scale_factor, .. } => self.scale = *scale_factor as f32,
            // A touch moves the cursor too, as Iced's window did, so a tap
            // on a field reaches the widget under the finger.
            WinitEvent::CursorMoved { position, .. }
            | WinitEvent::Touch(winit::event::Touch { location: position, .. }) => {
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
        if let Some(converted) = iced_winit::conversion::window_event(event.clone(), self.scale, self.modifiers) {
            // The shell hands the runtime the event itself.
            let _ = self.pointers.before(&parts.panels, converted, &mut self.events);
        }
    }

    fn report(&mut self, mut parts: Parts<'_, Hosted, IcedTarget>) -> Report {
        let app = &parts.app.0;
        // Iced's subscriptions hear a batch as the app stood before it.
        let listeners = app.listeners();
        let events = std::mem::take(&mut self.events);
        let mut pointers = std::mem::take(&mut self.pointers);
        let size = size(&parts);
        let mut iced = WindowIced {
            layers: self,
            app,
            size,
            listeners,
            messages: Vec::new(),
        };
        let redraw = pointers.update(events, parts.target, &mut parts.panels, &mut iced);
        // Iced's messages after what the pointer step queued, so a focus
        // our elements took is the first thing the next turn hears.
        parts.waker.send_all(iced.messages);
        self.pointers = pointers;
        if redraw {
            self.redraw = Redraw::NextFrame;
        }
        // The turn's window work, one after another, as the bridge chained
        // it: focus and scrolls, the window questions, then the focus query.
        let mut window = Task::batch(parts.target.take_tasks());
        let field = parts.panels.focused_field().cloned();
        for question in app.take_window_questions() {
            window = window.chain(answer_task(question, field.as_ref()).map(Bridged::App));
        }
        if app.take_focus_question() {
            window = window.chain(focus_query(Bridged::Focus));
        }
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
                Bridged::Woken | Bridged::Deadline | Bridged::Presented => {}
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

    fn draw(&mut self, mut parts: Parts<'_, Hosted, IcedTarget>, frame: &Frame<'_>) {
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
        let target = super::paint::Target {
            device: frame.device,
            queue: frame.queue,
            view: frame.view,
            format: frame.format,
            physical,
            scale: frame.scale,
        };
        if frame.capture {
            // A capture is the window's last frame drawn again, as Iced's
            // screenshot was: the renderer still holds what that frame
            // recorded. Building the view anew would draw each widget before
            // its redraw update has set its status, so buttons would show as
            // disabled, and running that update here would be frame work a
            // capture must not do. A change the turn before is not in it
            // yet: the control channel's `screenshot` asks only after two
            // frames have been shown (`control::FRAMES_BEFORE_ANSWER`), and
            // the shell holds a capture until the first frame is shown.
            //
            // The last frame may be at an older size: the shell can run a
            // capture right after a resize, before a frame at the new size.
            // Iced prepares its layers again for every present, so they fit
            // the capture. The canvas is drawn again from its last frame, at
            // the bounds it last had, and prepared again only when the
            // capture's size differs (`paint::draw_again`). After New or Open
            // the last frame is the new patch's, since a frame of it was
            // shown first.
            if let (Some(renderer), Some(pass)) = (self.renderer.as_mut(), self.canvas.as_mut()) {
                super::paint::draw_again(&target, wgpu(renderer), pass, background, self.panels.as_mut());
            }
            return;
        }
        let Some(mut pass) = self.canvas.take() else { return };
        // The runtime's own panels: laid out, then painted below and above
        // Iced, with the one typesetter (ui-layer.md, section 5, rule 9).
        let mut panels = self.panels.take();
        if let Some(panels) = panels.as_mut() {
            let (below, above) = self.paint_panels(&mut parts, frame, &theme);
            panels.set(&target, below, above);
        }
        if app.pointer.waiting() {
            app.pointer.place_runtime_targets(&parts.panels);
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
            // The canvas widget leaves its frame in the app's slot while
            // Iced draws; emptied first, so only this frame's is drawn.
            app.canvas_drawn.clear();
            ui.draw(renderer, &theme, &style, cursor);
            if let State::Updated { input_method, .. } = &state
                && let Some(composed) = super::preedit::of(input_method)
                && let iced::advanced::InputMethod::Enabled { cursor: caret, .. } = input_method
            {
                super::preedit::draw(renderer, composed, *caret, window, &theme, crate::style::BODY);
            }
            let drawing = app.canvas_drawn.take();
            super::paint::draw(&target, wgpu(renderer), &mut pass, drawing, background, panels.as_ref());
            state
        });
        self.canvas = Some(pass);
        self.panels = panels;
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
        // The control channel's scripted pointer events go into Iced on a
        // redraw, so what Iced left of them goes on to our layers here.
        let (_, cancelled) = app.pointer.take_physical_changes();
        if super::pointers::hand_scripted(app.pointer.take_for_runtime(), &mut parts.panels)
            | super::pointers::cancel_holds(cancelled, &mut parts.panels)
        {
            self.redraw = Redraw::NextFrame;
        }
    }
}

/// Iced in the window, for [`super::pointers::Pointers::update`].
struct WindowIced<'a> {
    layers: &'a mut WindowLayers,
    app: &'a App,
    size: iced::Size,
    /// The listeners as the app stood before the batch.
    listeners: crate::subscriptions::Listeners,
    /// The update's messages, routed, sent after the pointer step.
    messages: Vec<Message>,
}

impl super::pointers::IcedSide for WindowIced<'_> {
    /// A release the runtime owes, on the window's widgets; Iced's word on
    /// focus after it runs alone, as every focus answer does.
    fn run(&mut self, task: Task<Bridged>) {
        let ran = tasks::run(
            task,
            &mut WindowSink {
                layers: self.layers,
                app: self.app,
                size: self.size,
            },
        );
        for output in ran.outputs {
            if let Bridged::Focus(answer) = output {
                self.layers.answers.push_back(answer);
            }
        }
    }

    fn update(&mut self, events: &[iced::Event]) -> Option<Vec<iced::event::Status>> {
        let mut messages = Vec::new();
        let (state, statuses) = self
            .layers
            .with_ui(self.app, self.size, |ui, renderer, cursor, clipboard| {
                ui.update(events, cursor, renderer, clipboard, &mut messages)
            })?;
        self.layers.take_state(&state);
        self.messages.extend(batch(messages, events, &statuses, self.listeners));
        Some(statuses)
    }

    fn take_scripted(&mut self) -> Vec<(iced::Event, iced::Point, crate::pointer::Delivery)> {
        self.app.pointer.take_for_runtime()
    }

    fn physical_changes(&mut self) -> (Vec<loom_ui_core::Event>, Vec<iced::mouse::Button>) {
        self.app.pointer.take_physical_changes()
    }
}

impl WindowLayers {
    /// Lays out the runtime's own panels and paints them into two lists:
    /// the canvas and panels layers, drawn below Iced, and the overlays
    /// and question layers, drawn above it. Text with no colour of its own
    /// takes the theme's text colour.
    fn paint_panels(
        &self,
        parts: &mut Parts<'_, Hosted, IcedTarget>,
        frame: &Frame<'_>,
        theme: &iced::Theme,
    ) -> (loom_ui::paint::PaintList, loom_ui::paint::PaintList) {
        use loom_ui::paint::ListPainter;
        self.text.borrow_mut().begin_frame();
        parts.panels.lay_out(&mut *self.text.borrow_mut());
        let whole = loom_ui_core::Rect::new(loom_ui_core::Point::ORIGIN, frame.size);
        let style = super::paint::panel_style(theme);
        super::paint::paint_sides(whole, |layer, list| {
            parts
                .panels
                .paint(layer, &mut ListPainter::new(&self.text, list), &style);
        })
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
        iced::advanced::Clipboard::read(&self.layers.clipboard, iced::advanced::clipboard::Kind::Standard)
    }

    fn clipboard_write(&mut self, contents: String) {
        iced::advanced::Clipboard::write(
            &mut self.layers.clipboard,
            iced::advanced::clipboard::Kind::Standard,
            contents,
        );
    }
}

/// The wgpu renderer the window draws with. The window only ever makes the
/// wgpu half of the app's renderer type (see `WindowLayers::renderer`).
fn wgpu(renderer: &mut crate::Renderer) -> &mut iced_wgpu::Renderer {
    match renderer {
        crate::Renderer::Primary(wgpu) => wgpu,
        crate::Renderer::Secondary(_) => unreachable!("the window draws with wgpu, never the measure renderer"),
    }
}

#[cfg(test)]
mod tests;
