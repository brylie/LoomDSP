use super::*;

/// Signals that fade and signals that switch, by type (rule 9).
#[test]
fn audio_modulation_and_pitch_fade_and_gates_and_phases_switch() {
    assert!(fades(SignalType::Audio));
    assert!(fades(SignalType::Modulation));
    assert!(fades(SignalType::Pitch));
    assert!(!fades(SignalType::Gate));
    assert!(!fades(SignalType::Phase));
}
