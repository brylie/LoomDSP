//! Tests for a custom element's ticks and frames: its deadline, its
//! animation frames, and a frame it asks for with no message (ui-layer.md,
//! section 9, rule 4).

use crate::app::{App, Context, InputState, WindowSpec};
use crate::custom::{Custom, CustomEvent, CustomKind, Moment, Out, Used};
use crate::effect::Effects;
use crate::element::{Element, Length};
use crate::event::{DriverEvent, WindowState};
use crate::headless::{Harness, Headless};
use crate::layout::tests::Mono;
use crate::routing::NoTarget;
use crate::runtime::Runtime;
use crate::view::{Layer, View};
use loom_ui_core::{Event, Point, PointerButton, PointerEvent, Size, UiId};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// The meter's kind.
const METER: CustomKind = CustomKind::new("meter");

/// How long after a left press the meter's deadline comes.
const WAIT: Duration = Duration::from_millis(100);

/// What the test app hears.
#[derive(Debug, Clone, PartialEq)]
enum Message {
    /// The meter's deadline came.
    Ticked,
    /// Open or close the overlay over the meter.
    Overlay(bool),
}

/// The meter's state: what it heard, its deadline, and how many frames it
/// still animates for.
struct MeterState {
    heard: Vec<CustomEvent>,
    deadline: Option<Instant>,
    frames: u32,
}

impl Custom<Meters> for MeterState {
    fn event(&mut self, app: &Meters, event: &CustomEvent, moment: &Moment, out: &mut Out<'_, Message>) -> Used {
        self.heard.push(event.clone());
        match event {
            // A move over its right half lights it: a frame, no message.
            CustomEvent::Pointer(PointerEvent::Moved) => {
                if moment.pointer.is_some_and(|at| at.x > 100.0) {
                    out.redraw();
                }
            }
            // A left press sets a deadline, a right press starts frames,
            // each with no message and no update.
            CustomEvent::Pointer(PointerEvent::Pressed(PointerButton::Left)) => {
                self.deadline = Some(moment.now + WAIT);
            }
            CustomEvent::Pointer(PointerEvent::Pressed(PointerButton::Right)) => self.frames = 2,
            CustomEvent::Deadline => {
                if !app.keep_deadline {
                    self.deadline = None;
                }
                out.send(Message::Ticked);
            }
            CustomEvent::Frame => {
                self.frames = self.frames.saturating_sub(1);
                out.redraw();
                if app.send_from_frames {
                    out.send(Message::Ticked);
                }
            }
            _ => {}
        }
        Used::Yes
    }

    fn next_deadline(&self, _app: &Meters) -> Option<Instant> {
        self.deadline
    }

    fn animating(&self, _app: &Meters) -> bool {
        self.frames > 0
    }
}

/// An app with the meter in one panel, and an overlay over it when open.
struct Meters {
    overlay: bool,
    /// How many frames a new meter animates for from its first layout.
    start_frames: u32,
    /// True when the meter sends a message from its frame's tick, which a
    /// debug build refuses.
    send_from_frames: bool,
    /// True when the meter's tick forgets to move its deadline on, which a
    /// debug build refuses.
    keep_deadline: bool,
    /// Every message, in order.
    log: Vec<Message>,
}

impl App for Meters {
    type Message = Message;

    fn update(&mut self, message: Message, _cx: &Context<'_, Message>) -> Effects<Message> {
        if let Message::Overlay(open) = message {
            self.overlay = open;
        }
        self.log.push(message);
        Effects::none()
    }

    fn view(&self, view: &mut View) {
        view.panel_with(UiId::panel("main"), Layer::Panels, &(), |_| {
            Element::custom(METER)
                .key("meter")
                .width(Length::Fixed(200.0))
                .height(Length::Fixed(100.0))
        });
        if self.overlay {
            view.panel_with(UiId::panel("dialog"), Layer::Overlays, &(), |_| {
                Element::space(Length::Fixed(50.0), Length::Fixed(50.0)).key("box")
            });
        }
    }

    fn window(&self) -> WindowSpec {
        WindowSpec::default()
    }

    fn input(&self) -> InputState {
        InputState {
            layer: if self.overlay { Layer::Overlays } else { Layer::Canvas },
            ..InputState::default()
        }
    }

