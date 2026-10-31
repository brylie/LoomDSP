//! The patch transport and default inputs (ADR-0015).
//! Spec: `docs/design/clock.md`, acceptance criteria for the engine.

mod common;

use common::out::*;
use common::*;
use loom_core::{
    InputRef, LoomError, ModuleConfig, ModuleRole, TimeSignature, TransportInfo, TransportSettings, testing,
};

/// Beats per sample at 120 BPM and 48 kHz.
const BPS_120: f64 = 120.0 / 60.0 / 48_000.0;

fn bar_phase_120(frame: usize) -> f64 {
    let position = frame as f64 * BPS_120 / 4.0;
    position - position.floor()
}

/// Distance between two phases, going round the circle.
fn phase_distance(a: f64, b: f64) -> f64 {
    let d = (a - b).abs();
    d.min(1.0 - d)
}

#[test]
fn a_new_engine_runs_at_120_in_four_four() {
    let parts = engine(128);
    assert_eq!(parts.controller.transport(), TransportSettings::default());
    assert_eq!(
        TransportSettings::default(),
        TransportSettings {
            tempo: 120.0,
            time_signature: TimeSignature::FOUR_FOUR,
            running: true,
        }
    );
}

#[test]
fn unconnected_inputs_follow_their_defaults_and_cables_override_them() {
    let mut parts = engine(128);
    let follower = parts.controller.add_module("test.follower").unwrap();
    let source = parts.controller.add_module("test.follower").unwrap();
    let mut capture = Capture::new(
        &mut parts,
        &[
            (follower, BAR),
            (follower, BEAT),
            (follower, PLAY),
            (follower, VALUE),
            (follower, CONNECTED),
        ],
    );
    parts.controller.commit().unwrap();
    render(&mut parts, &mut capture, 4096, 128, |_, _| {});
    for i in 0..4096 {
        assert!(
            phase_distance(f64::from(capture.get(0)[i]), bar_phase_120(i)) < 1e-6,
            "bar at {i}"
        );
        let beat = (i as f64 * BPS_120).fract();
        assert!(phase_distance(f64::from(capture.get(1)[i]), beat) < 1e-6, "beat at {i}");
    }
    assert!(capture.get(2).iter().all(|&x| x == 1.0), "playing");
    assert!(capture.get(3).iter().all(|&x| x == 0.25), "value default");
    assert!(capture.get(4).iter().all(|&x| x == 0.0), "not connected");

    // A cable replaces the default for this module.
    parts
        .controller
        .connect_named(source, "value", follower, "bar")
        .unwrap();
    parts.controller.commit().unwrap();
    render(&mut parts, &mut capture, 1024, 128, |_, _| {});
    assert!(capture.get(0)[4096..].iter().all(|&x| x == 0.25), "cable overrides");
    assert!(capture.get(4)[4096..].iter().all(|&x| x == 1.0), "connected");

    // Removing the cable returns to the transport.
    parts
        .controller
        .disconnect(InputRef {
            module: follower,
            port: 0,
        })
        .unwrap();
    parts.controller.commit().unwrap();
    render(&mut parts, &mut capture, 1024, 128, |_, _| {});
    for i in 5120..6144 {
        assert!(
            phase_distance(f64::from(capture.get(0)[i]), bar_phase_120(i)) < 1e-6,
            "bar at {i}"
        );
    }
    assert!(capture.get(4)[5120..].iter().all(|&x| x == 0.0), "not connected again");
}

/// Plays through a tempo change, a stop, a start, a reset, and a new time
/// signature, at the given block size. Returns every captured output.
fn eventful_run(block: usize) -> Vec<Vec<f32>> {
    let mut parts = engine(block);
    let plain = parts.controller.add_module("test.follower").unwrap();
    // A self-loop makes the engine call this one a frame at a time.
    let looped = parts.controller.add_module("test.follower").unwrap();
    parts
        .controller
        .connect_named(looped, "feedback", looped, "feedback")
        .unwrap();
    let mut capture = Capture::new(
        &mut parts,
        &[
            (plain, BAR),
            (plain, BEAT),
            (plain, PLAY),
            (plain, JUMPED),
            (plain, CTX_BAR),
            (looped, BAR),
            (looped, JUMPED),
            (looped, CTX_BAR),
        ],
    );
    let report = parts.controller.commit().unwrap();
    assert_eq!(report.loops.len(), 1);
    render(&mut parts, &mut capture, 16 * 1024, block, |frame, parts| {
        let controller = &mut parts.controller;
        match frame {
            2048 => assert_eq!(controller.set_tempo(90.0).unwrap(), 90.0),
            4096 => controller.set_running(false).unwrap(),
            6144 => controller.set_running(true).unwrap(),
            8192 => controller.reset_transport().unwrap(),
            10_240 => controller.set_time_signature(TimeSignature::new(7, 8)).unwrap(),
            _ => {}
        }
    });
    (0..8).map(|i| capture.get(i).to_vec()).collect()
}

