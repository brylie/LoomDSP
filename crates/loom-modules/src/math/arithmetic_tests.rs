use super::*;
use loom_core::testing::run;

/// Runs a module over `a` and `b`, with `b` cabled or not.
fn apply(module: &mut dyn Module, a: &[f32], b: &[f32], b_connected: bool) -> Vec<f32> {
    let mut out = vec![0.0; a.len()];
    run(module, 48_000.0, &[a, b], &[true, b_connected], &mut [&mut out], &[]);
    out
}

/// 3 minus 5 gives -2.
#[test]
fn subtract_known_point() {
    assert_eq!(apply(&mut Sub, &[3.0], &[5.0], true), [-2.0]);
}

/// 6 divided by 3 gives 2.
#[test]
fn divide_known_point() {
    assert_eq!(apply(&mut Div, &[6.0], &[3.0], true), [2.0]);
}

/// A zero or tiny divisor gives 0, never infinity or NaN, even for 0 / 0.
#[test]
fn divide_by_zero_gives_zero() {
    let a = [1.0, -1.0, 0.0, 5.0, 5.0, 5.0];
    let b = [0.0, 0.0, 0.0, -0.0, 1e-9, -1e-9];
    assert_eq!(apply(&mut Div, &a, &b, true), [0.0; 6]);
    assert_eq!(apply(&mut Div, &[1.0], &[2e-9], true), [5e8]);
}

/// With no cable on B, Divide passes A unchanged.
#[test]
fn divide_without_cable_passes_a() {
    let a = [0.25, -3.0, 7.5];
    assert_eq!(apply(&mut Div, &a, &[0.0; 3], false), a);
}

/// The modules keep no state, so a NaN or infinity on one frame does not
/// touch the next one.
#[test]
fn non_finite_input_does_not_stick() {
    let a = [f32::NAN, f32::INFINITY, 1.0, 6.0];
    let b = [1.0, 1.0, f32::NEG_INFINITY, 3.0];
    let sub = apply(&mut Sub, &a, &b, true);
    assert!(sub[0].is_nan() && sub[1].is_infinite() && sub[2].is_infinite());
    assert_eq!(sub[3], 3.0);
    let div = apply(&mut Div, &a, &b, true);
    assert!(div[0].is_nan() && div[1].is_infinite());
    assert_eq!(div[2], 0.0);
    assert_eq!(div[3], 2.0);
}
