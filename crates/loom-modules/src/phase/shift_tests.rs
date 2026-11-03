use super::*;
use loom_core::testing::run;

/// Runs a module of `descriptor` over the inputs in one block, with its
/// parameters at `params`, and returns its first output.
fn process(descriptor: &ModuleDescriptor, params: &[(usize, f32)], inputs: &[&[f32]]) -> Vec<f32> {
    let ModuleRole::Dsp(create) = descriptor.role else {
        unreachable!()
    };
    let mut m = create(&ModuleConfig {
        sample_rate: 48_000.0,
        max_block: inputs[0].len(),
    });
    for &(index, value) in params {
        m.set_param(index, value);
    }
    m.reset();
    let mut out = vec![0.0; inputs[0].len()];
    let connected = vec![true; inputs.len()];
    run(m.as_mut(), 48_000.0, inputs, &connected, &mut [&mut out], &[]);
    out
}

/// A phase moving one cycle in 1000 frames, up or down, from 0.
fn slow_phase(falling: bool) -> Vec<f32> {
    (0..1_000)
        .map(|i| {
            let t = i as f32 / 1_000.0;
            if falling { 1.0 - t } else { t }
        })
        .collect()
}

/// The frames where Phase Trigger fires on a shift of `offset` over `phase`.
fn fires(phase: &[f32], offset: f32) -> Vec<usize> {
    let silent = vec![0.0; phase.len()];
    let shifted = process(&SHIFT, &[(PhaseShift::OFFSET, offset)], &[phase, &silent]);
    let fired = process(&PHASE_TRIGGER, &[], &[&shifted]);
    (0..fired.len()).filter(|&i| fired[i] > 0.5).collect()
}

/// Shift by -0.3, then Phase Trigger, fires when a rising phase passes 0.3
/// (spec, acceptance).
#[test]
fn a_shift_fires_at_its_point_on_a_rising_phase() {
    assert_eq!(fires(&slow_phase(false), -0.3), [300]);
}

/// The same when the phase falls past 0.3 (spec, acceptance). Frame 700 lands
/// on 0.3, and frame 701 is the first below it.
#[test]
fn a_shift_fires_at_its_point_on_a_falling_phase() {
    assert_eq!(fires(&slow_phase(true), -0.3), [701]);
}

/// The output is the phase plus the knob and the input, wrapped.
#[test]
fn the_knob_and_the_input_add() {
    let out = process(&SHIFT, &[(PhaseShift::OFFSET, 0.25)], &[&[0.5, 0.9], &[0.125, -0.25]]);
    assert_eq!(out[0], 0.875);
    assert!((out[1] - 0.9).abs() < 1e-6, "{out:?}");
}

/// A bad phase gives 0, and a bad offset input counts as no offset. Neither
/// reaches a later module as NaN.
#[test]
fn non_finite_inputs_stay_out() {
    let out = process(&SHIFT, &[], &[&[f32::NAN, 0.5, 0.5], &[0.0, f32::NAN, f32::INFINITY]]);
    assert_eq!(out, [0.0, 0.5, 0.5]);
}

/// A huge phase or offset leaves a value inside 0 to 1, not an overflow.
#[test]
fn huge_inputs_stay_in_range() {
    let out = process(
        &SHIFT,
        &[],
        &[&[f32::MAX, -f32::MAX, 0.5], &[f32::MAX, f32::MAX, -f32::MAX]],
    );
    assert!(out.iter().all(|value| (0.0..1.0).contains(value)), "{out:?}");
}

/// Reverse turns 0 into 0 and 0.25 into 0.75 (spec, acceptance).
#[test]
fn reverse_turns_the_phase_round() {
    assert_eq!(
        process(&REVERSE, &[], &[&[0.0, 0.25, 0.75, 1.25]]),
        [0.0, 0.75, 0.25, 0.75]
    );
}

/// A reversed phase still fires on its first frame when it starts at 0, by
/// Phase Trigger's first-frame rule (spec).
#[test]
fn reverse_keeps_the_first_frame_rule() {
    let reversed = process(&REVERSE, &[], &[&[0.0, 0.1, 0.2]]);
    assert_eq!(process(&PHASE_TRIGGER, &[], &[&reversed])[0], 1.0);
}

/// A bad phase gives 0, and a huge one stays in range.
#[test]
fn reverse_keeps_bad_values_out() {
    assert_eq!(process(&REVERSE, &[], &[&[f32::NAN, f32::INFINITY]]), [0.0, 0.0]);
    let out = process(&REVERSE, &[], &[&[f32::MAX, -f32::MAX]]);
    assert!(out.iter().all(|value| (0.0..1.0).contains(value)), "{out:?}");
}
