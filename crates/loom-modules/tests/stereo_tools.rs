//! End-to-end checks for the stereo channel tools (stereo spec, rule 21).

use loom_core::{EngineConfig, ProbeKind, create_engine};

/// Join and Split preserve each mono input at every supported block size.
#[test]
fn join_then_split_preserves_both_inputs() {
    for block in [1, 77, 512] {
        let mut parts = create_engine(loom_modules::registry(), EngineConfig::with_sample_rate(48_000.0));
        let left = parts.controller.add_module("loom.math.constant").unwrap();
        let right = parts.controller.add_module("loom.math.constant").unwrap();
        let join = parts.controller.add_module("loom.mix.join").unwrap();
        let split = parts.controller.add_module("loom.mix.split").unwrap();
        parts.controller.set_param_named(left, "value", 0.25).unwrap();
        parts.controller.set_param_named(right, "value", -0.5).unwrap();
        parts.controller.connect_named(left, "out", join, "left").unwrap();
        parts.controller.connect_named(right, "out", join, "right").unwrap();
        parts.controller.connect_named(join, "out", split, "in").unwrap();
        let left_output = parts.controller.output_ref(split, "left").unwrap();
        let right_output = parts.controller.output_ref(split, "right").unwrap();
        let left_probe = parts.controller.add_probe(left_output, ProbeKind::Scope).unwrap();
        let right_probe = parts.controller.add_probe(right_output, ProbeKind::Scope).unwrap();
        parts.controller.commit().unwrap();
        let mut l = vec![0.0; block];
        let mut r = vec![0.0; block];
        parts.engine.process(&mut l, &mut r);
        let mut seen = [0, 0];
        for chunk in parts.telemetry.scope() {
            let (index, expected) = if chunk.probe == left_probe {
                (0, 0.25)
            } else {
                assert_eq!(chunk.probe, right_probe);
                (1, -0.5)
            };
            assert!(chunk.samples().iter().all(|&value| value == expected));
            seen[index] += chunk.len as usize;
        }
        assert_eq!(seen, [block, block]);
    }
}

/// A running filter's new right copy starts with its left copy's state.
#[test]
fn running_filter_keeps_its_state_when_stereo_arrives() {
    for block in [1, 77, 512] {
        let config = EngineConfig {
            max_block: 512,
            ..EngineConfig::with_sample_rate(48_000.0)
        };
        let mut parts = create_engine(loom_modules::registry(), config);
        let c = &mut parts.controller;
        let source = c.add_module("loom.math.constant").unwrap();
        let filter = c.add_module("loom.filter.svf").unwrap();
        let join = c.add_module("loom.mix.join").unwrap();
        let split = c.add_module("loom.mix.split").unwrap();
        c.set_param_named(source, "value", 0.5).unwrap();
        c.set_param_named(filter, "cutoff", 150.0).unwrap();
        c.connect_named(source, "out", filter, "in").unwrap();
        c.connect_named(filter, "lowpass", split, "in").unwrap();
        let l = c.output_ref(split, "left").unwrap();
        let r = c.output_ref(split, "right").unwrap();
        let left_probe = c.add_probe(l, ProbeKind::Scope).unwrap();
        let right_probe = c.add_probe(r, ProbeKind::Scope).unwrap();
        c.commit().unwrap();
        let mut left = vec![0.0; block];
        let mut right = vec![0.0; block];
        for _ in 0..4 {
            parts.engine.process(&mut left, &mut right);
            parts.telemetry.scope().for_each(drop);
        }
        let c = &mut parts.controller;
        c.connect_named(source, "out", join, "left").unwrap();
        c.connect_named(source, "out", join, "right").unwrap();
        c.connect_named(join, "out", filter, "in").unwrap();
        assert!(c.commit().unwrap().runs_per_channel(filter));
        parts.engine.process(&mut left, &mut right);
        let mut l = Vec::new();
        let mut r = Vec::new();
        for chunk in parts.telemetry.scope() {
            if chunk.probe == left_probe {
                l.extend_from_slice(chunk.samples());
            }
            if chunk.probe == right_probe {
                r.extend_from_slice(chunk.samples());
            }
        }
        assert_eq!(l.len(), block);
        assert_eq!(r.len(), block);
        assert_eq!(l, r, "block {block}");
    }
}

