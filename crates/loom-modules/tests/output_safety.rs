//! The safety stage on the final output (ADR-0019). Each test is an
//! acceptance criterion of `docs/design/output-safety.md`.

mod common;

use common::*;
use loom_core::safety::{CEILING, FADE_SECONDS, LOOKAHEAD_SECONDS, samples};
use loom_core::{EngineConfig, EngineParts, ModuleId, OutputSafety, ProbeKind, create_engine};

const MODES: [OutputSafety; 3] = [OutputSafety::LowLatency, OutputSafety::Clean, OutputSafety::Off];
const BLOCKS: [usize; 3] = [1, 77, 512];

fn engine(mode: OutputSafety, sample_rate: f32) -> EngineParts {
    let config = EngineConfig {
        output_safety: mode,
        ..EngineConfig::with_sample_rate(sample_rate)
    };
    create_engine(loom_modules::registry(), config)
}

/// Patches `source`'s `out` into both sides of an Audio Out.
fn play(parts: &mut EngineParts, source: ModuleId) {
    let c = &mut parts.controller;
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.connect_named(source, "out", out, "left").unwrap();
    c.connect_named(source, "out", out, "right").unwrap();
}

/// A Constant of `value` into Audio Out.
fn constant(mode: OutputSafety, value: f32) -> EngineParts {
    let mut parts = engine(mode, SAMPLE_RATE);
    let c = &mut parts.controller;
    let constant = c.add_module("loom.math.constant").unwrap();
    c.set_param_named(constant, "value", value).unwrap();
    play(&mut parts, constant);
    parts.controller.commit().unwrap();
    parts
}

/// Noise at `gain` into Audio Out, and the Multiply that scales it.
fn noise(mode: OutputSafety, gain: f32) -> (EngineParts, ModuleId) {
    let mut parts = engine(mode, SAMPLE_RATE);
    let c = &mut parts.controller;
    let noise = c.add_module("loom.osc.noise").unwrap();
    let mul = c.add_module("loom.math.mul").unwrap();
    c.connect_named(noise, "out", mul, "a").unwrap();
    c.set_param_named(mul, "gain", gain).unwrap();
    play(&mut parts, mul);
    parts.controller.commit().unwrap();
    (parts, mul)
}

/// Renders at every block size in [`BLOCKS`], checks they agree bit for
/// bit, and returns the left channel.
fn render_all(mut make: impl FnMut() -> EngineParts, frames: usize) -> Vec<f32> {
    let renders: Vec<_> = BLOCKS.iter().map(|&b| render(&mut make(), frames, b, &[])).collect();
    for (render, block) in renders.iter().zip(BLOCKS) {
        assert_eq!(render.0, render.1, "both sides play the same");
        assert_eq!(render, &renders[0], "block size {block} matches block size 1");
    }
    renders.into_iter().next().unwrap().0
}

/// The first frame from `start` where `signal` is above `threshold` in size.
fn first_above(signal: &[f32], start: usize, threshold: f32) -> usize {
    start + signal[start..].iter().position(|x| x.abs() > threshold).unwrap()
}

#[test]
fn dc_is_removed() {
    for mode in [OutputSafety::LowLatency, OutputSafety::Clean] {
        let left = render_all(|| constant(mode, 1.0), 36_000);
        assert!(left[..12_000].iter().any(|&x| x > 0.5), "{mode:?}: the step is heard");
        assert!(
            left[24_000..].iter().all(|x| x.abs() < 0.001),
            "{mode:?}: the offset is gone after 0.5 s"
        );
    }
    let left = render_all(|| constant(OutputSafety::Off, 1.0), 12_000);
    assert!(left[2_000..].iter().all(|&x| x == 1.0), "Off keeps DC");
}

#[test]
fn full_scale_noise_is_held_under_the_ceiling() {
    for mode in [OutputSafety::LowLatency, OutputSafety::Clean] {
        let left = render_all(|| noise(mode, 4.0).0, 24_000);
        assert!(left.iter().all(|x| x.abs() <= CEILING), "{mode:?}");
        let peak = left.iter().fold(0.0f32, |p, x| p.max(x.abs()));
        assert!(peak > 0.8 * CEILING, "{mode:?}: still loud, only limited ({peak})");
    }
}

