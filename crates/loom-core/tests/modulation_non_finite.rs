//! A modulation source that outputs NaN or infinity still gives the target
//! module a finite parameter value inside the parameter's range
//! (`docs/design/modulation.md`, Engine and API).
//!
//! The engine fills each modulated parameter buffer through
//! `ParamSpec::denormalize`, which turns NaN into the default and clamps
//! infinity. Modules rely on this: the module test harness feeds non-finite
//! values only on inputs, not on parameters (#127).

use loom_core::{
    Channels, EngineConfig, EngineParts, Module, ModuleDescriptor, ModuleRole, Panel, ParamOptions, ParamSpec,
    Polarity, PortDefault, PortIo, PortSpec, ProcessContext, Registry, SignalType, create_engine,
};
use std::sync::Arc;

/// An output port for the test modules.
const OUT: PortSpec = PortSpec {
    name: "out",
    label: "Out",
    signal: SignalType::Modulation,
    description: "A test output.",
    default: PortDefault::Value(0.0),
    range: loom_core::PortRange::Signal,
    channels: Channels::PerChannel,
};

/// `test.bad`: outputs NaN, infinity, minus infinity, and 0.5 in turn, one
/// per frame.
static BAD: ModuleDescriptor = ModuleDescriptor {
    id: "test.bad",
    version: 1,
    name: "Bad",
    category: "Test",
    description: "Outputs non-finite values.",
    inputs: &[],
    outputs: &[OUT],
    params: &[],
    role: ModuleRole::Dsp(|_| Box::new(Bad { frame: 0 })),
    panel: Panel::EMPTY,
    displays: &[],
};

/// The state of [`BAD`]: frames written so far.
struct Bad {
    frame: usize,
}

impl Module for Bad {
    fn process(&mut self, _: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        for out in io.output(0) {
            *out = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.5][self.frame % 4];
            self.frame += 1;
        }
    }
}

/// `test.watch`: counts the modulated frames of its parameters, and the
/// frames that were not finite or were outside the parameter's range.
static WATCH: ModuleDescriptor = ModuleDescriptor {
    id: "test.watch",
    version: 1,
    name: "Watch",
    category: "Test",
    description: "Checks its modulated parameters.",
    inputs: &[],
    outputs: &[OUT],
    params: &[
        ParamSpec {
            name: "linear",
            label: "Linear",
            description: "A linear parameter.",
            min: -1.0,
            max: 1.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        },
        ParamSpec {
            name: "log",
            label: "Log",
            description: "A logarithmic, smoothed parameter.",
            min: 20.0,
            max: 20_000.0,
            default: 1_000.0,
            unit: "Hz",
            options: ParamOptions::DEFAULT,
        }
        .logarithmic()
        .smooth(0.005),
    ],
    role: ModuleRole::Dsp(|_| Box::new(Watch { seen: 0, bad: 0 })),
    panel: Panel::EMPTY,
    displays: &[
        loom_core::DisplaySpec {
            name: "seen",
            label: "Seen",
            description: "Blocks the module has run.",
            kind: loom_core::DisplayKind::Level,
        },
        loom_core::DisplaySpec {
            name: "bad",
            label: "Bad",
            description: "Values that were not finite.",
            kind: loom_core::DisplayKind::Level,
        },
    ],
};

/// The counts of [`WATCH`].
struct Watch {
    seen: u32,
    bad: u32,
}

impl Module for Watch {
    fn process(&mut self, _: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        for (index, spec) in WATCH.params.iter().enumerate() {
            if let Some(samples) = io.modulated(index) {
                for &value in samples {
                    self.seen += 1;
                    if !value.is_finite() || value < spec.min || value > spec.max {
                        self.bad += 1;
                    }
                }
            }
        }
        io.output(0).fill(0.0);
    }

    fn displays(&self, values: &mut [f32]) {
        values[0] = self.seen as f32;
        values[1] = self.bad as f32;
    }
}

/// Every modulated parameter value is finite and in range, for one or two
/// assignments from a non-finite source, with and without a polarity
/// override, at block sizes 1 and 64.
#[test]
fn non_finite_sources_give_finite_parameters() {
    for block in [1, 64] {
        let mut registry = Registry::new();
        registry.register(&BAD).unwrap();
        registry.register(&WATCH).unwrap();
        let config = EngineConfig {
            max_block: 64,
            ..EngineConfig::with_sample_rate(48_000.0)
        };
        let EngineParts {
            mut controller,
            mut engine,
            ..
        } = create_engine(Arc::new(registry), config);
        let bad = controller.add_module("test.bad").unwrap();
        let watch = controller.add_module("test.watch").unwrap();
        let source = controller.output_ref(bad, "out").unwrap();
        controller.add_assignment(source, watch, 0, 0.5).unwrap();
        // Two assignments on one parameter: infinity plus minus infinity
        // is NaN.
        controller.add_assignment(source, watch, 1, 1.0).unwrap();
        let second = controller.add_assignment(source, watch, 1, -0.5).unwrap();
        controller
            .set_assignment_polarity(second, Some(Polarity::Unipolar))
            .unwrap();
        controller.commit().unwrap();

        let mut left = [0.0; 64];
        let mut right = [0.0; 64];
        for _ in 0..(1_024 / block) {
            engine.process(&mut left[..block], &mut right[..block]);
        }
        let seen = controller.display(watch, 0).unwrap();
        assert!(seen >= 2_048.0, "block {block}: only {seen} modulated frames");
        assert_eq!(controller.display(watch, 1), Some(0.0), "block {block}");
    }
}
