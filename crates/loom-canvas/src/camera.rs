//! Pan and zoom.

use iced::{Point, Rectangle, Size, Vector};

/// Smallest zoom. Below this, tiles are a few pixels wide.
pub const MIN_ZOOM: f32 = 0.1;
/// Largest zoom.
pub const MAX_ZOOM: f32 = 4.0;
/// Space kept around a framed patch, in logical pixels.
const FRAME_MARGIN: f32 = 40.0;

/// Maps world coordinates to positions inside the canvas widget.
///
/// `screen = (world - origin) * zoom`, in logical pixels relative to the
/// widget's top-left corner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// World point shown at the widget's top-left corner.
    pub origin: Point,
    /// Logical pixels per world unit.
    pub zoom: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            origin: Point::ORIGIN,
            zoom: 1.0,
        }
    }
}

impl Camera {
    /// World point to widget-relative screen point.
    pub fn to_screen(&self, world: Point) -> Point {
        Point::new(
            (world.x - self.origin.x) * self.zoom,
            (world.y - self.origin.y) * self.zoom,
        )
    }

    /// Widget-relative screen point to world point.
    pub fn to_world(&self, screen: Point) -> Point {
        Point::new(
            screen.x / self.zoom + self.origin.x,
            screen.y / self.zoom + self.origin.y,
        )
    }

    /// The world rectangle visible in a widget of this size.
    pub fn visible(&self, size: Size) -> Rectangle {
        Rectangle::new(self.origin, Size::new(size.width / self.zoom, size.height / self.zoom))
    }

    /// Moves the view by a screen offset (for example a mouse drag).
    pub fn pan(&mut self, screen_delta: Vector) {
        self.origin -= screen_delta * (1.0 / self.zoom);
    }

    /// Zooms by `factor`, keeping the world point under `screen` in place.
    pub fn zoom_at(&mut self, screen: Point, factor: f32) {
        let anchor = self.to_world(screen);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        self.origin = Point::new(anchor.x - screen.x / self.zoom, anchor.y - screen.y / self.zoom);
    }

    /// Sets the zoom to `zoom`, keeping the world point under `screen` in
    /// place. Cmd/Ctrl+0 uses it to go back to 100%.
    pub fn zoom_to(&mut self, screen: Point, zoom: f32) {
        let anchor = self.to_world(screen);
        // Set exactly, not by a factor, so 100% is exactly 1.0.
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        self.origin = Point::new(anchor.x - screen.x / self.zoom, anchor.y - screen.y / self.zoom);
    }

    /// Shows all of `world` inside a widget of this size, with a margin.
    /// Never zooms in past 1.0, so small patches stay at natural size.
    pub fn frame(&mut self, world: Rectangle, size: Size) {
        let zoom_x = (size.width - 2.0 * FRAME_MARGIN).max(1.0) / world.width.max(1.0);
        let zoom_y = (size.height - 2.0 * FRAME_MARGIN).max(1.0) / world.height.max(1.0);
        self.zoom = zoom_x.min(zoom_y).clamp(MIN_ZOOM, 1.0);
        let centre = world.center();
        self.origin = Point::new(
            centre.x - size.width / (2.0 * self.zoom),
            centre.y - size.height / (2.0 * self.zoom),
        );
    }

    /// Like [`frame`](Self::frame), but never below `readable`, the zoom
    /// where port labels and captions are drawn. A patch too big to fit
    /// at that zoom shows its left edge, where its sources are placed, and
    /// its top edge if it is also too tall; the user pans to the rest.
    /// *Why:* a patch that opens with every label turned into a bar shows
    /// a new user no words at all (panels.md, Level of detail).
    pub fn frame_readable(&mut self, world: Rectangle, size: Size, readable: f32) {
        self.frame(world, size);
        if self.zoom >= readable {
            return;
        }
        self.zoom = readable.clamp(MIN_ZOOM, MAX_ZOOM);
        let left = world.x - FRAME_MARGIN / self.zoom;
        let top = if world.height * self.zoom + 2.0 * FRAME_MARGIN <= size.height {
            world.center_y() - size.height / (2.0 * self.zoom)
        } else {
            world.y - FRAME_MARGIN / self.zoom
        };
        self.origin = Point::new(left, top);
    }
}

#[cfg(test)]
mod tests {
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
}
