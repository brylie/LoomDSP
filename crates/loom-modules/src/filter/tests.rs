use super::*;
use loom_core::testing::run;
use std::f32::consts::TAU;

const RATE: f32 = 48_000.0;

/// Builds a module from a descriptor, settled at the given parameters.
fn make(descriptor: &ModuleDescriptor, params: &[(usize, f32)]) -> Box<dyn Module> {
    let ModuleRole::Dsp(create) = descriptor.role else {
        unreachable!()
    };
    let mut m = create(&ModuleConfig {
        sample_rate: RATE,
        max_block: 48_000,
    });
    for &(index, value) in params {
        m.set_param(index, value);
    }
    m.reset();
    m
}

/// RMS of the second half of a buffer, after the filter has settled.
fn settled_rms(samples: &[f32]) -> f32 {
    let tail = &samples[samples.len() / 2..];
    (tail.iter().map(|v| v * v).sum::<f32>() / tail.len() as f32).sqrt()
}

/// A sine of `hz` lasting one second.
fn sine(hz: f32) -> Vec<f32> {
    (0..RATE as usize).map(|i| (TAU * hz * i as f32 / RATE).sin()).collect()
}

/// Gain in dB of one SVF output (0 low, 1 band, 2 high, 3 notch) for a sine.
fn svf_gain_db(output: usize, cutoff: f32, hz: f32) -> f32 {
    let mut svf = make(&SVF, &[(0, cutoff), (1, 0.2)]);
    // Let the 10 ms smoother settle, so the cutoff is the parameter.
    let input = sine(hz);
    let cutoff_mod = vec![0.0; input.len()];
    let mut outs = vec![vec![0.0; input.len()]; 4];
    let [a, b, c, d] = &mut outs[..] else { unreachable!() };
    run(
        svf.as_mut(),
        RATE,
        &[&input, &cutoff_mod],
        &[true, false],
        &mut [a, b, c, d],
        &[],
    );
    20.0 * (settled_rms(&outs[output]) / settled_rms(&input)).log10()
}

/// The notch cuts a sine at the cutoff by 30 dB or more, and passes a
/// sine two octaves away within 1 dB (MT-FILTER-01).
#[test]
fn svf_notch_cuts_the_cutoff_and_passes_the_rest() {
    assert!(svf_gain_db(3, 1_000.0, 1_000.0) < -30.0);
    assert!(svf_gain_db(3, 1_000.0, 250.0).abs() < 1.0);
    assert!(svf_gain_db(3, 1_000.0, 4_000.0).abs() < 1.0);
}

/// Both one-pole outputs are 3 dB down at their cutoff.
#[test]
fn onepole_is_three_db_down_at_the_cutoff() {
    for cutoff in [100.0, 1_000.0, 10_000.0] {
        let mut filter = make(&ONEPOLE, &[(0, cutoff)]);
        let input = sine(cutoff);
        let cutoff_mod = vec![0.0; input.len()];
        let (mut lp, mut hp) = (vec![0.0; input.len()], vec![0.0; input.len()]);
        run(
            filter.as_mut(),
            RATE,
            &[&input, &cutoff_mod],
            &[true, false],
            &mut [&mut lp, &mut hp],
            &[],
        );
        for out in [&lp, &hp] {
            let db = 20.0 * (settled_rms(out) / settled_rms(&input)).log10();
            assert!((db + 3.01).abs() < 0.1, "cutoff {cutoff}: {db} dB");
        }
    }
}

/// A NaN on the cutoff cable does not poison the filter state: the
/// output is finite during the NaN and the filter works afterwards.
#[test]
fn onepole_recovers_from_nan_on_the_cutoff_cable() {
    let mut filter = make(&ONEPOLE, &[(0, 1_000.0)]);
    let input = vec![0.5; 256];
    let mut cutoff_mod = vec![0.0; 256];
    cutoff_mod[10..14].fill(f32::NAN);
    let (mut lp, mut hp) = (vec![0.0; 256], vec![0.0; 256]);
    run(
        filter.as_mut(),
        RATE,
        &[&input, &cutoff_mod],
        &[true, true],
        &mut [&mut lp, &mut hp],
        &[],
    );
    assert!(lp.iter().chain(&hp).all(|v| v.is_finite()), "{lp:?}");
    // A constant input of 0.5 settles near 0.5 at the lowpass.
    assert!((lp[255] - 0.5).abs() < 0.05, "{}", lp[255]);
}

