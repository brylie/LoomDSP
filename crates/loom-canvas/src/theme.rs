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
    /// Bars standing in for titles too small to draw.
    pub title_bar: Color,
    /// Bars standing in for port labels and captions too small to draw.
    pub text_bar: Color,
    /// Outline of selected modules.
    pub selection: Color,
    /// Highlight of the port under the cursor.
    pub hover: Color,
    /// Selection box fill.
    pub marquee: Color,
    /// Knob ring and toggle track when off.
    pub control_track: Color,
    /// Knob value arc, toggle when on, chosen radio button.
    pub control_value: Color,
    /// Knob body.
    pub control_body: Color,
    /// A light at brightness 0.
    pub light_off: Color,
    /// A light at brightness 1.
    pub light_on: Color,
    /// Ring around the control that has keyboard focus.
    pub focus: Color,
    /// A meter in its loudest zone.
    pub warning: Color,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            background: Color::from_rgb8(0x1C, 0x1E, 0x23),
            tile: Color::from_rgb8(0x2B, 0x2E, 0x35),
            tile_border: Color::from_rgb8(0x3C, 0x40, 0x4A),
            text: Color::from_rgb8(0xEC, 0xEE, 0xF2),
            text_dim: Color::from_rgb8(0x9A, 0xA0, 0xAD),
            title_bar: Color::from_rgba8(0xEC, 0xEE, 0xF2, 0.4),
            text_bar: Color::from_rgba8(0x9A, 0xA0, 0xAD, 0.3),
            selection: Color::from_rgb8(0xF5, 0xA6, 0x23),
            hover: Color::WHITE,
            marquee: Color::from_rgba8(0xF5, 0xA6, 0x23, 0.12),
            control_track: Color::from_rgb8(0x45, 0x49, 0x54),
            control_value: Color::from_rgb8(0xF5, 0xA6, 0x23),
            control_body: Color::from_rgb8(0x3A, 0x3E, 0x47),
            light_off: Color::from_rgb8(0x35, 0x38, 0x40),
            light_on: Color::from_rgb8(0x7C, 0xE0, 0x8A),
            focus: Color::from_rgb8(0x6C, 0xB4, 0xF0),
            warning: Color::from_rgb8(0xE0, 0x4F, 0x4F),
        }
    }
}
