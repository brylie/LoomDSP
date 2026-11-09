//! Colours as four plain channels.

/// A colour: red, green, blue, and how opaque it is.
///
/// Each channel runs from 0 to 1. The type stores the channels as given and
/// converts nothing. Whether they are read as sRGB or as linear light is up
/// to the renderer that draws them, which is the part that knows its target.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Color {
    /// Red, from 0 to 1.
    pub r: f32,
    /// Green, from 0 to 1.
    pub g: f32,
    /// Blue, from 0 to 1.
    pub b: f32,
    /// Opacity, from 0 (clear) to 1 (solid).
    pub a: f32,
}

impl Color {
    /// Solid black.
    pub const BLACK: Self = Self::rgb(0.0, 0.0, 0.0);
    /// Solid white.
    pub const WHITE: Self = Self::rgb(1.0, 1.0, 1.0);
    /// Fully clear, so nothing is drawn.
    pub const TRANSPARENT: Self = Self::rgba(0.0, 0.0, 0.0, 0.0);

    /// A solid colour from channels between 0 and 1.
    #[must_use]
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self::rgba(r, g, b, 1.0)
    }

    /// A colour from channels and an opacity, all between 0 and 1.
    #[must_use]
    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// A solid colour from 8-bit channels, as design tools and hex codes
    /// give them: 0 is none and 255 is full.
    #[must_use]
    pub const fn rgb8(r: u8, g: u8, b: u8) -> Self {
        Self::rgba8(r, g, b, 1.0)
    }

    /// A colour from 8-bit channels and an opacity between 0 and 1.
    #[must_use]
    pub const fn rgba8(r: u8, g: u8, b: u8, a: f32) -> Self {
        Self::rgba(channel(r), channel(g), channel(b), a)
    }
}

/// An 8-bit channel as a fraction of full: 0 gives 0.0 and 255 gives 1.0.
const fn channel(value: u8) -> f32 {
    value as f32 / 255.0
}

#[cfg(test)]
mod tests;
