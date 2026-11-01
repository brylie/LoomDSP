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
mod tests;
