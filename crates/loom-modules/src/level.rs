//! Level building blocks (FR-LIB-05).
//!
//! An amplifier makes a signal quieter or louder and never flips its sign.

use loom_core::{
    Channels, Module, ModuleDescriptor, ModuleRole, Panel, ParamOptions, ParamSpec, PortDefault, PortIo, PortSpec,
    ProcessContext, SignalType, Widget,
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
    family: loom_core::Family::LevelAndRouting,
    description: "Scales a signal by a control signal, with a linear or exponential response.",
    inputs: &[
        PortSpec {
            name: "in",
            label: "In",
            signal: SignalType::Audio,
            description: "The signal to scale.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "level",
            label: "Level",
            signal: SignalType::Modulation,
            description: "How loud the output is. Values below 0 count as 0. Reads 1 while no cable is connected.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Span(0.0, 1.0),
            channels: Channels::Downmix,
        },
    ],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "The input times the level, after the response curve, times the gain.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
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
        .integer()
        .choices(&["Linear", "Exponential"]),
    ],
    role: ModuleRole::Dsp(|config| {
        Box::new(Amp {
            gain: Smoothed::new(1.0, config.sample_rate, AMP.params[Amp::GAIN].options.smooth_seconds),
            exponential: false,
        })
    }),
    // A knob is 3 by 3 and a two-way radio is 3 by 3, so the tile is 3 by 6.
    panel: Panel::new(
        9,
        5,
        &[Widget::segmented(0, 0, 9, "response"), Widget::knob(3, 2, "gain")],
    ),
    displays: &[],
};

#[derive(Clone)]
struct Amp {
    gain: Smoothed,
    exponential: bool,
}

impl Amp {
    const GAIN: usize = AMP.param_index("gain").unwrap();
    const RESPONSE: usize = AMP.param_index("response").unwrap();
    const LEVEL_IN: usize = AMP.input_index("level").unwrap();

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
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let level_connected = io.is_connected(Self::LEVEL_IN);
        let modulated = io.param(Self::GAIN);
        let ([input, level], [out]) = io.split::<2, 1>();
        for (i, ((out, &input), &level)) in out.iter_mut().zip(input).zip(level).enumerate() {
            let level = if level_connected { self.curve(level) } else { 1.0 };
            let gain = modulated.at_or(i, self.gain.tick());
            *out = input * level * gain;
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            Self::GAIN => self.gain.set_target(value),
            Self::RESPONSE => self.exponential = value >= 0.5,
            _ => {}
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        if index == Self::GAIN { self.gain.gliding() } else { None }
    }

    fn reset(&mut self) {
        let target = self.gain.target();
        self.gain.reset(target);
    }
}

/// `loom.level.constant` (MT-LEVEL-01): a fixed level, set in decibels.
pub static CONSTANT: ModuleDescriptor = ModuleDescriptor {
    id: "loom.level.constant",
    version: 1,
    name: "Level Constant",
    category: "Level",
    family: loom_core::Family::LevelAndRouting,
    description: "Outputs a fixed level, set in decibels. 0 dB is a factor of 1.",
    inputs: &[],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Modulation,
        description: "The level as a factor: 10 to the power of dB over 20.",
        default: PortDefault::Value(1.0),
        range: loom_core::PortRange::Span(0.0, 1.0),
        channels: Channels::PerChannel,
    }],
    params: &[ParamSpec {
        name: "level",
        label: "Level",
        description: "The level in decibels. 0 dB is 1.0 and -6 dB is about 0.5.",
        min: -60.0,
        max: 12.0,
        default: 0.0,
        unit: "dB",
        options: ParamOptions::DEFAULT,
    }
    .smooth(0.005)],
    role: ModuleRole::Dsp(|config| {
        Box::new(ConstantLevel {
            db: Smoothed::new(
                0.0,
                config.sample_rate,
                CONSTANT.params[ConstantLevel::LEVEL].options.smooth_seconds,
            ),
        })
    }),
    panel: Panel::new(3, 3, &[Widget::knob(0, 0, "level")]),
    displays: &[],
};

#[derive(Clone)]
struct ConstantLevel {
    /// The level in dB. Smoothed in dB, so a turn sounds even.
    db: Smoothed,
}

impl ConstantLevel {
    const LEVEL: usize = CONSTANT.param_index("level").unwrap();
}

