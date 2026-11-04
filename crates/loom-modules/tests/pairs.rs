//! Paired probes through a real engine (`docs/design/telemetry.md`, Paired
//! probes): both values of a frame come from the same engine frame, whether
//! the queue is drained or full.

mod common;

use common::*;
use loom_core::{EngineConfig, EngineParts, LoomError, ModuleId, OutputRef, PairChunk, ProbeKind, Telemetry};
use std::sync::atomic::Ordering;

/// An engine with two free-running oscillators at different pitches, and
/// nothing connected to the output.
fn two_oscillators(config: EngineConfig) -> (EngineParts, OutputRef, OutputRef) {
    let mut parts = loom_core::create_engine(loom_modules::registry(), config);
    let c = &mut parts.controller;
    let phase = c.add_module("loom.phase.generator").unwrap();
    let saw = c.add_module("loom.osc.saw").unwrap();
    let sine = c.add_module("loom.osc.sine").unwrap();
    c.connect_named(phase, "phase", saw, "phase").unwrap();
    c.connect_named(phase, "phase", sine, "phase").unwrap();
    let (x, y) = (c.output_ref(saw, "out").unwrap(), c.output_ref(sine, "out").unwrap());
    (parts, x, y)
}

/// Plays `frames` frames in blocks of 77, draining with `drain` after each.
fn play(parts: &mut EngineParts, frames: usize, mut drain: impl FnMut(&mut Telemetry)) -> Vec<f32> {
    let (mut left, mut right) = ([0.0; 77], [0.0; 77]);
    let mut out = Vec::new();
    for _ in 0..frames / 77 {
        parts.engine.process(&mut left, &mut right);
        out.extend_from_slice(&left);
        drain(&mut parts.telemetry);
    }
    out
}

/// Every pair holds the `x` and `y` of the same engine frame, as two scope
/// probes on the same outputs show, with the frame stamps in order.
#[test]
fn every_pair_holds_the_same_frame_of_both_signals() {
    let (mut parts, x, y) = two_oscillators(EngineConfig {
        scope_capacity: 16_384,
        ..EngineConfig::with_sample_rate(SAMPLE_RATE)
    });
    let pair = parts.controller.add_probe_pair(x, y, None).unwrap();
    let x_scope = parts.controller.add_probe(x, ProbeKind::Scope).unwrap();
    let y_scope = parts.controller.add_probe(y, ProbeKind::Scope).unwrap();
    parts.controller.commit().unwrap();
    let (mut pairs, mut xs, mut ys) = (Vec::new(), Vec::new(), Vec::new());
    let mut next = 0;
    play(&mut parts, 4800, |telemetry| {
        for chunk in telemetry.pairs() {
            assert_eq!(chunk.probe, pair);
            assert_eq!(chunk.frame, next, "no gap");
            next += u64::from(chunk.len);
            pairs.extend(chunk.pairs());
        }
        for chunk in telemetry.scope() {
            let side = if chunk.probe == x_scope { &mut xs } else { &mut ys };
            assert!(chunk.probe == x_scope || chunk.probe == y_scope);
            side.extend_from_slice(chunk.samples());
        }
    });
    assert!(pairs.len() > 4000);
    assert_eq!(pairs.len(), xs.len());
    for (i, pair) in pairs.iter().enumerate() {
        assert_eq!(*pair, [xs[i], ys[i]], "frame {i}");
    }
    assert_ne!(xs, ys, "the two signals differ, so this can see a swap");
    assert_eq!(parts.controller.stats().dropped_pairs.load(Ordering::Relaxed), 0);
}

/// With a full queue the newest chunks are dropped and counted, audio goes on,
/// and the chunks that remain still hold matched frames, in order.
#[test]
fn a_full_queue_drops_whole_chunks() {
    let (mut parts, x, y) = two_oscillators(EngineConfig {
        pair_capacity: 4,
        ..EngineConfig::with_sample_rate(SAMPLE_RATE)
    });
    parts.controller.add_probe_pair(x, y, None).unwrap();
    parts.controller.commit().unwrap();
    play(&mut parts, 770, |_| {});
    let stats = parts.controller.stats();
    assert!(stats.dropped_pairs.load(Ordering::Relaxed) > 0);
    let chunks: Vec<PairChunk> = parts.telemetry.pairs().collect();
    assert_eq!(chunks.len(), 4);
    assert_eq!(chunks[0].frame, 0, "the oldest are kept");
    assert!(chunks.windows(2).all(|c| c[0].frame < c[1].frame));
}

/// A pair never changes the sound.
#[test]
fn a_pair_does_not_change_the_output() {
    let reference = render(&mut load("sine.ron"), 9_600, 128, &one_note());
    let mut parts = load("sine.ron");
    let port = parts.controller.output_ref(ModuleId(3), "out").unwrap();
    let other = parts.controller.output_ref(ModuleId(4), "out").unwrap();
    parts.controller.add_probe_pair(port, other, None).unwrap();
    parts.controller.commit().unwrap();
    assert_eq!(render(&mut parts, 9_600, 128, &one_note()), reference);
}

