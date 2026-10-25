//! The control channel's pointer: `hover`, `press`, `release`, `drag`, and
//! `scroll` (`docs/design/control-channel.md`, Pointer commands).
//!
//! A command becomes pointer events in a queue. [`Injector`] wraps the
//! whole window view. On each redraw it takes one event from the queue and
//! sends it to the view, with the script's pointer as the cursor, so the
//! event goes through the same widget handling as a real one (rule 3).
//! The messages it produces reach `App::update` as usual, and the window is
//! drawn before the next event goes in.
//!
//! A target (`"4:cutoff"`, `"Cutoff"`) is found when its event goes in,
//! from the live view, as `locate` finds it.
//!
//! While the control channel is on, the injector also reads what the
//! canvas says the pointer is on, once per drawn frame, for the snapshot's
//! `pointer` field.

use crate::App;
use crate::control::Found;
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{Id, Operation, Tree, tree};
use iced::advanced::{Clipboard, Shell, Widget, overlay};
use iced::{Element, Event, Length, Point, Rectangle, Size, Vector, keyboard, mouse, window};
use loom_canvas::{CanvasPointer, CanvasView, Hit, PointerDrag, PortRef};
use serde::Serialize;
use std::any::Any;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::Instant;

/// Where an event goes: a window point, or a target found when the event
/// goes in.
#[derive(Debug, Clone, PartialEq)]
pub enum Aim {
    /// Logical window pixels.
    At(Point),
    /// A target as `locate` takes it.
    Target(String),
}

/// One step of a pointer command.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// Move the pointer there. A drag starts from here.
    MoveTo(Aim),
    /// Move a share `t` of the way from where the drag started to `to`.
    /// `to` is found anew for each move; until it is found the pointer
    /// stays put, so a target that appears during the drag works.
    Toward { to: Aim, t: f32 },
    /// A mouse button goes down where the pointer is.
    Press(mouse::Button),
    /// A mouse button goes up where the pointer is.
    Release(mouse::Button),
    /// A wheel step where the pointer is, in lines.
    Wheel { dx: f32, dy: f32 },
    /// The modifier keys held from now on. Sent with the next event, not
    /// on a frame of its own.
    Modifiers(keyboard::Modifiers),
    /// A key press or release, at the pointer. What no widget takes goes to
    /// the app as the keyboard listener would send it.
    Key(keyboard::Event),
}

/// A step and the command it belongs to.
#[derive(Debug, Clone)]
struct Queued {
    serial: u64,
    step: Step,
}

/// What the view last drew of the pointer.
#[derive(Debug, Clone, Default)]
struct Seen {
    /// The pointer in window coordinates, if it is in the window.
    cursor: Option<Point>,
    canvas: Option<CanvasPointer>,
}

#[derive(Debug, Default)]
struct Inner {
    /// Read the canvas's pointer state for the snapshot. True while the
    /// control channel is on, so an app without it does no extra work.
    record: bool,
    queue: VecDeque<Queued>,
    /// The script's pointer, in window coordinates, while it leads. A real
    /// pointer event in the window takes over.
    at: Option<Point>,
    /// Buttons the script holds down.
    buttons: Vec<mouse::Button>,
    /// Buttons a real pointer holds down.
    real_buttons: Vec<mouse::Button>,
    /// Where the current drag started.
    anchor: Option<Point>,
    /// Where the current drag's `to` was last found.
    toward: Option<Point>,
    /// The redraw an event last went in at. The runtime can send the same
    /// redraw to the view up to three times when it produces messages; only
    /// the first takes an event, so every event is drawn before the next.
    last_redraw: Option<Instant>,
    /// The first error of each command, by serial.
    errors: Vec<(u64, String)>,
    next_serial: u64,
    seen: Option<Seen>,
}

/// The script's pointer and its queue of events, shared by the app and the
/// view it builds.
#[derive(Debug, Clone, Default)]
pub struct Pointer(Rc<RefCell<Inner>>);

impl Pointer {
    /// A pointer that reads the canvas's pointer state for the snapshot
    /// when `record` is true.
    pub fn new(record: bool) -> Self {
        let pointer = Self::default();
        pointer.0.borrow_mut().record = record;
        pointer
    }

