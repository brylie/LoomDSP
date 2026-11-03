//! Both sides of a stereo signal make the same picks through a Selector
//! (ADR-0036). This needs the engine, because each side is its own instance,
//! which the harness does not run.

use loom_core::{Controller, EngineConfig, EngineParts, ModuleId, OutputSafety, create_engine};

/// Frames rendered in each stretch of a test.
const FRAMES: usize = 9_600;

/// An engine with the safety stage off, so the output reads as exact values.
fn engine() -> EngineParts {
    let config = EngineConfig {
        output_safety: OutputSafety::Off,
        ..EngineConfig::with_sample_rate(48_000.0)
    };
    let mut parts = create_engine(loom_modules::registry(), config);
    // Play past the fade-in, which runs even with the stage off.
    let mut silence = [0.0; 1024];
    parts.engine.process(&mut silence.clone(), &mut silence);
    parts
}

/// A constant source.
fn constant(controller: &mut Controller, value: f32) -> ModuleId {
    let id = controller.add_module("loom.math.constant").unwrap();
    controller.set_param_named(id, "value", value).unwrap();
    id
}

/// A random Selector on all eight inputs, each input holding its own number,
/// stepped many times a second and sent to the output. Returns the selector and
/// its first input's source, which a test can replace with a stereo cable.
fn patch(controller: &mut Controller) -> (ModuleId, ModuleId) {
    let selector = controller.add_module("loom.route.selector").unwrap();
    controller.set_param_named(selector, "mode", 2.0).unwrap();
    controller.set_param_named(selector, "count", 8.0).unwrap();
    controller.set_param_named(selector, "fade", 0.0).unwrap();
    let mut first = None;
    for n in 1..=8 {
        let source = constant(controller, n as f32 * 0.1);
        controller
            .connect_named(source, "out", selector, &format!("in{n}"))
            .unwrap();
        first.get_or_insert(source);
    }
    let phase = controller.add_module("loom.phase.generator").unwrap();
    let fired = controller.add_module("loom.phase.trigger").unwrap();
    controller.connect_named(phase, "phase", fired, "phase").unwrap();
    controller.connect_named(fired, "trigger", selector, "step").unwrap();
    let audio = controller.add_module("loom.io.audio-out").unwrap();
    controller.connect_named(selector, "out", audio, "in").unwrap();
    (selector, first.unwrap())
}

/// Makes the signal on input 1 stereo, so the selector runs as two copies. Both
/// sides carry the same value, so any difference at the output is a pick.
fn make_stereo(controller: &mut Controller, selector: ModuleId, source: ModuleId) {
    let join = controller.add_module("loom.mix.join").unwrap();
    controller.connect_named(source, "out", join, "left").unwrap();
    controller.connect_named(source, "out", join, "right").unwrap();
    controller.connect_named(join, "out", selector, "in1").unwrap();
}

/// `FRAMES` frames of output, both sides, in blocks of 128.
fn render(parts: &mut EngineParts) -> (Vec<f32>, Vec<f32>) {
    let (mut left, mut right) = (vec![0.0; FRAMES], vec![0.0; FRAMES]);
    for (l, r) in left.chunks_mut(128).zip(right.chunks_mut(128)) {
        parts.engine.process(l, r);
    }
    (left, right)
}

/// How many different values `samples` holds, in thousandths.
fn distinct(samples: &[f32]) -> usize {
    let mut values: Vec<i32> = samples.iter().map(|value| (value * 1_000.0).round() as i32).collect();
    values.sort_unstable();
    values.dedup();
    values.len()
}

/// A selector loaded as stereo makes the same picks on both sides.
#[test]
fn a_stereo_load_picks_the_same_on_both_sides() {
    let mut parts = engine();
    let (selector, source) = patch(&mut parts.controller);
    make_stereo(&mut parts.controller, selector, source);
    parts.controller.commit().unwrap();
    let (left, right) = render(&mut parts);
    assert!(distinct(&left) >= 4, "it must really pick: {}", distinct(&left));
    assert_eq!(left, right);
}

/// A selector that turns stereo during playback carries on with the same picks
/// on both sides, from the frame of the swap.
#[test]
fn turning_stereo_during_playback_picks_the_same_on_both_sides() {
    let mut parts = engine();
    let (selector, source) = patch(&mut parts.controller);
    parts.controller.commit().unwrap();
    render(&mut parts);

    make_stereo(&mut parts.controller, selector, source);
    parts.controller.commit().unwrap();
    let (left, right) = render(&mut parts);
    assert!(distinct(&left) >= 4, "it must really pick: {}", distinct(&left));
    assert_eq!(left, right);
}

/// A stereo modulation source on a parameter can part the sides, because each
/// copy gets its own side (stereo rule 17). The guarantee names this case.
#[test]
fn a_stereo_modulation_source_can_part_the_sides() {
    let mut parts = engine();
    let (selector, source) = patch(&mut parts.controller);
    make_stereo(&mut parts.controller, selector, source);
    parts.controller.set_param_named(selector, "count", 2.0).unwrap();
    let (low, high) = (
        constant(&mut parts.controller, 0.0),
        constant(&mut parts.controller, 1.0),
    );
    let join = parts.controller.add_module("loom.mix.join").unwrap();
    parts.controller.connect_named(low, "out", join, "left").unwrap();
    parts.controller.connect_named(high, "out", join, "right").unwrap();
    parts
        .controller
        .add_assignment_named(join, "out", selector, "count", 1.0)
        .unwrap();
    parts.controller.commit().unwrap();
    let (left, right) = render(&mut parts);
    assert_ne!(left, right);
}
