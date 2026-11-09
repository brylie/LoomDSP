//! Points, vectors, sizes, and rectangles.
//!
//! A [`Point`] is a place and a [`Vector`] is a movement. They are separate
//! types so the compiler refuses to add two places together, which has no
//! meaning, while it allows moving a place by a movement.

use std::ops::{Add, AddAssign, Mul, Sub, SubAssign};

/// A place, as a distance from the origin along each axis.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    /// The distance to the right of the origin.
    pub x: f32,
    /// The distance below the origin.
    pub y: f32,
}

impl Point {
    /// The point where both axes are zero.
    pub const ORIGIN: Self = Self::new(0.0, 0.0);

    /// Makes a point from its two coordinates.
    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// The straight-line distance from this point to `other`.
    #[must_use]
    pub fn distance(self, other: Self) -> f32 {
        let step = other - self;
        step.x.hypot(step.y)
    }
}

/// A movement: how far to go along each axis.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vector {
    /// How far to the right.
    pub x: f32,
    /// How far down.
    pub y: f32,
}

impl Vector {
    /// The movement that goes nowhere.
    pub const ZERO: Self = Self::new(0.0, 0.0);

    /// Makes a vector from its two parts.
    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// Moving a place by a movement gives a place.
impl Add<Vector> for Point {
    type Output = Self;

    fn add(self, step: Vector) -> Self {
        Self::new(self.x + step.x, self.y + step.y)
    }
}

/// Moving a place back by a movement gives a place.
impl Sub<Vector> for Point {
    type Output = Self;

    fn sub(self, step: Vector) -> Self {
        Self::new(self.x - step.x, self.y - step.y)
    }
}

/// Moves a place by a movement, in place.
impl AddAssign<Vector> for Point {
    fn add_assign(&mut self, step: Vector) {
        *self = *self + step;
    }
}

/// Moves a place back by a movement, in place.
impl SubAssign<Vector> for Point {
    fn sub_assign(&mut self, step: Vector) {
        *self = *self - step;
    }
}

/// The movement that leads from `start` to `self`.
impl Sub for Point {
    type Output = Vector;

    fn sub(self, start: Self) -> Vector {
        Vector::new(self.x - start.x, self.y - start.y)
    }
}

/// The difference between two movements.
impl Sub for Vector {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y)
    }
}

/// A movement made longer or shorter by a factor.
impl Mul<f32> for Vector {
    type Output = Self;

    fn mul(self, factor: f32) -> Self {
        Self::new(self.x * factor, self.y * factor)
    }
}

/// How wide and how tall something is.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Size {
    /// The extent along the X axis.
    pub width: f32,
    /// The extent along the Y axis.
    pub height: f32,
}

impl Size {
    /// The size with no width and no height.
    pub const ZERO: Self = Self::new(0.0, 0.0);

    /// Makes a size from a width and a height.
    #[must_use]
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

/// How much wider and taller one size is than another.
impl Sub for Size {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self::new(self.width - other.width, self.height - other.height)
    }
}

/// A size made larger or smaller by a factor, keeping its shape.
impl Mul<f32> for Size {
    type Output = Self;

    fn mul(self, factor: f32) -> Self {
        Self::new(self.width * factor, self.height * factor)
    }
}

/// A rectangle with sides along the axes: a top-left corner and a size.
///
/// The rectangle is half-open. It holds its left and top edges, and not its
/// right and bottom edges. Tiles laid side by side then never both claim the
/// point on the edge they share.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    /// The left edge.
    pub x: f32,
    /// The top edge.
    pub y: f32,
    /// The distance from the left edge to the right edge.
    pub width: f32,
    /// The distance from the top edge to the bottom edge.
    pub height: f32,
}

impl Rect {
    /// Makes a rectangle from its top-left corner and its size.
    #[must_use]
    pub const fn new(position: Point, size: Size) -> Self {
        Self {
            x: position.x,
            y: position.y,
            width: size.width,
            height: size.height,
        }
    }

    /// The top-left corner.
    #[must_use]
    pub const fn position(&self) -> Point {
        Point::new(self.x, self.y)
    }

    /// The width and height.
    #[must_use]
    pub const fn size(&self) -> Size {
        Size::new(self.width, self.height)
    }

    /// The X coordinate halfway between the left and right edges.
    #[must_use]
    pub fn center_x(&self) -> f32 {
        self.x + self.width / 2.0
    }

    /// The Y coordinate halfway between the top and bottom edges.
    #[must_use]
    pub fn center_y(&self) -> f32 {
        self.y + self.height / 2.0
    }

    /// The point in the middle.
    #[must_use]
    pub fn center(&self) -> Point {
        Point::new(self.center_x(), self.center_y())
    }

    /// The X coordinate of the right edge, which the rectangle does not hold.
    fn right(&self) -> f32 {
        self.x + self.width
    }

    /// The Y coordinate of the bottom edge, which the rectangle does not hold.
    fn bottom(&self) -> f32 {
        self.y + self.height
    }

    /// Whether `point` is inside.
    ///
    /// A point on the left or top edge is inside. A point on the right or
    /// bottom edge is not, so a rectangle with no width or no height holds
    /// no point at all.
    #[must_use]
    pub fn contains(&self, point: Point) -> bool {
        let across = self.x <= point.x && point.x < self.right();
        let down = self.y <= point.y && point.y < self.bottom();
        across && down
    }

    /// The area both rectangles cover, or `None` when they share no area.
    ///
    /// Rectangles that only touch along an edge or at a corner share no
    /// area, which follows from the half-open rule.
    #[must_use]
    pub fn intersection(&self, other: &Self) -> Option<Self> {
        let left = self.x.max(other.x);
        let top = self.y.max(other.y);
        let width = self.right().min(other.right()) - left;
        let height = self.bottom().min(other.bottom()) - top;
        (width > 0.0 && height > 0.0).then_some(Self {
            x: left,
            y: top,
            width,
            height,
        })
    }

    /// Whether the two rectangles share some area.
    #[must_use]
    pub fn intersects(&self, other: &Self) -> bool {
        self.intersection(other).is_some()
    }

    /// The smallest rectangle that covers both.
    #[must_use]
    pub fn union(&self, other: &Self) -> Self {
        let left = self.x.min(other.x);
        let top = self.y.min(other.y);
        Self {
            x: left,
            y: top,
            width: self.right().max(other.right()) - left,
            height: self.bottom().max(other.bottom()) - top,
        }
    }

    /// The rectangle with every side moved outwards by `margin`.
    ///
    /// The centre stays where it is. A negative margin moves the sides
    /// inwards.
    ///
    /// The result is not clamped. A negative margin larger than half a side
    /// moves the two sides past each other and gives a negative width or
    /// height. Such a rectangle holds no point and overlaps nothing, by the
    /// same rules as any other, so a caller that shrinks too far draws and
    /// hits nothing, and can still expand back to where it started.
    #[must_use]
    pub fn expand(self, margin: f32) -> Self {
        Self {
            x: self.x - margin,
            y: self.y - margin,
            width: self.width + 2.0 * margin,
            height: self.height + 2.0 * margin,
        }
    }
}

#[cfg(test)]
mod tests;
