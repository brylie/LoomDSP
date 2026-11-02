use super::*;
use loom_core::testing::run;

/// Runs Phase Reset over the inputs in one block.
fn process(phase: &[f32], reset: &[f32]) -> Vec<f32> {
    let ModuleRole::Dsp(create) = RESET.role else {
        unreachable!()
    };
    let mut m = create(&ModuleConfig {
        sample_rate: 48_000.0,
        max_block: phase.len(),
    });
    let mut out = vec![0.0; phase.len()];
    run(
        m.as_mut(),
        48_000.0,
        &[phase, reset],
        &[true, true],
        &mut [&mut out],
        &[],
    );
    out
}

/// With no reset, the input passes through unchanged.
#[test]
fn no_reset_passes_the_input_through() {
    let phase = [0.1, 0.4, 0.9, 0.2];
    assert_eq!(process(&phase, &[0.0; 4]), phase);
}

/// After a reset, the output rises at the input's rate from 0, and wraps
/// when it reaches 1, not when the input does.
#[test]
fn after_a_reset_the_output_rises_at_the_input_rate_and_wraps() {
    let phase = [0.0, 0.25, 0.5, 0.75, 0.0, 0.25, 0.5, 0.75];
    let reset = [0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    let out = process(&phase, &reset);
    assert_eq!(out, [0.0, 0.25, 0.0, 0.25, 0.5, 0.75, 0.0, 0.25]);
}

/// A reset that is high on the first frame counts.
#[test]
fn a_reset_on_the_first_frame_counts() {
    let out = process(&[0.3, 0.4], &[1.0, 1.0]);
    assert_eq!(out[0], 0.0);
    assert!((out[1] - 0.1).abs() < 1e-6, "{}", out[1]);
}

/// Only the rising edge restarts: a held reset lets the phase run on.
#[test]
fn a_held_reset_restarts_once() {
    let out = process(&[0.25, 0.5, 0.75], &[0.0, 1.0, 1.0]);
    assert_eq!(out, [0.25, 0.0, 0.25]);
}

/// A jump in the input moves the output by the same amount.
#[test]
fn a_jump_in_the_input_moves_the_output_by_the_same_amount() {
    let out = process(&[0.25, 0.25, 0.75], &[1.0, 0.0, 0.0]);
    assert_eq!(out, [0.0, 0.0, 0.5]);
}

/// A non-finite input gives 0 and never becomes the offset, even when a
/// reset lands on the same frame.
#[test]
fn a_non_finite_input_gives_zero_and_is_not_kept() {
    let out = process(&[f32::NAN, 0.25, f32::INFINITY, 0.5], &[1.0, 0.0, 0.0, 0.0]);
    assert_eq!(out, [0.0, 0.25, 0.0, 0.5]);
}

/// Finite inputs of opposite sign can overflow when subtracted. The output
/// is still a phase, and the next ordinary input recovers.
#[test]
fn opposite_extremes_do_not_produce_nan() {
    let out = process(&[-f32::MAX, f32::MAX, 0.25, 0.5], &[1.0, 0.0, 0.0, 1.0]);
    assert_eq!(out[1], 0.0);
    assert!(out.iter().all(|v| (0.0..1.0).contains(v)), "{out:?}");
    assert_eq!(out[3], 0.0);
}
