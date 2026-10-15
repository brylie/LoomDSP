//! Phase generator: the timing source for oscillators (FR-LIB-01).
//!
//! Why a separate phase module: separating timing from wave shape lets one
//! ramp drive any shaper, and lets users bend time (sync, phase distortion,
//! step indexing) before it becomes sound.

use loom_core::signal::{EdgeDetector, pitch};
use loom_core::{
    Module, ModuleConfig, ModuleDescriptor, ModuleRole, ParamSpec, PortIo, PortSpec, ProcessContext, SignalType,
};

/// `loom.phase.generator`: a ramp from 0 to 1 at the frequency set by pitch.
pub static PHASE: ModuleDescriptor = ModuleDescriptor {
    id: "loom.phase.generator",
    version: 1,
    name: "Phase",
    category: "Phase",
    description: "Ramp from 0 to 1 at the pitch frequency. Drives oscillators and LFOs.",
    inputs: &[
        PortSpec::new("pitch", SignalType::Pitch),
        PortSpec::new("reset", SignalType::Gate),
    ],
    outputs: &[PortSpec::new("phase", SignalType::Phase)],
    params: &[
        ParamSpec::new("tune", -48.0, 48.0, 0.0, "st"),
        ParamSpec::new("octave", -10.0, 10.0, 0.0, "oct"),
    ],
    role: ModuleRole::Dsp(|config| Box::new(Phase::new(config))),
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
