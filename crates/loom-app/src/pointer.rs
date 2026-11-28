//! The control channel's pointer: `hover`, `press`, `release`, `drag`, and
//! `scroll` (`docs/design/control-channel.md`, Pointer commands), and its
//! `key` and `type`.
//!
//! A command becomes steps in a queue. After each frame the window's layers
//! (`layers.rs`), or the `Ui` harness, take the next step as one input
//! event with the script's pointer ([`Pointer::next_input`]), and the
//! driver gives it to the runtime as it gives a real event, so it goes
//! through the same routing: the runtime's layers, its fields and keys, and
//! the canvas (rule 3; ui-layer.md, section 10, rule 6). The window is
//! drawn before the next step goes in.
//!
//! A target (`"4:cutoff"`, `"Cutoff"`) is found when its step goes in, from
//! the runtime's layout and the canvas's camera, as `locate` finds it.
//!
//! A real pointer takes over ([`Pointer::gate`]): while a command runs, the
//! window holds real pointer events back, and the next real one after it
//! first lets go of what the script holds.
//!
//! While the control channel is on, the pointer also keeps what the canvas
//! says it is on, once per drawn frame, for the snapshot's `pointer` field.

use crate::control::Found;
use crate::{App, Message};
use loom_canvas::{CanvasPointer, CanvasTyped, Hit, PointerDrag, PortRef};
use loom_ui::shell::{Gate, ScriptedInput};
use loom_ui_core::{Event, KeyEvent, Point, PointerButton, PointerEvent, ScrollDelta};
use serde::Serialize;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

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
    Press(PointerButton),
    /// A mouse button goes up where the pointer is.
    Release(PointerButton),
    /// A wheel step where the pointer is, in lines.
    Wheel { dx: f32, dy: f32 },
    /// The modifier keys held from now on. Sent with the next event, not
    /// on a frame of its own.
    Modifiers(loom_ui_core::Modifiers),
    /// A key press or release, at the pointer, routed as a real key is.
    Key(KeyEvent),
}

/// A step and the command it belongs to.
#[derive(Debug, Clone)]
struct Queued {
    serial: u64,
    step: Step,
}

/// What the window last drew of the pointer.
#[derive(Debug, Clone, Default)]
struct Seen {
    /// The pointer in window coordinates, if it is in the window.
    cursor: Option<Point>,
    canvas: Option<CanvasPointer>,
    /// The typed field the canvas had open, if any.
    typed: Option<CanvasTyped>,
}

#[derive(Debug, Default)]
struct Inner {
    /// Keep the canvas's pointer state for the snapshot. True while the
    /// control channel is on, so an app without it does no extra work.
    record: bool,
    queue: VecDeque<Queued>,
    /// The script's pointer, in window coordinates, while it leads. A real
    /// pointer event in the window takes over.
    at: Option<Point>,
    /// Buttons the script holds down.
    buttons: Vec<PointerButton>,
    /// Buttons a real pointer holds down.
    real_buttons: Vec<PointerButton>,
    /// Real button releases that came while a command's events went in.
    /// They go to the runtime once the command is done, so nothing the
    /// real pointer let go of stays held.
    deferred: Vec<PointerButton>,
    /// A real pointer was lost while a command's events went in. The loss
    /// goes to the runtime once the command is done, so a press it heard
    /// before does not stay held: a cancelled touch sends no release.
    lost_deferred: bool,
    /// Where the current drag started.
    anchor: Option<Point>,
    /// Where the current drag's `to` was last found.
    toward: Option<Point>,
    /// The first error of each command, by serial.
    errors: Vec<(u64, String)>,
    next_serial: u64,
    seen: Option<Seen>,
}

/// The script's pointer and its queue of events, shared by the app and
/// what drives the window.
#[derive(Debug, Clone, Default)]
pub struct Pointer(Rc<RefCell<Inner>>);

impl Pointer {
    /// A pointer that keeps the canvas's pointer state for the snapshot
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
    /// else where the window last saw the real one.
    pub fn position(&self) -> Option<Point> {
        let inner = self.0.borrow();
        inner.at.or_else(|| inner.seen.as_ref().and_then(|s| s.cursor))
    }

