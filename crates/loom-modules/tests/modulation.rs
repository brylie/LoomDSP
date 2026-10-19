//! Engine acceptance tests for parameter modulation.

use loom_core::{
    AssignmentId, Channels, Controller, EngineConfig, EngineParts, MidiEvent, Module, ModuleConfig, ModuleDescriptor,
    ModuleId, ModuleRole, OutputSafety, Panel, ParamOptions, ParamSpec, Polarity, PortDefault, PortIo, PortSpec,
    ProbeKind, ProcessContext, SignalType, create_engine,
};
use loom_modules::register_all;
use std::sync::Arc;

struct Mirror {
    value: f32,
}

impl Module for Mirror {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let samples = io.modulated(0);
        let ([], [out]) = io.split::<0, 1>();
        for (i, sample) in out.iter_mut().enumerate() {
            *sample = samples.map(|values| values[i]).unwrap_or(self.value);
        }
    }

    fn set_param(&mut self, _index: usize, value: f32) {
        self.value = value;
    }
}

fn mirror(_config: &ModuleConfig) -> Box<dyn Module> {
    Box::new(Mirror { value: 0.0 })
}

const fn test_param(name: &'static str, min: f32, max: f32, default: f32, unit: &'static str) -> ParamSpec {
    ParamSpec {
        name,
        label: name,
        description: "A test parameter.",
        min,
        max,
        default,
        unit,
        options: ParamOptions::DEFAULT,
    }
}

macro_rules! mirror_module {
    ($name:ident, $id:literal, $param:expr) => {
        static $name: ModuleDescriptor = ModuleDescriptor {
            id: $id,
            version: 1,
            name: $id,
            category: "Test",
            description: "Writes its parameter, or the modulated buffer.",
            inputs: &[],
            outputs: &[PortSpec {
                name: "out",
                label: "Out",
                signal: SignalType::Modulation,
                description: "The parameter, or its modulated value.",
                default: PortDefault::Value(0.0),
                polarity: Polarity::Bipolar,
                channels: Channels::PerChannel,
            }],
            params: &[$param],
            role: ModuleRole::Dsp(mirror),
            panel: Panel::EMPTY,
            displays: &[],
        };
    };
}

mirror_module!(LINEAR, "test.linear", test_param("level", 0.0, 1.0, 0.0, ""));
mirror_module!(
    LOG,
    "test.log",
    test_param("cutoff", 20.0, 20_000.0, 1_000.0, "Hz").logarithmic()
);
mirror_module!(STEPS, "test.steps", test_param("steps", 0.0, 10.0, 0.0, "").integer());

