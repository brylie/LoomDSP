//! Math building blocks (FR-LIB-05).
//!
//! Unconnected inputs fall back to a sensible default (0 for add, 1 for
//! multiply and divide), so a module is useful with only one cable.

use loom_core::{
    Channels, Module, ModuleConfig, ModuleDescriptor, ModuleRole, Panel, ParamOptions, ParamRange, ParamSpec,
    PortDefault, PortIo, PortSpec, ProcessContext, SignalType, Widget,
};
use loom_rt::Smoothed;

/// The control ranges of the wide math parameters: Add `offset`, Multiply
/// `gain`, and Constant `value` (`docs/design/control-ranges.md`). From the
/// smallest up, so the first that holds a value is the smallest. A player
/// picks one per module in the inspector. It limits the knob, not the
/// signal on a cable.
pub const WIDE_RANGES: &[ParamRange] = &[
    ParamRange::new(-0.1, 0.1),
    ParamRange::new(-1.0, 1.0),
    ParamRange::new(0.0, 1.0),
    ParamRange::new(-5.0, 5.0),
    ParamRange::new(0.0, 5.0),
    ParamRange::new(-10.0, 10.0),
    ParamRange::new(0.0, 10.0),
];

/// The range a new Add, Multiply, or Constant starts with: safe, and fits
/// pitch and audio (control-ranges.md, Open points).
pub const DEFAULT_RANGE: ParamRange = ParamRange::new(-1.0, 1.0);

/// `loom.math.add`: `a + b + offset`.
pub static ADD: ModuleDescriptor = ModuleDescriptor {
    id: "loom.math.add",
    version: 1,
    name: "Add",
    category: "Math",
    family: loom_core::Family::LogicAndMath,
    description: "Adds two signals and an offset.",
    inputs: &[
        PortSpec {
            name: "a",
            label: "A",
            signal: SignalType::Modulation,
            description: "The first signal to add.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::Downmix,
        },
        PortSpec {
            name: "b",
            label: "B",
            signal: SignalType::Modulation,
            description: "The second signal to add.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::Downmix,
        },
    ],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Modulation,
        description: "A plus B plus the offset.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
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
    .smooth(0.005)
    .ranges(WIDE_RANGES, DEFAULT_RANGE)],
    role: ModuleRole::Dsp(|config| {
        Box::new(Add {
            offset: Smoothed::new(0.0, config.sample_rate, ADD.params[0].options.smooth_seconds),
        })
    }),
    panel: Panel::new(3, 3, &[Widget::knob(0, 0, "offset")]),
    displays: &[],
};

#[derive(Clone)]
struct Add {
    offset: Smoothed,
}

impl Module for Add {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let param = io.param(0);
        let ([a, b], [out]) = io.split::<2, 1>();
        for (i, ((out, &a), &b)) in out.iter_mut().zip(a).zip(b).enumerate() {
            let offset = param.at_or(i, self.offset.tick());
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

    /// Snaps the offset to its target (live patching spec, rule 7).
    fn reset(&mut self) {
        let target = self.offset.target();
        self.offset.reset(target);
    }
}

/// `loom.math.mul`: `a * b * gain`. An unconnected `b` counts as 1.
pub static MUL: ModuleDescriptor = ModuleDescriptor {
    id: "loom.math.mul",
    version: 1,
    name: "Multiply",
    category: "Math",
    family: loom_core::Family::LogicAndMath,
    description: "Multiplies two signals and a gain, for ring modulation. To control loudness, use Amplifier.",
    inputs: &[
        PortSpec {
            name: "a",
            label: "A",
            signal: SignalType::Audio,
            description: "The signal to scale.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "b",
            label: "B",
            signal: SignalType::Modulation,
            description: "Scales A. Reads 1 while no cable is connected.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::Downmix,
        },
    ],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "A times B times the gain.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
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
    .smooth(0.005)
    .ranges(WIDE_RANGES, DEFAULT_RANGE)],
    role: ModuleRole::Dsp(|config| {
        Box::new(Mul {
            gain: Smoothed::new(1.0, config.sample_rate, MUL.params[0].options.smooth_seconds),
        })
    }),
    panel: Panel::new(3, 3, &[Widget::knob(0, 0, "gain")]),
    displays: &[],
};

#[derive(Clone)]
struct Mul {
    gain: Smoothed,
}

