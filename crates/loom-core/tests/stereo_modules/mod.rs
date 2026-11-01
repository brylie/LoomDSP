//! Test-only modules for the stereo tests (ADR-0020). Built-in stereo
//! modules (Join, Split) come in slice 2b; these stand in for them.

#![allow(dead_code)]

use loom_core::{
    Channels, EngineConfig, EngineParts, Module, ModuleDescriptor, ModuleId, ModuleRole, OutputSafety, Panel,
    ParamOptions, ParamSpec, PortDefault, PortIo, PortSpec, ProbeKind, ProcessContext, Registry, SignalType,
    create_engine,
};
use std::collections::BTreeMap;
use std::sync::Arc;

pub const SAMPLE_RATE: f32 = 48_000.0;

const fn port(name: &'static str, signal: SignalType, channels: Channels) -> PortSpec {
    PortSpec {
        name,
        label: name,
        signal,
        description: "A test port.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels,
    }
}

const fn param(name: &'static str, min: f32, max: f32, default: f32) -> ParamSpec {
    ParamSpec {
        name,
        label: name,
        description: "A test parameter.",
        min,
        max,
        default,
        unit: "",
        options: ParamOptions::DEFAULT,
    }
}

macro_rules! descriptor {
    ($id:literal, $role:expr, $inputs:expr, $outputs:expr, $params:expr) => {
        ModuleDescriptor {
            id: $id,
            version: 1,
            name: $id,
            category: "Test",
            description: "A test module.",
            inputs: $inputs,
            outputs: $outputs,
            params: $params,
            role: $role,
            panel: Panel::EMPTY,
            displays: &[],
        }
    };
}

/// The left side of [`Ramp`] at frame `n`.
pub fn ramp_left(n: usize) -> f32 {
    0.001 * (n % 1000) as f32
}

/// The right side of [`Ramp`] at frame `n`.
pub fn ramp_right(n: usize) -> f32 {
    -0.003 * (n % 700) as f32
}

/// `test.ramp`: a stereo source whose sides differ.
pub static RAMP: ModuleDescriptor = descriptor!(
    "test.ramp",
    ModuleRole::Dsp(|_| Box::new(Ramp(0))),
    &[],
    &[port("out", SignalType::Audio, Channels::Stereo)],
    &[]
);

struct Ramp(usize);

impl Module for Ramp {
    fn process(&mut self, _: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let [left, right] = io.stereo_output(0);
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            *l = ramp_left(self.0);
            *r = ramp_right(self.0);
            self.0 += 1;
        }
    }
}

/// `test.mono-ramp`: a mono source, the left side of [`Ramp`].
pub static MONO_RAMP: ModuleDescriptor = descriptor!(
    "test.mono-ramp",
    ModuleRole::Dsp(|_| Box::new(MonoRamp(0))),
    &[],
    &[port("out", SignalType::Audio, Channels::PerChannel)],
    &[]
);

struct MonoRamp(usize);

impl Module for MonoRamp {
    fn process(&mut self, _: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        for x in io.output(0) {
            *x = ramp_left(self.0);
            self.0 += 1;
        }
    }
}

/// `test.pair`: a stereo source holding its two parameters.
pub static PAIR: ModuleDescriptor = descriptor!(
    "test.pair",
    ModuleRole::Dsp(|_| Box::new(Pair([0.0; 2]))),
    &[],
    &[port("out", SignalType::Audio, Channels::Stereo)],
    &[param("left", -10.0, 10.0, 0.0), param("right", -10.0, 10.0, 0.0)]
);

struct Pair([f32; 2]);

impl Module for Pair {
    fn process(&mut self, _: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let [left, right] = io.stereo_output(0);
        left.fill(self.0[0]);
        right.fill(self.0[1]);
    }

    fn set_param(&mut self, index: usize, value: f32) {
        self.0[index] = value;
    }
}

/// `test.lag`: a one-pole low-pass, so it has state. Also reports its
/// channel and the coefficient it used.
pub static LAG: ModuleDescriptor = descriptor!(
    "test.lag",
    ModuleRole::Dsp(|_| Box::new(Lag { y: 0.0, coef: 0.25 })),
    &[port("in", SignalType::Audio, Channels::PerChannel)],
    &[
        port("out", SignalType::Audio, Channels::PerChannel),
        port("channel", SignalType::Modulation, Channels::PerChannel),
        port("coef", SignalType::Modulation, Channels::PerChannel),
    ],
    &[param("coef", 0.0, 1.0, 0.25)]
);

/// One step of [`Lag`].
pub fn lag_step(y: f32, x: f32, coef: f32) -> f32 {
    y + coef * (x - y)
}

#[derive(Clone)]
struct Lag {
    y: f32,
    coef: f32,
}

impl Module for Lag {
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let modulated = io.modulated(0);
        let channel = ctx.channel() as f32;
        let ([input], [out, ch, coef]) = io.split::<1, 3>();
        for i in 0..input.len() {
            let c = modulated.map_or(self.coef, |m| m[i]);
            self.y = lag_step(self.y, input[i], c);
            out[i] = self.y;
            ch[i] = channel;
            coef[i] = c;
        }
    }

    fn set_param(&mut self, _: usize, value: f32) {
        self.coef = value;
    }

    fn reset(&mut self) {
        self.y = 0.0;
    }
}

