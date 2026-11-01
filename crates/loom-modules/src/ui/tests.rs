use super::*;
use loom_core::testing::run;

/// Renders 8 samples with the parameters set to `mode`, `on`, `hold`.
fn render(mode: f32, on: f32, hold: f32) -> Vec<f32> {
    let mut module = Button::default();
    // The engine clamps and rounds before `set_param`. The module copies
    // what it is given.
    for (index, value) in [(MODE, mode), (ON, on), (HOLD, hold)] {
        module.set_param(index, BUTTON.params[index].clamp(value));
    }
    let mut out = vec![0.0; 8];
    run(&mut module, 48_000.0, &[], &[], &mut [&mut out], &[]);
    out
}

/// In Toggle mode the gate equals `on`, rounded to 0 or 1.
#[test]
fn the_toggle_gate_equals_on() {
    assert_eq!(render(0.0, 0.0, 0.0), vec![0.0; 8]);
    assert_eq!(render(0.0, 1.0, 0.0), vec![1.0; 8]);
    // 0.6 rounds to 1, and 0.4 rounds to 0, before the module sees them.
    assert_eq!(render(0.0, 0.6, 0.0), vec![1.0; 8]);
    assert_eq!(render(0.0, 0.4, 0.0), vec![0.0; 8]);
}

/// In Momentary mode the gate equals `hold`, and the latch is ignored.
#[test]
fn the_momentary_gate_equals_hold() {
    assert_eq!(render(1.0, 1.0, 0.0), vec![0.0; 8]);
    assert_eq!(render(1.0, 0.0, 1.0), vec![1.0; 8]);
}

/// A hold does not show in Toggle mode, so switching mode needs no
/// clearing (clock.md, Button details).
#[test]
fn a_hold_does_not_show_in_toggle_mode() {
    assert_eq!(render(0.0, 0.0, 1.0), vec![0.0; 8]);
}

/// The hold is transient and the latch is saved.
#[test]
fn only_the_hold_is_transient() {
    assert!(BUTTON.params[HOLD].is_transient());
    assert!(!BUTTON.params[ON].is_transient());
    assert!(!BUTTON.params[MODE].is_transient());
}

/// The gate is always 0 or 1.
#[test]
fn every_sample_is_off_or_on() {
    for &(mode, on, hold) in &[(0.0, 0.0, 0.0), (0.0, 1.0, 0.0), (1.0, 0.0, 1.0)] {
        for sample in render(mode, on, hold) {
            assert!(sample == 0.0 || sample == 1.0, "{sample}");
        }
    }
}
