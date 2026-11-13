//! Tests for points, vectors, sizes, and rectangles, each against the rule
//! its doc comment states.

use super::*;

/// A rectangle from its left, top, width, and height, to keep tests short.
fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect::new(Point::new(x, y), Size::new(width, height))
}

/// The origin is zero on both axes, and so is the default point.
#[test]
fn origin_is_zero_on_both_axes() {
    assert_eq!(Point::ORIGIN, Point::new(0.0, 0.0));
    assert_eq!(Point::default(), Point::ORIGIN);
}

/// A 3-4-5 triangle gives a distance of 5, in either direction.
#[test]
fn distance_is_the_straight_line_between_two_points() {
    let a = Point::new(1.0, 2.0);
    let b = Point::new(4.0, 6.0);
    assert_eq!(a.distance(b), 5.0);
    assert_eq!(b.distance(a), 5.0);
    assert_eq!(a.distance(a), 0.0);
}

/// Adding a vector moves a point, and subtracting the same vector moves it
/// back.
#[test]
fn a_vector_moves_a_point_and_back() {
    let start = Point::new(10.0, 20.0);
    let step = Vector::new(3.0, -5.0);
    assert_eq!(start + step, Point::new(13.0, 15.0));
    assert_eq!(start + step - step, start);
}

/// The difference of two points is the vector from the second to the first.
#[test]
fn two_points_differ_by_the_vector_between_them() {
    let start = Point::new(10.0, 20.0);
    let end = Point::new(13.0, 15.0);
    assert_eq!(end - start, Vector::new(3.0, -5.0));
    assert_eq!(start + (end - start), end);
}

/// Vectors subtract and scale part by part.
#[test]
fn vectors_combine_part_by_part() {
    let a = Vector::new(1.0, 2.0);
    let b = Vector::new(10.0, 20.0);
    assert_eq!(b - a, Vector::new(9.0, 18.0));
    assert_eq!(a - Vector::ZERO, a);
    assert_eq!(a * 3.0, Vector::new(3.0, 6.0));
    assert_eq!(Vector::default(), Vector::ZERO);
}

/// `+=` and `-=` move a point in place, as `+` and `-` do.
#[test]
fn a_point_moves_in_place() {
    let step = Vector::new(3.0, -5.0);
    let mut at = Point::new(10.0, 20.0);
    at += step;
    assert_eq!(at, Point::new(13.0, 15.0));
    at -= step;
    assert_eq!(at, Point::new(10.0, 20.0));
}

/// Sizes subtract and scale part by part.
#[test]
fn sizes_combine_part_by_part() {
    let big = Size::new(30.0, 40.0);
    let small = Size::new(10.0, 15.0);
    assert_eq!(big - small, Size::new(20.0, 25.0));
    assert_eq!(small * 2.0, Size::new(20.0, 30.0));
    assert_eq!(big - Size::ZERO, big);
}

/// The zero size has no width and no height, and so does the default size.
#[test]
fn zero_size_has_no_extent() {
    assert_eq!(Size::ZERO, Size::new(0.0, 0.0));
    assert_eq!(Size::default(), Size::ZERO);
}

/// A rectangle gives back the corner and size it was made from.
#[test]
fn a_rect_keeps_its_corner_and_size() {
    let r = rect(10.0, 20.0, 30.0, 40.0);
    assert_eq!(r.position(), Point::new(10.0, 20.0));
    assert_eq!(r.size(), Size::new(30.0, 40.0));
    assert_eq!((r.x, r.y, r.width, r.height), (10.0, 20.0, 30.0, 40.0));
    assert_eq!(Rect::default(), rect(0.0, 0.0, 0.0, 0.0));
}

/// The centre is halfway along each side.
#[test]
fn the_center_is_halfway_along_each_side() {
    let r = rect(10.0, 20.0, 30.0, 40.0);
    assert_eq!(r.center_x(), 25.0);
    assert_eq!(r.center_y(), 40.0);
    assert_eq!(r.center(), Point::new(25.0, 40.0));
}