/// `test.read`: copies three downmixed inputs, one of each kind, and its
/// modulated parameter. It has no `PerChannel` input, so it always runs
/// as one.
pub static READ: ModuleDescriptor = descriptor!(
    "test.read",
    ModuleRole::Dsp(|_| Box::new(Read(0.0))),
    &[
        port("level", SignalType::Modulation, Channels::Downmix),
        port("gate", SignalType::Gate, Channels::Downmix),
        port("phase", SignalType::Phase, Channels::Downmix),
    ],
    &[
        port("level", SignalType::Modulation, Channels::PerChannel),
        port("gate", SignalType::Gate, Channels::PerChannel),
        port("phase", SignalType::Phase, Channels::PerChannel),
        port("p", SignalType::Modulation, Channels::PerChannel),
    ],
    &[param("p", 0.0, 1.0, 0.0)]
);

struct Read(f32);

impl Module for Read {
    fn process(&mut self, _: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let modulated = io.modulated(0);
        let base = self.0;
        let ([level, gate, phase], [o_level, o_gate, o_phase, o_p]) = io.split::<3, 4>();
        o_level.copy_from_slice(level);
        o_gate.copy_from_slice(gate);
        o_phase.copy_from_slice(phase);
        match modulated {
            Some(m) => o_p.copy_from_slice(m),
            None => o_p.fill(base),
        }
    }

    fn set_param(&mut self, _: usize, value: f32) {
        self.0 = value;
    }
}

/// `test.add`: `a + b / 2`, with two audio inputs, for feedback loops.
pub static ADD: ModuleDescriptor = descriptor!(
    "test.add",
    ModuleRole::Dsp(|_| Box::new(Add)),
    &[
        port("a", SignalType::Audio, Channels::PerChannel),
        port("b", SignalType::Audio, Channels::PerChannel),
    ],
    &[port("out", SignalType::Audio, Channels::PerChannel)],
    &[]
);

struct Add;

impl Module for Add {
    fn process(&mut self, _: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let ([a, b], [out]) = io.split::<2, 1>();
        for ((o, a), b) in out.iter_mut().zip(a).zip(b) {
            *o = a + 0.5 * b;
        }
    }
}

/// `test.split`: one `Stereo` input, one mono output per side.
pub static SPLIT: ModuleDescriptor = descriptor!(
    "test.split",
    ModuleRole::Dsp(|_| Box::new(Split)),
    &[port("in", SignalType::Audio, Channels::Stereo)],
    &[
        port("left", SignalType::Audio, Channels::PerChannel),
        port("right", SignalType::Audio, Channels::PerChannel),
    ],
    &[]
);

struct Split;

impl Module for Split {
    fn process(&mut self, _: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let [left, right] = io.stereo_input(0);
        io.output(0).copy_from_slice(left);
        io.output(1).copy_from_slice(right);
    }
}

pub static DELAY: ModuleDescriptor = descriptor!(
    "test.delay1",
    ModuleRole::UnitDelay,
    &[port("in", SignalType::Audio, Channels::PerChannel)],
    &[port("out", SignalType::Audio, Channels::PerChannel)],
    &[]
);

pub static OUT: ModuleDescriptor = descriptor!(
    "test.out",
    ModuleRole::AudioOutput,
    &[port("in", SignalType::Audio, Channels::Stereo)],
    &[],
    &[]
);

pub fn registry() -> Arc<Registry> {
    let mut registry = Registry::new();
    for descriptor in [&RAMP, &MONO_RAMP, &PAIR, &LAG, &READ, &ADD, &SPLIT, &DELAY, &OUT] {
        registry.register(descriptor).unwrap();
    }
    Arc::new(registry)
}

/// An engine with the stereo test registry and no output safety stage, so
/// the device output is the raw sum. `max_block` is also the block size.
pub fn engine(max_block: usize) -> EngineParts {
    let config = EngineConfig {
        max_block,
        scope_capacity: 8192,
        output_safety: OutputSafety::Off,
        ..EngineConfig::with_sample_rate(SAMPLE_RATE)
    };
    create_engine(registry(), config)
}

/// Probes outputs by name and collects their scope samples.
pub struct Capture {
    probes: Vec<loom_core::ProbeId>,
    samples: BTreeMap<loom_core::ProbeId, Vec<f32>>,
}

impl Capture {
    pub fn new(parts: &mut EngineParts, outputs: &[(ModuleId, &str)]) -> Self {
        let probes: Vec<_> = outputs
            .iter()
            .map(|&(module, name)| {
                let output = parts.controller.output_ref(module, name).unwrap();
                parts.controller.add_probe(output, ProbeKind::Scope).unwrap()
            })
            .collect();
        Self {
            samples: probes.iter().map(|&p| (p, Vec::new())).collect(),
            probes,
        }
    }

    fn drain(&mut self, parts: &mut EngineParts) {
        for chunk in parts.telemetry.scope() {
            if let Some(samples) = self.samples.get_mut(&chunk.probe) {
                samples.extend_from_slice(&chunk.samples[..chunk.len as usize]);
            }
        }
    }

    pub fn get(&self, index: usize) -> &[f32] {
        &self.samples[&self.probes[index]]
    }

    /// Renders `frames` frames in buffers of `block` and returns the device
    /// output, left and right.
    pub fn render(&mut self, parts: &mut EngineParts, frames: usize, block: usize) -> (Vec<f32>, Vec<f32>) {
        let (mut out_l, mut out_r) = (Vec::new(), Vec::new());
        let mut left = vec![0.0; block];
        let mut right = vec![0.0; block];
        let mut position = 0;
        while position < frames {
            let n = block.min(frames - position);
            parts.engine.process(&mut left[..n], &mut right[..n]);
            out_l.extend_from_slice(&left[..n]);
            out_r.extend_from_slice(&right[..n]);
            self.drain(parts);
            position += n;
        }
        (out_l, out_r)
    }
}
