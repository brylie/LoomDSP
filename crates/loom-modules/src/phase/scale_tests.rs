use super::*;
use loom_core::signal::WrapDetector;
use loom_core::testing::run;

/// Runs Phase Scale over a phase that wraps every 100 frames, for
/// `cycles` input cycles, in blocks of `block` frames.
fn scaled(multiply: f32, divide: f32, cycles: usize, block: usize, reset: &[f32]) -> Vec<f32> {
    let ModuleRole::Dsp(create) = SCALE.role else {
        unreachable!()
    };
    let frames = cycles * 100;
    let phase: Vec<f32> = (0..frames).map(|i| (i % 100) as f32 / 100.0).collect();
    let mut reset_in = vec![0.0; frames];
    reset_in[..reset.len()].copy_from_slice(reset);
    let mut m = create(&ModuleConfig {
        sample_rate: 48_000.0,
        max_block: block,
    });
    m.set_param(0, multiply);
    m.set_param(1, divide);
    m.reset();
    let mut out = vec![0.0; frames];
    for start in (0..frames).step_by(block) {
        let end = (start + block).min(frames);
        run(
            m.as_mut(),
            48_000.0,
            &[&phase[start..end], &reset_in[start..end]],
            &[true, false],
            &mut [&mut out[start..end]],
            &[],
        );
    }
    out
}

/// How many times an output phase wraps.
fn wraps(phase: &[f32]) -> usize {
    let mut detector = WrapDetector::new();
    // The first frame counts as a wrap when it is 0; skip it.
    phase.iter().skip(1).filter(|&&p| detector.wrapped(p)).count()
}

/// Divide 2 wraps once per two input wraps; multiply 3 divide 2 wraps
/// three times per two input wraps; multiply 4 wraps four times per
/// cycle. All at block sizes 1, 32, and 1024.
#[test]
fn ratios_hold_at_any_block_size() {
    for block in [1, 32, 1024] {
        // Four input cycles, 400 frames. The wrap on the first frame is
        // not counted, and the one that would land on frame 400 is past
        // the end. So divide 2 gives 2 - 1 = 1 wrap, 3 over 2 gives
        // 6 - 1 = 5, and multiply 4 gives 16 - 1 = 15.
        assert_eq!(wraps(&scaled(1.0, 2.0, 4, block, &[])), 1, "divide 2, block {block}");
        assert_eq!(wraps(&scaled(3.0, 2.0, 4, block, &[])), 5, "3 over 2, block {block}");
        assert_eq!(wraps(&scaled(4.0, 1.0, 4, block, &[])), 15, "multiply 4, block {block}");
    }
}

/// The output is the same at every block size, and reset starts the
/// count again.
#[test]
fn block_size_does_not_change_the_output_and_reset_restarts() {
    let mut reset = vec![0.0; 150];
    reset[150 - 1] = 1.0;
    let reference = scaled(1.0, 2.0, 4, 1024, &reset);
    assert_eq!(scaled(1.0, 2.0, 4, 1, &reset), reference);
    assert_eq!(scaled(1.0, 2.0, 4, 32, &reset), reference);
    // Without a reset, cycle 1 is the second half of the divided cycle.
    let plain = scaled(1.0, 2.0, 2, 1024, &[]);
    assert!((plain[100] - 0.5).abs() < 1e-6, "{}", plain[100]);
    // A reset on frame 149 sends the count back, so frame 149 reads like
    // the middle of the first of the two cycles.
    assert!((reference[149] - 0.245).abs() < 1e-6, "{}", reference[149]);
}
