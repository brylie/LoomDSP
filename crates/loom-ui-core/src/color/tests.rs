//! Tests for colours, each against the rule its doc comment states.

use super::*;

/// The four channels of a colour, in the order red, green, blue, opacity.
fn channels(colour: Color) -> [f32; 4] {
    [colour.r, colour.g, colour.b, colour.a]
}

/// The named colours have the channels their names say.
#[test]
fn named_colours_have_their_channels() {
    assert_eq!(channels(Color::BLACK), [0.0, 0.0, 0.0, 1.0]);
    assert_eq!(channels(Color::WHITE), [1.0, 1.0, 1.0, 1.0]);
    assert_eq!(channels(Color::TRANSPARENT), [0.0, 0.0, 0.0, 0.0]);
}

/// The default colour is fully clear, so a forgotten colour draws nothing.
#[test]
fn the_default_colour_is_clear() {
    assert_eq!(Color::default(), Color::TRANSPARENT);
}

/// `rgb` is solid, and `rgba` keeps the opacity it is given. Neither
/// converts a channel.
#[test]
fn fraction_channels_are_stored_as_given() {
    assert_eq!(channels(Color::rgb(0.25, 0.5, 0.75)), [0.25, 0.5, 0.75, 1.0]);
    assert_eq!(channels(Color::rgba(0.25, 0.5, 0.75, 0.125)), [0.25, 0.5, 0.75, 0.125]);
}

/// 8-bit channels map 0 to 0.0 and 255 to 1.0, with even steps between.
#[test]
fn eight_bit_channels_span_zero_to_one() {
    assert_eq!(Color::rgb8(0, 255, 0), Color::rgb(0.0, 1.0, 0.0));
    let colour = Color::rgb8(51, 102, 204);
    assert_eq!(channels(colour), [51.0 / 255.0, 102.0 / 255.0, 204.0 / 255.0, 1.0]);
}

/// `rgba8` scales the colour channels and leaves the opacity as given.
#[test]
fn eight_bit_colour_keeps_its_opacity() {
    assert_eq!(Color::rgba8(255, 0, 255, 0.5), Color::rgba(1.0, 0.0, 1.0, 0.5));
}

/// The constructors work in a `const`, so a palette can be a constant.
#[test]
fn colours_can_be_constants() {
    const ACCENT: Color = Color::rgb8(255, 128, 0);
    assert_eq!(ACCENT.r, 1.0);
    assert_eq!(ACCENT.a, 1.0);
}
