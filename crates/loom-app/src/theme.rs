//! App colours and checks for the text pairs used by the app and canvas.

use iced::Color;

/// The app palette, shared by the view and its contrast checks.
pub const PALETTE: iced::theme::Palette = iced::theme::Palette {
    background: Color::from_rgb8(0x22, 0x24, 0x2A),
    text: Color::from_rgb8(0xE6, 0xE8, 0xEE),
    primary: Color::from_rgb8(0xF5, 0xA6, 0x23),
    success: Color::from_rgb8(0x4C, 0xB0, 0x6E),
    warning: Color::from_rgb8(0xE8, 0xD2, 0x4A),
    danger: Color::from_rgb8(0xE0, 0x4F, 0x4F),
};

/// Secondary text, including off assignments in the inspector's Drives list.
pub const DIM: Color = Color::from_rgb(0.62, 0.65, 0.71);

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG relative luminance for an opaque sRGB colour.
    /// See <https://www.w3.org/WAI/WCAG21/Understanding/contrast-minimum.html#dfn-relative-luminance>.
    fn relative_luminance(colour: Color) -> f64 {
        /// Convert an sRGB channel to linear light before weighting it.
        fn linear(channel: f32) -> f64 {
            let value = f64::from(channel);
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * linear(colour.r) + 0.7152 * linear(colour.g) + 0.0722 * linear(colour.b)
    }

    /// WCAG contrast, with the lighter luminance above the darker one.
    fn contrast_ratio(first: Color, second: Color) -> f64 {
        let first = relative_luminance(first);
        let second = relative_luminance(second);
        (first.max(second) + 0.05) / (first.min(second) + 0.05)
    }

    /// Check the actual, opaque text pair without rounding the 4.5 threshold.
    fn assert_text_contrast(name: &str, text: Color, background: Color) {
        assert_eq!((text.a, background.a), (1.0, 1.0), "{name}: colours must be opaque");
        let ratio = contrast_ratio(text, background);
        assert!(ratio >= 4.5, "{name}: contrast {ratio:.4}:1 is below 4.5:1");
    }

    /// Black, white and primary colours check the range and channel weights.
    #[test]
    fn wcag_reference_colours() {
        assert_eq!(relative_luminance(Color::BLACK), 0.0);
        assert_eq!(relative_luminance(Color::WHITE), 1.0);
        assert_eq!(relative_luminance(Color::from_rgb(1.0, 0.0, 0.0)), 0.2126);
        assert_eq!(relative_luminance(Color::from_rgb(0.0, 1.0, 0.0)), 0.7152);
        assert_eq!(relative_luminance(Color::from_rgb(0.0, 0.0, 1.0)), 0.0722);
        assert_eq!(contrast_ratio(Color::BLACK, Color::WHITE), 21.0);
        assert_eq!(contrast_ratio(Color::WHITE, Color::BLACK), 21.0);
        assert_eq!(contrast_ratio(DIM, DIM), 1.0);
    }

    /// Dark and middle grey exercise both sRGB conversion branches.
    #[test]
    fn srgb_greys_are_linearised() {
        let dark = relative_luminance(Color::from_rgb(0.02, 0.02, 0.02));
        let middle = relative_luminance(Color::from_rgb(0.5, 0.5, 0.5));
        assert!((dark - 0.0015479876160990713).abs() < 1e-9);
        assert!((middle - 0.21404114048223255).abs() < 1e-12);
    }

    /// Guard the text/fill pairs used by the inspector, tiles and badges.
    #[test]
    fn text_colours_meet_wcag_minimum() {
        let canvas = loom_canvas::Palette::default();
        for (name, text, background) in [
            ("app dim / app background (Drives)", DIM, PALETTE.background),
            ("app text / app background", PALETTE.text, PALETTE.background),
            ("canvas text / tile", canvas.text, canvas.tile),
            ("canvas dim label / tile", canvas.text_dim, canvas.tile),
            // gpu.rs fills count badges with modulation; widget.rs draws
            // their numbers in the canvas background colour.
            ("canvas badge number / badge fill", canvas.background, canvas.modulation),
        ] {
            assert_text_contrast(name, text, background);
        }
    }
}
