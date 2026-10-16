//! Phase generator: the timing source for oscillators (FR-LIB-01).
//!
//! Why a separate phase module: separating timing from wave shape lets one
//! ramp drive any shaper, and lets users bend time (sync, phase distortion,
//! step indexing) before it becomes sound.

use loom_core::signal::{EdgeDetector, WrapDetector, pitch, trigger};
use loom_core::{
    Module, ModuleConfig, ModuleDescriptor, ModuleRole, Panel, ParamSpec, PortIo, PortSpec, ProcessContext, SignalType,
    Widget,
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

/// Phase step per sample for a frequency, at most half a cycle (Nyquist).
///
/// A high pitch overflows `to_hz` to infinity, and `inf - floor(inf)` is
/// NaN. Without this limit the phase would stay NaN for good, so a patch
/// stays silent even after the pitch comes back (#121). Frequencies above
/// Nyquist cannot be heard as themselves anyway. NaN pitch stops the phase.
fn increment(hz: f32, sample_rate: f64) -> f64 {
    let step = f64::from(hz) / sample_rate;
    if step.is_nan() { 0.0 } else { step.min(0.5) }
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
            self.phase += increment(pitch::to_hz(p + offset), self.sample_rate);
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
    inputs: &[PortSpec::new("phase", SignalType::Phase)],
    outputs: &[PortSpec::new("trigger", SignalType::Gate)],
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
    fn extreme_pitch_never_leaves_the_phase_stuck() {
        // #121: pitch 20 overflows `to_hz` to infinity. Then NaN, then a
        // normal pitch: the phase must stay in range and move again.
        let mut module = Phase::new(&ModuleConfig {
            sample_rate: 48_000.0,
            max_block: 16,
        });
        let pitch = [20.0, 20.0, f32::INFINITY, f32::NAN, 0.0, 0.0, 0.0, 0.0];
        let reset = [0.0; 8];
        let mut out = [0.0; 8];
        run(
            &mut module,
            48_000.0,
            &[&pitch, &reset],
            &[true, true],
            &mut [&mut out],
            &[],
        );
        assert!(out.iter().all(|p| (0.0..1.0).contains(p)), "{out:?}");
        // Middle C after the extremes: the phase moves by 261.6 / 48000.
        let step = out[7] - out[6];
        assert!((step - pitch::to_hz(0.0) / 48_000.0).abs() < 1e-6, "{out:?}");
    }

    #[test]
    fn unconnected_input_fires_once_then_stays_low() {
        // An unconnected input reads zeros: a phase standing still at 0.
        assert_eq!(phase_trigger(&[0.0; 4]), [1.0, 0.0, 0.0, 0.0]);
    }
}
