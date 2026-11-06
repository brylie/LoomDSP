use super::*;

#[test]
fn polarity_remap_follows_the_override() {
    assert_eq!(Polarity::Bipolar.remap(Polarity::Bipolar, -0.4), -0.4);
    assert_eq!(Polarity::Unipolar.remap(Polarity::Bipolar, -1.0), 0.0);
    assert_eq!(Polarity::Unipolar.remap(Polarity::Bipolar, 1.0), 1.0);
    assert_eq!(Polarity::Bipolar.remap(Polarity::Unipolar, 0.0), -1.0);
    assert_eq!(Polarity::Bipolar.remap(Polarity::Unipolar, 1.0), 1.0);
    // A source past the nominal range is kept. Only the parameter is clamped.
    assert_eq!(Polarity::Bipolar.remap(Polarity::Bipolar, 3.0), 3.0);
}
