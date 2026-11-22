//! Tests for the slider through the runtime and the headless harness
//! (ui-layer.md, section 5, rule 16; section 6, rule 10; section 7, rules
//! 5 and 30): keys and their groups, a drag, Shift, a double-click, focus
//! from a press, a screen reader's steps, and a slider turned off.
//!
//! The geometry: a 16 px stop, then the slider, 400 px wide and 14 tall,
//! at y 16. Its thumb's centre travels from x 7 to 393.

use crate::SlidePhase::{End, Move, Start};
use crate::access::{Access, Action, Role, States, Value};
use crate::element::Element;
use crate::event::{AccessEvent, DriverEvent};
use crate::headless::Harness;
use crate::slider::Slider;
use crate::view::Layer;
use crate::window::scroll::tests::{
    Message, Shown, Toolkit, id, key, list_panel, pointer, shifted, showing, step, stop,
};
use accesskit::ActionRequest;
use loom_ui_core::{Event, Key, KeyEvent, Modifiers, Point, PointerButton, PointerEvent, UiId};
use std::time::Duration;

/// The slider's UI ID.
fn level() -> UiId {
    id("level")
}

/// The slider's node: a slider named Level, at the parameter's own value,
/// offering Increment, Decrement, and SetValue.
fn level_access(states: States) -> Access {
    Access::new(Role::Slider, "Level")
        .value(Value::Number {
            value: 0.5,
            min: 0.0,
            max: 1.0,
            step: None,
        })
        .action(Action::Increment)
        .action(Action::Decrement)
        .action(Action::SetValue)
        .states(states)
}

/// A stop, then a slider at 0.5 with its default at 0.2.
fn one(_: u32, _: Option<&UiId>) -> Vec<(UiId, Layer, Element)> {
    let slider = Element::slider(Slider::new(0.5).default(0.2))
        .key("level")
        .access(level_access(States::default()));
    list_panel(Element::column([stop("before"), slider]))
}

/// The same slider, turned off.
fn off(_: u32, _: Option<&UiId>) -> Vec<(UiId, Layer, Element)> {
    let states = States {
        disabled: true,
        ..States::default()
    };
    let slider = Element::slider(Slider::new(0.5))
        .key("level")
        .access(level_access(states));
    list_panel(Element::column([stop("before"), slider]))
}

/// The point over `position` on the slider.
fn over(position: f32) -> Point {
    Point::new(7.0 + position * 386.0, 23.0)
}

/// A left press at `point`.
fn press(point: Point) -> DriverEvent {
    pointer(PointerEvent::Pressed(PointerButton::Left), point)
}

/// A left release at `point`.
fn release(point: Point) -> DriverEvent {
    pointer(PointerEvent::Released(PointerButton::Left), point)
}

/// The pointer moved to `point`.
fn move_to(point: Point) -> DriverEvent {
    pointer(PointerEvent::Moved, point)
}

/// The modifiers held changed to `modifiers`.
fn modifiers(modifiers: Modifiers) -> DriverEvent {
    DriverEvent::Input {
        event: Event::Key(KeyEvent::ModifiersChanged(modifiers)),
        pointer: None,
    }
}

/// Asserts the app heard `expected` from the slider, positions to a
/// millionth.
#[track_caller]
fn assert_slid(harness: &Harness<Shown, Toolkit>, expected: &[(f32, crate::SlidePhase)]) {
    let got: Vec<(f32, crate::SlidePhase)> = harness
        .app()
        .slid
        .iter()
        .map(|(slider, at, phase)| {
            assert_eq!(slider, &level());
            (*at, *phase)
        })
        .collect();
    let same = got.len() == expected.len()
        && got
            .iter()
            .zip(expected)
            .all(|((at, phase), (want, want_phase))| (at - want).abs() < 1e-6 && phase == want_phase);
    assert!(same, "{got:?} is not {expected:?}");
}

/// Tab to the slider, past the stop before it.
fn tab_in(harness: &mut Harness<Shown, Toolkit>) {
    step(harness, key(Key::Tab));
    step(harness, key(Key::Tab));
    assert_eq!(harness.runtime().focused(), Some(&level()));
}

/// Section 6, rule 10: the arrows move the focused slider and show the
/// ring; keys within 500 ms of each other are one group, which ends 500
/// ms after the last.
#[test]
fn keys_move_it_in_one_group_that_ends_after_500_ms() {
    let mut harness = showing(one);
    tab_in(&mut harness);
    step(&mut harness, key(Key::Right));
    assert!(harness.runtime().focus_ring().is_some(), "a ring from the keyboard");
    harness.advance(Duration::from_millis(300));
    step(&mut harness, key(Key::Right));
    assert_slid(&harness, &[(0.5, Start), (0.51, Move), (0.52, Move)]);
    assert!(harness.next().deadline.is_some(), "the runtime wakes to end the group");
    harness.advance(Duration::from_millis(499));
    assert_eq!(harness.app().slid.len(), 3, "not yet");
    harness.advance(Duration::from_millis(1));
    assert_slid(&harness, &[(0.5, Start), (0.51, Move), (0.52, Move), (0.52, End)]);
    assert!(harness.next().deadline.is_none());
}

