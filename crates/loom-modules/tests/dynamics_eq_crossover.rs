//! Acceptance tests of the Dynamics, EQ, and Crossover modules, through the
//! module API and the engine (`docs/design/dynamics-eq-crossover.md`). The
//! algorithms have their own tests against theory in `loom-dsp`.

use loom_core::{EngineConfig, ProbeKind, create_engine};
use loom_modules::{dynamics::DYNAMICS, eq::EQ, filter::CROSSOVER};
use loom_test::{Harness, Signal};

const RATE: f32 = 48_000.0;

/// A sine of this amplitude at about 382 Hz.
fn sine(amplitude: f32) -> Signal {
    Signal::from_fn(move |frame| amplitude * (frame as f32 * 0.05).sin())
}

/// The highest absolute value in the last `tail` samples.
fn peak(signal: &[f32], tail: usize) -> f32 {
    signal[signal.len() - tail..]
        .iter()
        .fold(0.0, |top, x| top.max(x.abs()))
}

/// A factor as dB.
fn db(factor: f32) -> f32 {
    20.0 * factor.log10()
}

/// Compress settles on the static curve, across thresholds and ratios.
#[test]
fn compression_settles_on_the_curve() {
    for (threshold, ratio, level) in [(-20.0f32, 4.0f32, -6.0f32), (-30.0, 2.0, -12.0), (-12.0, 20.0, 0.0)] {
        let render = Harness::new(&DYNAMICS)
            .seconds(1.0)
            .input("in", sine(10.0f32.powf(level / 20.0)))
            .param("threshold", threshold)
            .param("ratio", ratio)
            .param("knee", 0.0)
            .param("attack", 0.000_1)
            .param("release", 2.0)
            .render();
        let expected = threshold + (level - threshold) / ratio;
        let measured = db(peak(render.output("out"), 4_800));
        assert!(
            (measured - expected).abs() < 0.1,
            "{threshold} {ratio} {level}: {measured} against {expected}"
        );
    }
}

/// With Limit, no sample passes the threshold, at any block size.
#[test]
fn limit_holds_its_ceiling_at_any_block_size() {
    let harness = Harness::new(&DYNAMICS)
        .seconds(0.2)
        .input("in", sine(8.0))
        .param("mode", 1.0)
        .param("threshold", -6.0);
    let ceiling = 10.0f32.powf(-6.0 / 20.0) * (1.0 + 1e-5);
    for block in [1, 77, 512] {
        let render = harness.render_with_block(block);
        let top = peak(render.output("out"), render.output("out").len());
        assert!(top <= ceiling, "block {block}: {top} against {ceiling}");
    }
}

/// A cable on the sidechain sets the gain, and the input's own level does not.
#[test]
fn the_sidechain_sets_the_gain() {
    let render = Harness::new(&DYNAMICS)
        .seconds(0.5)
        .input("in", sine(0.01))
        .input(
            "sidechain",
            Signal::from_fn(|frame| if frame < 12_000 { 0.0 } else { 1.0 }),
        )
        .param("mode", 4.0)
        .param("threshold", -20.0)
        .param("range", 20.0)
        .param("attack", 0.001)
        .param("release", 0.01)
        .render();
    let gain = render.output("gain");
    assert_eq!(gain[11_000], 1.0, "a quiet key leaves it alone");
    assert!((db(gain[gain.len() - 1]) - -20.0).abs() < 0.01, "a loud key ducks it");
}

/// An unconnected sidechain does not read as silence: the input sets the gain.
#[test]
fn with_no_sidechain_the_input_sets_the_gain() {
    let render = Harness::new(&DYNAMICS)
        .seconds(0.5)
        .input("in", sine(1.0))
        .param("attack", 0.001)
        .render();
    assert!(peak(render.output("gain"), 1) < 1.0);
}

