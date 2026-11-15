//! Tests for the window's frame function. What it draws is checked through
//! the harness's captures (`ui_test/tests.rs`); these check where it cuts.

use super::*;
use loom_ui_core::{Point, Size};

/// The scissor is the canvas's bounds times the scale, cut to the target
/// and snapped to whole pixels, as Iced cut its primitive's: a canvas that
/// runs past the window's edge is cut there, and one wholly outside has
/// none.
#[test]
fn the_scissor_is_the_bounds_scaled_and_cut_to_the_target() {
    let window = iced::Size::new(200, 100);
    let inside = Rect::new(Point::new(10.0, 10.5), Size::new(50.0, 30.0));
    let cut_in = scissor(inside, window, 2.0).expect("inside the window");
    assert_eq!((cut_in.x, cut_in.y, cut_in.width, cut_in.height), (20, 21, 100, 60));
    let past = Rect::new(Point::new(80.0, 10.0), Size::new(50.0, 80.0));
    let cut = scissor(past, window, 2.0).expect("partly inside");
    assert_eq!((cut.x, cut.y, cut.width, cut.height), (160, 20, 40, 80));
    let outside = Rect::new(Point::new(300.0, 0.0), Size::new(10.0, 10.0));
    assert!(scissor(outside, window, 1.0).is_none());
}
