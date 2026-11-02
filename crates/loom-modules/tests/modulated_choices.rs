//! Choice and toggle parameters follow modulation, one value per frame.
//!
//! Every parameter can be a modulation target. A module that only reads the
//! value `set_param` gave it ignores an assignment, and the knob lies.

use loom_core::{Controller, EngineConfig, EngineParts, ModuleId, OutputSafety, create_engine};
use loom_modules::register_all;
use loom_modules::test_modules;
use std::sync::Arc;

/// An engine with the built-in and test modules, and the safety stage off so
/// values are exact.
fn engine() -> EngineParts {
    let mut registry = loom_core::Registry::new();
    register_all(&mut registry).expect("built-ins");
    test_modules::register(&mut registry).expect("test modules");
    let config = EngineConfig {
        output_safety: OutputSafety::Off,
        ..EngineConfig::with_sample_rate(48_000.0)
    };
    let mut parts = create_engine(Arc::new(registry), config);
    // Play past the fade-in, which runs even with the stage off.
    let mut silence = [0.0; 1024];
    parts.engine.process(&mut silence.clone(), &mut silence);
    parts
}

/// Renders `frames` frames and returns the left channel.
fn render(parts: &mut EngineParts, frames: usize) -> Vec<f32> {
    let mut left = vec![0.0; frames];
    let mut right = vec![0.0; frames];
    parts.engine.process(&mut left, &mut right);
    left
}

/// Sends `port` of `module` to the left output.
fn listen(controller: &mut Controller, module: ModuleId, port: &str) {
    let audio = controller.add_module("loom.io.audio-out").unwrap();
    controller.connect_named(module, port, audio, "in").unwrap();
}

/// Adds a module with one parameter set.
fn module(controller: &mut Controller, kind: &str, param: &str, value: f32) -> ModuleId {
    let id = controller.add_module(kind).unwrap();
    controller.set_param_named(id, param, value).unwrap();
    id
}

/// Adds a constant source (`test.linear`) of the given level.
fn source(controller: &mut Controller, level: f32) -> ModuleId {
    module(controller, "test.linear", "level", level)
}

/// A full-depth source of 1 on a choice moves it from its base to its top.
#[test]
fn wrap_follows_a_modulated_mode() {
    let mut parts = engine();
    let input = source(&mut parts.controller, 1.0);
    let amount = source(&mut parts.controller, 1.0);
    let wrap = parts.controller.add_module("loom.phase.wrap").unwrap();
    parts.controller.connect_named(input, "out", wrap, "in").unwrap();
    parts
        .controller
        .add_assignment_named(amount, "out", wrap, "mode", 1.0)
        .unwrap();
    listen(&mut parts.controller, wrap, "out");
    parts.controller.commit().unwrap();
    // Wrap sends 1.0 to 0; Fold keeps it at 1.
    assert_eq!(render(&mut parts, 16)[8], 1.0);
}

/// Crossfade at mix 0.5 gives 0.7071 of A (equal power); modulated to
/// linear it gives 0.5.
#[test]
fn crossfade_follows_a_modulated_law() {
    let mut parts = engine();
    let a = source(&mut parts.controller, 1.0);
    let amount = source(&mut parts.controller, 1.0);
    let fade = parts.controller.add_module("loom.mix.crossfade").unwrap();
    parts.controller.connect_named(a, "out", fade, "a").unwrap();
    parts
        .controller
        .add_assignment_named(amount, "out", fade, "law", 1.0)
        .unwrap();
    listen(&mut parts.controller, fade, "out");
    parts.controller.commit().unwrap();
    assert!((render(&mut parts, 16)[8] - 0.5).abs() < 1e-6);
}

/// A mute modulated on silences the input, with the usual fade.
#[test]
fn mixer_follows_a_modulated_mute() {
    let mut parts = engine();
    let input = source(&mut parts.controller, 1.0);
    let amount = source(&mut parts.controller, 1.0);
    let mixer = parts.controller.add_module("loom.mix.mixer").unwrap();
    parts.controller.connect_named(input, "out", mixer, "in1").unwrap();
    parts
        .controller
        .add_assignment_named(amount, "out", mixer, "mute1", 1.0)
        .unwrap();
    listen(&mut parts.controller, mixer, "out");
    parts.controller.commit().unwrap();
    let left = render(&mut parts, 960);
    assert!(left[0] > 0.9, "the fade starts near full level: {}", left[0]);
    assert!(left[959] < 0.01, "and ends silent: {}", left[959]);
}

