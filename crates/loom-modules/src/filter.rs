//! Filters (FR-LIB-03).

use loom_core::signal::pitch;
use loom_core::{
    Channels, Module, ModuleConfig, ModuleDescriptor, ModuleRole, Panel, ParamOptions, ParamSpec, Polarity,
    PortDefault, PortIo, PortSpec, ProcessContext, SignalType, Widget,
};
use loom_rt::Smoothed;
use std::f32::consts::PI;

/// `loom.filter.svf`: state variable filter with lowpass, bandpass,
/// highpass, and notch outputs.
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
            polarity: Polarity::Bipolar,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "cutoff",
            label: "Cutoff",
            signal: SignalType::Pitch,
            description: "Moves the cutoff as pitch: +0.1 doubles it.",
            default: PortDefault::Value(0.0),
            polarity: Polarity::Bipolar,
            channels: Channels::Downmix,
        },
    ],
    outputs: &[
        PortSpec {
            name: "lowpass",
            label: "Lowpass",
            signal: SignalType::Audio,
            description: "Keeps the frequencies below the cutoff.",
            default: PortDefault::Value(0.0),
            polarity: Polarity::Bipolar,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "bandpass",
            label: "Bandpass",
            signal: SignalType::Audio,
            description: "Keeps the frequencies near the cutoff.",
            default: PortDefault::Value(0.0),
            polarity: Polarity::Bipolar,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "highpass",
            label: "Highpass",
            signal: SignalType::Audio,
            description: "Keeps the frequencies above the cutoff.",
            default: PortDefault::Value(0.0),
            polarity: Polarity::Bipolar,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "notch",
            label: "Notch",
            signal: SignalType::Audio,
            description: "Cuts a narrow band around the cutoff and passes the rest.",
            default: PortDefault::Value(0.0),
            polarity: Polarity::Bipolar,
            channels: Channels::PerChannel,
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
        .logarithmic()
        .smooth(0.01),
        ParamSpec {
            name: "resonance",
            label: "Resonance",
            description: "Boosts the frequencies near the cutoff. High values ring.",
            min: 0.0,
            max: 1.0,
            default: 0.2,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .smooth(0.01),
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
            // The time matches `ParamSpec::smooth_seconds`, which the engine
            // uses once an assignment is modulating this knob.
            cutoff: Smoothed::new(
                pitch::from_hz(1_000.0),
                config.sample_rate,
                SVF.params[0].options.smooth_seconds,
            ),
            resonance: Smoothed::new(0.2, config.sample_rate, SVF.params[1].options.smooth_seconds),
            ic1: 0.0,
            ic2: 0.0,
        }
    }
}

impl Module for Svf {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let cutoff_samples = io.modulated(0);
        let resonance_samples = io.modulated(1);
        let ([input, cutoff_mod], [lp, bp, hp, notch]) = io.split::<2, 4>();
        let nyquist_limit = self.sample_rate * 0.49;
        for i in 0..input.len() {
            // Tick even when modulated, so removing the assignment continues
            // the glide from here. The heard value is the engine buffer.
            let cutoff_pitch = self.cutoff.tick();
            let cutoff_hz = if let Some(samples) = cutoff_samples {
                pitch::to_hz(pitch::from_hz(samples[i]) + cutoff_mod[i])
            } else {
                pitch::to_hz(cutoff_pitch + cutoff_mod[i])
            }
            .clamp(5.0, nyquist_limit);
            let resonance = if let Some(samples) = resonance_samples {
                let _ = self.resonance.tick();
                samples[i]
            } else {
                self.resonance.tick()
            };
            // Resonance 0..1 maps to damping k = 2..0.02 (Q from 0.5 to 50).
            let k = 2.0 - 1.98 * resonance;
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
            // The notch is the lowpass plus the highpass: the bandpass terms
            // cancel and `x - k * v1` is left. One addition, always in step.
            notch[i] = lp[i] + hp[i];
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            0 => self.cutoff.set_target(pitch::from_hz(value)),
            1 => self.resonance.set_target(value),
            _ => {}
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        match index {
            // The cutoff glides in pitch. The engine glides in the knob's
            // position, which is also logarithmic, so the paths match.
            0 => self.cutoff.gliding().map(pitch::to_hz),
            1 => self.resonance.gliding(),
            _ => None,
        }
    }

