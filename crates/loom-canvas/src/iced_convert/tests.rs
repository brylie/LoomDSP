//! Tests that a value keeps every field on its way to Iced and back.

use super::*;

/// A point keeps both coordinates each way.
#[test]
fn a_point_keeps_its_coordinates() {
    let ours = Point::new(1.5, -2.5);
    let theirs = ours.to_iced();
    assert_eq!((theirs.x, theirs.y), (1.5, -2.5));
    assert_eq!(theirs.to_core(), ours);
}

/// A vector keeps both parts each way.
#[test]
fn a_vector_keeps_its_parts() {
    let ours = Vector::new(-3.0, 4.25);
    let theirs = ours.to_iced();
    assert_eq!((theirs.x, theirs.y), (-3.0, 4.25));
    assert_eq!(theirs.to_core(), ours);
}

/// A size keeps its width and height each way, and does not swap them.
#[test]
fn a_size_keeps_its_width_and_height() {
    let ours = Size::new(640.0, 480.0);
    let theirs = ours.to_iced();
    assert_eq!((theirs.width, theirs.height), (640.0, 480.0));
    assert_eq!(theirs.to_core(), ours);
}

/// A rectangle keeps its corner and its size each way.
#[test]
fn a_rect_keeps_its_corner_and_size() {
    let ours = Rect::new(Point::new(10.0, 20.0), Size::new(30.0, 40.0));
    let theirs = ours.to_iced();
    assert_eq!(
        (theirs.x, theirs.y, theirs.width, theirs.height),
        (10.0, 20.0, 30.0, 40.0)
    );
    assert_eq!(theirs.to_core(), ours);
}

/// A colour keeps its four channels each way, in the same order.
#[test]
fn a_colour_keeps_its_channels() {
    let ours = Color::rgba(0.1, 0.2, 0.3, 0.4);
    let theirs = ours.to_iced();
    assert_eq!((theirs.r, theirs.g, theirs.b, theirs.a), (0.1, 0.2, 0.3, 0.4));
    assert_eq!(theirs.to_core(), ours);
}
