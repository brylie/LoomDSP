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

/// Each redraw request becomes the Iced request of the same meaning.
#[test]
fn applying_a_reaction_asks_iced_for_its_redraw() {
    let scene = Scene::new(Vec::new(), Vec::new());
    let none = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &none, |e| e);
    let time = Instant::now() + Duration::from_millis(250);
    for (redraw, expected) in [
        (Redraw::No, window::RedrawRequest::Wait),
        (Redraw::At(time), window::RedrawRequest::At(time)),
        (Redraw::Now, window::RedrawRequest::NextFrame),
    ] {
        let reaction = Reaction {
            redraw,
            ..Reaction::default()
        };
        let mut messages = Vec::new();
        let mut shell = iced::advanced::Shell::new(&mut messages);
        canvas.apply(reaction, &mut shell);
        assert_eq!(shell.redraw_request(), expected, "{redraw:?}");
    }
}