/// A NaN in the input does not poison the filter state.
#[test]
fn onepole_recovers_from_nan() {
    let mut filter = make(&ONEPOLE, &[(0, 1_000.0)]);
    let mut input = vec![0.5; 64];
    input[3] = f32::NAN;
    let cutoff_mod = vec![0.0; 64];
    let (mut lp, mut hp) = (vec![0.0; 64], vec![0.0; 64]);
    run(
        filter.as_mut(),
        RATE,
        &[&input, &cutoff_mod],
        &[true, false],
        &mut [&mut lp, &mut hp],
        &[],
    );
    assert!(lp.iter().chain(&hp).all(|v| v.is_finite()));
}

/// Indices of the ladder's parameters.
const CUTOFF: usize = 0;
const RESONANCE: usize = 1;
const DRIVE: usize = 2;
const SLOPE: usize = 3;

/// Runs a new ladder over `input` with nothing on its cutoff input.
fn ladder(params: &[(usize, f32)], input: &[f32]) -> Vec<f32> {
    let mut filter = make(&LADDER, params);
    ladder_run(filter.as_mut(), input)
}

/// Runs an existing ladder over `input`.
fn ladder_run(filter: &mut dyn Module, input: &[f32]) -> Vec<f32> {
    let cutoff_mod = vec![0.0; input.len()];
    let mut out = vec![0.0; input.len()];
    run(
        filter,
        RATE,
        &[input, &cutoff_mod],
        &[true, false],
        &mut [&mut out],
        &[],
    );
    out
}

/// Gain in dB of the ladder for a quiet sine, so the tanh stays linear.
/// The sine is computed in `f64`: an `f32` phase jitters enough to
/// leave noise below the cutoff louder than a tone far above it.
fn ladder_gain_db(params: &[(usize, f32)], hz: f32) -> f32 {
    let input: Vec<f32> = (0..RATE as usize)
        .map(|i| (0.01 * (std::f64::consts::TAU * f64::from(hz) * i as f64 / f64::from(RATE)).sin()) as f32)
        .collect();
    let out = ladder(params, &input);
    20.0 * (settled_rms(&out) / settled_rms(&input)).log10()
}

/// The frequency of a settled oscillation, from its rising zero
/// crossings in the second half of `samples`.
fn frequency(samples: &[f32]) -> f32 {
    let tail = &samples[samples.len() / 2..];
    let crossings: Vec<usize> = (1..tail.len())
        .filter(|&i| tail[i - 1] < 0.0 && tail[i] >= 0.0)
        .collect();
    let (first, last) = (crossings[0], crossings[crossings.len() - 1]);
    (crossings.len() - 1) as f32 * RATE / (last - first) as f32
}

/// One click at frame 0, then a second of silence: a start for
/// self-oscillation.
fn click() -> Vec<f32> {
    let mut input = vec![0.0; RATE as usize];
    input[0] = 0.1;
    input
}

/// The largest jump between neighbouring samples.
fn max_step(samples: &[f32]) -> f32 {
    samples.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max)
}

/// An octave well above the cutoff, each slope cuts about 12, 18, or
/// 24 dB more (p1-modules.md, Acceptance criteria).
#[test]
fn ladder_slopes_are_12_18_and_24_db() {
    for (slope, db) in [(0.0, 12.0), (1.0, 18.0), (2.0, 24.0)] {
        let params = [(CUTOFF, 100.0), (RESONANCE, 0.0), (SLOPE, slope)];
        let drop = ladder_gain_db(&params, 1_600.0) - ladder_gain_db(&params, 3_200.0);
        assert!((drop - db).abs() < 1.0, "slope {slope}: {drop} dB per octave");
    }
}