#[test]
fn transport_signals_do_not_depend_on_block_size() {
    let reference = eventful_run(1024);
    for block in [1, 32] {
        assert!(eventful_run(block) == reference, "block size {block} differs from 1024");
    }

    let [bar, beat, play, jumped, ctx_bar, loop_bar, loop_jumped, loop_ctx_bar] = &reference[..] else {
        unreachable!()
    };
    assert_eq!(bar.len(), 16 * 1024);
    // Modules that compute from their ProcessContext agree with the shared
    // buffers, bit for bit, inside feedback loops too.
    assert_eq!(ctx_bar, bar);
    assert_eq!(loop_bar, bar);
    assert_eq!(loop_ctx_bar, bar);

    // The tempo change keeps the position: no phase moves by more than one
    // step at the faster tempo.
    let step_120 = BPS_120 / 4.0;
    let step_90 = 90.0 / 60.0 / 48_000.0 / 4.0;
    for i in 1..4096 {
        let step = (f64::from(bar[i]) - f64::from(bar[i - 1])).rem_euclid(1.0);
        assert!(step < step_120 * 1.001, "bar phase jumped at {i}: {step}");
    }
    let before = f64::from(bar[2047]) - f64::from(bar[2046]);
    let after = f64::from(bar[2049]) - f64::from(bar[2048]);
    assert!((before - step_120).abs() < 1e-7);
    assert!((after - step_90).abs() < 1e-7);

    // Stopping holds the position; starting continues from there.
    assert!(play[..4096].iter().all(|&x| x == 1.0));
    assert!(play[4096..6144].iter().all(|&x| x == 0.0));
    assert!(play[6144..].iter().all(|&x| x == 1.0));
    let held = bar[4096];
    assert!(held > bar[4095]);
    assert!(bar[4096..=6144].iter().all(|&x| x == held), "stopped phase holds");
    assert!(bar[6145] > held, "running continues");

    // Reset returns to bar 1, and every module sees one jump. A new time
    // signature is a jump too (rule 10), seen once, at the next buffer.
    assert_eq!(bar[8192], 0.0);
    assert_eq!(beat[8192], 0.0);
    for jumps in [jumped, loop_jumped] {
        let at: Vec<usize> = (0..jumps.len()).filter(|&i| jumps[i] != 0.0).collect();
        assert_eq!(at, vec![8192, 10_240]);
    }
}

#[test]
fn transport_stays_in_step_for_an_hour() {
    // Acceptance: after one hour at 120 BPM and 48 kHz, bar phase at the
    // last sample matches the exact value within one sample.
    let hour = 3_600 * 48_000;
    let mut parts = engine(1024);
    let bar = parts.controller.add_module("test.bar").unwrap();
    parts.controller.commit().unwrap();
    let mut nothing = Capture::new(&mut parts, &[]);
    render(&mut parts, &mut nothing, hour - 1024, 1024, |_, _| {});
    let mut capture = Capture::new(&mut parts, &[(bar, 0)]);
    parts.controller.commit().unwrap();
    render(&mut parts, &mut capture, 1024, 1024, |_, _| {});

    let measured = f64::from(capture.get(0)[1023]);
    // One bar is exactly 96 000 samples here, so the exact phase is a ratio
    // of integers.
    let exact = ((hour - 1) % 96_000) as f64 / 96_000.0;
    assert!(
        phase_distance(measured, exact) < 1.0 / 96_000.0,
        "{measured} vs {exact}"
    );
}

#[test]
fn tempo_is_clamped_and_bad_time_signatures_are_refused() {
    let mut parts = engine(128);
    assert_eq!(parts.controller.set_tempo(5.0).unwrap(), 20.0);
    assert_eq!(parts.controller.transport().tempo, 20.0);
    assert_eq!(parts.controller.set_tempo(500.0).unwrap(), 300.0);
    assert_eq!(parts.controller.set_tempo(120.0).unwrap(), 120.0);

    let before = parts.controller.transport();
    for bad in [
        TimeSignature::new(7, 3),
        TimeSignature::new(0, 4),
        TimeSignature::new(33, 8),
    ] {
        let error = parts.controller.set_time_signature(bad).unwrap_err();
        assert!(
            matches!(error, LoomError::InvalidTimeSignature(ts) if ts == bad),
            "{error}"
        );
    }
    assert_eq!(parts.controller.transport(), before);

    // The engine still plays in 4/4.
    let follower = parts.controller.add_module("test.follower").unwrap();
    let mut capture = Capture::new(&mut parts, &[(follower, BAR)]);
    parts.controller.commit().unwrap();
    render(&mut parts, &mut capture, 256, 128, |_, _| {});
    for i in 0..256 {
        assert!(phase_distance(f64::from(capture.get(0)[i]), bar_phase_120(i)) < 1e-6);
    }
}

#[test]
fn modules_can_be_tested_with_a_transport() {
    let ModuleRole::Dsp(factory) = FOLLOWER.role else {
        unreachable!()
    };
    let mut module = factory(&ModuleConfig {
        sample_rate: SAMPLE_RATE,
        max_block: 4,
    });
    let zeros = [0.0f32; 4];
    let inputs: [&[f32]; 5] = [&zeros; 5];
    let mut outputs = [[0.0f32; 4]; 8];

    // The default test transport is stopped at position 0.
    let mut outs: Vec<&mut [f32]> = outputs.iter_mut().map(|o| &mut o[..]).collect();
    testing::run(module.as_mut(), SAMPLE_RATE, &inputs, &[false; 5], &mut outs, &[]);
    assert_eq!(outputs[CTX_BAR], [0.0; 4]);
    assert_eq!(outputs[JUMPED], [0.0; 4]);

    // Two beats into a 4/4 bar, just after a jump.
    let transport = TransportInfo::new(SAMPLE_RATE, TransportSettings::default(), 2.0).with_jump(true);
    let mut outs: Vec<&mut [f32]> = outputs.iter_mut().map(|o| &mut o[..]).collect();
    testing::run_with_transport(
        module.as_mut(),
        SAMPLE_RATE,
        transport,
        &inputs,
        &[false; 5],
        &mut outs,
        &[],
    );
    assert_eq!(outputs[CTX_BAR][0], 0.5);
    assert!(outputs[CTX_BAR][3] > 0.5);
    assert_eq!(outputs[JUMPED], [1.0, 0.0, 0.0, 0.0]);
}
