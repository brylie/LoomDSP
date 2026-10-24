//! Level building blocks: scaling a signal by a control signal.

use loom_core::{
    Channels, Module, ModuleDescriptor, ModuleRole, Panel, ParamOptions, ParamSpec, Polarity, PortDefault, PortIo,
    PortSpec, ProcessContext, SignalType, Widget,
};
use loom_rt::Smoothed;

/// `loom.level.amp`: the input times a level, a response curve and a gain (MT-LEVEL-02).
pub static AMP: ModuleDescriptor = ModuleDescriptor {
    id: "loom.level.amp",
    version: 1,
    name: "Amplifier",
    category: "Level",
    description: "Scales a signal by a level, with a linear or exponential response.",
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
            description: "How loud the output is; values below 0 count as 0. Unconnected it reads 1, so only the gain knob acts.",
            default: PortDefault::Value(1.0),
            polarity: Polarity::Unipolar,
            channels: Channels::Downmix,
        },
    ],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "The input times the level, after the response curve, times the gain.",
        default: PortDefault::Value(0.0),
        polarity: Polarity::Bipolar,
        channels: Channels::PerChannel,
    }],
    params: &[
        ParamSpec {
            name: "gain",
            label: "Gain",
            description: "A fixed multiplier applied after the response curve.",
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
            description: "Linear follows the level directly; exponential maps level 1 to 0 dB and spreads lower levels over 60 dB.",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer(),
    ],
    role: ModuleRole::Dsp(|config| {
        Box::new(Amp {
            gain: Smoothed::new(1.0, config.sample_rate, AMP.params[0].options.smooth_seconds),
            response: 0,
        })
    }),
    panel: Panel::new(6, 3, &[
        Widget::knob(0, 0, "gain"),
        Widget::radio(3, 0, "response", &["Linear", "Exponential"]),
    ]),
    displays: &[],
};

struct Amp {
    gain: Smoothed,
    response: u8,
}

impl Module for Amp {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let exponential = self.response == 1;
        let ([input, level], [out]) = io.split::<2, 1>();
        for ((out, &input), &level) in out.iter_mut().zip(input).zip(level) {
            let level = level.max(0.0);
            let amp = if exponential {
                // Level 1 is 0 dB, level 0 is silence, and the range between
                // spreads over 60 dB (MT-LEVEL-02).
                if level <= 0.0 {
                    0.0
                } else {
                    10.0_f32.powf(-3.0 * (1.0 - level))
                }
            } else {
                level
            };
            *out = input * amp * self.gain.tick();
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            0 => self.gain.set_target(value),
            1 => self.response = value.round().clamp(0.0, 1.0) as u8,
            _ => {}
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        if index == 0 { self.gain.gliding() } else { None }
    }
}