/// Compare at A = 0.2 against B = 0.5 is low in Above; modulated to Below
/// it is high.
#[test]
fn compare_follows_a_modulated_mode() {
    let mut parts = engine();
    let a = source(&mut parts.controller, 0.2);
    let b = source(&mut parts.controller, 0.5);
    let amount = source(&mut parts.controller, 1.0);
    let compare = parts.controller.add_module("loom.logic.compare").unwrap();
    parts.controller.connect_named(a, "out", compare, "a").unwrap();
    parts.controller.connect_named(b, "out", compare, "b").unwrap();
    parts
        .controller
        .add_assignment_named(amount, "out", compare, "mode", 1.0)
        .unwrap();
    listen(&mut parts.controller, compare, "gate");
    parts.controller.commit().unwrap();
    assert_eq!(render(&mut parts, 16)[8], 1.0);
}

/// One high input into And is high. Modulating `invert` on makes it low.
#[test]
fn logic_follows_a_modulated_invert() {
    let mut parts = engine();
    let high = source(&mut parts.controller, 1.0);
    let amount = source(&mut parts.controller, 1.0);
    let logic = parts.controller.add_module("loom.logic.logic").unwrap();
    parts.controller.connect_named(high, "out", logic, "in1").unwrap();
    parts
        .controller
        .add_assignment_named(amount, "out", logic, "invert", 1.0)
        .unwrap();
    listen(&mut parts.controller, logic, "out");
    parts.controller.commit().unwrap();
    assert_eq!(render(&mut parts, 16)[8], 0.0);
}

/// Logic's `op` modulated up to Xor: two high inputs are low (even count).
#[test]
fn logic_follows_a_modulated_op() {
    let mut parts = engine();
    let one = source(&mut parts.controller, 1.0);
    let two = source(&mut parts.controller, 1.0);
    let amount = source(&mut parts.controller, 1.0);
    let logic = parts.controller.add_module("loom.logic.logic").unwrap();
    parts.controller.connect_named(one, "out", logic, "in1").unwrap();
    parts.controller.connect_named(two, "out", logic, "in2").unwrap();
    parts
        .controller
        .add_assignment_named(amount, "out", logic, "op", 1.0)
        .unwrap();
    listen(&mut parts.controller, logic, "out");
    parts.controller.commit().unwrap();
    assert_eq!(render(&mut parts, 16)[8], 0.0);
}

/// Memory in Data mode takes the level of A on a clock edge. Set/Reset
/// mode (the base) would ignore A. Modulated to Data (mode 2), a high A and
/// a rising clock give a high output.
#[test]
fn memory_follows_a_modulated_mode() {
    let mut parts = engine();
    let data = source(&mut parts.controller, 1.0);
    let clock = source(&mut parts.controller, 1.0);
    let amount = source(&mut parts.controller, 1.0);
    let memory = parts.controller.add_module("loom.logic.memory").unwrap();
    parts.controller.connect_named(data, "out", memory, "a").unwrap();
    parts.controller.connect_named(clock, "out", memory, "b").unwrap();
    parts
        .controller
        .add_assignment_named(amount, "out", memory, "mode", 1.0)
        .unwrap();
    listen(&mut parts.controller, memory, "out");
    parts.controller.commit().unwrap();
    // Both inputs rise on frame 0 (sources are 1.0 from the start). In Set
    // and reset, reset wins and the output stays low. In Data it goes high.
    assert_eq!(render(&mut parts, 16)[8], 1.0);
}

