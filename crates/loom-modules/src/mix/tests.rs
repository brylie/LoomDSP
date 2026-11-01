use super::*;
use loom_core::testing::run;
use std::f32::consts::FRAC_1_SQRT_2;

const RATE: f32 = 48_000.0;

/// Builds a module from a descriptor, settled at the given parameters.
fn make(descriptor: &ModuleDescriptor, params: &[(usize, f32)]) -> Box<dyn Module> {
    let ModuleRole::Dsp(create) = descriptor.role else {
        unreachable!()
    };
    let mut m = create(&ModuleConfig {
        sample_rate: RATE,
        max_block: 4_800,
    });
    for &(index, value) in params {
        m.set_param(index, value);
    }
    m.reset();
    m
}

/// Mixing sums inputs times their levels.
#[test]
fn mixer_sums_inputs_times_levels() {
    let mut mixer = make(&MIXER, &[(0, 0.5), (1, 2.0)]);
    let (a, b, c, d) = ([1.0; 8], [0.25; 8], [0.1; 8], [0.0; 8]);
    let mut out = [0.0; 8];
    run(
        mixer.as_mut(),
        RATE,
        &[&a, &b, &c, &d],
        &[true, true, true, true],
        &mut [&mut out],
        &[],
    );
    // 1.0 * 0.5 + 0.25 * 2.0 + 0.1 * 1.0 = 1.1
    assert!(out.iter().all(|v| (v - 1.1).abs() < 1e-6), "{out:?}");
}

/// A modulated mute from 0.5 up is muted; below it, and NaN, is open.
#[test]
fn modulated_mute_maps_nan_to_open() {
    assert_eq!(open_state(0.0), 1.0);
    assert_eq!(open_state(0.49), 1.0);
    assert_eq!(open_state(0.5), 0.0);
    assert_eq!(open_state(3.0), 0.0);
    assert_eq!(open_state(f32::NAN), 1.0);
    assert_eq!(open_state(f32::NEG_INFINITY), 1.0);
}

/// A mute fades over 5 ms instead of cutting (no click).
#[test]
fn mixer_mute_fades() {
    let mut mixer = make(&MIXER, &[]);
    mixer.set_param(4, 1.0);
    let one = vec![1.0; 480];
    let zero = vec![0.0; 480];
    let mut out = vec![0.0; 480];
    run(
        mixer.as_mut(),
        RATE,
        &[&one, &zero, &zero, &zero],
        &[true, false, false, false],
        &mut [&mut out],
        &[],
    );
    assert!(out[0] > 0.9, "the fade starts near full level: {}", out[0]);
    assert!(out[479] < 0.01, "and ends near silence: {}", out[479]);
    assert!(out.windows(2).all(|w| w[1] <= w[0]), "and only falls");
    let biggest_step = out.windows(2).map(|w| w[0] - w[1]).fold(0.0, f32::max);
    assert!(biggest_step < 0.05, "no click: {biggest_step}");
}

/// Runs a crossfade on constant inputs and returns the first output frame.
fn crossfade_value(law: f32, mix: f32, a: f32, b: f32) -> f32 {
    let mut fade = make(&CROSSFADE, &[(0, mix), (1, law)]);
    let (a, b) = ([a; 4], [b; 4]);
    let mut out = [0.0; 4];
    run(fade.as_mut(), RATE, &[&a, &b], &[true, true], &mut [&mut out], &[]);
    out[0]
}

/// Equal power gives 0.7071 of each side at mix 0.5; linear gives 0.5.
#[test]
fn crossfade_laws_at_the_middle() {
    assert!((crossfade_value(0.0, 0.5, 1.0, 0.0) - FRAC_1_SQRT_2).abs() < 1e-6);
    assert!((crossfade_value(0.0, 0.5, 0.0, 1.0) - FRAC_1_SQRT_2).abs() < 1e-6);
    assert_eq!(crossfade_value(1.0, 0.5, 1.0, 0.0), 0.5);
    assert_eq!(crossfade_value(1.0, 0.5, 0.0, 1.0), 0.5);
}

/// The ends of the mix are exactly one side, for both laws.
#[test]
fn crossfade_ends_are_one_side() {
    for law in [0.0, 1.0] {
        assert!((crossfade_value(law, 0.0, 0.8, -0.3) - 0.8).abs() < 1e-6);
        assert!((crossfade_value(law, 1.0, 0.8, -0.3) + 0.3).abs() < 1e-6);
    }
}