/// With level match on, a compressed signal comes back to the loudness of the input.
#[test]
fn level_match_restores_the_loudness() {
    let harness = |on: f32| {
        Harness::new(&DYNAMICS)
            .seconds(4.0)
            .input("in", sine(0.5))
            .param("threshold", -30.0)
            .param("ratio", 8.0)
            .param("match", on)
            .render()
    };
    let (plain, matched) = (harness(0.0), harness(1.0));
    let tail = 48_000;
    assert!(db(peak(plain.output("out"), tail) / 0.5) < -10.0, "it compresses");
    let change = db(peak(matched.output("out"), tail) / 0.5);
    assert!(change.abs() < 0.5, "{change} dB from the input");
}

/// At a mix of 0, the output is the input, whatever the gain does.
#[test]
fn mix_of_zero_is_the_input() {
    let render = Harness::new(&DYNAMICS)
        .seconds(0.1)
        .input("in", sine(0.9))
        .param("threshold", -40.0)
        .param("mix", 0.0)
        .render();
    for (frame, y) in render.output("out").iter().enumerate() {
        assert_eq!(*y, 0.9 * (frame as f32 * 0.05).sin(), "frame {frame}");
    }
}

/// Bad input is 0 for its frame and the module recovers, in every module.
#[test]
fn every_module_recovers_from_bad_input() {
    for (descriptor, port) in [
        (&DYNAMICS, "in"),
        (&DYNAMICS, "sidechain"),
        (&EQ, "in"),
        (&CROSSOVER, "in"),
    ] {
        Harness::new(descriptor)
            .seconds(0.1)
            .input(port, sine(0.5))
            .check_non_finite_recovery();
    }
}

/// A sine of 0.1, with a few huge samples at the start, as a feedback patch can
/// send: cables are never clipped (ADR-0007).
fn huge_then_sine(huge: f32) -> Signal {
    Signal::from_fn(move |frame| {
        if frame < 8 {
            if frame % 2 == 0 { huge } else { -huge }
        } else {
            0.1 * (frame as f32 * 0.05).sin()
        }
    })
}

/// Huge finite samples leave no bad state: the output stays finite, and comes
/// back to a normal level. Spec, rule 3 of all three modules.
#[test]
fn every_module_recovers_from_huge_samples() {
    let check = |name: String, render: &loom_test::Render, ports: &[&str], most: f32| {
        for port in ports {
            let out = render.output(port);
            assert!(out.iter().all(|x| x.is_finite()), "{name} {port}: not finite");
            let top = peak(out, 4_800);
            assert!(top <= most, "{name} {port}: still {top}");
        }
    };
    for huge in [1e20, f32::MAX] {
        for mode in 0..5 {
            for detect in [0.0, 1.0] {
                for level_match in [0.0, 1.0] {
                    let render = Harness::new(&DYNAMICS)
                        .seconds(4.0)
                        .input("in", huge_then_sine(huge))
                        .param("mode", mode as f32)
                        .param("detect", detect)
                        .param("match", level_match)
                        .render();
                    let name = format!("{huge} mode {mode} detect {detect} match {level_match}");
                    check(name.clone(), &render, &["out", "gain"], 1.0);
                    // Quiet input, below the default threshold: it passes
                    // unless Expand or Gate has closed. Either way the gain is
                    // not stuck at nothing or at a bad value.
                    let gain = *render.output("gain").last().unwrap();
                    assert!((0.0..=1.0).contains(&gain), "{name}: gain {gain}");
                }
            }
        }
        let eq = Harness::new(&EQ)
            .seconds(4.0)
            .input("in", huge_then_sine(huge))
            .param("mid_gain", 12.0)
            .param("low_cut", 100.0)
            .render();
        check(format!("EQ {huge}"), &eq, &["out"], 1.0);
        let crossover = Harness::new(&CROSSOVER)
            .seconds(4.0)
            .input("in", huge_then_sine(huge))
            .render();
        check(format!("Crossover {huge}"), &crossover, &["low", "mid", "high"], 1.0);
    }
}

