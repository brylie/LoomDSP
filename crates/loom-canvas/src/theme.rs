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
    /// Header colours of the eight module families, in `Family::ALL` order
    /// (ADR-0029). Muted and dark, each echoing the signal its family
    /// mostly makes. They take the light [`text`](Self::text).
    pub families: [Color; 8],
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
            families: [
                Color::from_rgb8(0x62, 0x22, 0x2C), // Sources: red, like audio
                Color::from_rgb8(0x32, 0x45, 0x53), // Filters and effects: slate
                Color::from_rgb8(0x38, 0x58, 0x9F), // Modulation: blue
                Color::from_rgb8(0x46, 0x31, 0x72), // Time and rhythm: violet, like phase
                Color::from_rgb8(0x62, 0x42, 0x22), // Notes and pitch: brown, near pitch
                Color::from_rgb8(0x71, 0x6A, 0x28), // Logic and math: olive, like gates
                Color::from_rgb8(0x2A, 0x79, 0x47), // Level and routing: green
                Color::from_rgb8(0x60, 0x66, 0x76), // Interface: grey
            ],
        }
    }
}

impl Palette {
    /// The 16 swatches users and families pick from (visual-system.md,
    /// Module families): each family colour, then its tint. A tint is the
    /// colour mixed halfway to white and takes the dark
    /// [`background`](Self::background) as text.
    pub fn swatches(&self) -> [Color; 16] {
        let mut all = [Color::BLACK; 16];
        for (i, colour) in self.families.iter().enumerate() {
            all[i] = *colour;
            all[i + 8] = tint(*colour);
        }
        all
    }

    /// The colours kept for one meaning each, which no swatch may look
    /// like: editable values, modulation, lights, focus, and warnings.
    pub fn reserved(&self) -> [Color; 5] {
        [
            self.control_value,
            self.modulation,
            self.light_on,
            self.focus,
            self.warning,
        ]
    }
}

/// A swatch's lighter tint: halfway to white.
fn tint(colour: Color) -> Color {
    Color::from_rgb(
        colour.r + (1.0 - colour.r) * 0.5,
        colour.g + (1.0 - colour.g) * 0.5,
        colour.b + (1.0 - colour.b) * 0.5,
    )
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
mod tests;
