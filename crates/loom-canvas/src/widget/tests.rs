//! Tests for what the Iced widget itself does: handing a reaction to Iced.
//! The input handling has its own tests in `crate::input`.

use super::*;
use std::time::Duration;

/// Applying a reaction hands Iced the events as messages in their order,
/// and leaves the input not captured and no redraw asked for when the
/// reaction says neither.
#[test]
fn applying_a_reaction_publishes_its_events_in_order() {
    let scene = Scene::new(Vec::new(), Vec::new());
    let none = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &none, |e| e);
    let reaction = Reaction {
        events: vec![
            CanvasEvent::GestureStart,
            CanvasEvent::PointerDown,
            CanvasEvent::GestureEnd,
        ],
        ..Reaction::default()
    };
    let mut messages = Vec::new();
    let mut shell = iced::advanced::Shell::new(&mut messages);
    canvas.apply(reaction, &mut shell);
    assert_eq!(shell.event_status(), iced::event::Status::Ignored);
    assert_eq!(shell.redraw_request(), window::RedrawRequest::Wait);
    assert_eq!(
        messages,
        [
            CanvasEvent::GestureStart,
            CanvasEvent::PointerDown,
            CanvasEvent::GestureEnd
        ]
    );
}

/// Applying a taken reaction captures the Iced event.
#[test]
fn applying_a_taken_reaction_captures_the_event() {
    let scene = Scene::new(Vec::new(), Vec::new());
    let none = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &none, |e| e);
    let reaction = Reaction {
        taken: true,
        ..Reaction::default()
    };
    let mut messages = Vec::new();
    let mut shell = iced::advanced::Shell::new(&mut messages);
    canvas.apply(reaction, &mut shell);
    assert_eq!(shell.event_status(), iced::event::Status::Captured);
}

/// A reaction that asks for a redraw asks Iced for the next frame, and one
/// that does not asks for none.
#[test]
fn applying_a_reaction_asks_iced_for_its_redraw() {
    let scene = Scene::new(Vec::new(), Vec::new());
    let none = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &none, |e| e);
    for (redraw, expected) in [
        (false, window::RedrawRequest::Wait),
        (true, window::RedrawRequest::NextFrame),
    ] {
        let reaction = Reaction {
            redraw,
            ..Reaction::default()
        };
        let mut messages = Vec::new();
        let mut shell = iced::advanced::Shell::new(&mut messages);
        canvas.apply(reaction, &mut shell);
        assert_eq!(shell.redraw_request(), expected, "{redraw}");
    }
}

/// With an arrow group released, the widget asks Iced to wake the canvas at
/// the group's deadline, and a redraw asked for in the same update still
/// comes first.
#[test]
fn the_widget_asks_iced_to_wake_at_the_next_deadline() {
    let scene = Scene::new(Vec::new(), Vec::new());
    let none = BTreeSet::new();
    let due = Instant::now() + Duration::from_millis(400);
    let canvas = PatchCanvas::new(&scene, &none, |e| e).arrow_group(ArrowGroupView::Released { due });
    let input = CanvasInput::default();
    let mut messages: Vec<CanvasEvent> = Vec::new();
    let mut shell = iced::advanced::Shell::new(&mut messages);
    canvas.schedule(&input, &mut shell);
    assert_eq!(shell.redraw_request(), window::RedrawRequest::At(due));

    let mut messages: Vec<CanvasEvent> = Vec::new();
    let mut shell = iced::advanced::Shell::new(&mut messages);
    canvas.apply(
        Reaction {
            redraw: true,
            ..Reaction::default()
        },
        &mut shell,
    );
    canvas.schedule(&input, &mut shell);
    assert_eq!(shell.redraw_request(), window::RedrawRequest::NextFrame);
}

/// With nothing pending, the widget asks Iced for no wake.
#[test]
fn the_widget_asks_for_no_wake_with_no_deadline() {
    let scene = Scene::new(Vec::new(), Vec::new());
    let none = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &none, |e| e);
    let mut messages: Vec<CanvasEvent> = Vec::new();
    let mut shell = iced::advanced::Shell::new(&mut messages);
    canvas.schedule(&CanvasInput::default(), &mut shell);
    assert_eq!(shell.redraw_request(), window::RedrawRequest::Wait);
}
