//! Modules that reach nothing the user hears or sees are not processed, and
//! keep their state for when a cable connects them again (FR-GRAPH-09).

use loom_core::{
    Channels, EngineConfig, EngineParts, InputRef, Module, ModuleDescriptor, ModuleId, ModuleRole, OutputRef,
    OutputSafety, Panel, PortDefault, PortIo, PortSpec, ProcessContext, Registry, SignalType, create_engine,
};
use std::sync::Arc;

/// Frames per buffer in these tests.
const BLOCK: usize = 64;

/// A port for test descriptors.
const fn port(name: &'static str) -> PortSpec {
    PortSpec {
        name,
        label: name,
        signal: SignalType::Audio,
        description: "A test port.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }
}

/// `test.counter`: outputs how many frames it has processed, scaled by
/// [`SCALE`], so the value shows how long it ran and stays inside the
/// output clip.
static COUNTER: ModuleDescriptor = ModuleDescriptor {
    id: "test.counter",
    version: 1,
    name: "Counter",
    category: "Test",
    family: loom_core::Family::Interface,
    description: "Counts the frames it processes.",
    inputs: &[],
    outputs: &[port("out")],
    params: &[],
    role: ModuleRole::Dsp(|_| Box::new(Counter(0))),
    panel: Panel::EMPTY,
    displays: &[],
};

/// One counted frame on [`COUNTER`]'s output.
const SCALE: f32 = 1.0 / 65_536.0;

/// The state of [`COUNTER`]: frames processed so far.
struct Counter(u32);

impl Module for Counter {
    fn process(&mut self, _: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let ([], [out]) = io.split::<0, 1>();
        for x in out.iter_mut() {
            *x = self.0 as f32 * SCALE;
            self.0 += 1;
        }
    }
}

/// `test.pass`: copies its input to its output.
static PASS: ModuleDescriptor = ModuleDescriptor {
    id: "test.pass",
    version: 1,
    name: "Pass",
    category: "Test",
    family: loom_core::Family::Interface,
    description: "Copies its input.",
    inputs: &[port("in")],
    outputs: &[port("out")],
    params: &[],
    role: ModuleRole::Dsp(|_| Box::new(Pass)),
    panel: Panel::EMPTY,
    displays: &[],
};

/// The module behind [`PASS`].
struct Pass;

impl Module for Pass {
    fn process(&mut self, _: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let ([input], [out]) = io.split::<1, 1>();
        out.copy_from_slice(input);
    }
}

/// `test.out`: the audio output.
static OUT: ModuleDescriptor = ModuleDescriptor {
    id: "test.out",
    version: 1,
    name: "Out",
    category: "Test",
    family: loom_core::Family::Interface,
    description: "The engine output.",
    inputs: &[PortSpec {
        channels: Channels::Stereo,
        ..port("in")
    }],
    outputs: &[],
    params: &[],
    role: ModuleRole::AudioOutput,
    panel: Panel::EMPTY,
    displays: &[],
};

/// A one-sample delay whose held value survives an unreachable schedule.
static DELAY: ModuleDescriptor = ModuleDescriptor {
    id: "test.delay1",
    version: 1,
    name: "Delay 1",
    category: "Test",
    family: loom_core::Family::Interface,
    description: "Holds one sample.",
    inputs: &[port("in")],
    outputs: &[port("out")],
    params: &[],
    role: ModuleRole::UnitDelay,
    panel: Panel::EMPTY,
    displays: &[],
};

/// An engine with the test modules and no output safety, so the output
/// holds the exact values the modules wrote.
fn engine() -> EngineParts {
    let mut registry = Registry::new();
    for descriptor in [&COUNTER, &PASS, &OUT, &DELAY] {
        registry.register(descriptor).unwrap();
    }
    let config = EngineConfig {
        max_block: BLOCK,
        output_safety: OutputSafety::Off,
        ..EngineConfig::with_sample_rate(48_000.0)
    };
    create_engine(Arc::new(registry), config)
}

/// Output 0 of `module`.
fn out(module: ModuleId) -> OutputRef {
    OutputRef { module, port: 0 }
}

/// Input `port` of `module`.
fn input(module: ModuleId, port: usize) -> InputRef {
    InputRef { module, port }
}

/// Runs `buffers` buffers and returns the right channel of the last one.
fn run(parts: &mut EngineParts, buffers: usize) -> Vec<f32> {
    let mut left = vec![0.0; BLOCK];
    let mut right = vec![0.0; BLOCK];
    for _ in 0..buffers {
        parts.engine.process(&mut left, &mut right);
    }
    right
}

/// A placeholder passes no signal, even with named ports and a live input.
#[test]
fn unknown_module_outputs_silence_at_each_block_size() {
    for frames in [1, 77, 512] {
        let mut parts = engine();
        let source = parts.controller.add_module("test.counter").unwrap();
        let unknown = ModuleId(100);
        parts
            .controller
            .add_unknown_with_id(
                unknown,
                loom_core::UnknownModule {
                    kind: "example.missing@1".into(),
                    label: None,
                    params: Default::default(),
                    ranges: Default::default(),
                    inputs: vec!["signal".into()],
                    outputs: vec!["result".into()],
                    parameters: vec![],
                },
            )
            .unwrap();
        let output = parts.controller.add_module("test.out").unwrap();
        parts
            .controller
            .connect_named(source, "out", unknown, "signal")
            .unwrap();
        parts.controller.connect_named(unknown, "result", output, "in").unwrap();
        parts.controller.commit().unwrap();
        let mut left = vec![1.0; frames];
        let mut right = vec![1.0; frames];
        for _ in 0..(1024 / frames + 2) {
            parts.engine.process(&mut left, &mut right);
            assert!(left.iter().chain(&right).all(|&sample| sample == 0.0));
        }
    }
}

/// The counter values `first..first + BLOCK`, as the output carries them.
fn counted(first: u32) -> Vec<f32> {
    (first..first + BLOCK as u32).map(|n| n as f32 * SCALE).collect()
}

/// A counter and a pass that lead nowhere are not scheduled, so the
/// counter does not count. A cable to the output makes it run from where
/// it stands: 0, because it never ran.
#[test]
fn an_unreachable_chain_is_not_processed() {
    let mut parts = engine();
    let c = &mut parts.controller;
    let heard = c.add_module("test.counter").unwrap();
    let counter = c.add_module("test.counter").unwrap();
    let pass = c.add_module("test.pass").unwrap();
    let speaker = c.add_module("test.out").unwrap();
    c.connect(out(heard), input(speaker, 0)).unwrap();
    c.connect(out(counter), input(pass, 0)).unwrap();
    let report = c.commit().unwrap();
    assert_eq!(report.skipped.iter().copied().collect::<Vec<_>>(), vec![counter, pass]);

    // Mono into the left side plays on both: the right shows the heard one.
    assert_eq!(run(&mut parts, 10), counted(9 * BLOCK as u32));

    parts.controller.connect(out(pass), input(speaker, 0)).unwrap();
    let report = parts.controller.commit().unwrap();
    assert_eq!(report.skipped.iter().copied().collect::<Vec<_>>(), vec![heard]);
    assert_eq!(run(&mut parts, 1), counted(0));
}

/// A chain that is unplugged keeps its state while skipped. Plugging it in
/// again resumes it where it stopped: not reset, and not moved on by the
/// time it spent unplugged.
#[test]
fn reconnecting_resumes_the_state() {
    let mut parts = engine();
    let c = &mut parts.controller;
    let counter = c.add_module("test.counter").unwrap();
    let speaker = c.add_module("test.out").unwrap();
    c.connect(out(counter), input(speaker, 0)).unwrap();
    c.commit().unwrap();
    // Ten buffers play past the 10 ms fade-in of the output stage.
    assert_eq!(run(&mut parts, 10), counted(9 * BLOCK as u32));

    parts.controller.disconnect(input(speaker, 0)).unwrap();
    let report = parts.controller.commit().unwrap();
    assert_eq!(report.skipped.iter().copied().collect::<Vec<_>>(), vec![counter]);
    run(&mut parts, 20);

    parts.controller.connect(out(counter), input(speaker, 0)).unwrap();
    parts.controller.commit().unwrap();
    assert_eq!(run(&mut parts, 1), counted(10 * BLOCK as u32));
}

/// Disconnecting a unit delay preserves its held sample, even when its
/// source is skipped too. Reconnection emits that sample before capturing
/// a new one.
#[test]
fn skipped_unit_delay_keeps_its_held_sample() {
    let mut parts = engine();
    let c = &mut parts.controller;
    let counter = c.add_module("test.counter").unwrap();
    let delay = c.add_module("test.delay1").unwrap();
    let speaker = c.add_module("test.out").unwrap();
    c.connect(out(counter), input(delay, 0)).unwrap();
    c.connect(out(delay), input(speaker, 0)).unwrap();
    c.commit().unwrap();
    run(&mut parts, 10);

    parts.controller.disconnect(input(speaker, 0)).unwrap();
    let report = parts.controller.commit().unwrap();
    assert!(report.skipped.contains(&counter));
    assert!(report.skipped.contains(&delay));
    run(&mut parts, 20);

    parts.controller.connect(out(delay), input(speaker, 0)).unwrap();
    parts.controller.commit().unwrap();
    let resumed = run(&mut parts, 1);
    assert_eq!(resumed[0], 639.0 * SCALE);
    assert_eq!(resumed[1], 640.0 * SCALE);
}