/// Shift with an arrow is the fine step, and Home, End, Page Up, and Page
/// Down go where the pattern says. None reaches the toolkit.
#[test]
fn shift_home_end_and_page_keys() {
    let mut harness = showing(one);
    tab_in(&mut harness);
    step(&mut harness, shifted(Key::Right));
    step(&mut harness, key(Key::End));
    step(&mut harness, key(Key::PageDown));
    step(&mut harness, key(Key::Home));
    assert_slid(
        &harness,
        &[(0.5, Start), (0.501, Move), (1.0, Move), (0.9, Move), (0.0, Move)],
    );
    assert!(
        !harness.runtime().target().keys.contains(&Key::End),
        "the slider took its keys"
    );
}

/// A key the slider does not take, such as Space, goes on, here to the
/// toolkit, as there is no container around it.
#[test]
fn space_is_not_the_sliders() {
    let mut harness = showing(one);
    tab_in(&mut harness);
    step(&mut harness, key(Key::Space));
    assert!(harness.app().slid.is_empty());
    assert!(harness.runtime().target().keys.contains(&Key::Space));
}

/// Focus leaving the slider ends its group of keys at once.
#[test]
fn focus_leaving_ends_the_group() {
    let mut harness = showing(one);
    tab_in(&mut harness);
    step(&mut harness, key(Key::Left));
    step(&mut harness, shifted(Key::Tab));
    assert_slid(&harness, &[(0.5, Start), (0.49, Move), (0.49, End)]);
}

/// Section 5, rule 16: a press on the track jumps there and takes focus
/// with no ring; the drag follows the pointer, even off the slider, until
/// the release, which ends it.
#[test]
fn a_press_jumps_and_drags_until_the_release() {
    let mut harness = showing(one);
    step(&mut harness, press(over(0.25)));
    assert_eq!(harness.runtime().focused(), Some(&level()));
    assert!(harness.runtime().focus_ring().is_none(), "no ring from a press");
    step(&mut harness, move_to(over(0.35)));
    step(&mut harness, move_to(Point::new(over(0.45).x, 150.0)));
    step(&mut harness, release(Point::new(over(0.45).x, 150.0)));
    assert_slid(
        &harness,
        &[(0.5, Start), (0.25, Move), (0.35, Move), (0.45, Move), (0.45, End)],
    );
    assert!(harness.app().clicks.is_empty(), "a drag is no click");
}

/// Shift held while dragging is ten times finer.
#[test]
fn shift_while_dragging_is_finer() {
    let mut harness = showing(one);
    step(&mut harness, press(over(0.5)));
    step(
        &mut harness,
        modifiers(Modifiers {
            shift: true,
            ..Modifiers::NONE
        }),
    );
    step(&mut harness, move_to(over(0.6)));
    step(&mut harness, release(over(0.6)));
    assert_slid(&harness, &[(0.5, Start), (0.51, Move), (0.51, End)]);
}

/// A double-click resets it to its default as one Start, Move, and End.
#[test]
fn a_double_click_resets_it() {
    let mut harness = showing(one);
    step(&mut harness, press(over(0.5)));
    step(&mut harness, release(over(0.5)));
    harness.advance(Duration::from_millis(200));
    step(&mut harness, press(over(0.5)));
    step(&mut harness, release(over(0.5)));
    assert_slid(
        &harness,
        &[(0.5, Start), (0.5, End), (0.5, Start), (0.2, Move), (0.2, End)],
    );
}

/// A pointer press anywhere ends an open group of keys.
#[test]
fn a_press_ends_the_group() {
    let mut harness = showing(one);
    tab_in(&mut harness);
    step(&mut harness, key(Key::Right));
    step(&mut harness, press(Point::new(300.0, 150.0)));
    assert_slid(&harness, &[(0.5, Start), (0.51, Move), (0.51, End)]);
}

/// Section 7, rule 5: a screen reader's Increment moves one key step, as
/// one Start, Move, and End, and SetValue goes to the app, which knows the
/// parameter's units.
#[test]
fn a_screen_readers_increment_is_one_step() {
    let mut harness = showing(one);
    harness.send(Message::Describe);
    let nodes = harness.app().nodes.clone().expect("the nodes");
    let (node, slider) = nodes
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some("Level"))
        .cloned()
        .expect("the slider's node");
    assert_eq!(slider.role(), accesskit::Role::Slider);
    assert!(slider.supports_action(accesskit::Action::Increment));
    assert!(slider.supports_action(accesskit::Action::Focus));
    let action = |action| {
        DriverEvent::Accessibility(AccessEvent::Action(ActionRequest {
            action,
            target_tree: accesskit::TreeId::ROOT,
            target_node: node,
            data: None,
        }))
    };
    step(&mut harness, action(accesskit::Action::Increment));
    assert_slid(&harness, &[(0.5, Start), (0.51, Move), (0.51, End)]);
    step(&mut harness, action(accesskit::Action::Decrement));
    assert_eq!(harness.app().slid.len(), 6, "a Decrement too");
}

/// A slider turned off is no stop, and neither a press nor a screen
/// reader's step moves it.
#[test]
fn a_slider_turned_off_does_nothing() {
    let mut harness = showing(off);
    step(&mut harness, key(Key::Tab));
    step(&mut harness, key(Key::Tab));
    assert_ne!(harness.runtime().focused(), Some(&level()), "no stop");
    step(&mut harness, press(over(0.25)));
    step(&mut harness, move_to(over(0.35)));
    step(&mut harness, release(over(0.35)));
    assert!(harness.app().slid.is_empty());
}
