use super::*;
use loom_core::testing::run;

/// Runs a one-input module settled at `params` over `input`.
fn process(descriptor: &ModuleDescriptor, params: &[(usize, f32)], input: &[f32]) -> Vec<f32> {
    let ModuleRole::Dsp(create) = descriptor.role else {
        unreachable!()
    };
    let mut m = create(&loom_core::ModuleConfig {
        sample_rate: 48_000.0,
        max_block: input.len(),
    });
    for &(index, value) in params {
        m.set_param(index, value);
    }
    m.reset();
    let mut out = vec![0.0; input.len()];
    run(m.as_mut(), 48_000.0, &[input], &[true], &mut [&mut out], &[]);
    out
}

/// Level Constant outputs 1.0 at 0 dB and 0.5012 at -6 dB.
#[test]
fn constant_level_in_decibels() {
    for (db, expected) in [(0.0, 1.0), (-6.0, 0.5012), (12.0, 3.981), (-60.0, 0.001)] {
        let out = process(&CONSTANT, &[(0, db)], &[0.0; 4]);
        assert!(
            out.iter().all(|v| (v - expected).abs() < expected * 1e-3),
            "{db} dB: {out:?}"
        );
    }
}

/// A NaN limit is ignored, crossed limits swap, and nothing panics.
#[test]
fn limit_never_panics_on_nan_or_crossed_limits() {
    assert_eq!(limit(5.0, f32::NAN, 1.0), 1.0);
    assert_eq!(limit(-5.0, -1.0, f32::NAN), -1.0);
    // A NaN limit is open: it does not pin the output to the other one.
    assert_eq!(limit(0.25, f32::NAN, 1.0), 0.25);
    assert_eq!(limit(0.25, -1.0, f32::NAN), 0.25);
    assert_eq!(limit(5.0, f32::NAN, f32::NAN), 5.0);
    assert_eq!(limit(f32::NAN, f32::NAN, 1.0), 0.0);
    assert_eq!(limit(5.0, 1.0, -1.0), 1.0);
    assert_eq!(limit(f32::INFINITY, -1.0, 1.0), 1.0);
}

/// Clamp limits the signal, swaps crossed limits, and turns NaN into a
/// value inside the limits.
#[test]
fn clamp_limits_swaps_and_cleans_nan() {
    let input = [-5.0, -0.5, 0.5, 5.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY];
    assert_eq!(
        process(&CLAMP, &[(0, -1.0), (1, 1.0)], &input),
        [-1.0, -0.5, 0.5, 1.0, 0.0, 1.0, -1.0]
    );
    // Crossed limits give the same result as the right way round.
    assert_eq!(
        process(&CLAMP, &[(0, 1.0), (1, -1.0)], &input),
        [-1.0, -0.5, 0.5, 1.0, 0.0, 1.0, -1.0]
    );
    // NaN lands inside limits that do not contain 0.
    assert_eq!(process(&CLAMP, &[(0, 2.0), (1, 3.0)], &[f32::NAN]), [2.0]);
}

/// Scale inverts at factor -1 and maps bipolar to unipolar.
#[test]
fn scale_inverts_and_offsets() {
    let input = [-1.0, 0.0, 0.5, 1.0];
    assert_eq!(process(&SCALE, &[(0, -1.0)], &input), [1.0, 0.0, -0.5, -1.0]);
    assert_eq!(process(&SCALE, &[(0, 0.5), (1, 0.5)], &input), [0.0, 0.5, 0.75, 1.0]);
}
