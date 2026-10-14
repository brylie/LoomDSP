//! Pan and zoom.

use iced::{Point, Rectangle, Size, Vector};

/// Smallest zoom. Below this, tiles are a few pixels wide.
pub const MIN_ZOOM: f32 = 0.1;
/// Largest zoom.
pub const MAX_ZOOM: f32 = 4.0;

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

    /// Shows all of `world` inside a widget of this size, with a margin.
    /// Never zooms in past 1.0, so small patches stay at natural size.
    pub fn frame(&mut self, world: Rectangle, size: Size) {
        let margin = 40.0;
        let zoom_x = (size.width - 2.0 * margin).max(1.0) / world.width.max(1.0);
        let zoom_y = (size.height - 2.0 * margin).max(1.0) / world.height.max(1.0);
        self.zoom = zoom_x.min(zoom_y).clamp(MIN_ZOOM, 1.0);
        let centre = world.center();
        self.origin = Point::new(
            centre.x - size.width / (2.0 * self.zoom),
            centre.y - size.height / (2.0 * self.zoom),
        );
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
}
