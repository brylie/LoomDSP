//! Math building blocks (FR-LIB-05).
//!
//! Unconnected inputs fall back to a sensible default (0 for add, 1 for
//! multiply), so a module is useful with only one cable.

use loom_core::{
    Channels, Module, ModuleConfig, ModuleDescriptor, ModuleRole, Panel, ParamOptions, ParamSpec, Polarity,
    PortDefault, PortIo, PortSpec, ProcessContext, SignalType, Widget,
};
use loom_rt::Smoothed;

/// `loom.math.add`: `a + b + offset`.
pub static ADD: ModuleDescriptor = ModuleDescriptor {
    id: "loom.math.add",
    version: 1,
    name: "Add",
    category: "Math",
    description: "Adds two signals and an offset.",
    inputs: &[
        PortSpec {
            name: "a",
            label: "A",
            signal: SignalType::Modulation,
            description: "The first signal to add.",
            default: PortDefault::Value(0.0),
            polarity: Polarity::Bipolar,
            channels: Channels::Downmix,
        },
        PortSpec {
            name: "b",
            label: "B",
            signal: SignalType::Modulation,
            description: "The second signal to add.",
            default: PortDefault::Value(0.0),
            polarity: Polarity::Bipolar,
            channels: Channels::Downmix,
        },
    ],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Modulation,
        description: "A plus B plus the offset.",
        default: PortDefault::Value(0.0),
        polarity: Polarity::Bipolar,
        channels: Channels::PerChannel,
    }],
    params: &[ParamSpec {
        name: "offset",
        label: "Offset",
        description: "A fixed amount added to the sum.",
        min: -10.0,
        max: 10.0,
        default: 0.0,
        unit: "",
        options: ParamOptions::DEFAULT,
    }
    .smooth(0.005)],
    role: ModuleRole::Dsp(|config| {
        Box::new(Add {
            offset: Smoothed::new(0.0, config.sample_rate, ADD.params[0].options.smooth_seconds),
        })
    }),
    panel: Panel::new(3, 3, &[Widget::knob(0, 0, "offset")]),
    displays: &[],
};

struct Add {
    offset: Smoothed,
}

impl Module for Add {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let modulated = io.modulated(0);
        let ([a, b], [out]) = io.split::<2, 1>();
        for (i, ((out, &a), &b)) in out.iter_mut().zip(a).zip(b).enumerate() {
            let offset = if let Some(samples) = modulated {
                let _ = self.offset.tick();
                samples[i]
            } else {
                self.offset.tick()
            };
            *out = a + b + offset;
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if index == 0 {
            self.offset.set_target(value);
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        if index == 0 { self.offset.gliding() } else { None }
    }
}

/// `loom.math.mul`: `a * b * gain`. An unconnected `b` counts as 1.
pub static MUL: ModuleDescriptor = ModuleDescriptor {
    id: "loom.math.mul",
    version: 1,
    name: "Multiply",
    category: "Math",
    description: "Multiplies two signals and a gain. Use it as a ring modulator. To control loudness, use Amplifier.",
    inputs: &[
        PortSpec {
            name: "a",
            label: "A",
            signal: SignalType::Audio,
            description: "The signal to scale.",
            default: PortDefault::Value(0.0),
            polarity: Polarity::Bipolar,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "b",
            label: "B",
            signal: SignalType::Modulation,
            description: "Scales A. Reads 1 while no cable is connected.",
            default: PortDefault::Value(0.0),
            polarity: Polarity::Bipolar,
            channels: Channels::Downmix,
        },
    ],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "A times B times the gain.",
        default: PortDefault::Value(0.0),
        polarity: Polarity::Bipolar,
        channels: Channels::PerChannel,
    }],
    params: &[ParamSpec {
        name: "gain",
        label: "Gain",
        description: "A fixed amount the product is multiplied by.",
        min: -10.0,
        max: 10.0,
        default: 1.0,
        unit: "",
        options: ParamOptions::DEFAULT,
    }
    .smooth(0.005)],
    role: ModuleRole::Dsp(|config| {
        Box::new(Mul {
            gain: Smoothed::new(1.0, config.sample_rate, MUL.params[0].options.smooth_seconds),
        })
    }),
    panel: Panel::new(3, 3, &[Widget::knob(0, 0, "gain")]),
    displays: &[],
};

struct Mul {
    gain: Smoothed,
}

impl Module for Mul {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let b_connected = io.is_connected(1);
        let modulated = io.modulated(0);
        let ([a, b], [out]) = io.split::<2, 1>();
        for (i, ((out, &a), &b)) in out.iter_mut().zip(a).zip(b).enumerate() {
            let b = if b_connected { b } else { 1.0 };
            let gain = if let Some(samples) = modulated {
                let _ = self.gain.tick();
                samples[i]
            } else {
                self.gain.tick()
            };
            *out = a * b * gain;
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if index == 0 {
            self.gain.set_target(value);
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        if index == 0 { self.gain.gliding() } else { None }
    }
}

/// `loom.math.constant`: a fixed value, set by a parameter.
pub static CONSTANT: ModuleDescriptor = ModuleDescriptor {
    id: "loom.math.constant",
    version: 1,
    name: "Constant",
    category: "Math",
    description: "Outputs a constant value. Useful for pitch offsets, gates, and testing.",
    inputs: &[],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Modulation,
        description: "The value set by the parameter.",
        default: PortDefault::Value(0.0),
        polarity: Polarity::Bipolar,
        channels: Channels::PerChannel,
    }],
    params: &[ParamSpec {
        name: "value",
        label: "Value",
        description: "The number the output holds.",
        min: -10.0,
        max: 10.0,
        default: 0.0,
        unit: "",
        options: ParamOptions::DEFAULT,
    }
    .smooth(0.005)],
    role: ModuleRole::Dsp(|config: &ModuleConfig| {
        Box::new(Constant {
            value: Smoothed::new(0.0, config.sample_rate, CONSTANT.params[0].options.smooth_seconds),
        })
    }),
    panel: Panel::new(3, 3, &[Widget::knob(0, 0, "value")]),
    displays: &[],
};

