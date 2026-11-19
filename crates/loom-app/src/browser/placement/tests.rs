use super::*;

/// A tile that fits only off centre stays fully visible, including at a
/// negative patch position. Half-grid ties go up and left.
#[test]
fn nearest_visible_grid_corner() {
    let view = Rect::new(Point::new(-30.0, -30.0), Size::new(72.0, 72.0));
    assert_eq!(nearest_free(view, Size::new(12.0, 12.0), vec![]), Point::ORIGIN);
    assert_eq!(
        nearest_free(view, Size::new(60.0, 60.0), vec![]),
        Point::new(-24.0, -24.0)
    );
}

/// Even when every visible grid point is blocked, the new tile has
/// clearance. Top wins the tie between the nearest right and lower spots.
#[test]
fn a_full_view_still_leaves_the_ports_clear() {
    let view = Rect::new(Point::ORIGIN, Size::new(48.0, 48.0));
    let tile = Size::new(24.0, 24.0);
    assert_eq!(nearest_free(view, tile, vec![view]), Point::new(60.0, 24.0));
}
