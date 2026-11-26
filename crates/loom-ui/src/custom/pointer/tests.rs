//! Tests for a custom element's pointer: what it hears in the hit order,
//! its capture through a drag, the cancels, and the leave (ui-layer.md,
//! section 9, rule 1).

use crate::access::{Access, Role};
use crate::app::{App, Context, InputState, WindowSpec};
use crate::custom::{Custom, CustomEvent, CustomKind, Moment, Out, Used};
use crate::effect::Effects;
use crate::element::{Element, Length};
use crate::event::{DriverEvent, WindowEvent, WindowState};
use crate::headless::Harness;
use crate::layout::tests::{Mono, STYLE};
use crate::routing::NoTarget;
use crate::view::{Layer, View};
use loom_ui_core::{Cursor, Event, Point, PointerButton, PointerEvent, ScrollDelta, Size, UiId};
use std::time::Instant;

/// The pad's kind.
const PAD: CustomKind = CustomKind::new("pad");

/// What the pad sends, and what the test app hears.
#[derive(Debug, Clone, PartialEq)]
enum Message {
    /// A drag began.
    Began,
    /// The drag moved to this `x`.
    Dragged(f32),
    /// The drag finished, with a release.
    Done,
    /// The drag was cancelled, and undone.
    Undone,
    /// The pointer left the pad.
    Left,
    /// Open or close the overlay over the pad.
    Overlay(bool),
    /// Ask the runtime to end the pad's capture.
    Release,
    /// Take the pad out of the view.
    Hide,
}

/// The pad's state: what it heard, and how many messages the app had
/// handled when it heard each, so a test sees a drag's messages run
/// before the next event.
#[derive(Default)]
struct PadState {
    heard: Vec<CustomEvent>,
    seen: Vec<usize>,
    /// Where its moment put the pointer for each event.
    at: Vec<Option<Point>>,
    /// Whether it takes the wheel.
    wheel: bool,
}

impl Custom<Drags> for PadState {
    fn event(&mut self, app: &Drags, event: &CustomEvent, moment: &Moment, out: &mut Out<'_, Message>) -> Used {
        self.heard.push(event.clone());
        self.seen.push(app.log.len());
        self.at.push(moment.pointer);
        let x = moment.pointer.map_or(-1.0, |at| at.x);
        match event {
            CustomEvent::Pointer(PointerEvent::Pressed(PointerButton::Left)) => {
                out.send(Message::Began);
                Used::Capture
            }
            CustomEvent::Pointer(PointerEvent::Moved) if app.dragging() => {
                out.send(Message::Dragged(x));
                Used::Yes
            }
            CustomEvent::Pointer(PointerEvent::Released(PointerButton::Left)) if app.dragging() => {
                out.send(Message::Done);
                Used::Yes
            }
            CustomEvent::Pointer(PointerEvent::Scrolled(_)) if !self.wheel => Used::No,
            CustomEvent::CaptureLost => {
                out.send(Message::Undone);
                Used::Yes
            }
            CustomEvent::PointerLeft => {
                out.send(Message::Left);
                Used::Yes
            }
            _ => Used::Yes,
        }
    }

    fn cursor(&self, app: &Drags, _moment: &Moment) -> Cursor {
        if app.dragging() { Cursor::Grabbing } else { Cursor::Grab }
    }
}

/// An app with the pad above a label, in one panel, and an overlay over
/// the pad when it is open.
struct Drags {
    overlay: bool,
    /// Whether the pad shows.
    pad: bool,
    /// Every message, in the order the app handled them.
    log: Vec<Message>,
}

impl Drags {
    /// True between a drag's start and its end.
    fn dragging(&self) -> bool {
        let start = self.log.iter().rposition(|message| *message == Message::Began);
        let end = self
            .log
            .iter()
            .rposition(|message| matches!(message, Message::Done | Message::Undone));
        start.is_some_and(|start| end.is_none_or(|end| end < start))
    }
}

impl App for Drags {
    type Message = Message;

