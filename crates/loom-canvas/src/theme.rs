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
    /// Modulation range arcs and dots. Distinct from the value colour and
    /// from every signal colour, so it does not read as a cable or a value.
    pub modulation: Color,
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
            modulation: Color::from_rgb8(0xE8, 0x6A, 0xC8),
        }
    }
}

/// Relative luminance of a colour, from 0 (black) to 1 (white), as defined by
/// [WCAG 2](https://www.w3.org/TR/WCAG22/#dfn-relative-luminance). Alpha is
/// ignored: pass the colour as it looks over its background.
fn relative_luminance(colour: Color) -> f32 {
    let linear = |c: f32| {
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(colour.r) + 0.7152 * linear(colour.g) + 0.0722 * linear(colour.b)
}

/// Contrast ratio between two colours, from 1 (identical) to 21 (black on
/// white). The order of the colours does not matter. WCAG success criterion
/// [1.4.3](https://www.w3.org/TR/WCAG22/#contrast-minimum) asks for at least
/// 4.5 for normal text and 3 for large text.
pub fn contrast_ratio(a: Color, b: Color) -> f32 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Smallest contrast ratio for normal-size text (WCAG 1.4.3).
pub const MIN_TEXT_CONTRAST: f32 = 4.5;

#[cfg(test)]
mod tests {
    use super::*;

    /// Black on white is 21:1, a colour on itself is 1:1, and the order does
    /// not matter. #767676 on white is the well-known 4.54:1 grey.
    #[test]
    fn contrast_ratio_matches_reference_values() {
        assert!((contrast_ratio(Color::BLACK, Color::WHITE) - 21.0).abs() < 1e-3);
        assert!((contrast_ratio(Color::WHITE, Color::BLACK) - 21.0).abs() < 1e-3);
        assert!((contrast_ratio(Color::WHITE, Color::WHITE) - 1.0).abs() < 1e-6);
        let grey = Color::from_rgb8(0x76, 0x76, 0x76);
        assert!((contrast_ratio(grey, Color::WHITE) - 4.54).abs() < 0.01);
    }

    /// Every text colour the canvas draws meets 4.5:1 on the fill under it.
    #[test]
    fn canvas_text_meets_wcag_minimum_contrast() {
        let p = Palette::default();
        let pairs = [
            ("title text on tile", p.text, p.tile),
            ("port label text on tile", p.text_dim, p.tile),
            ("option and value text on tile", p.text, p.tile),
            ("badge number on badge fill", p.background, p.modulation),
        ];
        for (name, text, fill) in pairs {
            let ratio = contrast_ratio(text, fill);
            assert!(
                ratio >= MIN_TEXT_CONTRAST,
                "{name}: {ratio:.2}:1 is below {MIN_TEXT_CONTRAST}:1"
            );
        }
    }
}