/// An XY module with no path to the output still runs, because a pair keeps
/// its module scheduled, and it passes its inputs to the pair.
#[test]
fn an_xy_module_off_the_output_still_sends_pairs() {
    let mut parts = loom_core::create_engine(loom_modules::registry(), EngineConfig::with_sample_rate(SAMPLE_RATE));
    let c = &mut parts.controller;
    let (a, b) = (
        c.add_module("loom.math.constant").unwrap(),
        c.add_module("loom.math.constant").unwrap(),
    );
    let xy = c.add_module("loom.display.xy").unwrap();
    let value = c.graph().node(a).unwrap().descriptor.param_index("value").unwrap();
    c.set_param(a, value, 0.25).unwrap();
    c.set_param(b, value, -0.5).unwrap();
    c.connect_named(a, "out", xy, "x").unwrap();
    c.connect_named(b, "out", xy, "y").unwrap();
    let (x, y) = (c.output_ref(xy, "x_out").unwrap(), c.output_ref(xy, "y_out").unwrap());
    let input = c.input_ref(xy, "y").unwrap();
    c.add_probe_pair(x, y, Some(input)).unwrap();
    c.commit().unwrap();
    play(&mut parts, 770, |_| {});
    let last = parts
        .telemetry
        .pairs()
        .last()
        .expect("pairs arrive")
        .pairs()
        .last()
        .unwrap();
    assert_eq!(last, [0.25, -0.5]);
}

/// A patch with a stereo signal on an XY module's `x`, left at 0.25 and right
/// at 0.75, with or without a cable in `y`.
fn stereo_x(with_y: bool) -> [f32; 2] {
    let mut parts = loom_core::create_engine(loom_modules::registry(), EngineConfig::with_sample_rate(SAMPLE_RATE));
    let c = &mut parts.controller;
    let (a, b, d) = (
        c.add_module("loom.math.constant").unwrap(),
        c.add_module("loom.math.constant").unwrap(),
        c.add_module("loom.math.constant").unwrap(),
    );
    let join = c.add_module("loom.mix.join").unwrap();
    let xy = c.add_module("loom.display.xy").unwrap();
    let value = c.graph().node(a).unwrap().descriptor.param_index("value").unwrap();
    c.set_param(a, value, 0.25).unwrap();
    c.set_param(b, value, 0.75).unwrap();
    c.set_param(d, value, -0.5).unwrap();
    c.connect_named(a, "out", join, "left").unwrap();
    c.connect_named(b, "out", join, "right").unwrap();
    c.connect_named(join, "out", xy, "x").unwrap();
    if with_y {
        c.connect_named(d, "out", xy, "y").unwrap();
    }
    let (x, y) = (c.output_ref(xy, "x_out").unwrap(), c.output_ref(xy, "y_out").unwrap());
    let input = c.input_ref(xy, "y").unwrap();
    c.add_probe_pair(x, y, Some(input)).unwrap();
    c.commit().unwrap();
    play(&mut parts, 770, |_| {});
    parts
        .telemetry
        .pairs()
        .last()
        .expect("pairs arrive")
        .pairs()
        .last()
        .unwrap()
}

/// A stereo `x` reads as its mean when `y` has a cable.
#[test]
fn a_stereo_x_with_a_cable_in_y_reads_its_mean() {
    assert_eq!(stereo_x(true), [0.5, -0.5]);
}

/// A stereo `x` alone draws its left side across and its right side up.
#[test]
fn a_stereo_x_alone_draws_left_against_right() {
    assert_eq!(stereo_x(false), [0.25, 0.75]);
}

/// Removing the module removes its pair, and removing a pair stops it.
#[test]
fn a_pair_goes_with_its_module() {
    let (mut parts, x, y) = two_oscillators(EngineConfig::with_sample_rate(SAMPLE_RATE));
    let pair = parts.controller.add_probe_pair(x, y, None).unwrap();
    assert!(parts.controller.has_probe_pair(pair));
    parts.controller.remove_probe(pair).unwrap();
    assert!(!parts.controller.has_probe_pair(pair));
    let pair = parts.controller.add_probe_pair(x, y, None).unwrap();
    parts.controller.remove_module(y.module).unwrap();
    assert!(!parts.controller.has_probe_pair(pair));
}

/// The controller refuses an input that does not exist, or one on another
/// module, because either would read as having no cable and silently change
/// what the pair draws.
#[test]
fn a_pair_refuses_a_wrong_input() {
    let (mut parts, x, y) = two_oscillators(EngineConfig::with_sample_rate(SAMPLE_RATE));
    let c = &mut parts.controller;
    let missing = loom_core::InputRef {
        module: y.module,
        port: 99,
    };
    assert!(matches!(
        c.add_probe_pair(x, y, Some(missing)),
        Err(LoomError::NoSuchPort { .. })
    ));
    let elsewhere = c.input_ref(x.module, "phase").unwrap();
    assert!(matches!(
        c.add_probe_pair(x, y, Some(elsewhere)),
        Err(LoomError::NoSuchPort { .. })
    ));
    let own = c.input_ref(y.module, "phase").unwrap();
    assert!(c.add_probe_pair(x, y, Some(own)).is_ok());
    assert_eq!(c.probes().count(), 0, "a pair is not a port probe");
}
