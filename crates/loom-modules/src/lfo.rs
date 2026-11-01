//! The LFO (MT-MOD-01, FR-LIB-01).
//!
//! Spec: `docs/design/p1-modules.md`, LFO.

use loom_core::{
    Channels, Module, ModuleConfig, ModuleDescriptor, ModuleRole, Panel, ParamOptions, ParamSpec, PortDefault, PortIo,
    PortSpec, ProcessContext, SignalType, Widget,
};
use loom_rt::Smoothed;
use std::f64::consts::TAU;

/// `loom.mod.lfo`: a slow periodic signal, free-running or locked to the
/// transport.
pub static LFO: ModuleDescriptor = ModuleDescriptor {
    id: "loom.mod.lfo",
    version: 1,
    name: "LFO",
    category: "Modulation",
    family: loom_core::Family::Modulation,
    description: "A slow wave. Free mode runs at a rate in Hz; synced mode follows a phase, by default the bar phase.",
    inputs: &[PortSpec {
        name: "phase",
        label: "Phase",
        signal: SignalType::Phase,
        description: "In synced mode, the phase to follow. Follows the bar phase while no cable is connected. Free mode ignores it.",
        default: PortDefault::BarPhase,
        range: loom_core::PortRange::Signal,
        channels: Channels::Downmix,
    }],
    outputs: &[
        PortSpec {
            name: "out",
            label: "Out",
            signal: SignalType::Modulation,
            description: "The wave, from -1 to 1.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "out90",
            label: "Out +90°",
            signal: SignalType::Modulation,
            description: "The same wave a quarter cycle ahead. With Out on an XY scope, a sine draws a circle.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "phase",
            label: "Phase",
            signal: SignalType::Phase,
            description: "The phase, from 0 to 1, that the wave is using.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::PerChannel,
        },
    ],
    params: &[
        ParamSpec {
            name: "sync",
            label: "Sync",
            description: "Free: run at the rate. Synced: follow the phase input.",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer()
        .choices(&["Free", "Synced"]),
        ParamSpec {
            name: "rate",
            label: "Rate",
            description: "How many waves per second in free mode.",
            min: 0.01,
            max: 50.0,
            default: 1.0,
            unit: "Hz",
            options: ParamOptions::DEFAULT,
        }
        .logarithmic()
        .smooth(0.005)
        .shown_when("sync", &[0]),
        ParamSpec {
            name: "cycles",
            label: "Cycles",
            description: "How many waves fit in one cycle of the phase input in synced mode.",
            min: 1.0,
            max: 16.0,
            default: 1.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer()
        .shown_when("sync", &[1]),
        ParamSpec {
            name: "shape",
            label: "Shape",
            description: "The wave: sine, triangle, ramp up, ramp down, or square.",
            min: 0.0,
            max: 4.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer()
        .choices(&["Sine", "Triangle", "Ramp up", "Ramp down", "Square"]),
    ],
    role: ModuleRole::Dsp(|config| Box::new(Lfo::new(config))),
    panel: Panel::new(
        15,
        10,
        &[
            Widget::shape(0, 0, 15, "out", "Wave", lfo_preview),
            Widget::segmented(0, 5, 7, "sync"),
            Widget::select(9, 5, "shape"),
            // One of these shows, by the sync mode (ADR-0030).
            Widget::knob(3, 7, "rate"),
            Widget::knob(9, 7, "cycles"),
        ],
    ),
    displays: &[],
};

/// One cycle of the selected wave for the read-only tile preview.
fn lfo_preview(params: &[f32], x: f32) -> f32 {
    let shape = params.get(3).copied().unwrap_or_default().round() as u8;
    (Lfo::wave(shape, f64::from(x.rem_euclid(1.0))) + 1.0) * 0.5
}

#[cfg(test)]
mod preview_tests;

#[derive(Clone)]
struct Lfo {
    sample_rate: f64,
    synced: bool,
    cycles: f64,
    shape: u8,
    rate: Smoothed,
    /// The free-running phase, kept in `f64` so it does not drift.
    phase: f64,
}

impl Lfo {
    const SYNC: usize = LFO.param_index("sync").unwrap();
    const RATE: usize = LFO.param_index("rate").unwrap();
    const CYCLES: usize = LFO.param_index("cycles").unwrap();
    const SHAPE: usize = LFO.param_index("shape").unwrap();

    fn new(config: &ModuleConfig) -> Self {
        Self {
            sample_rate: f64::from(config.sample_rate),
            synced: false,
            cycles: 1.0,
            shape: 0,
            rate: Smoothed::new(1.0, config.sample_rate, LFO.params[Self::RATE].options.smooth_seconds),
            phase: 0.0,
        }
    }

    /// The wave at phase `p` in `[0, 1)`. Every shape crosses 0 going up at
    /// phase 0, so a sine and a triangle line up.
    fn wave(shape: u8, p: f64) -> f32 {
        let fract = |x: f64| x - x.floor();
        let value = match shape {
            0 => (TAU * p).sin(),
            1 => 1.0 - 4.0 * (fract(p + 0.25) - 0.5).abs(),
            2 => 2.0 * fract(p + 0.5) - 1.0,
            3 => -(2.0 * fract(p + 0.5) - 1.0),
            _ => {
                if p < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
        };
        value as f32
    }
}

impl Module for Lfo {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let (sync_mod, rate_mod, cycles_mod, shape_mod) = (
            io.param(Self::SYNC),
            io.param(Self::RATE),
            io.param(Self::CYCLES),
            io.param(Self::SHAPE),
        );
        let ([phase_in], [out, out90, phase_out]) = io.split::<1, 3>();
        for i in 0..out.len() {
            // Modulated values count for this frame only, so removing an
            // assignment returns to the base value in the field.
            let synced = sync_mod.map_or(i, self.synced, |v| v >= 0.5);
            let cycles = cycles_mod.map_or(i, self.cycles, |v| f64::from(v.round().max(1.0)));
            let shape = shape_mod.map_or(i, self.shape, |v| v.round().max(0.0) as u8);
            let rate = f64::from(rate_mod.at_or(i, self.rate.tick()));
            let p = if synced {
                // `cycles` waves per input cycle. A whole number keeps the
                // wave continuous where the input wraps.
                let x = f64::from(phase_in[i]) * cycles;
                if x.is_finite() { x - x.floor() } else { 0.0 }
            } else {
                // The wave starts at phase 0, then the phase moves on.
                let p = self.phase;
                self.phase += rate / self.sample_rate;
                self.phase -= self.phase.floor();
                p
            };
            out[i] = Self::wave(shape, p);
            let ahead = p + 0.25;
            out90[i] = Self::wave(shape, ahead - ahead.floor());
            phase_out[i] = p as f32;
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            Self::SYNC => self.synced = value >= 0.5,
            Self::RATE => self.rate.set_target(value),
            Self::CYCLES => self.cycles = f64::from(value.round().max(1.0)),
            Self::SHAPE => self.shape = value as u8,
            _ => {}
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        if index == Self::RATE { self.rate.gliding() } else { None }
    }

    fn reset(&mut self) {
        self.phase = 0.0;
        let target = self.rate.target();
        self.rate.reset(target);
    }
}

#[cfg(test)]
mod tests;