/// At resonance 1 the 24 dB slope sings at the cutoff and the 18 dB
/// slope at about 1.7 times it, after one click. The 18 dB check stays
/// below 0.29 times the sample rate. The tanh keeps the level bounded.
#[test]
fn ladder_self_oscillates_at_resonance_one() {
    for cutoff in [200.0, 1_000.0, 5_000.0] {
        let out = ladder(&[(CUTOFF, cutoff), (RESONANCE, 1.0), (SLOPE, 2.0)], &click());
        let rms = settled_rms(&out);
        assert!(rms > 0.1, "24 dB at {cutoff} Hz: rms {rms}");
        assert!(out.iter().all(|v| v.abs() < 2.0), "24 dB at {cutoff} Hz stays bounded");
        let hz = frequency(&out);
        assert!(
            (hz / cutoff - 1.0).abs() < 0.02,
            "24 dB at {cutoff} Hz sings at {hz} Hz"
        );

        let out = ladder(&[(CUTOFF, cutoff), (RESONANCE, 1.0), (SLOPE, 1.0)], &click());
        let rms = settled_rms(&out);
        assert!(rms > 0.1, "18 dB at {cutoff} Hz: rms {rms}");
        // √3 times the cutoff, through the same prewarping as the cutoff.
        let warped = (3f32.sqrt() * (PI * cutoff / RATE).tan()).atan() * RATE / PI;
        let hz = frequency(&out);
        assert!(
            (hz / warped - 1.0).abs() < 0.02,
            "18 dB at {cutoff} Hz sings at {hz} Hz, not {warped}"
        );
        assert!(hz / cutoff > 1.6, "18 dB at {cutoff} Hz: {hz} Hz");
    }
}

/// The 12 dB slope never sings. At resonance 1 its peak, at √7 times
/// the cutoff, is about 4 dB above its own gain at DC.
#[test]
fn ladder_12_db_has_a_peak_but_does_not_sing() {
    let params = [(CUTOFF, 500.0), (RESONANCE, 1.0), (SLOPE, 0.0)];
    let out = ladder(&params, &click());
    assert!(settled_rms(&out) < 1e-6, "rings out: {}", settled_rms(&out));
    let dc = ladder(&params, &vec![0.01; RATE as usize]);
    let dc_db = 20.0 * (dc[dc.len() - 1] / 0.01).log10();
    // Without gain compensation, DC is 1 / (1 + 8): about -19 dB.
    assert!((dc_db + 19.08).abs() < 0.1, "{dc_db}");
    let peak = ladder_gain_db(&params, 500.0 * 7f32.sqrt()) - dc_db;
    assert!((peak - 4.0).abs() < 0.3, "peak {peak} dB");
}

/// Resonance thins the low end, with no gain compensation.
#[test]
fn ladder_resonance_thins_the_bass() {
    let dc_gain = |resonance| {
        let out = ladder(&[(CUTOFF, 1_000.0), (RESONANCE, resonance)], &vec![0.01; 4_800]);
        out[out.len() - 1] / 0.01
    };
    assert!((dc_gain(0.0) - 1.0).abs() < 1e-3);
    // k = 0.5 × 4.2 = 2.1, so the DC gain is 1 / 3.1.
    assert!((dc_gain(0.5) - 1.0 / 3.1).abs() < 1e-3, "{}", dc_gain(0.5));
}

/// Drive raises the input level from 1 to 8 times, even in dB.
#[test]
fn ladder_drive_raises_the_level() {
    let gain = |drive| {
        let out = ladder(&[(RESONANCE, 0.0), (DRIVE, drive)], &vec![0.01; 4_800]);
        out[out.len() - 1] / 0.01
    };
    assert!((gain(0.0) - 1.0).abs() < 1e-3);
    assert!((gain(1.0) - 8.0).abs() < 1e-2, "{}", gain(1.0));
    assert!((gain(1.0 / 3.0) - 2.0).abs() < 1e-2, "{}", gain(1.0 / 3.0));
}