    fn update(&mut self, message: Message, _cx: &Context<'_, Message>) -> Effects<Message> {
        let effects = match &message {
            Message::Overlay(open) => {
                self.overlay = *open;
                Effects::none()
            }
            Message::Release => Effects::release_pointer(),
            Message::Hide => {
                self.pad = false;
                Effects::none()
            }
            _ => Effects::none(),
        };
        self.log.push(message);
        effects
    }

    fn view(&self, view: &mut View) {
        view.panel_with(UiId::panel("main"), Layer::Panels, &self.pad, |&pad| {
            let mut children = Vec::new();
            if pad {
                children.push(
                    Element::custom(PAD)
                        .key("pad")
                        .width(Length::Fixed(200.0))
                        .height(Length::Fixed(100.0)),
                );
            }
            children.push(Element::space(Length::Fixed(200.0), Length::Fixed(50.0)).key("label"));
            children.push(
                Element::field("", STYLE)
                    .key("name")
                    .width(Length::Fixed(200.0))
                    .access(Access::new(Role::TextInput, "Name")),
            );
            Element::column(children)
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
        (kind == PAD).then(|| Box::new(PadState::default()) as Box<dyn Custom<Self>>)
    }
}

/// A harness with the pad at (0, 0), 200 by 100, and the label under it,
/// in a 400 by 200 window.
fn harness() -> Harness<Drags, NoTarget> {
    let app = Drags {
        overlay: false,
        pad: true,
        log: Vec::new(),
    };
    let window = WindowState::new(Size::new(400.0, 200.0));
    let mut harness = Harness::new(app, NoTarget, window, Instant::now());
    harness.lay_out(&mut Mono::default());
    harness
}

/// A pointer event at `(x, y)`, then a layout, as a driver does before it
/// draws.
fn step(harness: &mut Harness<Drags, NoTarget>, x: f32, y: f32, event: PointerEvent) {
    harness.step(DriverEvent::Input {
        event: Event::Pointer(event),
        pointer: Some(Point::new(x, y)),
    });
    harness.lay_out(&mut Mono::default());
}

/// A window event, then a layout.
fn window(harness: &mut Harness<Drags, NoTarget>, event: WindowEvent) {
    harness.step(DriverEvent::Window(event));
    harness.lay_out(&mut Mono::default());
}

/// Sends `message`, then lays out.
fn send(harness: &mut Harness<Drags, NoTarget>, message: Message) {
    harness.send(message);
    harness.lay_out(&mut Mono::default());
}

/// The pad's UI ID.
fn pad() -> UiId {
    UiId::panel("main").with("pad")
}

/// A pointer event with no position, as a driver sends once the pointer
/// is outside the window.
fn unplaced(harness: &mut Harness<Drags, NoTarget>, event: PointerEvent) {
    harness.step(DriverEvent::Input {
        event: Event::Pointer(event),
        pointer: None,
    });
}

/// What the pad heard.
fn heard(harness: &Harness<Drags, NoTarget>) -> Vec<CustomEvent> {
    let id = UiId::panel("main").with("pad");
    harness
        .runtime()
        .custom::<PadState>(&id)
        .map(|pad| pad.heard.clone())
        .unwrap_or_default()
}

/// The app's messages, without the pointer leaving.
fn log(harness: &Harness<Drags, NoTarget>) -> Vec<Message> {
    harness
        .app()
        .log
        .iter()
        .filter(|message| **message != Message::Left)
        .cloned()
        .collect()
}

/// A left press on the pad.
fn press_pad(harness: &mut Harness<Drags, NoTarget>) {
    step(harness, 50.0, 50.0, PointerEvent::Pressed(PointerButton::Left));
}

/// A drag hears every move until the release, outside its bounds and
/// outside the window too, and its messages run in the turn of the event
/// that sent them: the next move already sees them in the app.
#[test]
fn a_drag_hears_every_move_until_the_release() {
    let mut harness = harness();
    let turns = harness.runtime().counts().turns;
    press_pad(&mut harness);
    assert_eq!(harness.runtime().counts().turns, turns + 1);
    assert_eq!(log(&harness), [Message::Began]);
    // Over the label, then outside the window.
    step(&mut harness, 100.0, 120.0, PointerEvent::Moved);
    step(&mut harness, 500.0, 300.0, PointerEvent::Moved);
    step(&mut harness, 500.0, 300.0, PointerEvent::Released(PointerButton::Left));
    assert_eq!(
        log(&harness),
        [
            Message::Began,
            Message::Dragged(100.0),
            Message::Dragged(500.0),
            Message::Done
        ]
    );
    let id = UiId::panel("main").with("pad");
    let pad = harness.runtime().custom::<PadState>(&id).unwrap();
    assert_eq!(pad.seen[..4], [0, 1, 2, 3]);
    // The label was never hovered while the drag held the pointer.
    assert_ne!(harness.runtime().hovered(), Some(&UiId::panel("main").with("label")));
}

/// The pointer leaving the window does not end a capture.
#[test]
fn leaving_the_window_keeps_the_capture() {
    let mut harness = harness();
    press_pad(&mut harness);
    harness.step(DriverEvent::Input {
        event: Event::Pointer(PointerEvent::LeftWindow),
        pointer: None,
    });
    step(&mut harness, 450.0, 50.0, PointerEvent::Moved);
    harness.step(DriverEvent::Input {
        event: Event::Pointer(PointerEvent::Released(PointerButton::Left)),
        pointer: None,
    });
    assert_eq!(log(&harness), [Message::Began, Message::Dragged(450.0), Message::Done]);
}

/// The capture lasts until the last button is up: a second button pressed
/// and let go mid-drag goes to the pad and does not end it.
#[test]
fn the_capture_lasts_until_the_last_button_is_up() {
    let mut harness = harness();
    press_pad(&mut harness);
    step(&mut harness, 300.0, 50.0, PointerEvent::Pressed(PointerButton::Right));
    step(&mut harness, 300.0, 50.0, PointerEvent::Released(PointerButton::Right));
    step(&mut harness, 310.0, 50.0, PointerEvent::Moved);
    assert_eq!(log(&harness), [Message::Began, Message::Dragged(310.0)]);
    step(&mut harness, 310.0, 50.0, PointerEvent::Released(PointerButton::Left));
    step(&mut harness, 320.0, 50.0, PointerEvent::Moved);
    assert_eq!(log(&harness), [Message::Began, Message::Dragged(310.0), Message::Done]);
}

/// An overlay that opens mid-drag cancels it: the pad hears one cancel
/// and no release, its undo runs, and later events go to the overlay's
/// layer (decision 5, 2026-11-26).
#[test]
fn an_overlay_that_opens_mid_drag_cancels_it() {
    let mut harness = harness();
    press_pad(&mut harness);
    send(&mut harness, Message::Overlay(true));
    step(&mut harness, 60.0, 50.0, PointerEvent::Moved);
    step(&mut harness, 60.0, 50.0, PointerEvent::Released(PointerButton::Left));
    assert_eq!(log(&harness), [Message::Began, Message::Overlay(true), Message::Undone]);
    let lost = heard(&harness)
        .iter()
        .filter(|event| **event == CustomEvent::CaptureLost)
        .count();
    assert_eq!(lost, 1);
    assert!(
        !heard(&harness).contains(&CustomEvent::Pointer(PointerEvent::Released(PointerButton::Left))),
        "the pad heard a release after the cancel"
    );
    // The move after the cancel was not the pad's: the layer it is in
    // takes no input now.
    let after = heard(&harness);
    let lost_at = after
        .iter()
        .position(|event| *event == CustomEvent::CaptureLost)
        .unwrap();
    assert!(
        after[lost_at..]
            .iter()
            .all(|event| !matches!(event, CustomEvent::Pointer(_))),
        "the pad heard {after:?}"
    );
    // It hears the pointer leave it, since the overlay's layer took it.
    assert!(after[lost_at..].contains(&CustomEvent::PointerLeft));
}

/// The window losing the keys, or hiding, cancels a drag.
#[test]
fn the_window_losing_the_keys_or_hiding_cancels_a_drag() {
    for event in [WindowEvent::Focused(false), WindowEvent::Hidden(true)] {
        let mut harness = harness();
        press_pad(&mut harness);
        window(&mut harness, event.clone());
        step(&mut harness, 60.0, 50.0, PointerEvent::Released(PointerButton::Left));
        assert_eq!(log(&harness), [Message::Began, Message::Undone], "after {event:?}");
    }
}

/// The app can release the capture, with a cancel.
#[test]
fn the_app_can_release_the_capture() {
    let mut harness = harness();
    press_pad(&mut harness);
    send(&mut harness, Message::Release);
    step(&mut harness, 300.0, 50.0, PointerEvent::Moved);
    assert_eq!(log(&harness), [Message::Began, Message::Release, Message::Undone]);
}

/// An element that leaves the layout mid-drag is dropped and hears
/// nothing more.
#[test]
fn an_element_that_leaves_mid_drag_hears_nothing() {
    let mut harness = harness();
    press_pad(&mut harness);
    send(&mut harness, Message::Hide);
    step(&mut harness, 60.0, 120.0, PointerEvent::Released(PointerButton::Left));
    assert_eq!(log(&harness), [Message::Began, Message::Hide]);
    assert!(harness.runtime().custom::<PadState>(&pad()).is_none());
    assert_eq!(harness.runtime().custom_bounds(&pad()), None);
}

/// The pad hears that the pointer left it once, when it moves off.
#[test]
fn the_pad_hears_the_pointer_leave_once() {
    let mut harness = harness();
    step(&mut harness, 50.0, 50.0, PointerEvent::Moved);
    step(&mut harness, 60.0, 50.0, PointerEvent::Moved);
    step(&mut harness, 60.0, 120.0, PointerEvent::Moved);
    step(&mut harness, 60.0, 130.0, PointerEvent::Moved);
    let left = harness.app().log.iter().filter(|m| **m == Message::Left).count();
    assert_eq!(left, 1);
}

/// The pad hears the pointer leave the window as leaving it.
#[test]
fn leaving_the_window_leaves_the_pad() {
    let mut harness = harness();
    step(&mut harness, 50.0, 50.0, PointerEvent::Moved);
    harness.step(DriverEvent::Input {
        event: Event::Pointer(PointerEvent::LeftWindow),
        pointer: None,
    });
    assert_eq!(harness.app().log, [Message::Left]);
}

/// A drag that ends off the pad tells it the pointer left after the
/// release, not during the drag.
#[test]
fn a_drag_hears_the_leave_after_its_release() {
    let mut harness = harness();
    press_pad(&mut harness);
    step(&mut harness, 60.0, 120.0, PointerEvent::Moved);
    assert!(!harness.app().log.contains(&Message::Left));
    step(&mut harness, 60.0, 120.0, PointerEvent::Released(PointerButton::Left));
    step(&mut harness, 60.0, 125.0, PointerEvent::Moved);
    assert_eq!(harness.app().log.last(), Some(&Message::Left));
}

/// The wheel is not captured, and a wheel the pad leaves goes on.
#[test]
fn the_wheel_goes_to_the_pad_under_the_pointer() {
    let wheel = PointerEvent::Scrolled(ScrollDelta::Lines { x: 0.0, y: 1.0 });
    let mut harness = harness();
    step(&mut harness, 50.0, 50.0, wheel);
    assert!(heard(&harness).contains(&CustomEvent::Pointer(wheel)));
    // During a drag, a wheel over the label is not the pad's.
    press_pad(&mut harness);
    let before = heard(&harness).len();
    step(&mut harness, 50.0, 120.0, wheel);
    assert_eq!(heard(&harness).len(), before);
}

/// A driver whose toolkit answers late offers events itself, outside a
/// turn: the drag still holds the pointer, the toolkit is kept from it,
/// and what the pad sends runs on the next turn, since there is no turn
/// to run it in.
#[test]
fn a_driver_outside_a_turn_reaches_the_pad() {
    let mut harness = harness();
    let press = Event::Pointer(PointerEvent::Pressed(PointerButton::Left));
    let point = Point::new(50.0, 50.0);
    let (_, _, mut panels) = harness.parts();
    let heard = panels.pointer_below(&press, point);
    assert!(heard.taken);
    let moved = Event::Pointer(PointerEvent::Moved);
    assert!(panels.over_target(&moved, Point::new(300.0, 50.0)));
    assert!(harness.app().log.is_empty());
    harness.step(DriverEvent::Woken);
    assert_eq!(log(&harness), [Message::Began]);
}

/// The driver reporting the pointer lost cancels the drag, and the
/// cancel's messages run in that same turn.
#[test]
fn a_lost_pointer_cancels_the_drag_in_its_turn() {
    let mut harness = harness();
    press_pad(&mut harness);
    let turns = harness.runtime().counts().turns;
    unplaced(&mut harness, PointerEvent::Lost);
    assert_eq!(harness.runtime().counts().turns, turns + 1);
    assert_eq!(log(&harness), [Message::Began, Message::Undone]);
    // The drag is over: a move is the pad's hover, not a drag.
    step(&mut harness, 60.0, 50.0, PointerEvent::Moved);
    assert_eq!(log(&harness), [Message::Began, Message::Undone]);
}

/// A lost pointer with no capture does nothing: no element hears it.
#[test]
fn a_lost_pointer_with_no_capture_does_nothing() {
    let mut harness = harness();
    step(&mut harness, 50.0, 50.0, PointerEvent::Moved);
    let before = heard(&harness);
    step(&mut harness, 50.0, 50.0, PointerEvent::Lost);
    unplaced(&mut harness, PointerEvent::Lost);
    assert_eq!(heard(&harness), before);
    assert!(harness.app().log.is_empty());
}

/// Out of a turn, a press and the cancel after it reach the next turn in
/// the order they happened, and the cancel is not dropped.
#[test]
fn out_of_a_turn_the_cancel_keeps_its_order() {
    let mut harness = harness();
    let point = Point::new(50.0, 50.0);
    let (_, _, mut panels) = harness.parts();
    let _ = panels.pointer_below(&Event::Pointer(PointerEvent::Pressed(PointerButton::Left)), point);
    let _ = panels.pointer_below(&Event::Pointer(PointerEvent::Lost), point);
    harness.step(DriverEvent::Woken);
    assert_eq!(log(&harness), [Message::Began, Message::Undone]);
}

/// An element with no state, as a release build leaves for a kind the app
/// did not know, is safe on every pointer path: it hears, takes, and sends
/// nothing.
#[test]
fn an_element_with_no_state_is_safe_on_every_pointer_path() {
    let mut harness = harness();
    harness.runtime_mut().customs_mut().empty(&pad());
    let wheel = PointerEvent::Scrolled(ScrollDelta::Lines { x: 0.0, y: 1.0 });
    for event in [
        PointerEvent::Moved,
        PointerEvent::Pressed(PointerButton::Left),
        PointerEvent::Moved,
        wheel,
        PointerEvent::Released(PointerButton::Left),
        PointerEvent::Lost,
    ] {
        step(&mut harness, 50.0, 50.0, event);
    }
    step(&mut harness, 50.0, 120.0, PointerEvent::Moved);
    unplaced(&mut harness, PointerEvent::LeftWindow);
    window(&mut harness, WindowEvent::Focused(false));
    send(&mut harness, Message::Release);
    assert_eq!(harness.app().log, [Message::Release]);
}

/// Making an element's state at layout sends no message: the turn after
/// the first layout runs no update.
#[test]
fn making_the_state_sends_no_message() {
    let mut harness = harness();
    let updates = harness.runtime().counts().updates;
    harness.step(DriverEvent::Woken);
    assert_eq!(harness.runtime().counts().updates, updates);
    assert!(harness.app().log.is_empty());
}

/// How many times the pad heard the pointer leave.
fn lefts(harness: &Harness<Drags, NoTarget>) -> usize {
    heard(harness)
        .iter()
        .filter(|event| **event == CustomEvent::PointerLeft)
        .count()
}

/// A drag released over another element: the pad hears the pointer leave
/// once, and the element under the pointer is hovered, in the turn of the
/// release.
#[test]
fn a_release_over_another_element_moves_the_hover_there() {
    let mut harness = harness();
    press_pad(&mut harness);
    step(&mut harness, 60.0, 120.0, PointerEvent::Moved);
    assert_eq!(lefts(&harness), 0);
    step(&mut harness, 60.0, 120.0, PointerEvent::Released(PointerButton::Left));
    assert_eq!(lefts(&harness), 1);
    assert_eq!(harness.runtime().hovered(), Some(&UiId::panel("main").with("label")));
    step(&mut harness, 60.0, 125.0, PointerEvent::Moved);
    assert_eq!(lefts(&harness), 1);
}

/// A drag that left the window and is released there: the pad hears the
/// pointer leave, and nothing is hovered.
#[test]
fn a_release_outside_the_window_clears_the_hover() {
    let mut harness = harness();
    press_pad(&mut harness);
    unplaced(&mut harness, PointerEvent::LeftWindow);
    unplaced(&mut harness, PointerEvent::Released(PointerButton::Left));
    assert_eq!(lefts(&harness), 1);
    assert_eq!(harness.runtime().hovered(), None);
}

/// A cancel after a move tells the pad where the pointer is now, not
/// where the press went down.
#[test]
fn a_cancel_tells_where_the_pointer_is_now() {
    let mut harness = harness();
    press_pad(&mut harness);
    step(&mut harness, 300.0, 70.0, PointerEvent::Moved);
    window(&mut harness, WindowEvent::Focused(false));
    let pad = harness.runtime().custom::<PadState>(&pad()).unwrap();
    let lost = pad
        .heard
        .iter()
        .position(|event| *event == CustomEvent::CaptureLost)
        .unwrap();
    assert_eq!(pad.at[lost], Some(Point::new(300.0, 70.0)));
}

/// The pad's shape shows over it, and none over a plain part of a panel;
/// over a field, the text beam.
#[test]
fn the_cursor_follows_the_hover() {
    let mut harness = harness();
    step(&mut harness, 50.0, 50.0, PointerEvent::Moved);
    assert_eq!(harness.runtime().cursor(), Some(Cursor::Grab));
    step(&mut harness, 50.0, 120.0, PointerEvent::Moved);
    assert_eq!(harness.runtime().cursor(), None);
    step(&mut harness, 50.0, 155.0, PointerEvent::Moved);
    assert_eq!(harness.runtime().hovered(), Some(&UiId::panel("main").with("name")));
    assert_eq!(harness.runtime().cursor(), Some(Cursor::Text));
}

/// While the pad holds the capture its shape wins wherever the pointer
/// is: over the label, over the field, and outside the window, as a
/// native drag keeps its shape. The release gives the shape back.
#[test]
fn a_drag_keeps_its_cursor_wherever_the_pointer_goes() {
    let mut harness = harness();
    press_pad(&mut harness);
    assert_eq!(harness.runtime().cursor(), Some(Cursor::Grabbing));
    for (x, y) in [(50.0, 120.0), (50.0, 155.0), (500.0, 300.0)] {
        step(&mut harness, x, y, PointerEvent::Moved);
        assert_eq!(harness.runtime().cursor(), Some(Cursor::Grabbing), "at ({x}, {y})");
    }
    step(&mut harness, 50.0, 155.0, PointerEvent::Released(PointerButton::Left));
    assert_eq!(harness.runtime().cursor(), Some(Cursor::Text));
}