/// With every gain at 0 and both cuts off, the EQ leaves the input bit for bit.
#[test]
fn a_flat_eq_leaves_the_input_untouched() {
    let render = Harness::new(&EQ).seconds(0.2).input("in", sine(0.7)).render();
    for (frame, y) in render.output("out").iter().enumerate() {
        let x = 0.7 * (frame as f32 * 0.05).sin();
        if x != 0.0 {
            assert_eq!(y.to_bits(), x.to_bits(), "frame {frame}");
        }
    }
}

/// The EQ's panel curve agrees with what it measures.
#[test]
fn the_eq_curve_agrees_with_the_sound() {
    let loom_core::WidgetKind::Shape { sample, .. } = EQ.panel.widgets[0].kind else {
        panic!("the first widget is the curve");
    };
    let param = |name: &str, fallback: f32| match name {
        "low_gain" => 6.0,
        "mid_gain" => -9.0,
        "mid_q" => 2.0,
        "low_slope" => 1.0,
        "low_cut" => 60.0,
        _ => fallback,
    };
    let values: Vec<f32> = EQ.params.iter().map(|p| param(p.name, p.default)).collect();
    for hz in [100.0f32, 300.0, 1_000.0, 3_000.0] {
        let step = std::f32::consts::TAU * hz / RATE;
        let mut harness = Harness::new(&EQ)
            .seconds(1.0)
            .input("in", Signal::from_fn(move |f| 0.1 * (f as f32 * step).sin()));
        for (spec, value) in EQ.params.iter().zip(&values) {
            harness = harness.param(spec.name, *value);
        }
        let render = harness.render();
        let measured = db(peak(render.output("out"), 24_000) / 0.1);
        let x = (hz / 20.0).log10() / 3.0;
        let drawn = sample(&values, x) * 48.0 - 24.0;
        // A sine's peak between samples reads a little low.
        assert!(
            (measured - drawn).abs() < 0.2,
            "{hz} Hz: heard {measured}, drawn {drawn}"
        );
    }
}

/// The Dynamics curve agrees with the sound, in every mode: both use one function.
#[test]
fn the_dynamics_curve_agrees_with_the_sound() {
    let loom_core::WidgetKind::Shape { sample, .. } = DYNAMICS.panel.widgets[0].kind else {
        panic!("the first widget is the curve");
    };
    for (mode, level) in [(0.0f32, -6.0f32), (1.0, -3.0), (2.0, -30.0), (3.0, -30.0), (4.0, -3.0)] {
        let param = |name: &str, fallback: f32| match name {
            "mode" => mode,
            "threshold" => -12.0,
            "ratio" => 3.0,
            "knee" => 4.0,
            "range" => 18.0,
            "attack" => 0.000_1,
            // A slow release holds a compressor at the sine's peak. Expand, Gate,
            // and Duck sit at their range, so they can settle fast.
            "release" => {
                if mode < 2.0 {
                    2.0
                } else {
                    0.01
                }
            }
            _ => fallback,
        };
        let values: Vec<f32> = DYNAMICS.params.iter().map(|p| param(p.name, p.default)).collect();
        let mut harness = Harness::new(&DYNAMICS).seconds(2.0).input(
            "in",
            Signal::from_fn(move |frame| 10.0f32.powf(level / 20.0) * (frame as f32 * 0.05).sin()),
        );
        for (spec, value) in DYNAMICS.params.iter().zip(&values) {
            harness = harness.param(spec.name, *value);
        }
        let render = harness.render();
        let measured = level + db(peak(render.output("gain"), 4_800));
        let drawn = sample(&values, (level + 60.0) / 60.0) * 60.0 - 60.0;
        assert!(
            (measured - drawn).abs() < 0.1,
            "mode {mode}: heard {measured}, drawn {drawn}"
        );
    }
}