    /// Queues the steps of one command and returns its serial.
    pub fn push(&self, steps: Vec<Step>) -> u64 {
        let mut inner = self.0.borrow_mut();
        inner.next_serial += 1;
        let serial = inner.next_serial;
        inner
            .queue
            .extend(steps.into_iter().map(|step| Queued { serial, step }));
        serial
    }

    /// True while a command still has events to send.
    pub fn busy(&self, serial: u64) -> bool {
        self.0.borrow().queue.iter().any(|q| q.serial == serial)
    }

    /// The error a command met, if any. It is forgotten once taken.
    pub fn take_error(&self, serial: u64) -> Option<String> {
        let mut inner = self.0.borrow_mut();
        let index = inner.errors.iter().position(|(s, _)| *s == serial)?;
        Some(inner.errors.remove(index).1)
    }

    /// The pointer in window coordinates: the script's while it leads,
    /// else where the view last saw the real one.
    pub fn position(&self) -> Option<Point> {
        let inner = self.0.borrow();
        inner.at.or_else(|| inner.seen.as_ref().and_then(|s| s.cursor))
    }

    /// True when a command is waiting for its events to go in.
    pub fn waiting(&self) -> bool {
        !self.0.borrow().queue.is_empty()
    }

    /// Takes one event from the queue for the redraw at `now`, with the
    /// modifier changes before it. `resolve` finds a target. Returns each
    /// event with the cursor to send it with.
    fn next_events(
        &self,
        now: Instant,
        mut resolve: impl FnMut(&str) -> Result<Point, String>,
    ) -> Vec<(Event, Option<Point>)> {
        let mut inner = self.0.borrow_mut();
        if inner.queue.is_empty() || inner.last_redraw == Some(now) {
            return Vec::new();
        }
        inner.last_redraw = Some(now);
        let mut events = Vec::new();
        while let Some(Queued { serial, step }) = inner.queue.pop_front() {
            let at = inner.at;
            let mut find = |aim: &Aim| match aim {
                Aim::At(p) => Ok(*p),
                Aim::Target(target) => resolve(target),
            };
            let event = match step {
                Step::Modifiers(modifiers) => {
                    // A change of keys needs no frame of its own.
                    events.push((Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)), at));
                    continue;
                }
                Step::Key(key) => {
                    events.push((Event::Keyboard(key), at));
                    break;
                }
                Step::MoveTo(aim) => match find(&aim) {
                    Ok(p) => {
                        inner.anchor = Some(p);
                        inner.toward = None;
                        Ok((Event::Mouse(mouse::Event::CursorMoved { position: p }), p))
                    }
                    Err(e) => Err(e),
                },
                Step::Toward { to, t } => {
                    let found = find(&to);
                    if let Ok(p) = found {
                        inner.toward = Some(p);
                    }
                    let last = t >= 1.0;
                    match (found, inner.anchor, inner.toward) {
                        // The last move must reach `to` (control-channel.md).
                        (Err(e), _, _) if last => Err(e),
                        (_, Some(a), Some(b)) => {
                            let p = Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
                            Ok((Event::Mouse(mouse::Event::CursorMoved { position: p }), p))
                        }
                        // Not found yet: stay put until it appears.
                        _ => match at {
                            Some(p) => Ok((Event::Mouse(mouse::Event::CursorMoved { position: p }), p)),
                            None => Err("the pointer is not in the window yet".to_string()),
                        },
                    }
                }
                Step::Press(button) | Step::Release(button) => match at {
                    Some(p) => {
                        let down = matches!(step, Step::Press(_));
                        if down {
                            inner.buttons.push(button);
                            Ok((Event::Mouse(mouse::Event::ButtonPressed(button)), p))
                        } else {
                            inner.buttons.retain(|b| *b != button);
                            Ok((Event::Mouse(mouse::Event::ButtonReleased(button)), p))
                        }
                    }
                    None => Err("the pointer is not in the window yet; give a `target` or `x` and `y`".to_string()),
                },
                Step::Wheel { dx, dy } => match at {
                    Some(p) => Ok((
                        Event::Mouse(mouse::Event::WheelScrolled {
                            delta: mouse::ScrollDelta::Lines { x: dx, y: dy },
                        }),
                        p,
                    )),
                    None => Err("the pointer is not in the window yet; give a `target` or `x` and `y`".to_string()),
                },
            };
            match event {
                Ok((event, p)) => {
                    inner.at = Some(p);
                    events.push((event, Some(p)));
                }
                Err(error) => {
                    // The command ends here. Its button releases and key
                    // changes still go in, so nothing stays held.
                    if !inner.errors.iter().any(|(s, _)| *s == serial) {
                        inner.errors.push((serial, error));
                    }
                    let rest = std::mem::take(&mut inner.queue);
                    inner.queue = rest
                        .into_iter()
                        .filter(|q| {
                            q.serial != serial || matches!(q.step, Step::Release(_) | Step::Modifiers(_) | Step::Key(_))
                        })
                        .collect();
                    continue;
                }
            }
            break;
        }
        events
    }
}