/// Sample and Hold takes the ramp's first value and holds it (about 0) in
/// Sample mode. Modulated to Track, it follows the ramp while the trigger is
/// high, so after 100 frames of a 261 Hz phase it is above 0.3.
#[test]
fn sample_hold_follows_a_modulated_mode() {
    let mut parts = engine();
    let ramp = parts.controller.add_module("loom.phase.generator").unwrap();
    let trigger = source(&mut parts.controller, 1.0);
    let amount = source(&mut parts.controller, 1.0);
    let hold = parts.controller.add_module("loom.rand.sample-hold").unwrap();
    parts.controller.connect_named(ramp, "phase", hold, "in").unwrap();
    parts.controller.connect_named(trigger, "out", hold, "trigger").unwrap();
    parts
        .controller
        .add_assignment_named(amount, "out", hold, "mode", 1.0)
        .unwrap();
    listen(&mut parts.controller, hold, "out");
    parts.controller.commit().unwrap();
    let left = render(&mut parts, 128);
    assert!(left[100] > 0.3, "{}", left[100]);
}

/// A walk stays inside its range while an assignment moves the range. With
/// `low` pulled up by a modulation source, the output stays above the new
/// `low` and below `high`, and still moves (M2 modules, Random).
#[test]
fn a_random_walk_stays_in_a_modulated_range() {
    let mut parts = engine();
    let ramp = parts.controller.add_module("loom.phase.generator").unwrap();
    let clock = parts.controller.add_module("loom.phase.trigger").unwrap();
    parts.controller.connect_named(ramp, "phase", clock, "phase").unwrap();
    let amount = source(&mut parts.controller, 1.0);
    let random = parts.controller.add_module("loom.rand.random").unwrap();
    parts
        .controller
        .connect_named(clock, "trigger", random, "trigger")
        .unwrap();
    let param = |name: &str| loom_modules::rand::RANDOM.param_index(name).unwrap();
    parts.controller.set_param(random, param("mode"), 1.0).unwrap();
    parts.controller.set_param(random, param("step"), 0.5).unwrap();
    parts
        .controller
        .add_assignment_named(amount, "out", random, "low", 0.25)
        .unwrap();
    listen(&mut parts.controller, random, "out");
    parts.controller.commit().unwrap();
    let left = render(&mut parts, 48_000);
    // The assignment moves `low` by a quarter of its -1 to 1 control: to 0.5.
    let settled = &left[4_800..];
    let (min, max) = settled
        .iter()
        .fold((f32::MAX, f32::MIN), |(low, high), &v| (low.min(v), high.max(v)));
    assert!(min >= 0.5 - 1e-3 && max <= 1.0, "{min} to {max}");
    assert!(max - min > 0.25, "the walk still moves: {min} to {max}");
}

/// What `output` of a seeded module plays on a fast clock, with or without an
/// assignment on its seed.
fn seeded_on_a_clock(kind: &str, trigger: &str, output: &str, modulate_seed: bool) -> Vec<f32> {
    let mut parts = engine();
    let ramp = parts.controller.add_module("loom.phase.generator").unwrap();
    let clock = parts.controller.add_module("loom.phase.trigger").unwrap();
    parts.controller.connect_named(ramp, "phase", clock, "phase").unwrap();
    let module = parts.controller.add_module(kind).unwrap();
    parts
        .controller
        .connect_named(clock, "trigger", module, trigger)
        .unwrap();
    if modulate_seed {
        let amount = source(&mut parts.controller, 1.0);
        parts
            .controller
            .add_assignment_named(amount, "out", module, "seed", 0.5)
            .unwrap();
    }
    listen(&mut parts.controller, module, output);
    parts.controller.commit().unwrap();
    render(&mut parts, 48_000)
}

/// A modulated seed picks another sequence, and it is still repeatable, as a
/// knob's seed is (M2 modules, rule 1).
#[test]
fn a_modulated_seed_picks_another_repeatable_sequence() {
    for (kind, trigger, output) in [
        ("loom.rand.random", "trigger", "out"),
        ("loom.rand.chance", "trigger1", "a1"),
    ] {
        let base = seeded_on_a_clock(kind, trigger, output, false);
        let modulated = seeded_on_a_clock(kind, trigger, output, true);
        assert_ne!(base, modulated, "{kind}");
        assert_eq!(modulated, seeded_on_a_clock(kind, trigger, output, true), "{kind}");
    }
}