    fn make_custom(&self, _id: &UiId, kind: CustomKind) -> Option<Box<dyn Custom<Self>>> {
        (kind == METER).then(|| {
            Box::new(MeterState {
                heard: Vec::new(),
                deadline: None,
                frames: self.start_frames,
            }) as Box<dyn Custom<Self>>
        })
    }
}

/// A new app whose meter animates for `start_frames` frames.
fn meters(start_frames: u32) -> Meters {
    Meters {
        overlay: false,
        start_frames,
        send_from_frames: false,
        keep_deadline: false,
        log: Vec::new(),
    }
}

/// A harness with the meter at (0, 0), 200 by 100, in a 400 by 200 window,
/// after the turn that gives the schedule of the state the layout made.
fn harness(app: Meters) -> Harness<Meters, NoTarget> {
    let window = WindowState::new(Size::new(400.0, 200.0));
    let mut harness = Harness::new(app, NoTarget, window, Instant::now());
    harness.lay_out(&mut Mono::default());
    harness.step(DriverEvent::Woken);
    harness
}

/// A pointer event at `(x, y)`, then a layout.
fn step(harness: &mut Harness<Meters, NoTarget>, x: f32, y: f32, event: PointerEvent) {
    harness.step(DriverEvent::Input {
        event: Event::Pointer(event),
        pointer: Some(Point::new(x, y)),
    });
    harness.lay_out(&mut Mono::default());
}

/// The meter's UI ID.
fn meter() -> UiId {
    UiId::panel("main").with("meter")
}

/// What the meter heard, ticks only.
fn ticks(harness: &Harness<Meters, NoTarget>) -> Vec<CustomEvent> {
    let state = harness
        .runtime()
        .custom::<MeterState>(&meter())
        .expect("the meter's state");
    state
        .heard
        .iter()
        .filter(|event| matches!(event, CustomEvent::Deadline | CustomEvent::Frame))
        .cloned()
        .collect()
}

/// A frame an element asks for with no message draws once, with no
/// update, and the next turn draws nothing: the request is used up.
#[test]
fn a_redraw_with_no_message_draws_once() {
    let mut harness = harness(meters(0));
    step(&mut harness, 50.0, 50.0, PointerEvent::Moved);
    let updates = harness.runtime().counts().updates;
    let redraws = harness.driver().redraws();
    // Over its left half it asks for nothing, so nothing draws.
    step(&mut harness, 60.0, 50.0, PointerEvent::Moved);
    assert_eq!(harness.driver().redraws(), redraws);
    step(&mut harness, 150.0, 50.0, PointerEvent::Moved);
    assert_eq!(harness.driver().redraws(), redraws + 1);
    assert_eq!(harness.runtime().counts().updates, updates);
    let next = harness.step(DriverEvent::Woken);
    assert!(!next.draw);
    assert_eq!(harness.driver().redraws(), redraws + 1);
}

/// An element that animates gets frames, and hears a tick at each; each
/// frame draws once and runs no update, and the frames stop when it does.
#[test]
fn a_frame_tick_draws_once() {
    let mut harness = harness(meters(0));
    assert!(!harness.next().frames);
    step(&mut harness, 50.0, 50.0, PointerEvent::Pressed(PointerButton::Right));
    assert!(harness.next().frames, "a state change with no update asks for frames");
    let updates = harness.runtime().counts().updates;
    let redraws = harness.driver().redraws();
    let next = harness.step(DriverEvent::Frame);
    assert!(next.draw && next.frames);
    assert_eq!(harness.driver().redraws(), redraws + 1);
    let next = harness.step(DriverEvent::Frame);
    assert!(next.draw && !next.frames, "the last frame ends the frames");
    assert_eq!(harness.driver().redraws(), redraws + 2);
    assert_eq!(harness.runtime().counts().updates, updates);
    assert_eq!(ticks(&harness), [CustomEvent::Frame, CustomEvent::Frame]);
    let next = harness.step(DriverEvent::Woken);
    assert!(!next.draw);
    assert_eq!(harness.driver().redraws(), redraws + 2);
}

