use super::*;
use loom_core::testing::run;

fn phase_trigger(input: &[f32]) -> Vec<f32> {
    let mut module = PhaseTrigger {
        wrap: WrapDetector::new(),
        fired: TriggerFlag::default(),
    };
    let mut out = vec![0.0; input.len()];
    run(&mut module, 48_000.0, &[input], &[true], &mut [&mut out], &[]);
    out
}

#[test]
fn fires_one_sample_on_each_wrap() {
    // Four frames per cycle, starting on a bar line.
    let input: Vec<f32> = (0..12).map(|i| (i % 4) as f32 / 4.0).collect();
    let out = phase_trigger(&input);
    let fired: Vec<usize> = (0..out.len()).filter(|&i| out[i] == 1.0).collect();
    assert_eq!(fired, [0, 4, 8]);
    assert!(out.iter().all(|&value| value == 0.0 || value == 1.0));
}

#[test]
fn extreme_pitch_never_leaves_the_phase_stuck() {
    // #121: pitch 20 overflows `to_hz` to infinity. Then NaN, then a
    // normal pitch: the phase must stay in range and move again.
    let mut module = Phase::new(&ModuleConfig {
        sample_rate: 48_000.0,
        max_block: 16,
    });
    let pitch = [20.0, 20.0, f32::INFINITY, f32::NAN, 0.0, 0.0, 0.0, 0.0];
    let reset = [0.0; 8];
    let mut out = [0.0; 8];
    run(
        &mut module,
        48_000.0,
        &[&pitch, &reset],
        &[true, true],
        &mut [&mut out],
        &[],
    );
    assert!(out.iter().all(|p| (0.0..1.0).contains(p)), "{out:?}");
    // Middle C after the extremes: the phase moves by 261.6 / 48000.
    let step = out[7] - out[6];
    assert!((step - pitch::to_hz(0.0) / 48_000.0).abs() < 1e-6, "{out:?}");
}

#[test]
fn phase_trigger_keeps_firing_at_the_highest_pitch() {
    // Pitch 20 runs Phase at its largest step. Every cycle must still
    // end in a jump that Phase Trigger sees.
    let mut generator = Phase::new(&ModuleConfig {
        sample_rate: 48_000.0,
        max_block: 100,
    });
    let pitch = [20.0; 100];
    let reset = [0.0; 100];
    let mut phase = [0.0; 100];
    run(
        &mut generator,
        48_000.0,
        &[&pitch, &reset],
        &[true, true],
        &mut [&mut phase],
        &[],
    );
    let fired = phase_trigger(&phase).iter().filter(|&&t| t == 1.0).count();
    // 100 frames at 0.49 cycles per frame: frame 0, then 48 wraps.
    assert_eq!(fired, 49, "{phase:?}");
}

#[test]
fn unconnected_input_fires_once_then_stays_low() {
    // An unconnected input reads zeros: a phase standing still at 0.
    assert_eq!(phase_trigger(&[0.0; 4]), [1.0, 0.0, 0.0, 0.0]);
}