struct Constant {
    value: Smoothed,
}

impl Module for Constant {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let modulated = io.modulated(0);
        let ([], [out]) = io.split::<0, 1>();
        for (i, out) in out.iter_mut().enumerate() {
            *out = if let Some(samples) = modulated {
                let _ = self.value.tick();
                samples[i]
            } else {
                self.value.tick()
            };
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if index == 0 {
            self.value.set_target(value);
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        if index == 0 { self.value.gliding() } else { None }
    }

    fn reset(&mut self) {
        let target = self.value.target();
        self.value.reset(target);
    }
}

/// Lowest level in decibels. A level of 0 gives this, never minus infinity.
const CONVERT_FLOOR_DB: f32 = -120.0;
/// Highest decibel value converted, so a huge input cannot make infinity.
const CONVERT_CEIL_DB: f32 = 120.0;
/// Lowest frequency converted to a pitch, in Hz. Inputs at or below 0 Hz use it.
const CONVERT_FLOOR_HZ: f32 = 0.001;
/// Highest frequency converted to a pitch, in Hz.
const CONVERT_CEIL_HZ: f32 = 1.0e9;
/// Highest pitch converted to Hz, so a huge input cannot make infinity.
const CONVERT_CEIL_PITCH: f32 = 1.0;

/// `loom.math.convert` (MT-MATH-03): converts one unit to another.
pub static CONVERT: ModuleDescriptor = ModuleDescriptor {
    id: "loom.math.convert",
    version: 1,
    name: "Convert",
    category: "Math",
    description: "Converts between decibels and level, and between pitch and frequency in Hz.",
    inputs: &[PortSpec {
        name: "in",
        label: "In",
        signal: SignalType::Modulation,
        description: "The value to convert.",
        default: PortDefault::Value(0.0),
        polarity: Polarity::Bipolar,
        channels: Channels::PerChannel,
    }],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Modulation,
        description: "The converted value.",
        default: PortDefault::Value(0.0),
        polarity: Polarity::Bipolar,
        channels: Channels::PerChannel,
    }],
    params: &[ParamSpec {
        name: "mode",
        label: "Mode",
        description: "Which conversion to do: dB to level, level to dB, pitch to Hz, or Hz to pitch.",
        min: 0.0,
        max: 3.0,
        default: 0.0,
        unit: "",
        options: ParamOptions::DEFAULT,
    }
    .integer()],
    role: ModuleRole::Dsp(|_| Box::new(Convert { mode: 0 })),
    // Four radio buttons are 3 wide and 5 tall.
    panel: Panel::new(
        3,
        5,
        &[Widget::radio(
            0,
            0,
            "mode",
            &["dB to level", "Level to dB", "Pitch to Hz", "Hz to pitch"],
        )],
    ),
    displays: &[],
};

struct Convert {
    mode: u8,
}

impl Convert {
    /// Converts one value in the current mode. Never gives infinity or NaN
    /// for a finite input. A NaN input stays NaN.
    fn convert(&self, x: f32) -> f32 {
        match self.mode {
            0 => 10.0_f32.powf(x.clamp(CONVERT_FLOOR_DB, CONVERT_CEIL_DB) / 20.0),
            1 => (20.0 * x.abs().log10()).clamp(CONVERT_FLOOR_DB, CONVERT_CEIL_DB),
            // Not `min`: it would turn NaN into a number.
            2 => loom_core::signal::pitch::to_hz(if x > CONVERT_CEIL_PITCH { CONVERT_CEIL_PITCH } else { x }),
            _ => loom_core::signal::pitch::from_hz(x.clamp(CONVERT_FLOOR_HZ, CONVERT_CEIL_HZ)),
        }
    }
}

impl Module for Convert {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let ([input], [out]) = io.split::<1, 1>();
        for (out, &x) in out.iter_mut().zip(input) {
            *out = self.convert(x);
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if index == 0 {
            self.mode = value.round().clamp(0.0, 3.0) as u8;
        }
    }
}

#[cfg(test)]
mod convert_tests {
    use super::*;

    fn run(mode: u8, x: f32) -> f32 {
        Convert { mode }.convert(x)
    }

    /// The conversions hit the known points from the issue.
    #[test]
    fn known_points() {
        assert_eq!(run(0, 0.0), 1.0);
        assert!((run(0, -6.02) - 0.5).abs() < 0.001);
        assert!((run(2, 0.0) - 261.63).abs() < 0.01);
        assert!(run(3, 261.625_58).abs() < 1e-6);
    }

    /// dB to level and back returns the input.
    #[test]
    fn db_round_trip() {
        for db in [-60.0, -6.0, 0.0, 12.0] {
            assert!((run(1, run(0, db)) - db).abs() < 1e-3, "{db}");
        }
    }

    /// A NaN input stays NaN in every mode.
    #[test]
    fn nan_passes_through() {
        for mode in 0..4 {
            assert!(run(mode, f32::NAN).is_nan(), "mode {mode}");
        }
    }

    /// Silence and extreme inputs stay finite.
    #[test]
    fn stays_finite() {
        assert_eq!(run(1, 0.0), -120.0);
        for mode in 0..4 {
            for x in [0.0, -1.0, 1e30, -1e30, f32::INFINITY, f32::NEG_INFINITY] {
                assert!(run(mode, x).is_finite(), "mode {mode} input {x}");
            }
        }
    }
}
