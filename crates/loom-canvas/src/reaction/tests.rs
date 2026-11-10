//! Tests for the three rules a reaction keeps.

use super::*;
use std::time::Duration;

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
    assert_eq!(reaction.redraw, Redraw::No);
}

/// Once taken, the input stays taken.
#[test]
fn taken_stays_true() {
    let mut reaction = Reaction::default();
    reaction.take();
    reaction.emit(CanvasEvent::PointerDown);
    reaction.redraw();
    reaction.take();
    assert!(reaction.taken);
}

/// A redraw at the next frame wins over any time, whichever came first.
#[test]
fn now_wins_over_a_time() {
    let later = Instant::now() + Duration::from_secs(1);
    let mut first = Reaction::default();
    first.redraw();
    first.redraw_at(later);
    assert_eq!(first.redraw, Redraw::Now);
    let mut second = Reaction::default();
    second.redraw_at(later);
    second.redraw();
    assert_eq!(second.redraw, Redraw::Now);
}

/// Of two times, the earlier wins, whichever came first.
#[test]
fn the_earlier_time_wins() {
    let soon = Instant::now() + Duration::from_millis(100);
    let late = soon + Duration::from_millis(300);
    let mut reaction = Reaction::default();
    reaction.redraw_at(late);
    reaction.redraw_at(soon);
    assert_eq!(reaction.redraw, Redraw::At(soon));
    reaction.redraw_at(late);
    assert_eq!(reaction.redraw, Redraw::At(soon));
}

/// Any request is sooner than none.
#[test]
fn any_request_is_sooner_than_none() {
    let time = Instant::now();
    assert_eq!(Redraw::No.sooner(Redraw::At(time)), Redraw::At(time));
    assert_eq!(Redraw::At(time).sooner(Redraw::No), Redraw::At(time));
    assert_eq!(Redraw::No.sooner(Redraw::Now), Redraw::Now);
    assert_eq!(Redraw::No.sooner(Redraw::No), Redraw::No);
}

/// The default cursor shape is the plain arrow.
#[test]
fn the_default_cursor_is_the_plain_arrow() {
    assert_eq!(CursorShape::default(), CursorShape::Default);
}
