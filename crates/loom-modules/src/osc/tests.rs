use super::*;
use loom_core::testing::run;

/// Checks that the saw wave stays within `[-1, 1]` and rises across the cycle.
#[test]
fn saw_is_bounded_and_rises() {
    let config = ModuleConfig {
        sample_rate: 48_000.0,
        max_block: 64,
    };
    let ModuleRole::Dsp(create) = SAW.role else {
        unreachable!()
    };
    let mut saw = create(&config);
    let phase: Vec<f32> = (0..64).map(|i| (i as f32 * 0.01) % 1.0).collect();
    let mut out = vec![0.0; 64];
    run(saw.as_mut(), 48_000.0, &[&phase], &[true], &mut [&mut out], &[]);
    assert!(out.iter().all(|v| v.abs() <= 1.0 + 1e-6));
    assert!(out[10] > out[5]);
}

/// Checks that pulse wave output stays within `[-1, 1]` and matches the expected duty cycle mean.
#[test]
fn pulse_duty_cycle_and_bounds() {
    let config = ModuleConfig {
        sample_rate: 48_000.0,
        max_block: 200,
    };
    let ModuleRole::Dsp(create) = PULSE.role else {
        unreachable!()
    };
    let phase: Vec<f32> = (0..200).map(|i| (i % 100) as f32 / 100.0).collect();
    for &(width, expected_mean) in &[(0.5, 0.0), (0.25, -0.5), (0.75, 0.5)] {
        let mut pulse = create(&config);
        pulse.set_param(0, width);
        pulse.reset();
        let mut out = vec![0.0; 200];
        run(pulse.as_mut(), 48_000.0, &[&phase], &[true], &mut [&mut out], &[]);
        assert!(out.iter().all(|v| v.abs() <= 1.0 + 1e-6), "width {width}: {out:?}");
        // Steady second cycle: average equals 2 * width - 1.
        let mean: f32 = out[100..].iter().sum::<f32>() / 100.0;
        assert!((mean - expected_mean).abs() < 0.02, "width {width}: mean {mean}");
    }
}

/// Checks that sweeping `width` across a stationary phase still smooths the transition with polyBLEP.
#[test]
fn pulse_width_sweep_at_stationary_phase_is_smoothed() {
    let config = ModuleConfig {
        sample_rate: 4_000.0,
        max_block: 64,
    };
    let ModuleRole::Dsp(create) = PULSE.role else {
        unreachable!()
    };
    let phase = vec![0.5; 64];
    for &(start, end) in &[(0.75, 0.25), (0.25, 0.75)] {
        let mut pulse = create(&config);
        pulse.set_param(0, start);
        pulse.reset();
        pulse.set_param(0, end);
        let mut out = vec![0.0; 64];
        run(pulse.as_mut(), 4_000.0, &[&phase], &[true], &mut [&mut out], &[]);
        assert!(out.iter().all(|v| v.abs() <= 1.0 + 1e-6), "{start}->{end}: {out:?}");
        assert!(
            out.iter().any(|v| v.abs() < 0.99),
            "expected polyBLEP-smoothed intermediate samples on {start}->{end} sweep, got {out:?}"
        );
    }
}

/// Regression test: overlapping BLEP windows with unequal edge velocities under audio-rate
/// width modulation must stay within `[-1, 1]`.
#[test]
fn pulse_overlapping_edges_with_unequal_velocities_stay_bounded() {
    // At 1 Hz, the 5 ms smoother reaches its target in a single sample while preserving
    // `previous` and `previous_fall`, reproducing audio-rate width jumps.
    let config = ModuleConfig {
        sample_rate: 1.0,
        max_block: 64,
    };
    let ModuleRole::Dsp(create) = PULSE.role else {
        unreachable!()
    };
    let mut pulse = create(&config);
    // Step 1 sets `previous = 0.75` and `width = 0.80` (`previous_fall = 0.95`).
    // Step 2 advances phase to `0.95` (`step = 0.20`) and width to `0.99` (`step_fall = 0.01`).
    let mut out = [0.0; 1];
    pulse.set_param(0, 0.80);
    pulse.reset();
    run(pulse.as_mut(), 1.0, &[&[0.75]], &[true], &mut [&mut out], &[]);
    assert!(out[0].abs() <= 1.0 + 1e-6, "step 1: {}", out[0]);

    pulse.set_param(0, 0.99);
    run(pulse.as_mut(), 1.0, &[&[0.95]], &[true], &mut [&mut out], &[]);
    assert!(out[0].abs() <= 1.0 + 1e-6, "step 2: {}", out[0]);
}