/// The left and top edges are inside, and the right and bottom edges are
/// not.
#[test]
fn a_rect_holds_its_left_and_top_edges_only() {
    let r = rect(10.0, 20.0, 30.0, 40.0);
    assert!(r.contains(Point::new(25.0, 40.0)), "the middle");
    assert!(r.contains(Point::new(10.0, 20.0)), "the top-left corner");
    assert!(r.contains(Point::new(10.0, 59.0)), "the left edge");
    assert!(r.contains(Point::new(39.0, 20.0)), "the top edge");
    assert!(!r.contains(Point::new(40.0, 40.0)), "the right edge");
    assert!(!r.contains(Point::new(25.0, 60.0)), "the bottom edge");
    assert!(!r.contains(Point::new(9.0, 40.0)), "left of it");
    assert!(!r.contains(Point::new(25.0, 19.0)), "above it");
}

/// The point on the edge two tiles share belongs to exactly one of them.
#[test]
fn a_shared_edge_belongs_to_one_neighbour() {
    let left = rect(0.0, 0.0, 10.0, 10.0);
    let right = rect(10.0, 0.0, 10.0, 10.0);
    let on_edge = Point::new(10.0, 5.0);
    assert!(!left.contains(on_edge));
    assert!(right.contains(on_edge));
}

/// A rectangle with no width or no height holds no point, not even its own
/// corner.
#[test]
fn an_empty_rect_holds_no_point() {
    assert!(!rect(5.0, 5.0, 0.0, 10.0).contains(Point::new(5.0, 5.0)));
    assert!(!rect(5.0, 5.0, 10.0, 0.0).contains(Point::new(5.0, 5.0)));
}

/// Overlapping rectangles share the area between the inner edges.
#[test]
fn the_intersection_is_the_shared_area() {
    let a = rect(0.0, 0.0, 10.0, 10.0);
    let b = rect(6.0, 4.0, 10.0, 10.0);
    assert_eq!(a.intersection(b), Some(rect(6.0, 4.0, 4.0, 6.0)));
    assert_eq!(b.intersection(a), a.intersection(b));
    assert!(a.intersects(b));
}

/// A rectangle inside another shares exactly its own area with it.
#[test]
fn a_rect_inside_another_intersects_as_itself() {
    let outer = rect(0.0, 0.0, 100.0, 100.0);
    let inner = rect(20.0, 30.0, 5.0, 6.0);
    assert_eq!(outer.intersection(inner), Some(inner));
}

/// Rectangles that touch along an edge or at a corner share no area.
#[test]
fn rects_that_only_touch_do_not_intersect() {
    let a = rect(0.0, 0.0, 10.0, 10.0);
    let beside = rect(10.0, 0.0, 10.0, 10.0);
    let below = rect(0.0, 10.0, 10.0, 10.0);
    let corner = rect(10.0, 10.0, 10.0, 10.0);
    for other in [beside, below, corner] {
        assert_eq!(a.intersection(other), None);
        assert!(!a.intersects(other));
    }
}

/// Rectangles apart from each other share no area.
#[test]
fn rects_apart_do_not_intersect() {
    let a = rect(0.0, 0.0, 10.0, 10.0);
    assert!(!a.intersects(rect(50.0, 0.0, 10.0, 10.0)));
    assert!(!a.intersects(rect(0.0, 50.0, 10.0, 10.0)));
}

/// The union reaches from the outermost edge on each side.
#[test]
fn the_union_covers_both() {
    let a = rect(0.0, 5.0, 10.0, 10.0);
    let b = rect(20.0, 0.0, 5.0, 8.0);
    let both = a.union(b);
    assert_eq!(both, rect(0.0, 0.0, 25.0, 15.0));
    assert_eq!(b.union(a), both);
    assert_eq!(a.union(a), a);
}

/// Expanding moves each side out by the margin and keeps the centre.
#[test]
fn expand_moves_every_side_out() {
    let r = rect(10.0, 20.0, 30.0, 40.0);
    let grown = r.expand(2.0);
    assert_eq!(grown, rect(8.0, 18.0, 34.0, 44.0));
    assert_eq!(grown.center(), r.center());
}

