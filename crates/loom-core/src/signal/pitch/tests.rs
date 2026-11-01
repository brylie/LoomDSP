use super::*;

#[test]
fn reference_points() {
    assert!((to_hz(0.0) - REFERENCE_HZ).abs() < 1e-3);
    assert!((to_hz(0.1) - 2.0 * REFERENCE_HZ).abs() < 1e-2);
    // A4 = MIDI 69 = 440 Hz.
    assert!((to_hz(from_midi_note(69.0)) - 440.0).abs() < 1e-2);
    assert!((to_midi_note(from_hz(440.0)) - 69.0).abs() < 1e-3);
}