    /// True when a command is waiting for its events to go in.
    pub fn waiting(&self) -> bool {
        !self.0.borrow().queue.is_empty()
    }

    /// What the window does with a real pointer `event` (control-channel.md,
    /// "A real pointer takes over"): while a command runs it is held back,
    /// a release or a loss kept for later so nothing stays held; after it,
    /// the first real event lets go of what the script holds, at the
    /// script's pointer, before it goes in.
    pub(crate) fn gate(&self, event: &Event) -> Gate {
        let mut inner = self.0.borrow_mut();
        let Event::Pointer(pointer) = event else {
            return Gate::Pass;
        };
        match pointer {
            PointerEvent::Pressed(button) => inner.real_buttons.push(*button),
            PointerEvent::Released(button) => inner.real_buttons.retain(|held| held != button),
            // A lost pointer holds no button, whether or not a script runs.
            // A release kept for later still goes in after the script, so
            // the runtime lets go of a press it heard before.
            PointerEvent::Lost => inner.real_buttons.clear(),
            _ => {}
        }
        if !inner.queue.is_empty() {
            match pointer {
                PointerEvent::Released(button) => inner.deferred.push(*button),
                // The loss lets go of every button, so no release need
                // follow it.
                PointerEvent::Lost => {
                    inner.deferred.clear();
                    inner.lost_deferred = true;
                }
                _ => {}
            }
            return Gate::Hold;
        }
        let first = take_over(&mut inner);
        if first.is_empty() {
            Gate::Pass
        } else {
            Gate::After(first)
        }
    }

    /// Keeps what the canvas says the pointer is on, and where the real
    /// pointer is, once per drawn frame, for the snapshot, while the
    /// control channel is on.
    pub(crate) fn record_seen(&self, cursor: Option<Point>, canvas: Option<CanvasPointer>, typed: Option<CanvasTyped>) {
        let mut inner = self.0.borrow_mut();
        if inner.record {
            inner.seen = Some(Seen { cursor, canvas, typed });
        }
    }

    /// True while the snapshot keeps the canvas's pointer state.
    pub(crate) fn records(&self) -> bool {
        self.0.borrow().record
    }

