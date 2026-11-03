use super::*;
use loom_core::ModuleConfig;
use loom_core::testing::run;

/// A Macro at `value`, as the `Controller` starts one.
fn knob(value: f32) -> Box<dyn Module> {
    let ModuleRole::Dsp(create) = MACRO.role else {
        unreachable!()
    };
    let mut m = create(&ModuleConfig {
        sample_rate: 48_000.0,
        max_block: 512,
    });
    m.set_param(Macro::VALUE, value);
    m.reset();
    m
}

/// `frames` frames of `module`'s output.
fn render(module: &mut dyn Module, frames: usize) -> Vec<f32> {
    let mut out = vec![0.0; frames];
    run(module, 48_000.0, &[], &[], &mut [&mut out], &[]);
    out
}

/// The output equals the knob (spec, acceptance).
#[test]
fn the_output_equals_the_knob() {
    assert!(render(knob(0.6).as_mut(), 64).iter().all(|&value| value == 0.6));
}

/// A turn of the knob glides over about 5 ms, so it never steps, and arrives.
#[test]
fn a_turn_of_the_knob_is_smoothed() {
    let mut macro_knob = knob(0.0);
    macro_knob.set_param(Macro::VALUE, 1.0);
    let out = render(macro_knob.as_mut(), 4_800);
    assert!(out[0] < 0.1, "{}", out[0]);
    assert!(out.windows(2).all(|pair| pair[1] >= pair[0]), "it must only rise");
    assert!(out[4_799] > 0.999, "{}", out[4_799]);
}

/// The output runs from 0 to 1, so it is unipolar. A knob at 0 then leaves every
/// parameter it is assigned to where it is, whatever polarity the assignment
/// takes from it.
#[test]
fn the_output_is_unipolar() {
    assert_eq!(MACRO.outputs[0].polarity(), loom_core::Polarity::Unipolar);
}
