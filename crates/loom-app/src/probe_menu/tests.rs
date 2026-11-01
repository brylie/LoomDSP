use super::*;

/// A free spot keeps the corner beside the anchor, with the input at
/// the anchor's height when that lies on the grid.
#[test]
fn places_beside_the_anchor() {
    let anchor = Point::new(132.0, 30.0);
    let corner = place(&loom_modules::display::SCOPE, anchor, &[]);
    assert_eq!(corner, Point::new(168.0, 0.0));
}

/// A tile in the way pushes the new module down, in steps of 2 grid
/// units, until nothing overlaps.
#[test]
fn moves_down_past_tiles() {
    let anchor = Point::new(132.0, 30.0);
    let blocker = Rectangle::new(Point::new(200.0, 0.0), Size::new(132.0, 60.0));
    let corner = place(&loom_modules::display::METER, anchor, &[blocker]);
    assert_eq!(corner, Point::new(168.0, 72.0));
    let descriptor = &loom_modules::display::METER;
    let size = tile_size(1, 1, descriptor.panel.width, descriptor.panel.height);
    assert!(!blocker.intersects(&Rectangle::new(corner, size)));
}

/// Tiles stacked below each other push the module past all of them.
#[test]
fn moves_down_past_a_stack_of_tiles() {
    let anchor = Point::new(132.0, 30.0);
    let column = |y: f32| Rectangle::new(Point::new(200.0, y), Size::new(132.0, 60.0));
    let tiles = [column(0.0), column(60.0), column(120.0)];
    let corner = place(&loom_modules::display::METER, anchor, &tiles);
    assert_eq!(corner, Point::new(168.0, 192.0));
}

/// Far from the origin, where f32 cannot add a step, placing still ends.
#[test]
fn placing_far_away_ends() {
    let anchor = Point::new(132.0, 1.0e9);
    let blocker = Rectangle::new(Point::new(150.0, 1.0e9 - 100.0), Size::new(132.0, 400.0));
    let corner = place(&loom_modules::display::SCOPE, anchor, &[blocker]);
    assert!(corner.y.is_finite());
}