    fn reset(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }
}

/// `loom.filter.onepole`: a first-order filter with lowpass and highpass
/// outputs, for smoothing and gentle tone control (MT-FILTER-03).
pub static ONEPOLE: ModuleDescriptor = ModuleDescriptor {
    id: "loom.filter.onepole",
    version: 1,
    name: "One-pole",
    category: "Filter",
    description: "A gentle first-order filter, 6 dB per octave. The cutoff input adds pitch: +0.1 doubles the cutoff. Low cutoffs smooth control signals.",
    inputs: &[
        PortSpec {
            name: "in",
            label: "In",
            signal: SignalType::Audio,
            description: "The signal to filter.",
            default: PortDefault::Value(0.0),
            polarity: Polarity::Bipolar,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "cutoff",
            label: "Cutoff",
            signal: SignalType::Pitch,
            description: "Moves the cutoff as pitch: +0.1 doubles it.",
            default: PortDefault::Value(0.0),
            polarity: Polarity::Bipolar,
            channels: Channels::Downmix,
        },
    ],
    outputs: &[
        PortSpec {
            name: "lowpass",
            label: "Lowpass",
            signal: SignalType::Audio,
            description: "Keeps the frequencies below the cutoff.",
            default: PortDefault::Value(0.0),
            polarity: Polarity::Bipolar,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "highpass",
            label: "Highpass",
            signal: SignalType::Audio,
            description: "Keeps the frequencies above the cutoff.",
            default: PortDefault::Value(0.0),
            polarity: Polarity::Bipolar,
            channels: Channels::PerChannel,
        },
    ],
    params: &[ParamSpec {
        name: "cutoff",
        label: "Cutoff",
        description: "The frequency where the output is 3 dB down.",
        min: 1.0,
        max: 20_000.0,
        default: 1_000.0,
        unit: "Hz",
        options: ParamOptions::DEFAULT,
    }
    .logarithmic()
    .smooth(0.01)],
    role: ModuleRole::Dsp(|config| Box::new(OnePole::new(config))),
    panel: Panel::new(3, 3, &[Widget::knob(0, 0, "cutoff")]),
    displays: &[],
};

struct OnePole {
    sample_rate: f32,
    cutoff: Smoothed,
    state: f32,
}

impl OnePole {
    fn new(config: &ModuleConfig) -> Self {
        Self {
            sample_rate: config.sample_rate,
            // Smoothed in the pitch domain, like the SVF.
            cutoff: Smoothed::new(
                pitch::from_hz(1_000.0),
                config.sample_rate,
                ONEPOLE.params[0].options.smooth_seconds,
            ),
            state: 0.0,
        }
    }
}

