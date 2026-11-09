//! Conversions between `loom-ui-core` values and Iced's.
//!
//! The canvas and the app keep geometry and colours in `loom-ui-core` types.
//! A value changes type only where it crosses into or out of an Iced call,
//! and it does so through this one module. That gives the move away from
//! Iced a single place to delete (ADR-0041,
//! `docs/dev/leaving-iced-plan.md`, step 1.1).
//!
//! These are extension traits and not `From` impls. Rust allows a `From`
//! impl only in a crate that owns one of the two types, and this crate owns
//! neither. `loom-ui-core` must not own them either, since it depends on no
//! toolkit.

use loom_ui_core::{Color, Point, Rect, Size, Vector};

/// Turns a `loom-ui-core` value into the Iced value with the same fields.
pub trait ToIced {
    /// The Iced type this value becomes.
    type Iced;

    /// The same value as an Iced type, field for field.
    fn to_iced(self) -> Self::Iced;
}

/// Turns an Iced value into the `loom-ui-core` value with the same fields.
pub trait ToCore {
    /// The `loom-ui-core` type this value becomes.
    type Core;

    /// The same value as a `loom-ui-core` type, field for field.
    fn to_core(self) -> Self::Core;
}

impl ToIced for Point {
    type Iced = iced::Point;

    fn to_iced(self) -> iced::Point {
        iced::Point { x: self.x, y: self.y }
    }
}

impl ToCore for iced::Point {
    type Core = Point;

    fn to_core(self) -> Point {
        Point { x: self.x, y: self.y }
    }
}

impl ToIced for Vector {
    type Iced = iced::Vector;

    fn to_iced(self) -> iced::Vector {
        iced::Vector { x: self.x, y: self.y }
    }
}

impl ToCore for iced::Vector {
    type Core = Vector;

    fn to_core(self) -> Vector {
        Vector { x: self.x, y: self.y }
    }
}

impl ToIced for Size {
    type Iced = iced::Size;

    fn to_iced(self) -> iced::Size {
        iced::Size {
            width: self.width,
            height: self.height,
        }
    }
}

impl ToCore for iced::Size {
    type Core = Size;

    fn to_core(self) -> Size {
        Size {
            width: self.width,
            height: self.height,
        }
    }
}

impl ToIced for Rect {
    type Iced = iced::Rectangle;

    fn to_iced(self) -> iced::Rectangle {
        iced::Rectangle {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
        }
    }
}

impl ToCore for iced::Rectangle {
    type Core = Rect;

    fn to_core(self) -> Rect {
        Rect {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
        }
    }
}

impl ToIced for Color {
    type Iced = iced::Color;

    fn to_iced(self) -> iced::Color {
        iced::Color {
            r: self.r,
            g: self.g,
            b: self.b,
            a: self.a,
        }
    }
}

impl ToCore for iced::Color {
    type Core = Color;

    fn to_core(self) -> Color {
        Color {
            r: self.r,
            g: self.g,
            b: self.b,
            a: self.a,
        }
    }
}

#[cfg(test)]
mod tests;