/// The output stays finite for any input: huge values, NaN, and
/// infinity on the input, at any setting.
#[test]
fn ladder_stays_finite() {
    let mut input = sine(300.0);
    for (i, v) in input.iter_mut().enumerate() {
        *v *= match i % 4_000 {
            0..1_000 => 1e30,
            1_000..1_010 => f32::NAN,
            1_010..1_020 => f32::INFINITY,
            2_000..2_010 => f32::NEG_INFINITY,
            _ => 50.0,
        };
    }
    for slope in [0.0, 1.0, 2.0] {
        for drive in [0.0, 1.0] {
            for cutoff in [20.0, 20_000.0] {
                let out = ladder(
                    &[(CUTOFF, cutoff), (RESONANCE, 1.0), (DRIVE, drive), (SLOPE, slope)],
                    &input,
                );
                assert!(
                    out.iter().all(|v| v.is_finite()),
                    "slope {slope}, drive {drive}, {cutoff} Hz"
                );
            }
        }
    }
}

/// During a slope change the feedback gain never passes the limit of a
/// tap that is heard, never jumps, and ends at the new slope's limit.
#[test]
fn ladder_slope_change_keeps_the_gain_under_every_heard_limit() {
    for (from, to) in [(0, 2), (2, 0), (1, 2), (2, 1), (0, 1), (1, 0)] {
        let mut ladder = Ladder::new(&ModuleConfig {
            sample_rate: RATE,
            max_block: 64,
        });
        ladder.set_param(SLOPE, from as f32);
        ladder.reset();
        ladder.change_slope(to);
        let mut last = ladder.limit;
        for _ in 0..2_000 {
            let k = ladder.step_slope();
            for (tap, &w) in ladder.weights.iter().enumerate() {
                assert!(w == 0.0 || k <= LADDER_K_MAX[tap], "{from} to {to}: k {k} on tap {tap}");
            }
            assert!((k - last).abs() <= ladder.limit_step * 1.001, "{from} to {to}: jump");
            last = k;
        }
        assert_eq!(ladder.weights[to], 1.0, "{from} to {to}");
        assert_eq!(last, LADDER_K_MAX[to], "{from} to {to}");
    }
}

/// Changing the slope while a note plays is click-free and does not
/// burst into oscillation: no step in the output is bigger than the
/// biggest step of a steady slope, and the level stays bounded.
#[test]
fn ladder_slope_change_is_click_free() {
    let input: Vec<f32> = sine(110.0).iter().map(|v| v * 0.5).collect();
    let block = 4_800;
    for resonance in [0.5, 0.95, 1.0] {
        let params = [(CUTOFF, 800.0), (RESONANCE, resonance), (SLOPE, 1.0)];
        let mut filter = make(&LADDER, &params);
        let mut out = Vec::new();
        // 18, 24, 12, 24, 18 dB, then 24 dB for only 1 ms, then 12 dB.
        for (n, slope) in [1.0, 2.0, 0.0, 2.0, 1.0, 2.0, 0.0].into_iter().enumerate() {
            filter.set_param(SLOPE, slope);
            let len = if n == 5 { 48 } else { block };
            let start = out.len();
            out.extend(ladder_run(filter.as_mut(), &input[start..start + len]));
        }
        assert!(out.iter().all(|v| v.is_finite()));
        let steady = [0.0, 1.0, 2.0]
            .map(|slope| {
                let out = ladder(
                    &[(CUTOFF, 800.0), (RESONANCE, resonance), (SLOPE, slope)],
                    &input[..block * 2],
                );
                max_step(&out[block / 2..])
            })
            .into_iter()
            .fold(0.0, f32::max);
        let step = max_step(&out[block / 2..]);
        assert!(
            step <= steady * 1.1,
            "resonance {resonance}: step {step}, steady {steady}"
        );
        let peak = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak < 1.5, "resonance {resonance}: peak {peak}");
    }
}