    /// The input for the next step, after the last one's frame: a modifier
    /// change with the event after it, or one event, each with the script's
    /// pointer. Real releases or a loss held back while a command ran go in
    /// once it is done. `locate` finds a target (`resolve`). Empty when
    /// nothing waits.
    pub(crate) fn next_input(&self, mut resolve: impl FnMut(&str) -> Result<Point, String>) -> Vec<ScriptedInput> {
        let mut inner = self.0.borrow_mut();
        if inner.queue.is_empty() {
            // Real releases, or a loss, held back while the command ran go
            // in now, after the script lets go of what it holds.
            if inner.deferred.is_empty() && !inner.lost_deferred {
                return Vec::new();
            }
            let mut input = take_over(&mut inner);
            for button in std::mem::take(&mut inner.deferred) {
                input.push(Event::Pointer(PointerEvent::Released(button)).into());
            }
            let lost = Event::Pointer(PointerEvent::Lost);
            if std::mem::take(&mut inner.lost_deferred) && !input.iter().any(|step| step.event == lost) {
                input.push(lost.into());
            }
            return input;
        }
        let mut input = Vec::new();
        while let Some(Queued { serial, step }) = inner.queue.pop_front() {
            // A target is found with the pointer let go of: finding one can
            // build the accessibility tree, whose snapshot reads the pointer.
            let aim = match &step {
                Step::MoveTo(aim) | Step::Toward { to: aim, .. } => Some(aim.clone()),
                _ => None,
            };
            let found = match aim {
                Some(Aim::At(p)) => Some(Ok(p)),
                Some(Aim::Target(target)) => {
                    drop(inner);
                    let found = resolve(&target);
                    inner = self.0.borrow_mut();
                    Some(found)
                }
                None => None,
            };
            let at = inner.at;
            let find = |_: &Aim| found.clone().expect("a moving step has a target");
            let event = match step {
                Step::Modifiers(modifiers) => {
                    // A change of keys needs no frame of its own.
                    input.push(ScriptedInput {
                        event: Event::Key(KeyEvent::ModifiersChanged(modifiers)),
                        at,
                    });
                    continue;
                }
                Step::Key(key) => {
                    input.push(ScriptedInput {
                        event: Event::Key(key),
                        at,
                    });
                    break;
                }
                Step::MoveTo(aim) => find(&aim).map(|p| {
                    inner.anchor = Some(p);
                    inner.toward = None;
                    (PointerEvent::Moved, p)
                }),
                Step::Toward { to, t } => {
                    let found = find(&to);
                    if let Ok(p) = found {
                        inner.toward = Some(p);
                    }
                    let last = t >= 1.0;
                    match (found, inner.anchor, inner.toward) {
                        // The last move must reach `to` (control-channel.md).
                        (Err(e), _, _) if last => Err(e),
                        (_, Some(a), Some(b)) => Ok((
                            PointerEvent::Moved,
                            Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t),
                        )),
                        // Not found yet: stay put until it appears.
                        _ => match at {
                            Some(p) => Ok((PointerEvent::Moved, p)),
                            None => Err("the pointer is not in the window yet".to_string()),
                        },
                    }
                }
                Step::Press(button) | Step::Release(button) => match at {
                    Some(p) => {
                        if matches!(step, Step::Press(_)) {
                            inner.buttons.push(button);
                            Ok((PointerEvent::Pressed(button), p))
                        } else {
                            inner.buttons.retain(|b| *b != button);
                            Ok((PointerEvent::Released(button), p))
                        }
                    }
                    None => Err("the pointer is not in the window yet; give a `target` or `x` and `y`".to_string()),
                },
                Step::Wheel { dx, dy } => match at {
                    Some(p) => Ok((PointerEvent::Scrolled(ScrollDelta::Lines { x: dx, y: dy }), p)),
                    None => Err("the pointer is not in the window yet; give a `target` or `x` and `y`".to_string()),
                },
            };
            match event {
                Ok((event, p)) => {
                    inner.at = Some(p);
                    input.push(ScriptedInput {
                        event: Event::Pointer(event),
                        at: Some(p),
                    });
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
        input
    }
}

/// The script lets go of what it holds, at its pointer, and leads no more:
/// what to give the runtime first. A held button is cancelled, never
/// released, so it finishes no click and no drag (control-channel.md, "A
/// real pointer takes over").
fn take_over(inner: &mut Inner) -> Vec<ScriptedInput> {
    let Some(at) = inner.at.take() else {
        return Vec::new();
    };
    if std::mem::take(&mut inner.buttons).is_empty() {
        return Vec::new();
    }
    vec![ScriptedInput {
        event: Event::Pointer(PointerEvent::Lost),
        at: Some(at),
    }]
}

impl App {
    /// The next scripted input after a frame, with its targets found in the
    /// layout `panels` hold and the canvas's camera, as `locate` finds
    /// them; and a press outside the cable jump list closes it first, as a
    /// real press does (`layers.rs`). Keeps the canvas's pointer state for
    /// the snapshot too.
    pub(crate) fn next_scripted(&self, panels: &loom_ui::Panels<'_, App>) -> Vec<ScriptedInput> {
        let view = crate::canvas::panel_view(panels);
        // The canvas's typed field too, so a step can aim at it, as
        // `locate` can.
        let (canvas, typed) = crate::canvas::panel_pointer(self, panels);
        if self.pointer.records() {
            self.pointer.record_seen(panels.pointer(), canvas, typed.clone());
        }
        self.pointer.next_input(|target| {
            let mut found = Found {
                canvas: view,
                places: crate::places::Places::live(view, typed.clone()),
                ..Found::default()
            };
            found.find_ours_in(target, panels);
            let rect = crate::control::locate(self, target, &found)?;
            let window = loom_ui_core::Rect::new(Point::ORIGIN, panels.size());
            if !window.contains(rect.center()) {
                return Err(format!("`{target}` is not on screen"));
            }
            Ok(rect.center())
        })
    }

    /// What a press at `at`, real or scripted, does before it goes in, as
    /// messages: one outside the open cable jump list closes it, and the
    /// press goes on (control-channel.md, Pointer commands); one while a
    /// value draft waits with no field focused ends the draft, as a press
    /// elsewhere did when the window answered where focus was.
    pub(crate) fn press_messages(
        &self,
        panels: &loom_ui::Panels<'_, App>,
        event: &Event,
        at: Option<Point>,
    ) -> Vec<Message> {
        if !matches!(event, Event::Pointer(PointerEvent::Pressed(_))) {
            return Vec::new();
        }
        let mut messages = Vec::new();
        if self.session.outline.has_jump() {
            let inside = at.is_some_and(|at| {
                panels
                    .bounds(&crate::outline::jump_id())
                    .is_some_and(|rect| rect.contains(at))
            });
            if !inside {
                messages.push(Message::DismissJump);
            }
        }
        let on_canvas = at.is_some_and(|at| {
            panels
                .bounds(&crate::canvas::canvas_id())
                .is_some_and(|rect| rect.contains(at))
        });
        if !on_canvas {
            messages.push(Message::PressedElsewhere);
        }
        if self.session.value_draft.is_some() && panels.focused_field().is_none() {
            messages.push(Message::FocusMoved(None));
        }
        messages
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

/// The canvas's typed field in the snapshot, so a script can check it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TypedFieldState {
    /// The control the field is over, as a name `locate` takes.
    pub target: String,
    /// The text in the field.
    pub text: String,
    /// Why the text is not a value yet, as the field shows it.
    pub reason: Option<String>,
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
    /// A growable tile's resize handle.
    Resize {
        module: u32,
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
pub fn button(name: Option<&str>) -> Result<PointerButton, String> {
    match name.unwrap_or("left") {
        "left" => Ok(PointerButton::Left),
        "right" => Ok(PointerButton::Right),
        "middle" => Ok(PointerButton::Middle),
        other => Err(format!("unknown mouse button `{other}`; buttons: left, right, middle")),
    }
}

/// A button's name in the snapshot.
fn button_name(button: PointerButton) -> Option<&'static str> {
    match button {
        PointerButton::Left => Some("left"),
        PointerButton::Right => Some("right"),
        PointerButton::Middle => Some("middle"),
        _ => None,
    }
}

impl App {
    /// The canvas typed field for the snapshot, as it was last drawn. The reason
    /// comes from the same check the canvas uses, so the two cannot disagree.
    pub(crate) fn typed_field_state(&self) -> Option<TypedFieldState> {
        let inner = self.pointer.0.borrow();
        if !inner.record {
            return None;
        }
        let typed = inner.seen.as_ref()?.typed.as_ref()?;
        let target = crate::targets::name(
            self.session.controller.graph(),
            &self.session.hierarchy,
            crate::targets::Part::Control(typed.module, typed.widget),
        )?;
        Some(TypedFieldState {
            target,
            text: typed.text.clone(),
            reason: self.typed_check(typed.module, typed.widget, &typed.text).err(),
        })
    }

    /// The pointer for the snapshot, or `None` when nothing has been drawn
    /// with the control channel on.
    pub(crate) fn pointer_state(&self) -> Option<PointerState> {
        let inner = self.pointer.0.borrow();
        if !inner.record {
            return None;
        }
        let seen = inner.seen.clone().unwrap_or_default();
        let graph = self.session.controller.graph();
        let hierarchy = &self.session.hierarchy;
        let part = |part| crate::targets::name(graph, hierarchy, part);
        let port = |p: PortRef| part(crate::targets::Part::Port(p));
        let widget = |module: u32, widget: usize| {
            if crate::surface::is_control(graph, hierarchy, module, widget) {
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
                Some(if crate::surface::is_control(graph, hierarchy, module, w) {
                    HoverState::Control { target }
                } else {
                    HoverState::Display { target }
                })
            }
            Hit::Module(module) => Some(HoverState::Tile {
                target: module.to_string(),
            }),
            Hit::Cable(index) => {
                let cable = self.session.scene.cables().get(index)?;
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
                PointerDrag::Resize(module) => DragState::Resize { module },
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