fn engine() -> EngineParts {
    let mut registry = loom_core::Registry::new();
    register_all(&mut registry).expect("built-ins");
    registry.register(&LINEAR).expect("linear");
    registry.register(&LOG).expect("log");
    registry.register(&STEPS).expect("steps");
    // These tests read exact parameter values through Audio Out, so the
    // safety stage is off (ADR-0019).
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

fn render(parts: &mut EngineParts, frames: usize, block: usize) -> Vec<f32> {
    let mut left = vec![0.0; frames];
    let mut right = vec![0.0; frames];
    let mut position = 0;
    while position < frames {
        let n = block.min(frames - position);
        parts
            .engine
            .process(&mut left[position..position + n], &mut right[position..position + n]);
        position += n;
    }
    left
}

/// Reads a module output through a scope probe, so values outside -1 to 1
/// are not clipped the way the final audio output is.
fn capture(parts: &mut EngineParts, module: ModuleId, frames: usize, block: usize) -> Vec<f32> {
    let output = parts.controller.output_ref(module, "out").unwrap();
    parts.controller.add_probe(output, ProbeKind::Scope).unwrap();
    parts.controller.commit().unwrap();
    let mut left = vec![0.0; block];
    let mut right = vec![0.0; block];
    let mut done = 0;
    while done < frames {
        let n = block.min(frames - done);
        parts.engine.process(&mut left[..n], &mut right[..n]);
        done += n;
    }
    let mut got = Vec::with_capacity(frames);
    for chunk in parts.telemetry.scope() {
        got.extend_from_slice(chunk.samples());
    }
    got
}

fn listen(controller: &mut Controller, module: ModuleId, port: &str) {
    let audio = controller.add_module("loom.io.audio-out").unwrap();
    controller.connect_named(module, port, audio, "left").unwrap();
}

fn module(controller: &mut Controller, kind: &str, param: &str, value: f32) -> ModuleId {
    let id = controller.add_module(kind).unwrap();
    controller.set_param_named(id, param, value).unwrap();
    id
}

#[test]
fn constant_source_lands_on_the_midpoint() {
    let mut parts = engine();
    let source = module(&mut parts.controller, "test.linear", "level", 1.0);
    let sink = parts.controller.add_module("test.linear").unwrap();
    parts
        .controller
        .add_assignment_named(source, "out", sink, "level", 0.5)
        .unwrap();
    listen(&mut parts.controller, sink, "out");
    parts.controller.commit().unwrap();

    let left = render(&mut parts, 32, 32);
    assert!(left.iter().all(|&sample| sample == 0.5), "{left:?}");
}

#[test]
fn cutoff_depth_moves_a_tenth_of_the_knob() {
    let spec = test_param("cutoff", 20.0, 20_000.0, 1_000.0, "Hz").logarithmic();
    let expected = spec.denormalize(spec.normalize(1_000.0) + 0.1);

    let mut parts = engine();
    let source = module(&mut parts.controller, "test.linear", "level", 1.0);
    let sink = parts.controller.add_module("test.log").unwrap();
    parts
        .controller
        .add_assignment_named(source, "out", sink, "cutoff", 0.1)
        .unwrap();
    let left = capture(&mut parts, sink, 8, 8);
    assert!(left.iter().all(|&sample| sample == expected), "{left:?} vs {expected}");
}

#[test]
fn assignments_sum_then_clamp_and_integers_stay_whole() {
    let mut parts = engine();
    let a = module(&mut parts.controller, "test.linear", "level", 1.0);
    let b = module(&mut parts.controller, "test.linear", "level", 1.0);
    let sink = parts.controller.add_module("test.linear").unwrap();
    parts
        .controller
        .add_assignment_named(a, "out", sink, "level", 0.25)
        .unwrap();
    parts
        .controller
        .add_assignment_named(b, "out", sink, "level", 0.25)
        .unwrap();
    listen(&mut parts.controller, sink, "out");
    parts.controller.commit().unwrap();
    assert!(render(&mut parts, 4, 4).iter().all(|&sample| sample == 0.5));

    let mut parts = engine();
    let a = module(&mut parts.controller, "test.linear", "level", 1.0);
    let b = module(&mut parts.controller, "test.linear", "level", 1.0);
    let sink = parts.controller.add_module("test.linear").unwrap();
    parts
        .controller
        .add_assignment_named(a, "out", sink, "level", 0.75)
        .unwrap();
    parts
        .controller
        .add_assignment_named(b, "out", sink, "level", 0.75)
        .unwrap();
    listen(&mut parts.controller, sink, "out");
    parts.controller.commit().unwrap();
    assert!(render(&mut parts, 4, 4).iter().all(|&sample| sample == 1.0));

    let spec = test_param("steps", 0.0, 10.0, 0.0, "").integer();
    let expected = spec.denormalize(0.125);
    assert_eq!(expected, 1.0);

    let mut parts = engine();
    let source = module(&mut parts.controller, "test.linear", "level", 1.0);
    let sink = parts.controller.add_module("test.steps").unwrap();
    parts
        .controller
        .add_assignment_named(source, "out", sink, "steps", 0.125)
        .unwrap();
    listen(&mut parts.controller, sink, "out");
    parts.controller.commit().unwrap();
    assert!(render(&mut parts, 4, 4).iter().all(|&sample| sample == expected));
}

#[test]
fn depth_zero_matches_no_assignment() {
    fn run(assign: bool) -> Vec<f32> {
        let mut parts = engine();
        let source = parts.controller.add_module("loom.phase.generator").unwrap();
        let sink = parts.controller.add_module("test.log").unwrap();
        if assign {
            parts
                .controller
                .add_assignment_named(source, "phase", sink, "cutoff", 0.0)
                .unwrap();
        }
        capture(&mut parts, sink, 256, 64)
    }

    let with = run(true);
    let without = run(false);
    assert_eq!(with, without);
    assert!(with.iter().all(|&sample| sample == 1_000.0));
}

#[test]
fn modulated_output_is_independent_of_block_size() {
    fn run(block: usize) -> Vec<f32> {
        let mut parts = engine();
        let source = parts.controller.add_module("loom.phase.generator").unwrap();
        let sink = parts.controller.add_module("test.linear").unwrap();
        parts
            .controller
            .add_assignment_named(source, "phase", sink, "level", 0.2)
            .unwrap();
        listen(&mut parts.controller, sink, "out");
        parts.controller.commit().unwrap();
        render(&mut parts, 512, block)
    }

    let small = run(1);
    let large = run(128);
    assert_eq!(small, large);
    assert_ne!(small[0], small[200]);
}

#[test]
fn assignment_loop_delays_the_assignment_and_plays() {
    let mut parts = engine();
    let source = module(&mut parts.controller, "test.linear", "level", 0.5);
    let filter = parts.controller.add_module("loom.math.add").unwrap();
    parts.controller.connect_named(source, "out", filter, "a").unwrap();
    parts
        .controller
        .add_assignment_named(filter, "out", source, "level", 0.5)
        .unwrap();
    listen(&mut parts.controller, filter, "out");
    let report = parts.controller.commit().unwrap();
    assert_eq!(report.inserted_assignment_delays().count(), 1);
    assert_eq!(report.inserted_delays().count(), 0);

    let left = render(&mut parts, 8, 8);
    assert_eq!(left[0], 0.5);
    assert_eq!(left[1], 0.75);
    assert_eq!(left[2], 0.875);
    assert!(left.iter().all(|sample| sample.is_finite()));
}

#[test]
fn self_assignment_is_a_one_sample_loop() {
    let mut parts = engine();
    let sink = module(&mut parts.controller, "test.linear", "level", 0.5);
    parts
        .controller
        .add_assignment_named(sink, "out", sink, "level", 1.0)
        .unwrap();
    listen(&mut parts.controller, sink, "out");
    let report = parts.controller.commit().unwrap();
    assert_eq!(report.inserted_assignment_delays().count(), 1);
    assert_eq!(report.inserted_delays().count(), 0);

    let left = render(&mut parts, 4, 4);
    assert_eq!(&left[..3], &[0.5, 1.0, 1.0]);
}

#[test]
fn polarity_override_remaps_a_bipolar_source() {
    fn run(polarity: Option<Polarity>) -> f32 {
        let mut parts = engine();
        let source = parts.controller.add_module("test.linear").unwrap();
        let sink = parts.controller.add_module("test.linear").unwrap();
        let id = parts
            .controller
            .add_assignment_named(source, "out", sink, "level", 1.0)
            .unwrap();
        if let Some(polarity) = polarity {
            parts.controller.set_assignment_polarity(id, Some(polarity)).unwrap();
        }
        listen(&mut parts.controller, sink, "out");
        parts.controller.commit().unwrap();
        render(&mut parts, 1, 1)[0]
    }

    // Source rests at 0, which is the middle of a bipolar output.
    assert_eq!(run(None), 0.0);
    assert_eq!(run(Some(Polarity::Unipolar)), 0.5);
}

#[test]
fn depth_and_enabled_apply_at_the_next_buffer() {
    let mut parts = engine();
    let source = module(&mut parts.controller, "test.linear", "level", 1.0);
    let sink = parts.controller.add_module("test.linear").unwrap();
    let id = parts
        .controller
        .add_assignment_named(source, "out", sink, "level", 0.5)
        .unwrap();
    listen(&mut parts.controller, sink, "out");
    parts.controller.commit().unwrap();
    assert!(render(&mut parts, 4, 4).iter().all(|&sample| sample == 0.5));

    parts.controller.set_assignment_depth(id, 0.25).unwrap();
    assert!(render(&mut parts, 4, 4).iter().all(|&sample| sample == 0.25));

    parts.controller.set_assignment_enabled(id, false).unwrap();
    assert!(render(&mut parts, 4, 4).iter().all(|&sample| sample == 0.0));
}

#[test]
fn deleting_a_module_removes_its_assignments() {
    let mut parts = engine();
    let source = parts.controller.add_module("test.linear").unwrap();
    let sink = parts.controller.add_module("test.linear").unwrap();
    parts
        .controller
        .add_assignment_named(source, "out", sink, "level", 0.5)
        .unwrap();
    parts.controller.remove_module(source).unwrap();
    assert!(parts.controller.graph().assignments().next().is_none());

    let source = parts.controller.add_module("test.linear").unwrap();
    parts
        .controller
        .add_assignment_named(source, "out", sink, "level", 0.5)
        .unwrap();
    parts.controller.remove_module(sink).unwrap();
    assert!(parts.controller.graph().assignments().next().is_none());
}

/// How a test takes the modulation away again.
#[derive(Clone, Copy, Debug)]
enum Removal {
    Assignment,
    SourceModule,
}

const REMOVALS: [Removal; 2] = [Removal::Assignment, Removal::SourceModule];

/// A sink module with base values, plus a constant source at 1.0 for its
/// assignments. Sending no assignment builds the unmodulated reference.
struct Rig {
    parts: EngineParts,
    sink: ModuleId,
    source: ModuleId,
    assignments: Vec<AssignmentId>,
}

impl Rig {
    fn new(kind: &str, base: &[(&str, f32)]) -> Self {
        let mut parts = engine();
        let sink = parts.controller.add_module(kind).unwrap();
        for &(param, value) in base {
            parts.controller.set_param_named(sink, param, value).unwrap();
        }
        let source = module(&mut parts.controller, "test.linear", "level", 1.0);
        Self {
            parts,
            sink,
            source,
            assignments: Vec::new(),
        }
    }

    fn assign(&mut self, param: &str, depth: f32) {
        let id = self
            .parts
            .controller
            .add_assignment_named(self.source, "out", self.sink, param, depth)
            .unwrap();
        self.assignments.push(id);
    }

    /// Plays `left` and `right` of the sink to the audio output.
    fn listen(&mut self, left: &str, right: &str) {
        let audio = self.parts.controller.add_module("loom.io.audio-out").unwrap();
        let controller = &mut self.parts.controller;
        controller.connect_named(self.sink, left, audio, "left").unwrap();
        controller.connect_named(self.sink, right, audio, "right").unwrap();
        controller.commit().unwrap();
    }

    fn remove(&mut self, how: Removal) {
        match how {
            Removal::Assignment => {
                for id in self.assignments.drain(..) {
                    self.parts.controller.remove_assignment(id).unwrap();
                }
            }
            Removal::SourceModule => {
                self.parts.controller.remove_module(self.source).unwrap();
                self.assignments.clear();
            }
        }
        self.parts.controller.commit().unwrap();
    }

    fn render(&mut self, frames: usize) -> (Vec<f32>, Vec<f32>) {
        let mut left = vec![0.0; frames];
        let mut right = vec![0.0; frames];
        for (l, r) in left.chunks_mut(64).zip(right.chunks_mut(64)) {
            self.parts.engine.process(l, r);
        }
        (left, right)
    }
}

/// Regression for a module that wrote modulated samples into the fields
/// `set_param` owns: after the assignment went away it kept the last
/// modulated value, because nothing calls `set_param` again.
#[test]
fn adsr_returns_to_base_sustain() {
    for how in REMOVALS {
        let mut rig = Rig::new(
            "loom.env.adsr",
            &[("attack", 0.0005), ("decay", 0.0005), ("sustain", 0.5)],
        );
        let gate = module(&mut rig.parts.controller, "test.linear", "level", 1.0);
        rig.parts
            .controller
            .connect_named(gate, "out", rig.sink, "gate")
            .unwrap();
        rig.assign("sustain", 0.5);
        rig.listen("out", "out");

        let (modulated, _) = rig.render(4_800);
        let last = modulated[modulated.len() - 1];
        assert!((last - 1.0).abs() < 1e-3, "{how:?}: modulated sustain, got {last}");

        rig.remove(how);
        let (base, _) = rig.render(4_800);
        let last = base[base.len() - 1];
        assert!((last - 0.5).abs() < 1e-3, "{how:?}: base sustain, got {last}");
    }
}

#[test]
fn divide_returns_to_base_n_and_offset() {
    fn run(how: Option<Removal>) -> (Vec<f32>, Vec<f32>) {
        let mut rig = Rig::new("loom.logic.divide", &[("n", 2.0), ("offset", 0.0)]);
        let controller = &mut rig.parts.controller;
        let phase = controller.add_module("loom.phase.generator").unwrap();
        let triggers = controller.add_module("loom.phase.trigger").unwrap();
        controller.connect_named(phase, "phase", triggers, "phase").unwrap();
        controller
            .connect_named(triggers, "trigger", rig.sink, "trigger")
            .unwrap();
        if how.is_some() {
            // n goes to 4 and offset to 1.
            rig.assign("n", 2.0 / 63.0);
            rig.assign("offset", 1.0 / 63.0);
        }
        rig.listen("trigger", "trigger");
        let before = rig.render(9_600).0;
        if let Some(how) = how {
            rig.remove(how);
        }
        (before, rig.render(9_600).0)
    }

    let (reference_before, reference) = run(None);
    assert!(reference.contains(&1.0), "the reference fires");
    for how in REMOVALS {
        let (before, after) = run(Some(how));
        assert_ne!(before, reference_before, "{how:?}: modulation changes the output");
        assert_eq!(after, reference, "{how:?}");
    }
}

#[test]
fn note_in_returns_to_base_channel_and_bend_range() {
    fn run(how: Option<Removal>) -> (Vec<f32>, Vec<f32>) {
        let mut rig = Rig::new("loom.midi.note", &[("channel", 0.0), ("bend-range", 2.0)]);
        if how.is_some() {
            // The channel goes to 2, which ignores channel 1 below, and the
            // bend range goes to 12 semitones.
            rig.assign("channel", 2.0 / 16.0);
            rig.assign("bend-range", 10.0 / 48.0);
        }
        rig.listen("pitch", "gate");
        rig.render(256);
        if let Some(how) = how {
            rig.remove(how);
        }
        // Channel 1 (0 on the wire): a note, then a bend halfway up.
        assert!(rig.parts.midi.send(MidiEvent::note_on(10, 0, 67, 100)));
        assert!(rig.parts.midi.send(MidiEvent::from_bytes(20, &[0xE0, 0x00, 0x60])));
        rig.render(256)
    }

    let reference = run(None);
    assert!(reference.1.contains(&1.0), "the reference plays the note");
    for how in REMOVALS {
        assert_eq!(run(Some(how)), reference, "{how:?}");
    }
}

#[test]
fn noise_returns_to_base_seed() {
    fn run(how: Option<Removal>) -> (Vec<f32>, Vec<f32>) {
        let mut rig = Rig::new("loom.osc.noise", &[("seed", 1.0)]);
        if how.is_some() {
            // The seed goes to 101.
            rig.assign("seed", 100.0 / 65_534.0);
        }
        rig.listen("out", "out");
        let before = rig.render(256).0;
        if let Some(how) = how {
            rig.remove(how);
        }
        (before, rig.render(256).0)
    }

    // Back on the base seed, the pattern starts again from that seed.
    let (reference, _) = run(None);
    for how in REMOVALS {
        let (before, after) = run(Some(how));
        assert_ne!(before, reference, "{how:?}: modulation changes the seed");
        assert_eq!(after, reference, "{how:?}");
    }
}

/// A cycle of cables needs a cable delay whatever the compiler does. Once
/// that delay is in, the assignment below is no longer in a loop, so it must
/// not be delayed too (review on #133).
#[test]
fn cable_cycle_is_broken_before_an_assignment_that_needs_no_delay() {
    let mut parts = engine();
    let controller = &mut parts.controller;
    let first = controller.add_module("loom.math.add").unwrap();
    let second = controller.add_module("loom.math.add").unwrap();
    controller.connect_named(first, "out", second, "a").unwrap();
    controller.connect_named(second, "out", first, "a").unwrap();
    controller
        .add_assignment_named(first, "out", second, "offset", 0.5)
        .unwrap();
    listen(controller, second, "out");
    let report = controller.commit().unwrap();
    let delays: Vec<_> = report.inserted_delays().collect();
    assert_eq!(delays.len(), 1, "{delays:?}");
    assert_eq!(delays[0].1.module, first, "the cable into the first add is delayed");
    assert_eq!(report.inserted_assignment_delays().count(), 0);
}

/// Turns a knob, then adds its first assignment halfway through the glide.
/// The depth is 0, so the output must follow the same glide as with no
/// assignment, instead of stepping to the new value (review on #133).
fn glide_with_depth_zero(
    kind: &str,
    param: &str,
    from: f32,
    to: f32,
    output: &str,
    setup: impl Fn(&mut Rig),
    assign: bool,
) -> Vec<f32> {
    let mut rig = Rig::new(kind, &[(param, from)]);
    setup(&mut rig);
    rig.listen(output, output);
    rig.render(256);
    rig.parts.controller.set_param_named(rig.sink, param, to).unwrap();
    let mut out = rig.render(64).0;
    if assign {
        rig.assign(param, 0.0);
    }
    rig.parts.controller.commit().unwrap();
    out.extend(rig.render(1_024).0);
    out
}

fn assert_close(got: &[f32], expected: &[f32], tolerance: f32) {
    assert_eq!(got.len(), expected.len());
    for (frame, (g, e)) in got.iter().zip(expected).enumerate() {
        assert!((g - e).abs() <= tolerance, "frame {frame}: {g} vs {e}");
    }
}

#[test]
fn depth_zero_added_during_a_glide_continues_the_glide() {
    let run = |assign| glide_with_depth_zero("loom.math.constant", "value", 0.0, 1.0, "out", |_| {}, assign);
    let without = run(false);
    let with = run(true);
    // Still gliding when the assignment arrives, so a step would show.
    assert!(without[64] < 0.9, "{}", without[64]);
    assert_close(&with, &without, 1e-5);

    let noise = |rig: &mut Rig| {
        let noise = rig.parts.controller.add_module("loom.osc.noise").unwrap();
        rig.parts
            .controller
            .connect_named(noise, "out", rig.sink, "in")
            .unwrap();
    };
    let run = |assign| glide_with_depth_zero("loom.filter.svf", "cutoff", 200.0, 8_000.0, "lowpass", noise, assign);
    assert_close(&run(true), &run(false), 1e-4);
}

/// Guards the controller path: an assignment to a module that is not a DSP
/// module must fail, not be stored and ignored. The built-in unit delay and
/// audio output have no parameters, so this passes through `NoSuchParam`. The
/// registry test `refuses_parameters_and_displays_on_non_dsp_modules` is the
/// one that proves such modules can never declare parameters.
#[test]
fn assignment_to_non_dsp_module_is_rejected() {
    let mut parts = engine();
    let source = parts.controller.add_module("loom.math.constant").unwrap();
    let delay = parts.controller.add_module("loom.time.delay1").unwrap();
    let audio = parts.controller.add_module("loom.io.audio-out").unwrap();
    let source_out = parts.controller.output_ref(source, "out").unwrap();

    assert!(matches!(
        parts.controller.add_assignment(source_out, delay, 0, 0.5),
        Err(loom_core::LoomError::NoSuchParam { .. })
    ));
    assert!(matches!(
        parts.controller.add_assignment(source_out, audio, 0, 0.5),
        Err(loom_core::LoomError::NoSuchParam { .. })
    ));
}

/// A whole-number parameter from 0 to 1, off, with one assignment at the
/// depth the app gives a toggle (one step, 1.0): a unipolar source at 1
/// turns it on, and at 0 leaves it off (modulation.md, slice 1). The
/// button's `on` is such a parameter, and its gate is a unipolar source.
#[test]
fn a_unipolar_source_at_one_turns_an_off_toggle_on() {
    fn run(source_on: f32) -> f32 {
        let mut parts = engine();
        let source = module(&mut parts.controller, "loom.ui.button", "on", source_on);
        let target = module(&mut parts.controller, "loom.ui.button", "on", 0.0);
        parts
            .controller
            .add_assignment_named(source, "gate", target, "on", 1.0)
            .unwrap();
        listen(&mut parts.controller, target, "gate");
        parts.controller.commit().unwrap();
        render(&mut parts, 4, 4)[3]
    }
    assert_eq!(run(1.0), 1.0);
    assert_eq!(run(0.0), 0.0);
}

fn engine_with_live_cells(cells: usize) -> EngineParts {
    let mut registry = loom_core::Registry::new();
    register_all(&mut registry).expect("built-ins");
    registry.register(&LINEAR).expect("linear");
    // Exact values through Audio Out, as in `engine`: safety off, and
    // past the fade-in (ADR-0019).
    let config = EngineConfig {
        max_live_values: cells,
        output_safety: OutputSafety::Off,
        ..EngineConfig::with_sample_rate(48_000.0)
    };
    let mut parts = create_engine(Arc::new(registry), config);
    let mut silence = [0.0; 1024];
    parts.engine.process(&mut silence.clone(), &mut silence);
    parts
}

/// The live value is the parameter's modulated value at the last frame of
/// the block, as a position from 0 to 1 (modulation.md, Live value). The
/// sink writes its modulated parameter to its output, so the two match.
#[test]
fn the_live_value_is_the_last_frame_of_the_block() {
    let mut parts = engine_with_live_cells(256);
    let ramp = parts.controller.add_module("loom.phase.generator").unwrap();
    let sink = parts.controller.add_module("test.linear").unwrap();
    let id = parts
        .controller
        .add_assignment_named(ramp, "phase", sink, "level", 1.0)
        .unwrap();
    parts
        .controller
        .set_assignment_polarity(id, Some(Polarity::Unipolar))
        .unwrap();
    listen(&mut parts.controller, sink, "out");
    parts.controller.commit().unwrap();
    // Not swapped in yet: nothing to read.
    assert_eq!(parts.controller.modulated(sink, 0), None);
    for block in [1, 77, 128] {
        let out = render(&mut parts, block, block);
        let live = parts.controller.modulated(sink, 0).expect("a live value");
        assert_eq!(live, out[block - 1], "block of {block}");
    }
    // A parameter with no assignment has no live value.
    assert_eq!(parts.controller.modulated(ramp, 0), None);
}

/// With more modulated parameters than cells, the extra ones still sound
/// the same but have no live value, and the compile report says so once.
#[test]
fn a_full_live_table_drops_the_dot_but_not_the_sound() {
    fn run(cells: usize) -> (Vec<f32>, Option<f32>, Option<f32>, usize) {
        let mut parts = engine_with_live_cells(cells);
        let source = module(&mut parts.controller, "test.linear", "level", 1.0);
        let a = parts.controller.add_module("test.linear").unwrap();
        let b = parts.controller.add_module("test.linear").unwrap();
        for (target, depth) in [(a, 0.25), (b, 0.5)] {
            parts
                .controller
                .add_assignment_named(source, "out", target, "level", depth)
                .unwrap();
        }
        let mix = parts.controller.add_module("loom.math.add").unwrap();
        parts.controller.connect_named(a, "out", mix, "a").unwrap();
        parts.controller.connect_named(b, "out", mix, "b").unwrap();
        listen(&mut parts.controller, mix, "out");
        let missing = parts.controller.commit().unwrap().without_live_value;
        let out = render(&mut parts, 64, 64);
        (
            out,
            parts.controller.modulated(a, 0),
            parts.controller.modulated(b, 0),
            missing,
        )
    }
    let (full, a, b, missing) = run(256);
    assert_eq!((a, b, missing), (Some(0.25), Some(0.5), 0));
    let (one, a, b, missing) = run(1);
    assert_eq!(missing, 1);
    assert_eq!(a.is_some() as u8 + b.is_some() as u8, 1);
    assert_eq!(one, full, "the sound does not change");
}

/// After a recompile gives a cell to another parameter, the old knob never
/// reads the new parameter's value, and the new one reads nothing until
/// the engine has the schedule that gives it the cell.
#[test]
fn a_moved_cell_never_shows_another_parameters_value() {
    let mut parts = engine_with_live_cells(1);
    let source = module(&mut parts.controller, "test.linear", "level", 1.0);
    let a = parts.controller.add_module("test.linear").unwrap();
    let b = parts.controller.add_module("test.linear").unwrap();
    let first = parts
        .controller
        .add_assignment_named(source, "out", a, "level", 0.25)
        .unwrap();
    listen(&mut parts.controller, a, "out");
    parts.controller.commit().unwrap();
    render(&mut parts, 8, 8);
    assert_eq!(parts.controller.modulated(a, 0), Some(0.25));

    parts.controller.remove_assignment(first).unwrap();
    parts
        .controller
        .add_assignment_named(source, "out", b, "level", 0.5)
        .unwrap();
    parts.controller.commit().unwrap();
    // The controller has moved the cell to b, the engine not yet.
    assert_eq!(parts.controller.modulated(a, 0), None);
    assert_eq!(parts.controller.modulated(b, 0), None);
    render(&mut parts, 8, 8);
    assert_eq!(parts.controller.modulated(a, 0), None);
    assert_eq!(parts.controller.modulated(b, 0), Some(0.5));
}

/// With the command queue full, changing depth, polarity, or on/off fails
/// and leaves the graph as it was, so the graph never holds a setting the
/// engine does not have.
#[test]
fn a_full_queue_leaves_the_assignment_unchanged() {
    let mut registry = loom_core::Registry::new();
    registry.register(&LINEAR).expect("linear");
    let config = EngineConfig {
        // Two inserts and one swap fill it.
        command_capacity: 3,
        ..EngineConfig::with_sample_rate(48_000.0)
    };
    let mut parts = create_engine(Arc::new(registry), config);
    let source = parts.controller.add_module("test.linear").unwrap();
    let sink = parts.controller.add_module("test.linear").unwrap();
    let id = parts
        .controller
        .add_assignment_named(source, "out", sink, "level", 0.25)
        .unwrap();
    parts.controller.commit().unwrap();
    let before = *parts.controller.graph().assignment(id).unwrap();
    assert!(parts.controller.set_assignment_depth(id, 0.5).is_err());
    assert!(
        parts
            .controller
            .set_assignment_polarity(id, Some(Polarity::Unipolar))
            .is_err()
    );
    assert!(parts.controller.set_assignment_enabled(id, false).is_err());
    assert_eq!(*parts.controller.graph().assignment(id).unwrap(), before);
}
