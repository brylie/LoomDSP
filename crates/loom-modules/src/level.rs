//! Level building blocks (FR-LIB-05).
//!
//! An amplifier makes a signal quieter or louder and never flips its sign.

use loom_core::{
    Channels, Module, ModuleDescriptor, ModuleRole, Panel, ParamOptions, ParamSpec, Polarity, PortDefault, PortIo,
    PortSpec, ProcessContext, SignalType, Widget,
};
use loom_rt::Smoothed;

/// How far below full level the exponential response reaches, in decibels.
const EXP_RANGE_DB: f32 = 60.0;

/// `loom.level.amp` (MT-LEVEL-02): `in * curve(level) * gain`.
pub static AMP: ModuleDescriptor = ModuleDescriptor {
    id: "loom.level.amp",
    version: 1,
    name: "Amplifier",
    category: "Level",
    description: "Scales a signal by a control signal, with a linear or exponential response.",
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
            description: "How loud the output is. Values below 0 count as 0. Reads 1 while no cable is connected.",
            default: PortDefault::Value(0.0),
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
            description: "A fixed amount the output is multiplied by.",
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
            description: "How the level maps to loudness: linear, or exponential over 60 dB.",
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
            exponential: false,
        })
    }),
    // A knob is 3 by 3 and a two-way radio is 3 by 3, so the tile is 3 by 6.
    panel: Panel::new(
        3,
        6,
        &[
            Widget::knob(0, 0, "gain"),
            Widget::radio(0, 3, "response", &["Linear", "Exponential"]),
        ],
    ),
    displays: &[],
};

struct Amp {
    gain: Smoothed,
    exponential: bool,
}

impl Amp {
    /// The factor for a level: 0 below zero, 1 at 1, then the chosen curve.
    fn curve(&self, level: f32) -> f32 {
        let level = level.max(0.0);
        if !self.exponential {
            level
        } else if level == 0.0 {
            // The curve never reaches zero, so silence is set apart.
            0.0
        } else {
            // 60 dB of range: level 1 is 0 dB, level 0.5 is -30 dB.
            10.0_f32.powf(EXP_RANGE_DB * (level - 1.0) / 20.0)
        }
    }
}

impl Module for Amp {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let level_connected = io.is_connected(1);
        let modulated = io.modulated(0);
        let ([input, level], [out]) = io.split::<2, 1>();
        for (i, ((out, &input), &level)) in out.iter_mut().zip(input).zip(level).enumerate() {
            let level = if level_connected { self.curve(level) } else { 1.0 };
            let gain = if let Some(samples) = modulated {
                let _ = self.gain.tick();
                samples[i]
            } else {
                self.gain.tick()
            };
            *out = input * level * gain;
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            0 => self.gain.set_target(value),
            _ => self.exponential = value >= 0.5,
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        if index == 0 { self.gain.gliding() } else { None }
    }

    fn reset(&mut self) {
        let target = self.gain.target();
        self.gain.reset(target);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::testing::run;

    /// Runs one block with a fixed input and level and returns the last sample.
    fn amp(response: f32, level: Option<f32>) -> f32 {
        let mut module = Amp {
            gain: Smoothed::new(1.0, 48_000.0, 0.005),
            exponential: false,
        };
        module.set_param(1, response);
        let input = [0.8_f32; 8];
        let levels = [level.unwrap_or(0.0); 8];
        let mut out = [0.0_f32; 8];
        run(
            &mut module,
            48_000.0,
            &[&input, &levels],
            &[true, level.is_some()],
            &mut [&mut out],
            &[],
        );
        out[7]
    }

    /// Level 0.5 halves the input in linear mode.
    #[test]
    fn linear_halves() {
        assert!((amp(0.0, Some(0.5)) - 0.4).abs() < 1e-6);
    }

    /// Level 0.5 is -30 dB in exponential mode.
    #[test]
    fn exponential_minus_30_db() {
        let db = 20.0 * (amp(1.0, Some(0.5)) / 0.8).log10();
        assert!((db + 30.0).abs() < 1e-3, "{db}");
        assert!((amp(1.0, Some(1.0)) - 0.8).abs() < 1e-6);
    }

    /// Level 0 and negative levels give silence in both modes.
    #[test]
    fn zero_and_negative_levels_are_silent() {
        for response in [0.0, 1.0] {
            assert_eq!(amp(response, Some(0.0)), 0.0);
            assert_eq!(amp(response, Some(-0.5)), 0.0);
        }
    }

    /// With no cable on `level`, only the gain acts.
    #[test]
    fn unconnected_level_reads_one() {
        assert!((amp(0.0, None) - 0.8).abs() < 1e-6);
        assert!((amp(1.0, None) - 0.8).abs() < 1e-6);
    }
}
