use super::*;
use loom_core::testing::run;

/// 12 semitones output 0.1, one octave in the pitch signal.
#[test]
fn twelve_semitones_is_one_octave() {
    let ModuleRole::Dsp(create) = PITCH_CONSTANT.role else {
        unreachable!()
    };
    let mut m = create(&ModuleConfig {
        sample_rate: 48_000.0,
        max_block: 4,
    });
    m.set_param(0, 12.0);
    m.reset();
    let mut out = [0.0; 4];
    run(m.as_mut(), 48_000.0, &[], &[], &mut [&mut out], &[]);
    assert!(out.iter().all(|v| (v - 0.1).abs() < 1e-7), "{out:?}");
}
