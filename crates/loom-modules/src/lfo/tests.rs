use super::*;
use loom_core::testing::run;

const RATE: f32 = 48_000.0;

/// Runs the LFO with the given parameters (sync, rate, cycles, shape)
/// over `phase_in`, in blocks of `block` frames. Returns the three outputs.
fn lfo(params: [f32; 4], phase_in: &[f32], block: usize) -> [Vec<f32>; 3] {
    let ModuleRole::Dsp(create) = LFO.role else {
        unreachable!()
    };
    let mut m = create(&ModuleConfig {
        sample_rate: RATE,
        max_block: block,
    });
    for (index, value) in params.into_iter().enumerate() {
        m.set_param(index, value);
    }
    m.reset();
    let frames = phase_in.len();
    let mut outs = [vec![0.0; frames], vec![0.0; frames], vec![0.0; frames]];
    for start in (0..frames).step_by(block) {
        let end = (start + block).min(frames);
        let [a, b, c] = &mut outs;
        run(
            m.as_mut(),
            RATE,
            &[&phase_in[start..end]],
            &[true],
            &mut [&mut a[start..end], &mut b[start..end], &mut c[start..end]],
            &[],
        );
    }
    outs
}

/// A free LFO starts at phase 0 and runs one wave per period, whatever
/// is on the phase input, and the output is the same at any block size.
#[test]
fn free_lfo_ignores_the_phase_input_and_ignores_block_size() {
    let params = [0.0, 50.0, 1.0, 0.0];
    let ramp: Vec<f32> = (0..1_000).map(|i| i as f32 / 1_000.0).collect();
    let quiet = vec![0.0; 1_000];
    let [out, _, phase] = lfo(params, &ramp, 1_000);
    assert_eq!(phase[0], 0.0);
    assert_eq!(out[0], 0.0);
    let [other, _, _] = lfo(params, &quiet, 77);
    assert_eq!(out, other, "the phase input changes nothing");
    assert_eq!(lfo(params, &quiet, 1)[0], out, "block size 1");
}

/// 50 Hz is 960 frames a wave: check the rate of a free sine.
#[test]
fn free_rate_sets_the_wave_length() {
    let [out, _, _] = lfo([0.0, 50.0, 1.0, 0.0], &vec![0.0; 2_000], 512);
    // The sine peaks a quarter wave in and returns to 0 after a full wave.
    assert!((out[240] - 1.0).abs() < 1e-3, "{}", out[240]);
    assert!(out[960].abs() < 1e-3, "{}", out[960]);
}

/// Synced with `cycles` 2 gives two waves per input cycle, and out90 is
/// the same wave a quarter ahead: on a sine, a cosine.
#[test]
fn synced_cycles_and_quadrature() {
    let phase_in: Vec<f32> = (0..400).map(|i| (i % 400) as f32 / 400.0).collect();
    let [out, out90, phase] = lfo([1.0, 1.0, 2.0, 0.0], &phase_in, 400);
    assert!(out[0].abs() < 1e-6 && (out90[0] - 1.0).abs() < 1e-6);
    // Two waves in 400 frames: the phase wraps back at 200.
    assert!(phase[199] > 0.98 && phase[200] == 0.0, "{} {}", phase[199], phase[200]);
    assert!(out[200].abs() < 1e-6);
}

/// Each shape is right at four points of the cycle.
#[test]
fn shapes() {
    let at = |shape: f32, p: f32| {
        let [out, _, _] = lfo([1.0, 1.0, 1.0, shape], &[p], 1);
        out[0]
    };
    // Phase 0, a quarter, a half, and three quarters.
    let table: [(f32, [f32; 4]); 5] = [
        (0.0, [0.0, 1.0, 0.0, -1.0]),
        (1.0, [0.0, 1.0, 0.0, -1.0]),
        (2.0, [0.0, 0.5, -1.0, -0.5]),
        (3.0, [0.0, -0.5, 1.0, 0.5]),
        (4.0, [1.0, 1.0, -1.0, -1.0]),
    ];
    for (shape, expected) in table {
        for (p, want) in [0.0, 0.25, 0.5, 0.75].into_iter().zip(expected) {
            assert!(
                (at(shape, p) - want).abs() < 1e-5,
                "shape {shape} at {p}: {}",
                at(shape, p)
            );
        }
    }
}
