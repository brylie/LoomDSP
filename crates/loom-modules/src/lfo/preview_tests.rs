use super::lfo_preview;

/// Shape changes the preview while rate leaves a single cycle alone.
#[test]
fn shape_changes_one_cycle() {
    let sine = [0.0, 1.0, 1.0, 0.0];
    let square = [0.0, 1.0, 1.0, 4.0];
    assert!((lfo_preview(&sine, 0.25) - 1.0).abs() < 1e-6);
    assert_ne!(lfo_preview(&sine, 0.1), lfo_preview(&square, 0.1));
    assert_eq!(lfo_preview(&sine, 0.1), lfo_preview(&[0.0, 10.0, 1.0, 0.0], 0.1));
}