impl Module for Mul {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let b_connected = io.is_connected(1);
        let param = io.param(0);
        let ([a, b], [out]) = io.split::<2, 1>();
        for (i, ((out, &a), &b)) in out.iter_mut().zip(a).zip(b).enumerate() {
            let b = if b_connected { b } else { 1.0 };
            let gain = param.at_or(i, self.gain.tick());
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

    /// Snaps the gain to its target (live patching spec, rule 7).
    fn reset(&mut self) {
        let target = self.gain.target();
        self.gain.reset(target);
    }
}

/// `loom.math.sub`: `a - b`.
pub static SUB: ModuleDescriptor = ModuleDescriptor {
    id: "loom.math.sub",
    version: 1,
    name: "Subtract",
    category: "Math",
    family: loom_core::Family::LogicAndMath,
    description: "Subtracts signal B from signal A.",
    inputs: &[
        PortSpec {
            name: "a",
            label: "A",
            signal: SignalType::Modulation,
            description: "The signal to subtract from.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::Downmix,
        },
        PortSpec {
            name: "b",
            label: "B",
            signal: SignalType::Modulation,
            description: "The signal to subtract.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::Downmix,
        },
    ],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Modulation,
        description: "A minus B.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[],
    role: ModuleRole::Dsp(|_| Box::new(Sub)),
    panel: Panel::new(2, 2, &[]),
    displays: &[],
};

/// Stateless: each output sample depends only on the inputs at that sample.
struct Sub;

impl Module for Sub {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let ([a, b], [out]) = io.split::<2, 1>();
        for ((out, &a), &b) in out.iter_mut().zip(a).zip(b) {
            *out = a - b;
        }
    }
}

/// Divisors with a magnitude at or below this give 0, not infinity.
const DIVISOR_FLOOR: f32 = 1e-9;

/// `loom.math.div`: `a / b`. An unconnected `b` counts as 1, and a `b` near
/// zero gives 0, because cables are never clipped (ADR-0007) and infinity
/// would reach every module downstream.
pub static DIV: ModuleDescriptor = ModuleDescriptor {
    id: "loom.math.div",
    version: 1,
    name: "Divide",
    category: "Math",
    family: loom_core::Family::LogicAndMath,
    description: "Divides signal A by signal B. Outputs 0 while B is near zero.",
    inputs: &[
        PortSpec {
            name: "a",
            label: "A",
            signal: SignalType::Modulation,
            description: "The signal to divide.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::Downmix,
        },
        PortSpec {
            name: "b",
            label: "B",
            signal: SignalType::Modulation,
            description: "The divisor. Reads 1 while no cable is connected.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::Downmix,
        },
    ],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Modulation,
        description: "A divided by B, or 0 while B is within 1e-9 of zero.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[],
    role: ModuleRole::Dsp(|_| Box::new(Div)),
    panel: Panel::new(2, 2, &[]),
    displays: &[],
};

/// Stateless, so a NaN or infinity on an input cannot stay in the module.
struct Div;

impl Module for Div {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let b_connected = io.is_connected(1);
        let ([a, b], [out]) = io.split::<2, 1>();
        for ((out, &a), &b) in out.iter_mut().zip(a).zip(b) {
            let b = if b_connected { b } else { 1.0 };
            *out = if b.abs() <= DIVISOR_FLOOR { 0.0 } else { a / b };
        }
    }
}

/// `loom.math.constant`: a fixed value, set by a parameter.
pub static CONSTANT: ModuleDescriptor = ModuleDescriptor {
    id: "loom.math.constant",
    version: 1,
    name: "Constant",
    category: "Math",
    family: loom_core::Family::LogicAndMath,
    description: "Outputs a constant value. Useful for pitch offsets, gates, and testing.",
    inputs: &[],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Modulation,
        description: "The value set by the parameter.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
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
    .smooth(0.005)
    .ranges(WIDE_RANGES, DEFAULT_RANGE)],
    role: ModuleRole::Dsp(|config: &ModuleConfig| {
        Box::new(Constant {
            value: Smoothed::new(0.0, config.sample_rate, CONSTANT.params[0].options.smooth_seconds),
        })
    }),
    panel: Panel::new(3, 3, &[Widget::knob(0, 0, "value")]),
    displays: &[],
};

#[derive(Clone)]
struct Constant {
    value: Smoothed,
}