/// Finds the canvas, its pointer state, and the first text widget that
/// reads `text`, in the live view.
struct FindNow<'a> {
    text: Option<&'a str>,
    found: Found,
    pointer: Option<CanvasPointer>,
}

impl Operation for FindNow<'_> {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn custom(&mut self, _id: Option<&Id>, _bounds: Rectangle, state: &mut dyn Any) {
        if let Some(view) = state.downcast_ref::<CanvasView>() {
            self.found.canvas = Some(*view);
        }
        if let Some(pointer) = state.downcast_ref::<CanvasPointer>() {
            self.pointer = Some(pointer.clone());
        }
    }

    fn text(&mut self, _id: Option<&Id>, bounds: Rectangle, text: &str) {
        if self.found.text.is_none() && Some(text) == self.text {
            self.found.text = Some(bounds);
        }
    }
}

/// Wraps the window view and sends it the script's pointer events.
pub struct Injector<'a, Message> {
    content: Element<'a, Message>,
    pointer: Pointer,
    app: &'a App,
    /// The messages a key event no widget took becomes, as the app's key
    /// subscriptions would send them.
    on_key: KeyHandler<'a, Message>,
}

/// Turns a key event, and whether a widget took it, into app messages.
type KeyHandler<'a, Message> = Box<dyn Fn(keyboard::Event, iced::event::Status) -> Vec<Message> + 'a>;

impl<'a, Message> Injector<'a, Message> {
    /// Wraps `content`, the whole window view of `app`.
    pub(crate) fn new(
        content: impl Into<Element<'a, Message>>,
        app: &'a App,
        on_key: impl Fn(keyboard::Event, iced::event::Status) -> Vec<Message> + 'a,
    ) -> Self {
        Self {
            content: content.into(),
            pointer: app.pointer.clone(),
            app,
            on_key: Box::new(on_key),
        }
    }

    /// Searches the live view for `text` and the canvas.
    fn find(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        text: Option<&str>,
    ) -> (Found, Option<CanvasPointer>) {
        let mut op = FindNow {
            text,
            found: Found::default(),
            pointer: None,
        };
        self.content.as_widget_mut().operate(tree, layout, renderer, &mut op);
        (op.found, op.pointer)
    }

    /// Where a target is in the window now: the centre of what `locate`
    /// finds.
    fn resolve(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        target: &str,
    ) -> Result<Point, String> {
        let (found, _) = self.find(tree, layout, renderer, Some(target));
        let rect = crate::control::locate(self.app, target, &found)?;
        let window = layout.bounds();
        if !window.contains(rect.center()) {
            return Err(format!("`{target}` is not on screen"));
        }
        Ok(rect.center())
    }
}

