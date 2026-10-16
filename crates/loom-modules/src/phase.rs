//! Phase generator: the timing source for oscillators (FR-LIB-01).
//!
//! Why a separate phase module: separating timing from wave shape lets one
//! ramp drive any shaper, and lets users bend time (sync, phase distortion,
//! step indexing) before it becomes sound.

use loom_core::signal::{EdgeDetector, WrapDetector, pitch, trigger};
use loom_core::{
    Module, ModuleConfig, ModuleDescriptor, ModuleRole, Panel, ParamOptions, ParamSpec, PortDefault, PortIo, PortSpec,
    ProcessContext, SignalType, Widget,
};

/// `loom.phase.generator`: a ramp from 0 to 1 at the frequency set by pitch.
pub static PHASE: ModuleDescriptor = ModuleDescriptor {
    id: "loom.phase.generator",
    version: 1,
    name: "Phase",
    category: "Phase",
    description: "Ramp from 0 to 1 at the pitch frequency. Drives oscillators and LFOs.",
    inputs: &[
        PortSpec {
            name: "pitch",
            label: "Pitch",
            signal: SignalType::Pitch,
            description: "Sets the frequency. 0 is middle C, and +0.1 is one octave up.",
            default: PortDefault::Value(0.0),
        },
        PortSpec {
            name: "reset",
            label: "Reset",
            signal: SignalType::Gate,
            description: "A rising edge sets the phase back to 0.",
            default: PortDefault::Value(0.0),
        },
    ],
    outputs: &[PortSpec {
        name: "phase",
        label: "Phase",
        signal: SignalType::Phase,
        description: "A ramp from 0 up to 1, once per cycle.",
        default: PortDefault::Value(0.0),
    }],
    params: &[
        ParamSpec {
            name: "tune",
            label: "Tune",
            description: "Shifts the pitch in semitones.",
            min: -48.0,
            max: 48.0,
            default: 0.0,
            unit: "st",
            options: ParamOptions::DEFAULT,
        },
        ParamSpec {
            name: "octave",
            label: "Octave",
            description: "Shifts the pitch in octaves.",
            min: -10.0,
            max: 10.0,
            default: 0.0,
            unit: "oct",
            options: ParamOptions::DEFAULT,
        },
    ],
    role: ModuleRole::Dsp(|config| Box::new(Phase::new(config))),
    panel: Panel::new(6, 3, &[Widget::knob(0, 0, "tune"), Widget::knob(3, 0, "octave")]),
    displays: &[],
};

struct Phase {
    phase: f64,
    sample_rate: f64,
    tune: f32,
    octave: f32,
    reset_in: EdgeDetector,
}

impl Phase {
    fn new(config: &ModuleConfig) -> Self {
        Self {
            phase: 0.0,
            sample_rate: f64::from(config.sample_rate),
            tune: 0.0,
            octave: 0.0,
            reset_in: EdgeDetector::new(),
        }
    }
}

impl Module for Phase {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let ([pitch_in, reset], [out]) = io.split::<2, 1>();
        let offset = self.tune * pitch::PER_SEMITONE + self.octave * pitch::PER_OCTAVE;
        for ((out, &p), &r) in out.iter_mut().zip(pitch_in).zip(reset) {
            if self.reset_in.rising(r) {
                self.phase = 0.0;
            }

            *out = self.phase as f32;
            let increment = f64::from(pitch::to_hz(p + offset)) / self.sample_rate;
            self.phase += increment;
            self.phase -= self.phase.floor();
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            0 => self.tune = value,
            1 => self.octave = value.round(),
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.phase = 0.0;
        self.reset_in.reset();
    }
}

/// `loom.phase.trigger`: fires a trigger each time its input phase wraps.
///
/// It turns any phase into triggers. With Phase Scale in front, it is a
/// clock multiplier that stays locked to its input. The wrap rule is
/// [`WrapDetector`]'s, shared with every end-of-cycle trigger.
pub static PHASE_TRIGGER: ModuleDescriptor = ModuleDescriptor {
    id: "loom.phase.trigger",
    version: 1,
    name: "Phase Trigger",
    category: "Phase",
    description: "Fires a trigger each time the phase wraps, forward or backward.",
    inputs: &[PortSpec {
        name: "phase",
        label: "Phase",
        signal: SignalType::Phase,
        description: "The phase to watch. Each wrap fires a trigger.",
        default: PortDefault::Value(0.0),
    }],
    outputs: &[PortSpec {
        name: "trigger",
        label: "Trigger",
        signal: SignalType::Gate,
        description: "Fires once each time the phase wraps.",
        default: PortDefault::Value(0.0),
    }],
    params: &[],
    role: ModuleRole::Dsp(|_| {
        Box::new(PhaseTrigger {
            wrap: WrapDetector::new(),
        })
    }),
    panel: Panel::EMPTY,
    displays: &[],
};

struct PhaseTrigger {
    wrap: WrapDetector,
}

impl Module for PhaseTrigger {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let ([phase], [out]) = io.split::<1, 1>();
        for (out, &p) in out.iter_mut().zip(phase) {
            *out = trigger(self.wrap.wrapped(p));
        }
    }

    fn reset(&mut self) {
        self.wrap.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::testing::run;

    fn phase_trigger(input: &[f32]) -> Vec<f32> {
        let mut module = PhaseTrigger {
            wrap: WrapDetector::new(),
        };
        let mut out = vec![0.0; input.len()];
        run(&mut module, 48_000.0, &[input], &[true], &mut [&mut out], &[]);
        out
    }

    #[test]
    fn fires_one_sample_on_each_wrap() {
        // Four frames per cycle, starting on a bar line.
        let input: Vec<f32> = (0..12).map(|i| (i % 4) as f32 / 4.0).collect();
        let out = phase_trigger(&input);
        let fired: Vec<usize> = (0..out.len()).filter(|&i| out[i] == 1.0).collect();
        assert_eq!(fired, [0, 4, 8]);
        assert!(out.iter().all(|&value| value == 0.0 || value == 1.0));
    }

    #[test]
    fn unconnected_input_fires_once_then_stays_low() {
        // An unconnected input reads zeros: a phase standing still at 0.
        assert_eq!(phase_trigger(&[0.0; 4]), [1.0, 0.0, 0.0, 0.0]);
    }
}