/// An element's deadline wakes the loop, and its tick's messages run in
/// the turn that woke, with the driver's moment.
#[test]
fn a_deadline_tick_sends_in_its_turn() {
    let mut harness = harness(meters(0));
    let start = harness.now();
    step(&mut harness, 50.0, 50.0, PointerEvent::Pressed(PointerButton::Left));
    assert_eq!(harness.next().deadline, Some(start + WAIT));
    harness.advance(WAIT / 2);
    assert!(ticks(&harness).is_empty());
    harness.advance(WAIT);
    assert_eq!(ticks(&harness), [CustomEvent::Deadline]);
    assert_eq!(harness.app().log, [Message::Ticked]);
    assert_eq!(harness.next().deadline, None, "the tick moved its deadline on");
}

/// A turn for one message alone leaves a due deadline for the next turn,
/// as it leaves the app's.
#[test]
fn a_delivered_message_leaves_a_due_deadline() {
    let mut harness = harness(meters(0));
    let start = harness.now();
    step(&mut harness, 50.0, 50.0, PointerEvent::Pressed(PointerButton::Left));
    let later = start + WAIT * 2;
    let mut driver = Headless::new();
    let runtime = harness.runtime_mut();
    runtime.deliver(Message::Overlay(false), later, &mut driver);
    assert!(ticks(&harness).is_empty());
    harness.runtime_mut().handle(DriverEvent::Woken, later, &mut driver);
    assert_eq!(ticks(&harness), [CustomEvent::Deadline]);
}

/// A tick is not input: an element under an open overlay still ticks.
#[test]
fn an_element_under_an_overlay_still_ticks() {
    let mut harness = harness(meters(3));
    harness.send(Message::Overlay(true));
    harness.lay_out(&mut Mono::default());
    assert!(harness.next().frames);
    harness.step(DriverEvent::Frame);
    assert_eq!(ticks(&harness), [CustomEvent::Frame]);
}

/// A state a layout makes, outside a turn, that animates from the start
/// wakes the loop once, so a turn asks for its frames. A layout that
/// changes nothing wakes nothing.
#[test]
fn a_state_made_at_layout_wakes_the_loop() {
    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&wakes);
    let window = WindowState::new(Size::new(400.0, 200.0));
    let mut runtime = Runtime::new(
        meters(3),
        NoTarget,
        window,
        Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }),
    );
    let mut driver = Headless::new();
    let now = Instant::now();
    let next = runtime.handle(DriverEvent::Woken, now, &mut driver);
    assert!(!next.frames, "no state exists before the first layout");
    runtime.lay_out(&mut Mono::default());
    assert_eq!(wakes.load(Ordering::SeqCst), 1);
    let next = runtime.handle(DriverEvent::Woken, now, &mut driver);
    assert!(next.frames);
    runtime.lay_out(&mut Mono::default());
    assert_eq!(wakes.load(Ordering::SeqCst), 1);
}

/// A frame's tick sends no message, since a frame runs no update: a debug
/// build panics, and names what to do instead.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "a custom element sent a message from a frame's tick: ask for a deadline instead")]
fn a_message_from_a_frame_tick_panics() {
    let mut app = meters(3);
    app.send_from_frames = true;
    let mut harness = harness(app);
    harness.step(DriverEvent::Frame);
}

/// A tick that leaves its deadline due would wake the loop at once, every
/// turn: a debug build panics, and names what to do instead.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "a custom element's deadline did not move past its tick: clear it or move it on in event")]
fn a_deadline_that_does_not_move_panics() {
    let mut app = meters(0);
    app.keep_deadline = true;
    let mut harness = harness(app);
    step(&mut harness, 50.0, 50.0, PointerEvent::Pressed(PointerButton::Left));
    harness.advance(WAIT);
}

/// In a release build, a deadline that did not move gets one tick, and the
/// schedule leaves it out, so the loop sleeps; a turn some other cause
/// brings ticks it again.
#[cfg(not(debug_assertions))]
#[test]
fn a_deadline_that_does_not_move_does_not_spin() {
    let mut app = meters(0);
    app.keep_deadline = true;
    let mut harness = harness(app);
    step(&mut harness, 50.0, 50.0, PointerEvent::Pressed(PointerButton::Left));
    harness.advance(WAIT);
    assert_eq!(ticks(&harness), [CustomEvent::Deadline]);
    assert_eq!(harness.next().deadline, None);
    harness.send(Message::Overlay(false));
    assert_eq!(ticks(&harness), [CustomEvent::Deadline, CustomEvent::Deadline]);
    assert_eq!(harness.next().deadline, None);
}

mod placed;