/// Checks that the triangle wave stays within `[-1, 1]` and matches sine phase alignment.
#[test]
fn triangle_is_bounded_and_sine_aligned() {
    let config = ModuleConfig {
        sample_rate: 48_000.0,
        max_block: 200,
    };
    let ModuleRole::Dsp(create) = TRIANGLE.role else {
        unreachable!()
    };
    let mut tri = create(&config);
    let phase: Vec<f32> = (0..200).map(|i| (i % 100) as f32 / 100.0).collect();
    let mut out = vec![0.0; 200];
    run(tri.as_mut(), 48_000.0, &[&phase], &[true], &mut [&mut out], &[]);
    assert!(out.iter().all(|v| v.abs() <= 1.0 + 1e-6), "{out:?}");
    // Second cycle: 0 at start, rises to +1 at quarter cycle, 0 at half, -1 at three-quarters.
    assert!(out[100].abs() < 0.05, "start of cycle: {}", out[100]);
    assert!(out[125] > 0.95, "quarter cycle peak: {}", out[125]);
    assert!(out[150].abs() < 0.05, "half cycle zero: {}", out[150]);
    assert!(out[175] < -0.95, "three-quarter trough: {}", out[175]);
}

/// NFR-SND-01: measures aliasing at a high note (~2859 Hz, near F7 at 48 kHz).
///
/// Using N = 1024 samples and prime cycle count k0 = 61 makes the phase
/// strictly periodic over N samples with zero window leakage, and places all
/// true harmonics below Nyquist on bins 61 * m (m = 1..=8). Every other bin
/// holds only aliased energy.
#[test]
fn aliasing_at_high_note_is_suppressed() {
    const N: usize = 1024;
    const WARMUP: usize = 16;
    const K0: usize = 61;

    let phase: Vec<f32> = (0..WARMUP + N).map(|i| ((i * K0) % N) as f32 / N as f32).collect();
    let config = ModuleConfig {
        sample_rate: 48_000.0,
        max_block: WARMUP + N,
    };

    // Returns (worst sub-fundamental alias in dB below the fundamental,
    // total alias RMS in dB below the fundamental).
    let measure_aliasing = |samples: &[f32]| -> (f32, f32) {
        let bin_mag_sq = |bin: usize| -> f32 {
            let mut re = 0.0f64;
            let mut im = 0.0f64;
            for (n, &x) in samples.iter().enumerate() {
                let angle = std::f64::consts::TAU * (bin * n) as f64 / N as f64;
                re += f64::from(x) * angle.cos();
                im -= f64::from(x) * angle.sin();
            }
            ((re * re + im * im) / (N * N) as f64) as f32
        };
        let fund_sq = bin_mag_sq(K0);
        assert!(fund_sq > 0.05, "fundamental must be present, got {fund_sq}");
        let mut max_sub_sq = 0.0f32;
        let mut total_alias_sq = 0.0f32;
        for bin in 1..N / 2 {
            if bin.is_multiple_of(K0) {
                continue;
            }
            let mag_sq = bin_mag_sq(bin);
            if bin < K0 {
                max_sub_sq = max_sub_sq.max(mag_sq);
            }
            total_alias_sq += mag_sq;
        }
        let sub_db = 10.0 * (max_sub_sq / fund_sq).log10();
        let total_db = 10.0 * (total_alias_sq / fund_sq).log10();
        (sub_db, total_db)
    };

    let render_with_phase = |descriptor: &ModuleDescriptor, phase_in: &[f32]| -> Vec<f32> {
        let ModuleRole::Dsp(create) = descriptor.role else {
            unreachable!()
        };
        let mut module = create(&config);
        let mut out = vec![0.0; WARMUP + N];
        run(module.as_mut(), 48_000.0, &[phase_in], &[true], &mut [&mut out], &[]);
        out[WARMUP..].to_vec()
    };
    let render_module = |descriptor: &ModuleDescriptor| render_with_phase(descriptor, &phase);

    // Naive shapes at the same high note for comparison.
    let naive_saw: Vec<f32> = phase[WARMUP..].iter().map(|&t| 2.0 * t - 1.0).collect();
    let naive_pulse: Vec<f32> = phase[WARMUP..]
        .iter()
        .map(|&t| if t < 0.5 { 1.0 } else { -1.0 })
        .collect();
    let naive_tri: Vec<f32> = phase[WARMUP..]
        .iter()
        .map(|&t| {
            let mut t_top = t - 0.25;
            t_top -= t_top.floor();
            4.0 * (t_top - 0.5).abs() - 1.0
        })
        .collect();

    let (saw_sub_db, saw_total_db) = measure_aliasing(&render_module(&SAW));
    let (naive_saw_sub_db, naive_saw_total_db) = measure_aliasing(&naive_saw);
    assert!(
        saw_sub_db < -40.0 && saw_total_db < naive_saw_total_db - 15.0,
        "saw aliasing: sub {saw_sub_db:.1} dB (naive {naive_saw_sub_db:.1}), total {saw_total_db:.1} dB (naive {naive_saw_total_db:.1})"
    );

    let (pulse_sub_db, pulse_total_db) = measure_aliasing(&render_module(&PULSE));
    let (naive_pulse_sub_db, naive_pulse_total_db) = measure_aliasing(&naive_pulse);
    assert!(
        pulse_sub_db < -40.0 && pulse_total_db < naive_pulse_total_db - 15.0,
        "pulse aliasing: sub {pulse_sub_db:.1} dB (naive {naive_pulse_sub_db:.1}), total {pulse_total_db:.1} dB (naive {naive_pulse_total_db:.1})"
    );

    let (tri_sub_db, tri_total_db) = measure_aliasing(&render_module(&TRIANGLE));
    let (naive_tri_sub_db, naive_tri_total_db) = measure_aliasing(&naive_tri);
    assert!(
        tri_sub_db < -65.0 && tri_total_db < -40.0 && tri_total_db < naive_tri_total_db - 10.0,
        "triangle aliasing: sub {tri_sub_db:.1} dB (naive {naive_tri_sub_db:.1}), total {tri_total_db:.1} dB (naive {naive_tri_total_db:.1})"
    );

    // Descending phase ramp at the same frequency must also suppress aliasing.
    let desc_phase: Vec<f32> = phase.iter().map(|&t| (1.0 - t) % 1.0).collect();
    let (desc_pulse_sub_db, desc_pulse_total_db) = measure_aliasing(&render_with_phase(&PULSE, &desc_phase));
    assert!(
        desc_pulse_sub_db < -40.0 && desc_pulse_total_db < naive_pulse_total_db - 15.0,
        "descending pulse aliasing: sub {desc_pulse_sub_db:.1} dB, total {desc_pulse_total_db:.1} dB"
    );
    let (desc_tri_sub_db, desc_tri_total_db) = measure_aliasing(&render_with_phase(&TRIANGLE, &desc_phase));
    assert!(
        desc_tri_sub_db < -65.0 && desc_tri_total_db < -40.0 && desc_tri_total_db < naive_tri_total_db - 10.0,
        "descending triangle aliasing: sub {desc_tri_sub_db:.1} dB, total {desc_tri_total_db:.1} dB"
    );
}