/// The Crossover's outputs sum to a flat level, with two bands and with three.
#[test]
fn the_crossover_sums_flat() {
    for bands in [0.0, 1.0] {
        for hz in [50.0f32, 200.0, 1_000.0, 2_000.0, 10_000.0] {
            let step = std::f32::consts::TAU * hz / RATE;
            let render = Harness::new(&CROSSOVER)
                .seconds(1.0)
                .input("in", Signal::from_fn(move |f| 0.5 * (f as f32 * step).sin()))
                .param("bands", bands)
                .render();
            let (low, mid, high) = (render.output("low"), render.output("mid"), render.output("high"));
            let (mut sum, mut input) = (0.0f64, 0.0f64);
            for i in 24_000..48_000 {
                let y = low[i] + mid[i] + high[i];
                sum += f64::from(y * y);
                let x = 0.5 * (i as f32 * step).sin();
                input += f64::from(x * x);
            }
            let change = 10.0 * (sum / input).log10();
            assert!(change.abs() < 0.01, "{bands} bands at {hz} Hz: {change} dB");
        }
    }
}

/// With two bands, the mid output is silent.
#[test]
fn two_bands_silence_the_mid() {
    let render = Harness::new(&CROSSOVER)
        .seconds(0.1)
        .input("in", sine(0.5))
        .param("bands", 0.0)
        .render();
    assert!(render.output("mid").iter().all(|&x| x == 0.0));
}

/// One gain for both sides: a loud left turns a quiet right down by the same
/// amount, through the real engine. This is why the ports are `Stereo`.
#[test]
fn a_loud_side_turns_both_sides_down() {
    for block in [1, 77, 512] {
        let config = EngineConfig {
            max_block: 512,
            ..EngineConfig::with_sample_rate(RATE)
        };
        let mut parts = create_engine(loom_modules::registry(), config);
        let c = &mut parts.controller;
        let left = c.add_module("loom.math.constant").unwrap();
        let right = c.add_module("loom.math.constant").unwrap();
        let join = c.add_module("loom.mix.join").unwrap();
        let dynamics = c.add_module("loom.dyn.dynamics").unwrap();
        let split = c.add_module("loom.mix.split").unwrap();
        c.set_param_named(left, "value", 1.0).unwrap();
        c.set_param_named(right, "value", 0.01).unwrap();
        c.set_param_named(dynamics, "threshold", -20.0).unwrap();
        c.set_param_named(dynamics, "knee", 0.0).unwrap();
        c.set_param_named(dynamics, "attack", 0.000_1).unwrap();
        c.connect_named(left, "out", join, "left").unwrap();
        c.connect_named(right, "out", join, "right").unwrap();
        c.connect_named(join, "out", dynamics, "in").unwrap();
        c.connect_named(dynamics, "out", split, "in").unwrap();
        let left_out = c.output_ref(split, "left").unwrap();
        let right_out = c.output_ref(split, "right").unwrap();
        let left_probe = c.add_probe(left_out, ProbeKind::Scope).unwrap();
        let right_probe = c.add_probe(right_out, ProbeKind::Scope).unwrap();
        c.commit().unwrap();
        let (mut out_left, mut out_right) = (vec![0.0; block], vec![0.0; block]);
        let (mut last_left, mut last_right) = (0.0, 0.0);
        for _ in 0..RATE as usize / block {
            parts.engine.process(&mut out_left, &mut out_right);
            for chunk in parts.telemetry.scope() {
                let Some(&last) = chunk.samples().last() else {
                    continue;
                };
                if chunk.probe == left_probe {
                    last_left = last;
                } else if chunk.probe == right_probe {
                    last_right = last;
                }
            }
        }
        // 20 dB over at a ratio of 4 is 15 dB down.
        let expected = -15.0;
        assert!(
            (db(last_left) - expected).abs() < 0.1,
            "block {block}: left {}",
            db(last_left)
        );
        let right_gain = db(last_right / 0.01);
        assert!((right_gain - expected).abs() < 0.1, "block {block}: right {right_gain}");
    }
}
