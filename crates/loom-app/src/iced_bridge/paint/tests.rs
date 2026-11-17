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

/// The runtime's panels draw in two passes around Iced's layers: what is
/// above Iced covers what is below it, and two empty lists draw exactly
/// what a frame with no panels draws (ui-layer.md, section 5, rule 10).
#[test]
fn panels_draw_below_and_above_and_empty_lists_draw_nothing() {
    use loom_ui::paint::{ListPainter, PaintList, Painter, Quad, Typesetter};
    use loom_ui_core::{Color as Core, Point, Rect, Size as CoreSize};
    let Some(mut offscreen) = crate::iced_bridge::offscreen::Offscreen::new() else {
        assert!(
            !std::env::var("LOOM_REQUIRE_GPU").is_ok_and(|value| value == "1"),
            "coverage requires a GPU adapter"
        );
        return;
    };
    let size = iced::Size::new(64, 32);
    let background = iced::Color::BLACK;
    let whole = Rect::new(Point::ORIGIN, CoreSize::new(64.0, 32.0));
    let text = std::cell::RefCell::new(Typesetter::new());
    let boxed = |rect: Rect, fill: Core| {
        let mut list = PaintList::new(whole);
        ListPainter::new(&text, &mut list).quad(Quad {
            rect,
            fill,
            border: Core::TRANSPARENT,
            border_width: 0.0,
            radius: 0.0,
        });
        list
    };
    let red = Core::rgb(1.0, 0.0, 0.0);
    let blue = Core::rgb(0.0, 0.0, 1.0);
    let plain = offscreen.pixels(size, 1.0, background, None).expect("read back");
    let empty = offscreen
        .pixels_with(
            size,
            1.0,
            background,
            None,
            Some((PaintList::new(whole), PaintList::new(whole))),
        )
        .expect("read back");
    assert_eq!(plain, empty, "empty lists draw nothing");
    let drawn = offscreen
        .pixels_with(
            size,
            1.0,
            background,
            None,
            Some((
                boxed(whole, red),
                boxed(Rect::new(Point::ORIGIN, CoreSize::new(16.0, 16.0)), blue),
            )),
        )
        .expect("read back");
    let at = |x: usize, y: usize| &drawn[(y * 64 + x) * 4..(y * 64 + x) * 4 + 3];
    assert_eq!(at(4, 4), [0, 0, 255], "above covers below");
    assert_eq!(at(40, 20), [255, 0, 0], "below shows where nothing is above");
}