impl Module for OnePole {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let cutoff_samples = io.modulated(0);
        let ([input, cutoff_mod], [lp, hp]) = io.split::<2, 2>();
        let nyquist_limit = self.sample_rate * 0.49;
        for i in 0..input.len() {
            let cutoff_pitch = self.cutoff.tick();
            let cutoff_hz = if let Some(samples) = cutoff_samples {
                pitch::to_hz(pitch::from_hz(samples[i]) + cutoff_mod[i])
            } else {
                pitch::to_hz(cutoff_pitch + cutoff_mod[i])
            }
            .clamp(0.5, nyquist_limit);
            // Trapezoidal one-pole. Warping the cutoff with `tan` puts the
            // -3 dB point exactly on the cutoff at any rate.
            let g = (PI * cutoff_hz / self.sample_rate).tan();
            let x = input[i];
            let x = if x.is_finite() { x } else { 0.0 };
            let v = (x - self.state) * g / (1.0 + g);
            let low = v + self.state;
            self.state = low + v;
            lp[i] = low;
            hp[i] = x - low;
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if index == 0 {
            self.cutoff.set_target(pitch::from_hz(value));
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        if index == 0 {
            self.cutoff.gliding().map(pitch::to_hz)
        } else {
            None
        }
    }

    fn reset(&mut self) {
        self.state = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::testing::run;
    use std::f32::consts::TAU;

    const RATE: f32 = 48_000.0;

    /// Builds a module from a descriptor, settled at the given parameters.
    fn make(descriptor: &ModuleDescriptor, params: &[(usize, f32)]) -> Box<dyn Module> {
        let ModuleRole::Dsp(create) = descriptor.role else {
            unreachable!()
        };
        let mut m = create(&ModuleConfig {
            sample_rate: RATE,
            max_block: 48_000,
        });
        for &(index, value) in params {
            m.set_param(index, value);
        }
        m.reset();
        m
    }

    /// RMS of the second half of a buffer, after the filter has settled.
    fn settled_rms(samples: &[f32]) -> f32 {
        let tail = &samples[samples.len() / 2..];
        (tail.iter().map(|v| v * v).sum::<f32>() / tail.len() as f32).sqrt()
    }

    /// A sine of `hz` lasting one second.
    fn sine(hz: f32) -> Vec<f32> {
        (0..RATE as usize).map(|i| (TAU * hz * i as f32 / RATE).sin()).collect()
    }

    /// Gain in dB of one SVF output (0 low, 1 band, 2 high, 3 notch) for a sine.
    fn svf_gain_db(output: usize, cutoff: f32, hz: f32) -> f32 {
        let mut svf = make(&SVF, &[(0, cutoff), (1, 0.2)]);
        // Let the 10 ms smoother settle, so the cutoff is the parameter.
        let input = sine(hz);
        let cutoff_mod = vec![0.0; input.len()];
        let mut outs = vec![vec![0.0; input.len()]; 4];
        let [a, b, c, d] = &mut outs[..] else { unreachable!() };
        run(
            svf.as_mut(),
            RATE,
            &[&input, &cutoff_mod],
            &[true, false],
            &mut [a, b, c, d],
            &[],
        );
        20.0 * (settled_rms(&outs[output]) / settled_rms(&input)).log10()
    }

    /// The notch cuts a sine at the cutoff by 30 dB or more, and passes a
    /// sine two octaves away within 1 dB (MT-FILTER-01).
    #[test]
    fn svf_notch_cuts_the_cutoff_and_passes_the_rest() {
        assert!(svf_gain_db(3, 1_000.0, 1_000.0) < -30.0);
        assert!(svf_gain_db(3, 1_000.0, 250.0).abs() < 1.0);
        assert!(svf_gain_db(3, 1_000.0, 4_000.0).abs() < 1.0);
    }

    /// Both one-pole outputs are 3 dB down at their cutoff.
    #[test]
    fn onepole_is_three_db_down_at_the_cutoff() {
        for cutoff in [100.0, 1_000.0, 10_000.0] {
            let mut filter = make(&ONEPOLE, &[(0, cutoff)]);
            let input = sine(cutoff);
            let cutoff_mod = vec![0.0; input.len()];
            let (mut lp, mut hp) = (vec![0.0; input.len()], vec![0.0; input.len()]);
            run(
                filter.as_mut(),
                RATE,
                &[&input, &cutoff_mod],
                &[true, false],
                &mut [&mut lp, &mut hp],
                &[],
            );
            for out in [&lp, &hp] {
                let db = 20.0 * (settled_rms(out) / settled_rms(&input)).log10();
                assert!((db + 3.01).abs() < 0.1, "cutoff {cutoff}: {db} dB");
            }
        }
    }

    /// A NaN in the input does not poison the filter state.
    #[test]
    fn onepole_recovers_from_nan() {
        let mut filter = make(&ONEPOLE, &[(0, 1_000.0)]);
        let mut input = vec![0.5; 64];
        input[3] = f32::NAN;
        let cutoff_mod = vec![0.0; 64];
        let (mut lp, mut hp) = (vec![0.0; 64], vec![0.0; 64]);
        run(
            filter.as_mut(),
            RATE,
            &[&input, &cutoff_mod],
            &[true, false],
            &mut [&mut lp, &mut hp],
            &[],
        );
        assert!(lp.iter().chain(&hp).all(|v| v.is_finite()));
    }
}