/// Each side of a stereo SVF matches a separate mono SVF on that signal.
#[test]
fn stereo_svf_matches_two_mono_filters() {
    for block in [1, 77, 512] {
        let mut parts = create_engine(loom_modules::registry(), EngineConfig::with_sample_rate(48_000.0));
        let c = &mut parts.controller;
        let left = c.add_module("loom.math.constant").unwrap();
        let right = c.add_module("loom.math.constant").unwrap();
        let join = c.add_module("loom.mix.join").unwrap();
        let stereo = c.add_module("loom.filter.svf").unwrap();
        let mono_left = c.add_module("loom.filter.svf").unwrap();
        let mono_right = c.add_module("loom.filter.svf").unwrap();
        let split = c.add_module("loom.mix.split").unwrap();
        c.set_param_named(left, "value", 0.25).unwrap();
        c.set_param_named(right, "value", -0.4).unwrap();
        for filter in [stereo, mono_left, mono_right] {
            c.set_param_named(filter, "cutoff", 180.0).unwrap();
            c.set_param_named(filter, "resonance", 0.6).unwrap();
        }
        c.connect_named(left, "out", join, "left").unwrap();
        c.connect_named(right, "out", join, "right").unwrap();
        c.connect_named(join, "out", stereo, "in").unwrap();
        c.connect_named(stereo, "lowpass", split, "in").unwrap();
        c.connect_named(left, "out", mono_left, "in").unwrap();
        c.connect_named(right, "out", mono_right, "in").unwrap();
        let outputs = [
            (split, "left"),
            (split, "right"),
            (mono_left, "lowpass"),
            (mono_right, "lowpass"),
        ];
        let probes: Vec<_> = outputs
            .iter()
            .map(|&(module, name)| {
                let output = c.output_ref(module, name).unwrap();
                c.add_probe(output, ProbeKind::Scope).unwrap()
            })
            .collect();
        let report = c.commit().unwrap();
        assert!(report.runs_per_channel(stereo));
        assert!(!report.runs_per_channel(mono_left));
        assert!(!report.runs_per_channel(mono_right));
        let mut results = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
        let mut remaining = 1024;
        while remaining > 0 {
            let count = block.min(remaining);
            parts.engine.process(&mut vec![0.0; count], &mut vec![0.0; count]);
            for chunk in parts.telemetry.scope() {
                let index = probes.iter().position(|&probe| probe == chunk.probe).unwrap();
                results[index].extend_from_slice(chunk.samples());
            }
            remaining -= count;
        }
        assert_eq!(results[0].len(), 1024);
        assert_eq!(results[0], results[2], "left, block {block}");
        assert_eq!(results[1], results[3], "right, block {block}");
        assert_ne!(results[0], results[1]);
    }
}

/// Wraps the built-in seeded Noise with an audio input that can turn stereo.
struct SeededNoise(Box<dyn loom_core::Module>);

impl loom_core::Module for SeededNoise {
    fn process(&mut self, ctx: &loom_core::ProcessContext<'_>, io: &mut loom_core::PortIo<'_>) {
        self.0.process(ctx, io);
    }

    fn reset(&mut self) {
        self.0.reset();
    }

    fn copy_state_from(&mut self, other: &dyn loom_core::Module) -> bool {
        other
            .as_any()
            .downcast_ref::<Self>()
            .is_some_and(|other| self.0.copy_state_from(other.0.as_ref()))
    }
}

/// The test input lets the engine create a right copy of built-in Noise.
static SEEDED_NOISE: loom_core::ModuleDescriptor = loom_core::ModuleDescriptor {
    id: "test.seeded-noise",
    version: 1,
    name: "Seeded noise test",
    category: "Test",
    description: "Tests a seeded module turning stereo.",
    inputs: &[loom_core::PortSpec {
        name: "in",
        label: "In",
        signal: loom_core::SignalType::Audio,
        description: "The input that selects the channel width.",
        default: loom_core::PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: loom_core::Channels::PerChannel,
    }],
    outputs: &[loom_core::PortSpec {
        name: "out",
        label: "Out",
        signal: loom_core::SignalType::Audio,
        description: "The seeded noise stream.",
        default: loom_core::PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: loom_core::Channels::PerChannel,
    }],
    params: &[],
    role: loom_core::ModuleRole::Dsp(|config| {
        let loom_core::ModuleRole::Dsp(factory) = loom_modules::osc::NOISE.role else {
            panic!("Noise is a DSP module");
        };
        Box::new(SeededNoise(factory(config)))
    }),
    panel: loom_core::Panel::EMPTY,
    displays: &[],
};

/// A running seeded module gets different, repeatable streams after stereo arrives.
#[test]
fn seeded_noise_turns_stereo_deterministically() {
    fn render(block: usize) -> [Vec<f32>; 2] {
        let mut registry = loom_core::Registry::new();
        loom_modules::register_all(&mut registry).unwrap();
        registry.register(&SEEDED_NOISE).unwrap();
        let mut parts = create_engine(std::sync::Arc::new(registry), EngineConfig::with_sample_rate(48_000.0));
        let c = &mut parts.controller;
        let source = c.add_module("loom.math.constant").unwrap();
        let join = c.add_module("loom.mix.join").unwrap();
        let noise = c.add_module("test.seeded-noise").unwrap();
        let split = c.add_module("loom.mix.split").unwrap();
        c.connect_named(source, "out", noise, "in").unwrap();
        c.connect_named(noise, "out", split, "in").unwrap();
        let probes = ["left", "right"].map(|name| {
            let out = c.output_ref(split, name).unwrap();
            c.add_probe(out, ProbeKind::Scope).unwrap()
        });
        assert!(!c.commit().unwrap().runs_per_channel(noise));
        let mut remaining = 200;
        while remaining > 0 {
            let count = block.min(remaining);
            parts.engine.process(&mut vec![0.0; count], &mut vec![0.0; count]);
            parts.telemetry.scope().for_each(drop);
            remaining -= count;
        }
        let c = &mut parts.controller;
        c.connect_named(source, "out", join, "left").unwrap();
        c.connect_named(source, "out", join, "right").unwrap();
        c.connect_named(join, "out", noise, "in").unwrap();
        assert!(c.commit().unwrap().runs_per_channel(noise));
        let mut result = [Vec::new(), Vec::new()];
        let mut remaining = 512;
        while remaining > 0 {
            let count = block.min(remaining);
            parts.engine.process(&mut vec![0.0; count], &mut vec![0.0; count]);
            for chunk in parts.telemetry.scope() {
                let channel = probes.iter().position(|&probe| probe == chunk.probe).unwrap();
                result[channel].extend_from_slice(chunk.samples());
            }
            remaining -= count;
        }
        result
    }
    let first = render(1);
    assert_eq!(first[0].len(), 512);
    assert_eq!(first[1].len(), 512);
    assert_ne!(first[0], first[1]);
    assert_eq!(first, render(77));
    assert_eq!(first, render(512));
}
