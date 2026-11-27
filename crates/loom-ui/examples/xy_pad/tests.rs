//! Tests for the XY pad, through the headless harness: its capture, its
//! keys, its cursor, and its nodes, together.

use super::*;
use loom_ui::headless::Harness;
use loom_ui::{DriverEvent, WindowState};
use loom_ui_core::{Modifiers, TextMeasure};

/// Measures every character 7 pixels wide, so the tests need no fonts.
struct Monospace;

impl TextMeasure for Monospace {
    fn measure(&mut self, text: &str, _style: TextStyle, _wrap_width: Option<f32>) -> Size {
        Size::new(text.chars().count() as f32 * 7.0, 16.0)
    }
}

/// The pad app with its dot in the middle, laid out in its window.
fn harness() -> Harness<XyApp, Keys> {
    let app = XyApp {
        x: 0.5,
        y: 0.5,
        ids: RefCell::new(IdTable::default()),
        capture_to: None,
    };
    let window = WindowState::new(Size::new(280.0, 320.0));
    let mut harness = Harness::new(app, Keys, window, Instant::now());
    harness.lay_out(&mut Monospace);
    harness.step(DriverEvent::Woken);
    harness.lay_out(&mut Monospace);
    harness
}

/// The pad's UI ID.
fn pad() -> UiId {
    UiId::panel("main").with("pad")
}

/// Where the pad is.
fn bounds(harness: &Harness<XyApp, Keys>) -> Rect {
    harness.runtime().custom_bounds(&pad()).expect("the pad is laid out")
}

/// A pointer event at `at`, or outside the window with `None`.
fn pointer(harness: &mut Harness<XyApp, Keys>, event: PointerEvent, at: Option<Point>) {
    harness.step(DriverEvent::Input {
        event: Event::Pointer(event),
        pointer: at,
    });
    harness.lay_out(&mut Monospace);
}

/// A key press.
fn key(harness: &mut Harness<XyApp, Keys>, key: Key) {
    key_with(harness, key, Modifiers::NONE);
}

/// A key press with `modifiers` held.
fn key_with(harness: &mut Harness<XyApp, Keys>, key: Key, modifiers: Modifiers) {
    harness.step(DriverEvent::Input {
        event: Event::Key(KeyEvent::Pressed {
            key,
            modifiers,
            repeat: false,
            text: None,
        }),
        pointer: None,
    });
    harness.lay_out(&mut Monospace);
}

/// The dot, in hundredths.
fn dot(harness: &Harness<XyApp, Keys>) -> (i32, i32) {
    (rounded(harness.app().x), rounded(harness.app().y))
}

/// A press captures the pointer: the drag goes on outside the pad, and the
/// dot stops at its edge; after the release a move does nothing. Over the
/// pad, and through the drag, the pointer is a crosshair.
#[test]
fn a_drag_holds_the_pointer_outside_the_pad() {
    let mut harness = harness();
    let pad = bounds(&harness);
    let quarter = Point::new(pad.x + pad.width / 4.0, pad.y + pad.height / 4.0);
    pointer(&mut harness, PointerEvent::Moved, Some(quarter));
    assert_eq!(harness.runtime().cursor(), Some(Cursor::Crosshair));
    pointer(&mut harness, PointerEvent::Pressed(PointerButton::Left), Some(quarter));
    assert_eq!(dot(&harness), (25, 75));
    pointer(&mut harness, PointerEvent::Moved, Some(Point::new(500.0, -100.0)));
    assert_eq!(
        dot(&harness),
        (100, 100),
        "outside the pad, the dot stops at its corner"
    );
    assert_eq!(harness.runtime().cursor(), Some(Cursor::Crosshair));
    pointer(
        &mut harness,
        PointerEvent::Released(PointerButton::Left),
        Some(Point::new(500.0, -100.0)),
    );
    pointer(&mut harness, PointerEvent::Moved, Some(quarter));
    assert_eq!(dot(&harness), (100, 100), "after the release, a move moves nothing");
}

/// Tab reaches the pad, and the arrow keys move the dot.
#[test]
fn the_arrow_keys_move_the_dot() {
    let mut harness = harness();
    key(&mut harness, Key::Tab);
    assert_eq!(harness.runtime().focused(), Some(&pad()));
    key(&mut harness, Key::Right);
    key(&mut harness, Key::Right);
    key(&mut harness, Key::Down);
    assert_eq!(dot(&harness), (60, 45));
}

/// Page Up and Page Down move Y a quarter, and X with Shift; Home and End
/// set X to its ends, and Y with Shift, as the slider pattern does on one
/// axis.
#[test]
fn page_and_edge_keys_follow_the_slider_pattern() {
    let mut harness = harness();
    let shift = Modifiers {
        shift: true,
        ..Modifiers::NONE
    };
    key(&mut harness, Key::Tab);
    key(&mut harness, Key::PageUp);
    assert_eq!(dot(&harness), (50, 75));
    key_with(&mut harness, Key::PageDown, shift);
    assert_eq!(dot(&harness), (25, 75));
    key(&mut harness, Key::End);
    assert_eq!(dot(&harness), (100, 75));
    key_with(&mut harness, Key::Home, shift);
    assert_eq!(dot(&harness), (100, 0));
}

/// A screen reader meets the pad's two sliders with their values, and its
/// Increment on one moves the dot, which the slider then reads.
#[test]
fn a_screen_reader_moves_the_dot_through_its_sliders() {
    let mut harness = harness();
    let nodes = harness.runtime().access_nodes(&mut harness.app().ids.borrow_mut());
    let slider = |name: &str| {
        nodes
            .nodes
            .iter()
            .find(|(_, node)| node.role() == accesskit::Role::Slider && node.label() == Some(name))
            .map(|(id, node)| (*id, node.numeric_value()))
            .expect("the slider is in the tree")
    };
    let (y, value) = slider("Y");
    assert_eq!(value, Some(0.5));
    assert!(slider("X").1.is_some());
    harness.step(DriverEvent::Accessibility(AccessEvent::Action(
        accesskit::ActionRequest {
            action: accesskit::Action::Increment,
            target_tree: TreeId::ROOT,
            target_node: y,
            data: None,
        },
    )));
    harness.lay_out(&mut Monospace);
    assert_eq!(dot(&harness), (50, 55));
    let nodes = harness.runtime().access_nodes(&mut harness.app().ids.borrow_mut());
    let value = nodes
        .nodes
        .iter()
        .find(|(id, _)| *id == y)
        .and_then(|(_, node)| node.numeric_value());
    assert!(value.is_some_and(|value| (value - 0.55).abs() < 1e-6));
}