/// A negative margin moves each side in, and undoes the same positive one.
#[test]
fn a_negative_margin_shrinks() {
    let r = rect(10.0, 20.0, 30.0, 40.0);
    assert_eq!(r.expand(-5.0), rect(15.0, 25.0, 20.0, 30.0));
    assert_eq!(r.expand(4.0).expand(-4.0), r);
    assert_eq!(r.expand(0.0), r);
}

/// Shrinking by more than half a side gives a negative size, not a clamped
/// one: the centre stays, and the rectangle holds and overlaps nothing.
#[test]
fn shrinking_past_the_centre_gives_a_negative_size() {
    let r = rect(10.0, 20.0, 30.0, 40.0);
    let gone = r.expand(-20.0);
    assert_eq!(gone, rect(30.0, 40.0, -10.0, 0.0));
    assert_eq!(gone.center(), r.center());
    assert!(!gone.contains(gone.center()));
    assert!(!gone.intersects(r));
    assert_eq!(gone.expand(20.0), r);
}

/// The union goes by edges, so a rectangle with no area still pulls the
/// union out to where it sits.
#[test]
fn the_union_reaches_an_empty_rect() {
    let tile = rect(0.0, 0.0, 10.0, 10.0);
    let far = Rect::new(Point::new(100.0, 50.0), Size::ZERO);
    assert_eq!(tile.union(far), rect(0.0, 0.0, 100.0, 50.0));
    assert_eq!(far.union(tile), tile.union(far));
}

/// A rectangle with a negative width is not put right first: `x` is read as
/// its left edge and `x + width` as its right, though they are swapped.
#[test]
fn the_union_uses_the_edges_of_a_negative_size_rect() {
    let tile = rect(0.0, 0.0, 10.0, 10.0);
    let inverted = rect(30.0, 0.0, -10.0, 10.0);
    assert_eq!(tile.union(inverted), rect(0.0, 0.0, 20.0, 10.0));
}

/// A part already in view needs no move.
#[test]
fn a_part_in_view_is_not_moved_to() {
    let view = rect(0.0, 100.0, 200.0, 50.0);
    assert_eq!(view.reveal(rect(10.0, 110.0, 50.0, 20.0)), Vector::new(0.0, 0.0));
    assert_eq!(view.reveal(view), Vector::new(0.0, 0.0), "a part filling it exactly");
}

/// A part above the view brings its top to the view's top; one below
/// brings its bottom to the view's bottom, as the last row does.
#[test]
fn a_part_above_or_below_is_moved_to_by_the_least() {
    let view = rect(0.0, 100.0, 200.0, 50.0);
    assert_eq!(view.reveal(rect(0.0, 80.0, 200.0, 20.0)), Vector::new(0.0, -20.0));
    assert_eq!(view.reveal(rect(0.0, 140.0, 200.0, 20.0)), Vector::new(0.0, 10.0));
    let last = rect(0.0, 480.0, 200.0, 20.0);
    assert_eq!(
        view.reveal(last),
        Vector::new(0.0, 350.0),
        "its bottom meets the view's"
    );
}

/// Each axis moves on its own: a part to the left and below moves the
/// view left and down at once.
#[test]
fn each_axis_moves_on_its_own() {
    let view = rect(100.0, 100.0, 200.0, 50.0);
    assert_eq!(view.reveal(rect(80.0, 140.0, 40.0, 20.0)), Vector::new(-20.0, 10.0));
    assert_eq!(view.reveal(rect(280.0, 90.0, 40.0, 20.0)), Vector::new(20.0, -10.0));
}

/// A part larger than the view cannot be shown whole, so its top-left
/// wins: reading begins there.
#[test]
fn a_part_larger_than_the_view_shows_its_start() {
    let view = rect(0.0, 100.0, 200.0, 50.0);
    assert_eq!(view.reveal(rect(-40.0, 60.0, 400.0, 200.0)), Vector::new(-40.0, -40.0));
    assert_eq!(view.reveal(rect(20.0, 120.0, 400.0, 200.0)), Vector::new(20.0, 20.0));
}
