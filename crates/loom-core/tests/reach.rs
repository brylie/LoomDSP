//! Modules that reach nothing the user hears or sees are not processed, and
//! keep their state for when a cable connects them again (FR-GRAPH-09).

use loom_core::{
    Channels, EngineConfig, EngineParts, InputRef, Module, ModuleDescriptor, ModuleId, ModuleRole, OutputRef,
    OutputSafety, Panel, Polarity, PortDefault, PortIo, PortSpec, ProcessContext, Registry, SignalType, create_engine,
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
        polarity: Polarity::Bipolar,
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
    description: "The engine output.",
    inputs: &[port("left"), port("right")],
    outputs: &[],
    params: &[],
    role: ModuleRole::AudioOutput,
    panel: Panel::EMPTY,
    displays: &[],
};

/// An engine with the test modules and no output safety, so the output
/// holds the exact values the modules wrote.
fn engine() -> EngineParts {
    let mut registry = Registry::new();
    for descriptor in [&COUNTER, &PASS, &OUT] {
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

    parts.controller.connect(out(pass), input(speaker, 1)).unwrap();
    let report = parts.controller.commit().unwrap();
    assert!(report.skipped.is_empty());
    assert_eq!(run(&mut parts, 1), counted(0));
}

/// A chain that is unplugged keeps its state while skipped. Plugging it in
/// again resumes it where it stopped: not reset, and not moved on by the
/// time it spent unplugged.
#[test]
fn reconnecting_resumes_the_state() {
    let mut parts = engine();
    let c = &mut parts.controller;
    let heard = c.add_module("test.counter").unwrap();
    let counter = c.add_module("test.counter").unwrap();
    let speaker = c.add_module("test.out").unwrap();
    c.connect(out(heard), input(speaker, 0)).unwrap();
    c.connect(out(counter), input(speaker, 1)).unwrap();
    c.commit().unwrap();
    // Ten buffers play past the 10 ms fade-in of the output stage.
    assert_eq!(run(&mut parts, 10), counted(9 * BLOCK as u32));

    parts.controller.disconnect(input(speaker, 1)).unwrap();
    let report = parts.controller.commit().unwrap();
    assert_eq!(report.skipped.iter().copied().collect::<Vec<_>>(), vec![counter]);
    run(&mut parts, 20);

    parts.controller.connect(out(counter), input(speaker, 1)).unwrap();
    parts.controller.commit().unwrap();
    assert_eq!(run(&mut parts, 1), counted(10 * BLOCK as u32));
}