#[test]
fn each_mode_adds_its_latency() {
    for sample_rate in [44_100.0, 48_000.0] {
        for mode in MODES {
            // A trigger once per cycle of a Phase at 5 octaves below middle C,
            // scaled to 0.5: impulses under the ceiling.
            let mut parts = engine(mode, sample_rate);
            let c = &mut parts.controller;
            let phase = c.add_module("loom.phase.generator").unwrap();
            let trigger = c.add_module("loom.phase.trigger").unwrap();
            let mul = c.add_module("loom.math.mul").unwrap();
            c.set_param_named(phase, "octave", -5.0).unwrap();
            c.set_param_named(mul, "gain", 0.5).unwrap();
            c.connect_named(phase, "phase", trigger, "phase").unwrap();
            c.connect_named(trigger, "trigger", mul, "a").unwrap();
            let port = c.output_ref(mul, "out").unwrap();
            c.add_probe(port, ProbeKind::Scope).unwrap();
            play(&mut parts, mul);
            parts.controller.commit().unwrap();

            let (left, _) = render(&mut parts, 24_000, 128, &[]);
            let probed: Vec<f32> = parts.telemetry.scope().flat_map(|c| c.samples().to_vec()).collect();
            // After the fade-in and the limiter's recovery from any impulse
            // in it.
            let start = 12_000;
            let sent = first_above(&probed, start, 0.25);
            let heard = first_above(&left, start, 0.25);
            let expected = match mode {
                OutputSafety::Clean => samples(LOOKAHEAD_SECONDS, sample_rate),
                _ => 0,
            };
            assert_eq!(heard - sent, expected, "{mode:?} at {sample_rate} Hz");
        }
    }
}

#[test]
fn audio_fades_in_when_a_patch_loads() {
    let length = samples(FADE_SECONDS, SAMPLE_RATE);
    for mode in MODES {
        // Each mode's patch at 0.5, which no mode limits.
        let left = render_all(|| constant(mode, 0.5), 2 * length);
        assert_eq!(left[0], 0.0, "{mode:?}: the first sample is silent");
    }
    // With the stage off the output is the signal times the fade, so the
    // fade shows exactly: a straight line from 0 to 1 over 10 ms.
    let (mut parts, mul) = noise(OutputSafety::Off, 0.9);
    let port = parts.controller.output_ref(mul, "out").unwrap();
    parts.controller.add_probe(port, ProbeKind::Scope).unwrap();
    parts.controller.commit().unwrap();
    let (left, _) = render(&mut parts, 2 * length, 128, &[]);
    let probed: Vec<f32> = parts.telemetry.scope().flat_map(|c| c.samples().to_vec()).collect();
    for (i, (&out, &signal)) in left.iter().zip(&probed).enumerate() {
        let gain = (i as f32 / length as f32).min(1.0);
        assert_eq!(out, signal * gain, "frame {i}");
    }
}

#[test]
fn off_is_the_raw_output_clipped() {
    let (mut parts, mul) = noise(OutputSafety::Off, 4.0);
    let port = parts.controller.output_ref(mul, "out").unwrap();
    parts.controller.add_probe(port, ProbeKind::Scope).unwrap();
    parts.controller.commit().unwrap();
    let (left, _) = render(&mut parts, 4_800, 128, &[]);
    let probed: Vec<f32> = parts.telemetry.scope().flat_map(|c| c.samples().to_vec()).collect();
    let after_fade = samples(FADE_SECONDS, SAMPLE_RATE);
    for i in after_fade..left.len() {
        assert_eq!(left[i], probed[i].clamp(-1.0, 1.0), "frame {i}");
    }
}

#[test]
fn renders_are_bit_identical_in_every_mode() {
    // NFR-SND-02
    for mode in MODES {
        let a = render(&mut noise(mode, 4.0).0, 9_600, 128, &[]);
        let b = render(&mut noise(mode, 4.0).0, 9_600, 128, &[]);
        assert_eq!(a, b, "{mode:?}");
    }
}

#[test]
fn a_mode_change_dips_to_silence_and_comes_back() {
    let length = samples(FADE_SECONDS, SAMPLE_RATE);
    let (mut parts, _) = noise(OutputSafety::LowLatency, 0.5);
    render(&mut parts, 4_800, 128, &[]);
    parts.controller.set_output_safety(OutputSafety::Clean).unwrap();
    let (left, _) = render(&mut parts, 4_800, 128, &[]);
    let silent = left.iter().position(|&x| x == 0.0).unwrap();
    assert!(silent <= length + 1, "dips within the fade ({silent})");
    // The fade-out is a straight line down, so no sample is louder than
    // the signal at that point of the fade.
    for (i, x) in left[..silent].iter().enumerate() {
        assert!(x.abs() <= 0.5 * (1.0 - i as f32 / length as f32) + 0.03, "frame {i}");
    }
    assert!(rms(&left[3_000..]) > 0.1, "and plays again in the new mode");
}
