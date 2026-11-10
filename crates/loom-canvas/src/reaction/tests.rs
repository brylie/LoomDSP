//! Tests for the three rules a reaction keeps.

use super::*;

/// Events stay in the order they were reported.
#[test]
fn events_keep_their_order() {
    let mut reaction = Reaction::default();
    reaction.emit(CanvasEvent::GestureStart);
    reaction.emit(CanvasEvent::PointerDown);
    reaction.emit(CanvasEvent::GestureEnd);
    assert_eq!(
        reaction.events,
        [
            CanvasEvent::GestureStart,
            CanvasEvent::PointerDown,
            CanvasEvent::GestureEnd
        ]
    );
}

/// A new reaction reports nothing, took nothing, and needs no redraw.
#[test]
fn a_new_reaction_is_empty() {
    let reaction = Reaction::default();
    assert!(reaction.events.is_empty());
    assert!(!reaction.taken);
    assert!(!reaction.redraw);
}

/// Once taken, the input stays taken, whatever else is reported.
#[test]
fn taken_stays_true() {
    let mut reaction = Reaction::default();
    reaction.take();
    reaction.emit(CanvasEvent::PointerDown);
    reaction.redraw();
    reaction.take();
    assert!(reaction.taken);
}

/// Once asked for, the redraw stays asked for, and taking the input does
/// not ask for one.
#[test]
fn redraw_stays_true_and_is_its_own_fact() {
    let mut reaction = Reaction::default();
    reaction.take();
    assert!(!reaction.redraw);
    reaction.redraw();
    reaction.emit(CanvasEvent::PointerDown);
    reaction.redraw();
    assert!(reaction.redraw);
}

/// The default cursor shape is the plain arrow.
#[test]
fn the_default_cursor_is_the_plain_arrow() {
    assert_eq!(CursorShape::default(), CursorShape::Default);
}
