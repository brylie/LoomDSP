use super::*;

fn close(a: Point, b: Point) -> bool {
    (a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3
}

#[test]
fn round_trip() {
    let camera = Camera {
        origin: Point::new(-50.0, 20.0),
        zoom: 1.7,
    };
    let p = Point::new(123.0, -45.0);
    assert!(close(camera.to_world(camera.to_screen(p)), p));
}

#[test]
fn zoom_keeps_anchor() {
    let mut camera = Camera::default();
    let screen = Point::new(300.0, 200.0);
    let before = camera.to_world(screen);
    camera.zoom_at(screen, 1.5);
    assert!(close(camera.to_world(screen), before));
    assert!((camera.zoom - 1.5).abs() < 1e-6);
}

/// `zoom_to` sets the zoom exactly and keeps the point under the
/// anchor in place, from any zoom.
#[test]
fn zoom_to_sets_the_zoom_about_the_anchor() {
    for start in [MIN_ZOOM, 0.37, 1.0, 2.9, MAX_ZOOM] {
        let mut camera = Camera {
            origin: Point::new(-80.0, 35.0),
            zoom: start,
        };
        let screen = Point::new(250.0, 120.0);
        let before = camera.to_world(screen);
        camera.zoom_to(screen, 1.0);
        assert_eq!(camera.zoom, 1.0, "from {start}");
        assert!(close(camera.to_world(screen), before), "from {start}");
    }
}

#[test]
fn zoom_is_clamped() {
    let mut camera = Camera::default();
    camera.zoom_at(Point::ORIGIN, 1000.0);
    assert_eq!(camera.zoom, MAX_ZOOM);
}

#[test]
fn frame_fits_everything() {
    let mut camera = Camera::default();
    let world = Rectangle::new(Point::new(0.0, 0.0), Size::new(4000.0, 1000.0));
    let size = Size::new(1000.0, 800.0);
    camera.frame(world, size);
    let visible = camera.visible(size);
    assert!(visible.x <= world.x && visible.y <= world.y);
    assert!(visible.x + visible.width >= world.x + world.width);
    assert!(visible.y + visible.height >= world.y + world.height);
}

/// A patch that fits at the readable zoom is framed as before.
#[test]
fn readable_framing_fits_a_small_patch_as_before() {
    let world = Rectangle::new(Point::new(10.0, 20.0), Size::new(600.0, 300.0));
    let size = Size::new(1000.0, 800.0);
    let mut fitted = Camera::default();
    fitted.frame(world, size);
    let mut readable = Camera::default();
    readable.frame_readable(world, size, 0.8);
    assert_eq!(readable, fitted);
}

/// A patch too wide to fit stays at the readable zoom and shows its
/// left edge at the margin. Its height fits, so it is centred
/// vertically.
#[test]
fn readable_framing_shows_the_left_edge_of_a_wide_patch() {
    let world = Rectangle::new(Point::new(-100.0, 0.0), Size::new(3000.0, 400.0));
    let size = Size::new(1000.0, 800.0);
    let mut camera = Camera::default();
    camera.frame_readable(world, size, 0.8);
    assert_eq!(camera.zoom, 0.8);
    let left = camera.to_screen(Point::new(world.x, world.center_y()));
    assert!((left.x - FRAME_MARGIN).abs() < 1e-3);
    assert!((left.y - size.height / 2.0).abs() < 1e-3);
}

/// A patch also too tall shows its top-left corner at the margin.
#[test]
fn readable_framing_shows_the_top_of_a_tall_patch() {
    let world = Rectangle::new(Point::new(0.0, 50.0), Size::new(3000.0, 3000.0));
    let size = Size::new(1000.0, 800.0);
    let mut camera = Camera::default();
    camera.frame_readable(world, size, 0.8);
    let corner = camera.to_screen(Point::new(world.x, world.y));
    assert!(close(corner, Point::new(FRAME_MARGIN, FRAME_MARGIN)));
}
