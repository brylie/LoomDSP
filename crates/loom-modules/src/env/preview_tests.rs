use super::envelope_preview;

/// The preview follows mode and sustain without changing the engine.
#[test]
fn mode_and_sustain_shape() {
    let adsr = [0.1, 0.1, 0.6, 0.1, 0.0, 0.0];
    let ad = [0.1, 0.1, 0.6, 0.1, 1.0, 0.0];
    assert_eq!(envelope_preview(&adsr, 0.0), 0.0);
    assert_eq!(envelope_preview(&adsr, 1.0), 0.0);
    assert!((envelope_preview(&adsr, 0.7) - 0.6).abs() < 0.01);
    assert_eq!(envelope_preview(&ad, 1.0), 0.0);
    assert_ne!(envelope_preview(&adsr, 0.5), envelope_preview(&ad, 0.5));
}

/// AD never releases, so the hidden Release knob leaves its view alone.
#[test]
fn release_does_not_change_the_ad_shape() {
    let short = [0.01, 0.1, 0.6, 0.01, 1.0, 0.0];
    let long = [0.01, 0.1, 0.6, 5.0, 1.0, 0.0];
    for x in [0.02, 0.05, 0.1, 0.5, 0.9] {
        assert_eq!(envelope_preview(&short, x), envelope_preview(&long, x), "at {x}");
    }
}
