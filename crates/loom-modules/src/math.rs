//! Math building blocks (FR-LIB-05).
//!
//! Unconnected inputs fall back to a sensible default (0 for add, 1 for
//! multiply), so a module is useful with only one cable.

use loom_core::{
    Module, ModuleConfig, ModuleDescriptor, ModuleRole, Panel, ParamSpec, PortIo, PortSpec, ProcessContext, SignalType,
    Widget,
};
use loom_rt::Smoothed;

/// `loom.math.add`: `a + b + offset`.
pub static ADD: ModuleDescriptor = ModuleDescriptor {
    id: "loom.math.add",
    version: 1,
    name: "Add",
    category: "Math",
    description: "Adds two signals and an offset.",
    inputs: &[
        PortSpec::new("a", SignalType::Modulation),
        PortSpec::new("b", SignalType::Modulation),
    ],
    outputs: &[PortSpec::new("out", SignalType::Modulation)],
    params: &[ParamSpec::new("offset", -10.0, 10.0, 0.0, "")],
    role: ModuleRole::Dsp(|config| {
        Box::new(Add {
            offset: Smoothed::new(0.0, config.sample_rate, 0.005),
        })
    }),
    panel: Panel::new(3, 3, &[Widget::knob(0, 0, "offset")]),
    displays: &[],
};

struct Add {
    offset: Smoothed,
}

impl Module for Add {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let ([a, b], [out]) = io.split::<2, 1>();
        for ((out, &a), &b) in out.iter_mut().zip(a).zip(b) {
            *out = a + b + self.offset.tick();
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if index == 0 {
            self.offset.set_target(value);
        }
    }
}

/// `loom.math.mul`: `a * b * gain`. An unconnected `b` counts as 1.
pub static MUL: ModuleDescriptor = ModuleDescriptor {
    id: "loom.math.mul",
    version: 1,
    name: "Multiply",
    category: "Math",
    description: "Multiplies two signals and a gain. Use it as a VCA or ring modulator.",
    inputs: &[
        PortSpec::new("a", SignalType::Audio),
        PortSpec::new("b", SignalType::Modulation),
    ],
    outputs: &[PortSpec::new("out", SignalType::Audio)],
    params: &[ParamSpec::new("gain", -10.0, 10.0, 1.0, "")],
    role: ModuleRole::Dsp(|config| {
        Box::new(Mul {
            gain: Smoothed::new(1.0, config.sample_rate, 0.005),
        })
    }),
    panel: Panel::new(3, 3, &[Widget::knob(0, 0, "gain")]),
    displays: &[],
};

struct Mul {
    gain: Smoothed,
}

impl Module for Mul {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let b_connected = io.is_connected(1);
        let ([a, b], [out]) = io.split::<2, 1>();
        for ((out, &a), &b) in out.iter_mut().zip(a).zip(b) {
            let b = if b_connected { b } else { 1.0 };
            *out = a * b * self.gain.tick();
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if index == 0 {
            self.gain.set_target(value);
        }
    }
}

/// `loom.math.constant`: a fixed value, set by a parameter.
pub static CONSTANT: ModuleDescriptor = ModuleDescriptor {
    id: "loom.math.constant",
    version: 1,
    name: "Constant",
    category: "Math",
    description: "Outputs a constant value. Useful for pitch offsets, gates, and testing.",
    inputs: &[],
    outputs: &[PortSpec::new("out", SignalType::Modulation)],
    params: &[ParamSpec::new("value", -10.0, 10.0, 0.0, "")],
    role: ModuleRole::Dsp(|config: &ModuleConfig| {
        Box::new(Constant {
            value: Smoothed::new(0.0, config.sample_rate, 0.005),
        })
    }),
    panel: Panel::new(3, 3, &[Widget::knob(0, 0, "value")]),
    displays: &[],
};

struct Constant {
    value: Smoothed,
}

impl Module for Constant {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let ([], [out]) = io.split::<0, 1>();
        for out in out.iter_mut() {
            *out = self.value.tick();
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if index == 0 {
            self.value.set_target(value);
        }
    }

    fn reset(&mut self) {
        let target = self.value.target();
        self.value.reset(target);
    }
}
