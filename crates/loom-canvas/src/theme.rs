//! Canvas colours.

use iced::Color;

/// Colours used by the canvas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    /// Behind everything.
    pub background: Color,
    /// Module tile body.
    pub tile: Color,
    /// Thin line around a tile.
    pub tile_border: Color,
    /// Titles.
    pub text: Color,
    /// Port labels.
    pub text_dim: Color,
    /// Outline of selected modules.
    pub selection: Color,
    /// Highlight of the port under the cursor.
    pub hover: Color,
    /// Selection box fill.
    pub marquee: Color,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            background: Color::from_rgb8(0x1C, 0x1E, 0x23),
            tile: Color::from_rgb8(0x2B, 0x2E, 0x35),
            tile_border: Color::from_rgb8(0x3C, 0x40, 0x4A),
            text: Color::from_rgb8(0xEC, 0xEE, 0xF2),
            text_dim: Color::from_rgb8(0x9A, 0xA0, 0xAD),
            selection: Color::from_rgb8(0xF5, 0xA6, 0x23),
            hover: Color::WHITE,
            marquee: Color::from_rgba8(0xF5, 0xA6, 0x23, 0.12),
        }
    }
}