impl<Message> Widget<Message, iced::Theme, iced::Renderer> for Injector<'_, Message> {
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &iced::Renderer, limits: &layout::Limits) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let cursor = self.pointer.0.borrow().at.map_or(cursor, mouse::Cursor::Available);
        self.content
            .as_widget()
            .draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::stateless()
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let child = &mut tree.children[0];
        if let Event::Mouse(mouse_event) = event {
            let held = {
                let mut inner = self.pointer.0.borrow_mut();
                // The script leads while its command runs.
                if !inner.queue.is_empty() {
                    return;
                }
                match mouse_event {
                    mouse::Event::ButtonPressed(b) => inner.real_buttons.push(*b),
                    mouse::Event::ButtonReleased(b) => inner.real_buttons.retain(|x| x != b),
                    _ => {}
                }
                // A real pointer event takes over from the script, which
                // first lets go of what it holds.
                inner.at.take().map(|at| (at, std::mem::take(&mut inner.buttons)))
            };
            if let Some((at, buttons)) = held {
                for button in buttons {
                    let release = Event::Mouse(mouse::Event::ButtonReleased(button));
                    self.content.as_widget_mut().update(
                        child,
                        &release,
                        layout,
                        mouse::Cursor::Available(at),
                        renderer,
                        clipboard,
                        shell,
                        viewport,
                    );
                }
            }
            self.content
                .as_widget_mut()
                .update(child, event, layout, cursor, renderer, clipboard, shell, viewport);
            return;
        }
        let redraw = match event {
            Event::Window(window::Event::RedrawRequested(now)) => Some(*now),
            _ => None,
        };
        if let Some(now) = redraw
            && self.pointer.waiting()
        {
            let pointer = self.pointer.clone();
            let events = pointer.next_events(now, |target| {
                self.resolve(&mut tree.children[0], layout, renderer, target)
            });
            for (injected, at) in events {
                // A shell of its own, to tell whether a widget took it.
                let mut messages = Vec::new();
                let mut own = Shell::new(&mut messages);
                self.content.as_widget_mut().update(
                    &mut tree.children[0],
                    &injected,
                    layout,
                    at.map_or(cursor, mouse::Cursor::Available),
                    renderer,
                    clipboard,
                    &mut own,
                    viewport,
                );
                let status = own.event_status();
                shell.merge(own, |m| m);
                if let Event::Keyboard(
                    key @ (keyboard::Event::KeyPressed { .. } | keyboard::Event::KeyReleased { .. }),
                ) = injected
                {
                    for message in (self.on_key)(key, status) {
                        shell.publish(message);
                    }
                }
            }
            // Draw this event, then send the next one.
            shell.request_redraw();
        }
        let cursor = self.pointer.0.borrow().at.map_or(cursor, mouse::Cursor::Available);
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        if redraw.is_some() && self.pointer.0.borrow().record {
            let (_, canvas) = self.find(&mut tree.children[0], layout, renderer, None);
            self.pointer.0.borrow_mut().seen = Some(Seen {
                cursor: cursor.position(),
                canvas,
            });
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let cursor = self.pointer.0.borrow().at.map_or(cursor, mouse::Cursor::Available);
        self.content
            .as_widget()
            .mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        self.content
            .as_widget_mut()
            .overlay(&mut tree.children[0], layout, renderer, viewport, translation)
    }
}

impl<'a, Message: 'a> From<Injector<'a, Message>> for Element<'a, Message> {
    fn from(injector: Injector<'a, Message>) -> Self {
        Element::new(injector)
    }
}

/// The pointer in the snapshot: where it is and what it is on, so a script
/// can check each step of a pointer command.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PointerState {
    /// Logical window pixels, or `None` when the pointer is not in the
    /// window.
    pub x: Option<f32>,
    pub y: Option<f32>,
    /// Buttons held down: `left`, `right`, `middle`.
    pub buttons: Vec<&'static str>,
    /// What is under the pointer on the canvas.
    pub hover: Option<HoverState>,
    /// The output whose badge is pinned, as `MODULE:OUTPUT`.
    pub pinned: Option<String>,
    /// The drag in progress on the canvas.
    pub drag: Option<DragState>,
}

/// What is under the pointer, by name.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HoverState {
    Output {
        target: String,
    },
    Input {
        target: String,
    },
    /// An output's assignment count badge.
    Badge {
        target: String,
    },
    /// A control, by its parameter.
    Control {
        target: String,
    },
    /// A light or a meter, by its display value.
    Display {
        target: String,
    },
    /// A tile, away from its ports and widgets: the module ID.
    Tile {
        target: String,
    },
    Cable {
        from: String,
        to: String,
    },
}

/// A drag in progress, by name.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DragState {
    Pan,
    Move {
        modules: Vec<u32>,
    },
    Knob {
        target: String,
    },
    Button {
        target: String,
    },
    /// A cable pulled from a port. `pulled` is true when it came off an
    /// input.
    Wire {
        from: String,
        pulled: bool,
    },
    /// A knob's range arc.
    Depth {
        target: String,
    },
    Select,
}

