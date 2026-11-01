use super::ladder_preview;

/// Cutoff moves the schematic edge and slope changes the high end.
#[test]
fn cutoff_and_slope_change_shape() {
    let low = [200.0, 0.0, 0.0, 0.0];
    let high = [2_000.0, 0.0, 0.0, 0.0];
    assert!(ladder_preview(&high, 0.6) > ladder_preview(&low, 0.6));
    assert!(ladder_preview(&low, 0.5) > ladder_preview(&[200.0, 0.0, 0.0, 2.0], 0.5));
}
