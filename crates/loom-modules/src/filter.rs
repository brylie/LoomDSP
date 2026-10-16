//! Filters (FR-LIB-03).

use loom_core::signal::pitch;
use loom_core::{
    Module, ModuleConfig, ModuleDescriptor, ModuleRole, Panel, ParamOptions, ParamSpec, PortDefault, PortIo, PortSpec,
    ProcessContext, SignalType, Widget,
};
use loom_rt::Smoothed;
use std::f32::consts::PI;

/// `loom.filter.svf`: state variable filter with lowpass, bandpass, and
/// highpass outputs.
pub static SVF: ModuleDescriptor = ModuleDescriptor {
    id: "loom.filter.svf",
    version: 1,
    name: "SVF",
    category: "Filter",
    description: "State variable filter (trapezoidal integration). The cutoff input adds pitch: +0.1 doubles the cutoff.",
    inputs: &[
        PortSpec {
            name: "in",
            label: "In",
            signal: SignalType::Audio,
            description: "The audio to filter.",
            default: PortDefault::Value(0.0),
        },
        PortSpec {
            name: "cutoff",
            label: "Cutoff",
            signal: SignalType::Pitch,
            description: "Moves the cutoff as pitch: +0.1 doubles it.",
            default: PortDefault::Value(0.0),
        },
    ],
    outputs: &[
        PortSpec {
            name: "lowpass",
            label: "Lowpass",
            signal: SignalType::Audio,
            description: "Keeps the frequencies below the cutoff.",
            default: PortDefault::Value(0.0),
        },
        PortSpec {
            name: "bandpass",
            label: "Bandpass",
            signal: SignalType::Audio,
            description: "Keeps the frequencies near the cutoff.",
            default: PortDefault::Value(0.0),
        },
        PortSpec {
            name: "highpass",
            label: "Highpass",
            signal: SignalType::Audio,
            description: "Keeps the frequencies above the cutoff.",
            default: PortDefault::Value(0.0),
        },
    ],
    params: &[
        ParamSpec {
            name: "cutoff",
            label: "Cutoff",
            description: "The frequency where the filter starts to cut.",
            min: 20.0,
            max: 20_000.0,
            default: 1_000.0,
            unit: "Hz",
            options: ParamOptions::DEFAULT,
        }
        .logarithmic(),
        ParamSpec {
            name: "resonance",
            label: "Resonance",
            description: "Boosts the frequencies near the cutoff. High values ring.",
            min: 0.0,
            max: 1.0,
            default: 0.2,
            unit: "",
            options: ParamOptions::DEFAULT,
        },
    ],
    role: ModuleRole::Dsp(|config| Box::new(Svf::new(config))),
    panel: Panel::new(6, 3, &[Widget::knob(0, 0, "cutoff"), Widget::knob(3, 0, "resonance")]),
    displays: &[],
};

struct Svf {
    sample_rate: f32,
    cutoff: Smoothed,
    resonance: Smoothed,
    ic1: f32,
    ic2: f32,
}

impl Svf {
    fn new(config: &ModuleConfig) -> Self {
        Self {
            sample_rate: config.sample_rate,
            // Smooth cutoff in the pitch domain, so sweeps sound even.
            cutoff: Smoothed::new(pitch::from_hz(1_000.0), config.sample_rate, 0.01),
            resonance: Smoothed::new(0.2, config.sample_rate, 0.01),
            ic1: 0.0,
            ic2: 0.0,
        }
    }
}

impl Module for Svf {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let ([input, cutoff_mod], [lp, bp, hp]) = io.split::<2, 3>();
        let nyquist_limit = self.sample_rate * 0.49;
        for i in 0..input.len() {
            let cutoff_hz = pitch::to_hz(self.cutoff.tick() + cutoff_mod[i]).clamp(5.0, nyquist_limit);
            // Resonance 0..1 maps to damping k = 2..0.02 (Q from 0.5 to 50).
            let k = 2.0 - 1.98 * self.resonance.tick();
            let g = (PI * cutoff_hz / self.sample_rate).tan();
            let a1 = 1.0 / (1.0 + g * (g + k));
            let a2 = g * a1;
            let a3 = g * a2;

            let x = input[i];
            let v3 = x - self.ic2;
            let v1 = a1 * self.ic1 + a2 * v3;
            let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
            self.ic1 = 2.0 * v1 - self.ic1;
            self.ic2 = 2.0 * v2 - self.ic2;

            lp[i] = v2;
            bp[i] = v1;
            hp[i] = x - k * v1 - v2;
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            0 => self.cutoff.set_target(pitch::from_hz(value)),
            1 => self.resonance.set_target(value),
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }
}
