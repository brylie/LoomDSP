use super::*;
use loom_core::testing::run;

/// Runs the slew over `input` with the given rise and fall times.
fn slew(rise: f32, fall: f32, input: &[f32]) -> Vec<f32> {
    let ModuleRole::Dsp(create) = SLEW.role else {
        unreachable!()
    };
    let mut m = create(&loom_core::ModuleConfig {
        sample_rate: 48_000.0,
        max_block: input.len(),
    });
    m.set_param(0, rise);
    m.set_param(1, fall);
    m.reset();
    let mut out = vec![0.0; input.len()];
    run(m.as_mut(), 48_000.0, &[input], &[true], &mut [&mut out], &[]);
    out
}

/// Going up by 1.0 takes `rise` seconds, down takes `fall`, and the
/// output stops on arrival.
#[test]
fn rises_and_falls_at_a_constant_rate() {
    // 10 ms at 48 kHz is 480 samples per 1.0 of change.
    let mut input = vec![0.0];
    input.extend(vec![1.0; 600]);
    let out = slew(0.01, 0.02, &input);
    assert_eq!(out[0], 0.0);
    assert!((out[240] - 0.5).abs() < 1e-3, "{}", out[240]);
    assert!((out[480] - 1.0).abs() < 1e-3, "{}", out[480]);
    assert_eq!(out[600], 1.0, "stops on arrival");
    // Fall by 1.0 in 20 ms, so 0.5 at 480 samples.
    let mut input = vec![1.0];
    input.extend(vec![0.0; 1_000]);
    let out = slew(0.01, 0.02, &input);
    assert_eq!(out[0], 1.0);
    assert!((out[480] - 0.5).abs() < 1e-3, "{}", out[480]);
}

/// A long slew of a large value keeps moving: at 100, a step of 2e-6 per
/// sample is lost in an `f32`, but not in the `f64` the slew keeps.
#[test]
fn a_long_slew_of_a_large_value_does_not_freeze() {
    // 10 s per 1.0 is a step of 1 / 480,000 per sample.
    let mut input = vec![100.0];
    input.extend(vec![200.0; 3_000]);
    let out = slew(10.0, 10.0, &input);
    let moved = f64::from(out[3_000]) - 100.0;
    let expected = 3_000.0 / 480_000.0;
    assert!((moved - expected).abs() < expected * 0.1, "{moved} vs {expected}");
}

/// A new slew starts at its input instead of sweeping from 0.
#[test]
fn starts_at_its_input() {
    let out = slew(1.0, 1.0, &[0.7; 4]);
    assert_eq!(out, [0.7; 4]);
}