/// A mouse button by name.
pub fn button(name: Option<&str>) -> Result<mouse::Button, String> {
    match name.unwrap_or("left") {
        "left" => Ok(mouse::Button::Left),
        "right" => Ok(mouse::Button::Right),
        "middle" => Ok(mouse::Button::Middle),
        other => Err(format!("unknown mouse button `{other}`; buttons: left, right, middle")),
    }
}

fn button_name(button: mouse::Button) -> Option<&'static str> {
    match button {
        mouse::Button::Left => Some("left"),
        mouse::Button::Right => Some("right"),
        mouse::Button::Middle => Some("middle"),
        _ => None,
    }
}

impl App {
    /// The pointer for the snapshot, or `None` when nothing has been drawn
    /// with the control channel on.
    pub(crate) fn pointer_state(&self) -> Option<PointerState> {
        let inner = self.pointer.0.borrow();
        if !inner.record {
            return None;
        }
        let seen = inner.seen.clone().unwrap_or_default();
        let graph = self.controller.graph();
        let part = |part| crate::targets::name(graph, part);
        let port = |p: PortRef| part(crate::targets::Part::Port(p));
        let widget = |module: u32, widget: usize| {
            let control = graph
                .node(loom_core::ModuleId(module))
                .and_then(|n| n.descriptor.panel.widgets.get(widget))
                .is_some_and(|w| w.is_control());
            if control {
                part(crate::targets::Part::Control(module, widget))
            } else {
                part(crate::targets::Part::Display(module, widget))
            }
        };
        let canvas = seen.canvas.unwrap_or(CanvasPointer {
            hover: None,
            pinned: None,
            drag: None,
        });
        let hover = canvas.hover.and_then(|hit| match hit {
            Hit::Port(p) => port(p).map(|target| {
                if p.output {
                    HoverState::Output { target }
                } else {
                    HoverState::Input { target }
                }
            }),
            Hit::Badge(p) => part(crate::targets::Part::Badge(p)).map(|target| HoverState::Badge { target }),
            Hit::Widget { module, widget: w } => {
                let target = widget(module, w)?;
                let control = graph
                    .node(loom_core::ModuleId(module))
                    .and_then(|n| n.descriptor.panel.widgets.get(w))
                    .is_some_and(|w| w.is_control());
                Some(if control {
                    HoverState::Control { target }
                } else {
                    HoverState::Display { target }
                })
            }
            Hit::Module(module) => Some(HoverState::Tile {
                target: module.to_string(),
            }),
            Hit::Cable(index) => {
                let cable = self.scene.cables().get(index)?;
                Some(HoverState::Cable {
                    from: port(PortRef {
                        module: cable.from.0,
                        output: true,
                        index: cable.from.1,
                    })?,
                    to: port(PortRef {
                        module: cable.to.0,
                        output: false,
                        index: cable.to.1,
                    })?,
                })
            }
            Hit::Nothing => None,
        });
        let drag = canvas.drag.and_then(|drag| {
            Some(match drag {
                PointerDrag::Pan => DragState::Pan,
                PointerDrag::Move(modules) => DragState::Move { modules },
                PointerDrag::Knob(m, w) => DragState::Knob { target: widget(m, w)? },
                PointerDrag::Button(m, w) => DragState::Button { target: widget(m, w)? },
                PointerDrag::Wire { from, pulled } => DragState::Wire {
                    from: port(from)?,
                    pulled,
                },
                PointerDrag::Depth(m, w) => DragState::Depth { target: widget(m, w)? },
                PointerDrag::Select => DragState::Select,
            })
        });
        let cursor = inner.at.or(seen.cursor);
        let mut buttons: Vec<&'static str> = inner
            .buttons
            .iter()
            .chain(&inner.real_buttons)
            .filter_map(|b| button_name(*b))
            .collect();
        buttons.dedup();
        Some(PointerState {
            x: cursor.map(|p| p.x),
            y: cursor.map(|p| p.y),
            buttons,
            hover,
            pinned: canvas.pinned.and_then(port),
            drag,
        })
    }
}