impl Module for Constant {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let param = io.param(0);
        let ([], [out]) = io.split::<0, 1>();
        for (i, out) in out.iter_mut().enumerate() {
            *out = param.at_or(i, self.value.tick());
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
    family: loom_core::Family::LogicAndMath,
    description: "Converts between decibels and level, and between pitch and frequency in Hz.",
    inputs: &[PortSpec {
        name: "in",
        label: "In",
        signal: SignalType::Modulation,
        description: "The value to convert.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Modulation,
        description: "The converted value.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
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
    .integer()
    .choices(&["dB to level", "Level to dB", "Pitch to Hz", "Hz to pitch"])],
    role: ModuleRole::Dsp(|_| Box::new(Convert { mode: 0 })),
    // Four radio buttons are 3 wide and 5 tall.
    panel: Panel::new(6, 2, &[Widget::select(0, 0, "mode")]),
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

/// `loom.pitch.constant` (MT-LEVEL-01): a fixed pitch, set in semitones.
pub static PITCH_CONSTANT: ModuleDescriptor = ModuleDescriptor {
    id: "loom.pitch.constant",
    version: 1,
    name: "Pitch Constant",
    category: "Pitch",
    family: loom_core::Family::NotesAndPitch,
    description: "Outputs a fixed pitch, set in semitones from middle C.",
    inputs: &[],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Pitch,
        description: "The pitch, at 0.1 per octave.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[ParamSpec {
        name: "semitones",
        label: "Semitones",
        description: "How far from middle C the pitch is. 12 is one octave up.",
        min: -48.0,
        max: 48.0,
        default: 0.0,
        unit: "st",
        options: ParamOptions::DEFAULT,
    }
    .smooth(0.005)],
    role: ModuleRole::Dsp(|config| {
        Box::new(PitchConstant {
            semitones: Smoothed::new(0.0, config.sample_rate, PITCH_CONSTANT.params[0].options.smooth_seconds),
        })
    }),
    panel: Panel::new(3, 3, &[Widget::knob(0, 0, "semitones")]),
    displays: &[],
};

#[derive(Clone)]
struct PitchConstant {
    semitones: Smoothed,
}

impl Module for PitchConstant {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let param = io.param(0);
        let ([], [out]) = io.split::<0, 1>();
        for (i, out) in out.iter_mut().enumerate() {
            let semitones = param.at_or(i, self.semitones.tick());
            // 0.1 per octave, so 12 semitones are 0.1.
            *out = semitones / 120.0;
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if index == 0 {
            self.semitones.set_target(value);
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        if index == 0 { self.semitones.gliding() } else { None }
    }

    fn reset(&mut self) {
        let target = self.semitones.target();
        self.semitones.reset(target);
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

#[cfg(test)]
mod arithmetic_tests {
    use super::*;
    use loom_core::testing::run;

    /// Runs a module over `a` and `b`, with `b` cabled or not.
    fn apply(module: &mut dyn Module, a: &[f32], b: &[f32], b_connected: bool) -> Vec<f32> {
        let mut out = vec![0.0; a.len()];
        run(module, 48_000.0, &[a, b], &[true, b_connected], &mut [&mut out], &[]);
        out
    }

    /// 3 minus 5 gives -2.
    #[test]
    fn subtract_known_point() {
        assert_eq!(apply(&mut Sub, &[3.0], &[5.0], true), [-2.0]);
    }

    /// 6 divided by 3 gives 2.
    #[test]
    fn divide_known_point() {
        assert_eq!(apply(&mut Div, &[6.0], &[3.0], true), [2.0]);
    }

    /// A zero or tiny divisor gives 0, never infinity or NaN, even for 0 / 0.
    #[test]
    fn divide_by_zero_gives_zero() {
        let a = [1.0, -1.0, 0.0, 5.0, 5.0, 5.0];
        let b = [0.0, 0.0, 0.0, -0.0, 1e-9, -1e-9];
        assert_eq!(apply(&mut Div, &a, &b, true), [0.0; 6]);
        assert_eq!(apply(&mut Div, &[1.0], &[2e-9], true), [5e8]);
    }

    /// With no cable on B, Divide passes A unchanged.
    #[test]
    fn divide_without_cable_passes_a() {
        let a = [0.25, -3.0, 7.5];
        assert_eq!(apply(&mut Div, &a, &[0.0; 3], false), a);
    }

    /// The modules keep no state, so a NaN or infinity on one frame does not
    /// touch the next one.
    #[test]
    fn non_finite_input_does_not_stick() {
        let a = [f32::NAN, f32::INFINITY, 1.0, 6.0];
        let b = [1.0, 1.0, f32::NEG_INFINITY, 3.0];
        let sub = apply(&mut Sub, &a, &b, true);
        assert!(sub[0].is_nan() && sub[1].is_infinite() && sub[2].is_infinite());
        assert_eq!(sub[3], 3.0);
        let div = apply(&mut Div, &a, &b, true);
        assert!(div[0].is_nan() && div[1].is_infinite());
        assert_eq!(div[2], 0.0);
        assert_eq!(div[3], 2.0);
    }
}

#[cfg(test)]
mod pitch_constant_tests {
    use super::*;
    use loom_core::testing::run;

    /// 12 semitones output 0.1, one octave in the pitch signal.
    #[test]
    fn twelve_semitones_is_one_octave() {
        let ModuleRole::Dsp(create) = PITCH_CONSTANT.role else {
            unreachable!()
        };
        let mut m = create(&ModuleConfig {
            sample_rate: 48_000.0,
            max_block: 4,
        });
        m.set_param(0, 12.0);
        m.reset();
        let mut out = [0.0; 4];
        run(m.as_mut(), 48_000.0, &[], &[], &mut [&mut out], &[]);
        assert!(out.iter().all(|v| (v - 0.1).abs() < 1e-7), "{out:?}");
    }
}