impl Module for ConstantLevel {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let modulated = io.param(Self::LEVEL);
        let ([], [out]) = io.split::<0, 1>();
        for (i, out) in out.iter_mut().enumerate() {
            let db = modulated.at_or(i, self.db.tick());
            *out = 10.0_f32.powf(db / 20.0);
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if index == Self::LEVEL {
            self.db.set_target(value);
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        if index == Self::LEVEL { self.db.gliding() } else { None }
    }

    fn reset(&mut self) {
        let target = self.db.target();
        self.db.reset(target);
    }
}

/// `loom.level.clamp` (MT-LEVEL-05): limits a signal to a range.
pub static CLAMP: ModuleDescriptor = ModuleDescriptor {
    id: "loom.level.clamp",
    version: 1,
    name: "Clamp",
    category: "Level",
    family: loom_core::Family::LevelAndRouting,
    description: "Limits a signal to the range between two limits. NaN becomes 0 first, so Clamp makes a signal safe.",
    inputs: &[PortSpec {
        name: "in",
        label: "In",
        signal: SignalType::Modulation,
        description: "The signal to limit.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Modulation,
        description: "The input, held between the two limits.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[
        ParamSpec {
            name: "low",
            label: "Low",
            description: "One limit. If it is above High, the two swap.",
            min: -10.0,
            max: 10.0,
            default: -1.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .smooth(0.005),
        ParamSpec {
            name: "high",
            label: "High",
            description: "The other limit. If it is below Low, the two swap.",
            min: -10.0,
            max: 10.0,
            default: 1.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .smooth(0.005),
    ],
    role: ModuleRole::Dsp(|config| {
        Box::new(Clamp {
            low: Smoothed::new(
                -1.0,
                config.sample_rate,
                CLAMP.params[Clamp::LOW].options.smooth_seconds,
            ),
            high: Smoothed::new(
                1.0,
                config.sample_rate,
                CLAMP.params[Clamp::HIGH].options.smooth_seconds,
            ),
        })
    }),
    panel: Panel::new(6, 3, &[Widget::knob(0, 0, "low"), Widget::knob(3, 0, "high")]),
    displays: &[],
};

/// Holds `x` between two limits, and never panics.
///
/// NaN in `x` becomes 0 first. The limits may cross (a modulated limit can
/// pass the other), so the smaller one is the floor. A NaN limit is open:
/// that side does not limit at all.
/// *Why not `f32::clamp`:* it panics when a bound is NaN or the bounds are
/// crossed, and a panic on the audio thread must never happen. And `min` and
/// `max` alone would not do, because with one NaN limit both return the
/// other limit, which would pin the output to it.
fn limit(x: f32, low: f32, high: f32) -> f32 {
    let x = if x.is_nan() { 0.0 } else { x };
    let (floor, ceiling) = match (low.is_nan(), high.is_nan()) {
        (false, false) => (low.min(high), low.max(high)),
        (true, false) => (f32::NEG_INFINITY, high),
        (false, true) => (low, f32::INFINITY),
        (true, true) => (f32::NEG_INFINITY, f32::INFINITY),
    };
    x.max(floor).min(ceiling)
}

#[derive(Clone)]
struct Clamp {
    low: Smoothed,
    high: Smoothed,
}

impl Clamp {
    const LOW: usize = CLAMP.param_index("low").unwrap();
    const HIGH: usize = CLAMP.param_index("high").unwrap();
}

impl Module for Clamp {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let (low_mod, high_mod) = (io.param(Self::LOW), io.param(Self::HIGH));
        let ([input], [out]) = io.split::<1, 1>();
        for (i, (out, &x)) in out.iter_mut().zip(input).enumerate() {
            let low = low_mod.at_or(i, self.low.tick());
            let high = high_mod.at_or(i, self.high.tick());
            *out = limit(x, low, high);
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            Self::LOW => self.low.set_target(value),
            Self::HIGH => self.high.set_target(value),
            _ => {}
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        match index {
            Self::LOW => self.low.gliding(),
            Self::HIGH => self.high.gliding(),
            _ => None,
        }
    }

    fn reset(&mut self) {
        for smoothed in [&mut self.low, &mut self.high] {
            let target = smoothed.target();
            smoothed.reset(target);
        }
    }
}

/// `loom.level.scale` (MT-LEVEL-03): `in * factor + offset`.
pub static SCALE: ModuleDescriptor = ModuleDescriptor {
    id: "loom.level.scale",
    version: 1,
    name: "Scale",
    category: "Level",
    family: loom_core::Family::LevelAndRouting,
    description: "Multiplies a signal by a factor and adds an offset. A factor of -1 inverts it; factor 0.5 with offset 0.5 turns bipolar into unipolar.",
    inputs: &[PortSpec {
        name: "in",
        label: "In",
        signal: SignalType::Modulation,
        description: "The signal to scale.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Modulation,
        description: "The input times the factor, plus the offset.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[
        ParamSpec {
            name: "factor",
            label: "Factor",
            description: "What the input is multiplied by. -1 flips it upside down.",
            min: -1.0,
            max: 1.0,
            default: 1.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .smooth(0.005),
        ParamSpec {
            name: "offset",
            label: "Offset",
            description: "A fixed amount added after the factor.",
            min: -10.0,
            max: 10.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .smooth(0.005),
    ],
    role: ModuleRole::Dsp(|config| {
        Box::new(Scale {
            factor: Smoothed::new(
                1.0,
                config.sample_rate,
                SCALE.params[Scale::FACTOR].options.smooth_seconds,
            ),
            offset: Smoothed::new(
                0.0,
                config.sample_rate,
                SCALE.params[Scale::OFFSET].options.smooth_seconds,
            ),
        })
    }),
    panel: Panel::new(6, 3, &[Widget::knob(0, 0, "factor"), Widget::knob(3, 0, "offset")]),
    displays: &[],
};

#[derive(Clone)]
struct Scale {
    factor: Smoothed,
    offset: Smoothed,
}

impl Scale {
    const FACTOR: usize = SCALE.param_index("factor").unwrap();
    const OFFSET: usize = SCALE.param_index("offset").unwrap();
}

impl Module for Scale {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let (factor_mod, offset_mod) = (io.param(Self::FACTOR), io.param(Self::OFFSET));
        let ([input], [out]) = io.split::<1, 1>();
        for (i, (out, &x)) in out.iter_mut().zip(input).enumerate() {
            let factor = factor_mod.at_or(i, self.factor.tick());
            let offset = offset_mod.at_or(i, self.offset.tick());
            *out = x * factor + offset;
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            Self::FACTOR => self.factor.set_target(value),
            Self::OFFSET => self.offset.set_target(value),
            _ => {}
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        match index {
            Self::FACTOR => self.factor.gliding(),
            Self::OFFSET => self.offset.gliding(),
            _ => None,
        }
    }

    fn reset(&mut self) {
        for smoothed in [&mut self.factor, &mut self.offset] {
            let target = smoothed.target();
            smoothed.reset(target);
        }
    }
}

/// `loom.level.slew` (MT-LEVEL-04): limits how fast a signal can change.
pub static SLEW: ModuleDescriptor = ModuleDescriptor {
    id: "loom.level.slew",
    version: 1,
    name: "Slew",
    category: "Level",
    family: loom_core::Family::LevelAndRouting,
    description: "Limits how fast a signal can rise and fall. Times are per 1.0 of change, so a glide takes the same time on any signal.",
    inputs: &[PortSpec {
        name: "in",
        label: "In",
        signal: SignalType::Modulation,
        description: "The signal to slow down.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Modulation,
        description: "The input, moving toward it at a limited speed.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[
        ParamSpec {
            name: "rise",
            label: "Rise",
            description: "The time to go up by 1.0.",
            min: 0.0005,
            max: 10.0,
            default: 0.05,
            unit: "s",
            options: ParamOptions::DEFAULT,
        }
        .logarithmic(),
        ParamSpec {
            name: "fall",
            label: "Fall",
            description: "The time to go down by 1.0.",
            min: 0.0005,
            max: 10.0,
            default: 0.05,
            unit: "s",
            options: ParamOptions::DEFAULT,
        }
        .logarithmic(),
    ],
    role: ModuleRole::Dsp(|config| {
        Box::new(Slew {
            sample_rate: config.sample_rate,
            rise: 0.05,
            fall: 0.05,
            out: 0.0,
            fresh: true,
        })
    }),
    panel: Panel::new(6, 3, &[Widget::knob(0, 0, "rise"), Widget::knob(3, 0, "fall")]),
    displays: &[],
};

/// State for `loom.level.slew`: the rise and fall times, and `out`, the value
/// the output has reached on its way to the input.
///
/// `out` is an `f64`. *Why:* a long rise makes a step of about 2e-6 per
/// sample, and an `f32` at 64 or more cannot add a step that small (its
/// spacing there is 7.6e-6), so a slewed Hz value would freeze. The output
/// is cast to `f32` only when it is written.
#[derive(Clone)]
struct Slew {
    sample_rate: f32,
    rise: f32,
    fall: f32,
    out: f64,
    /// True until the first frame, which sets the output with no ramp.
    fresh: bool,
}

impl Slew {
    const RISE: usize = SLEW.param_index("rise").unwrap();
    const FALL: usize = SLEW.param_index("fall").unwrap();
}

impl Module for Slew {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let (rise_mod, fall_mod) = (io.param(Self::RISE), io.param(Self::FALL));
        let ([input], [out]) = io.split::<1, 1>();
        for (i, (out, &x)) in out.iter_mut().zip(input).enumerate() {
            if x.is_finite() {
                if self.fresh {
                    // A new slew must not sweep up from 0.
                    self.out = f64::from(x);
                    self.fresh = false;
                } else {
                    let rise = rise_mod.at_or(i, self.rise).max(SLEW.params[Self::RISE].min);
                    let fall = fall_mod.at_or(i, self.fall).max(SLEW.params[Self::FALL].min);
                    // Constant rate: 1.0 of change takes `rise` or `fall` seconds.
                    let target = f64::from(x);
                    let rate = f64::from(self.sample_rate);
                    if target > self.out {
                        self.out = (self.out + 1.0 / (f64::from(rise) * rate)).min(target);
                    } else {
                        self.out = (self.out - 1.0 / (f64::from(fall) * rate)).max(target);
                    }
                }
            }
            *out = self.out as f32;
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            Self::RISE => self.rise = value,
            Self::FALL => self.fall = value,
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.out = 0.0;
        self.fresh = true;
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod shaping_tests;

#[cfg(test)]
mod slew_tests;
