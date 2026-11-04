//! The engine counts the output samples it silenced or clipped, in
//! `EngineStats`, so the stats bar can say why a patch went quiet
//! (`docs/design/panels.md`, Audio load and dropouts in the stats bar).

use loom_core::{
    Channels, EngineConfig, EngineParts, Module, ModuleDescriptor, ModuleRole, OutputSafety, Panel, PortDefault,
    PortIo, PortSpec, ProcessContext, Registry, SignalType, create_engine,
};
use std::sync::Arc;
use std::sync::atomic::Ordering;

/// An audio port for the test modules.
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

/// `test.source`: outputs infinity, 2, and 0.5 in turn.
static SOURCE: ModuleDescriptor = ModuleDescriptor {
    id: "test.source",
    version: 1,
    name: "Source",
    category: "Test",
    family: loom_core::Family::Interface,
    description: "Outputs infinity, then 2, then 0.5, in turn.",
    inputs: &[],
    outputs: &[port("out")],
    params: &[],
    role: ModuleRole::Dsp(|_| Box::new(Source(0))),
    panel: Panel::EMPTY,
    displays: &[],
};

/// The state of [`SOURCE`]: frames written so far.
struct Source(usize);

impl Module for Source {
    fn process(&mut self, _: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        for out in io.output(0) {
            *out = [f32::INFINITY, 2.0, 0.5][self.0 % 3];
            self.0 += 1;
        }
    }
}

/// `test.out`: the audio output.
static OUT: ModuleDescriptor = ModuleDescriptor {
    id: "test.out",
    version: 1,
    name: "Out",
    category: "Test",
    family: loom_core::Family::Interface,
    description: "The audio output.",
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

/// A running patch from the source to the output, with this safety mode.
fn patch(output_safety: OutputSafety) -> EngineParts {
    let mut registry = Registry::new();
    registry.register(&SOURCE).unwrap();
    registry.register(&OUT).unwrap();
    let config = EngineConfig {
        output_safety,
        ..EngineConfig::with_sample_rate(48_000.0)
    };
    let mut parts = create_engine(Arc::new(registry), config);
    let source = parts.controller.add_module("test.source").unwrap();
    let out = parts.controller.add_module("test.out").unwrap();
    parts
        .controller
        .connect(
            loom_core::OutputRef {
                module: source,
                port: 0,
            },
            loom_core::InputRef { module: out, port: 0 },
        )
        .unwrap();
    parts.controller.commit().unwrap();
    parts
}

/// Runs `blocks` blocks of `frames` frames, and returns the two counts.
fn play(parts: &mut EngineParts, blocks: usize, frames: usize) -> (u64, u64) {
    let (mut left, mut right) = (vec![0.0; frames], vec![0.0; frames]);
    for _ in 0..blocks {
        parts.engine.process(&mut left, &mut right);
    }
    let stats = parts.controller.stats();
    (
        stats.output_silenced.load(Ordering::Relaxed),
        stats.output_clipped.load(Ordering::Relaxed),
    )
}

/// With safety Off, each infinite sample is silenced and each loud one is
/// clipped.
#[test]
fn counts_silenced_and_clipped_samples_with_safety_off() {
    let mut parts = patch(OutputSafety::Off);
    // Past the 10 ms fade-in, so full level reaches the clip.
    let (silenced_before, clipped_before) = play(&mut parts, 3, 300);
    let (silenced, clipped) = play(&mut parts, 1, 300);
    // Each of the 100 samples counts once in each of the two channels.
    assert_eq!(silenced - silenced_before, 200);
    assert_eq!(clipped - clipped_before, 200);
}

/// The counts do not depend on the block size.
#[test]
fn the_counts_are_the_same_at_any_block_size() {
    let counts = |frames: usize| {
        let mut parts = patch(OutputSafety::Off);
        play(&mut parts, 3072 / frames, frames)
    };
    assert_eq!(counts(1), counts(64));
    assert_eq!(counts(1), counts(512));
}

/// With the limiter on, a loud finite signal is limited, not clipped. An
/// infinite one is still silenced.
#[test]
fn the_limiter_leaves_little_to_clip() {
    let mut parts = patch(OutputSafety::LowLatency);
    let (silenced, clipped) = play(&mut parts, 4, 512);
    assert!(silenced > 0);
    assert_eq!(clipped, 0, "the limiter holds the level under the ceiling");
}

/// With the safety stage off, infinity reaches the output as silence, and the
/// other values only clip (ADR-0007, #121). It looks past the first half second,
/// where a new patch fades in.
#[test]
fn infinity_becomes_silence_at_the_output() {
    let mut parts = patch(OutputSafety::Off);
    let (mut left, mut right) = (vec![0.0; 48_000], vec![0.0; 48_000]);
    parts.engine.process(&mut left, &mut right);
    let settled = &left[24_000..];
    assert!(settled.contains(&0.0), "infinity is silent");
    assert!(settled.contains(&1.0), "2 clips to 1");
    assert!(settled.contains(&0.5), "0.5 passes");
    let other = settled.iter().find(|&&sample| ![0.0, 1.0, 0.5].contains(&sample));
    assert_eq!(other, None);
}
