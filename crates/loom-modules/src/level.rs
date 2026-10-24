//! Level building blocks (FR-LIB-05).

use loom_core::{
    Channels, Module, ModuleConfig, ModuleDescriptor, ModuleRole, Panel, ParamOptions, ParamSpec, Polarity,
    PortDefault, PortIo, PortSpec, ProcessContext, SignalType, Widget,
};
use loom_rt::Smoothed;

/// `loom.level.amp`: scales a signal by a control level (MT-LEVEL-02).
pub static AMP: ModuleDescriptor = ModuleDescriptor {
    id: "loom.level.amp",
    version: 1,
    name: "Amplifier",
    category: "Level",
    description: "Scales a signal by a control signal, with linear or exponential response.",
    inputs: &[
        PortSpec {
            name: "in",
            label: "In",
            signal: SignalType::Audio,
            description: "The signal to scale.",
            default: PortDefault::Value(0.0),
            polarity: Polarity::Bipolar,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "level",
            label: "Level",
            signal: SignalType::Modulation,
            description: "How loud the output is. Values below 0 count as 0.",
            default: PortDefault::Value(1.0),
            polarity: Polarity::Unipolar,
            channels: Channels::Downmix,
        },
    ],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "The input times the level (after the response curve) times the gain.",
        default: PortDefault::Value(0.0),
        polarity: Polarity::Bipolar,
        channels: Channels::PerChannel,
    }],
    params: &[
        ParamSpec {
            name: "gain",
            label: "Gain",
            description: "A fixed scale on top of the level.",
            min: 0.0,
            max: 2.0,
            default: 1.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .smooth(0.005),
        ParamSpec {
            name: "response",
            label: "Response",
            description: "0 = Linear, 1 = Exponential. Exponential maps level 1 to 0 dB and spreads lower levels over 60 dB.",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        },
    ],
    role: ModuleRole::Dsp(|config| {
        Box::new(Amp {
            gain: Smoothed::new(1.0, config.sample_rate, AMP.params[0].options.smooth_seconds),
            response: 0.0,
        })
    }),
    panel: Panel::new(3, 3, &[Widget::knob(0, 0, "gain"), Widget::knob(0, 3, "response")]),
    displays: &[],
};

/// Map a unipolar level through the response curve to a linear multiplier.
fn level_to_gain(level: f32, response: f32) -> f32 {
    let level = if level < 0.0 { 0.0 } else { level };
    if level == 0.0 {
        return 0.0;
    }
    if response < 0.5 {
        level
    } else {
        // Exponential: level 1 is 0 dB; level 0.5 is -30 dB (60 dB across 0..=1).
        10f32.powf((level - 1.0) * 3.0)
    }
}

struct Amp {
    gain: Smoothed,
    response: f32,
}

impl Module for Amp {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let level_modulated = io.modulated(1);
        let ([input, level], [out]) = io.split::<2, 1>();
        for (i, ((out, &input), &level)) in out.iter_mut().zip(input).zip(level).enumerate() {
            let level_value = if let Some(samples) = level_modulated {
                samples[i]
            } else {
                level
            };
            let gain = self.gain.tick();
            *out = input * level_to_gain(level_value, self.response) * gain;
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            0 => self.gain.set_target(value),
            1 => self.response = value,
            _ => {}
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        if index == 0 {
            self.gain.gliding()
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::level_to_gain;

    #[test]
    fn linear_level_halves_input() {
        assert!((level_to_gain(0.5, 0.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn exponential_level_half_is_minus_30db() {
        let g = level_to_gain(0.5, 1.0);
        let db = 20.0 * g.log10();
        assert!((db - -30.0).abs() < 0.1, "db={db}");
    }

    #[test]
    fn level_zero_is_silence_in_both_modes() {
        assert_eq!(level_to_gain(0.0, 0.0), 0.0);
        assert_eq!(level_to_gain(0.0, 1.0), 0.0);
    }

    #[test]
    fn negative_level_counts_as_zero() {
        assert_eq!(level_to_gain(-0.5, 0.0), 0.0);
        assert_eq!(level_to_gain(-0.5, 1.0), 0.0);
    }
}
