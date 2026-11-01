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
            gain: Smoothed::new(1.0, config.sample_rate, AMP.params[0].options.smooth_seconds),
            exponential: false,
        })
    }),
    // A knob is 3 by 3 and a two-way radio is 3 by 3, so the tile is 3 by 6.
    panel: Panel::new(6, 6, &[Widget::knob(0, 0, "gain"), Widget::radio(0, 3, "response")]),
    displays: &[],
};

#[derive(Clone)]
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
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let level_connected = io.is_connected(1);
        let modulated = io.param(0);
        let ([input, level], [out]) = io.split::<2, 1>();
        for (i, ((out, &input), &level)) in out.iter_mut().zip(input).zip(level).enumerate() {
            let level = if level_connected { self.curve(level) } else { 1.0 };
            let gain = modulated.at_or(i, self.gain.tick());
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

/// `loom.level.constant` (MT-LEVEL-01): a fixed level, set in decibels.
pub static CONSTANT: ModuleDescriptor = ModuleDescriptor {
    id: "loom.level.constant",
    version: 1,
    name: "Constant (level)",
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
            db: Smoothed::new(0.0, config.sample_rate, CONSTANT.params[0].options.smooth_seconds),
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

impl Module for ConstantLevel {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let modulated = io.param(0);
        let ([], [out]) = io.split::<0, 1>();
        for (i, out) in out.iter_mut().enumerate() {
            let db = modulated.at_or(i, self.db.tick());
            *out = 10.0_f32.powf(db / 20.0);
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if index == 0 {
            self.db.set_target(value);
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        if index == 0 { self.db.gliding() } else { None }
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
            low: Smoothed::new(-1.0, config.sample_rate, CLAMP.params[0].options.smooth_seconds),
            high: Smoothed::new(1.0, config.sample_rate, CLAMP.params[1].options.smooth_seconds),
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

impl Module for Clamp {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let (low_mod, high_mod) = (io.param(0), io.param(1));
        let ([input], [out]) = io.split::<1, 1>();
        for (i, (out, &x)) in out.iter_mut().zip(input).enumerate() {
            let low = low_mod.at_or(i, self.low.tick());
            let high = high_mod.at_or(i, self.high.tick());
            *out = limit(x, low, high);
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            0 => self.low.set_target(value),
            1 => self.high.set_target(value),
            _ => {}
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        match index {
            0 => self.low.gliding(),
            1 => self.high.gliding(),
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
    description: "Multiplies a signal by a factor and adds an offset. A factor of -1 inverts. Bipolar to unipolar is factor 0.5 with offset 0.5.",
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
            factor: Smoothed::new(1.0, config.sample_rate, SCALE.params[0].options.smooth_seconds),
            offset: Smoothed::new(0.0, config.sample_rate, SCALE.params[1].options.smooth_seconds),
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

impl Module for Scale {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let (factor_mod, offset_mod) = (io.param(0), io.param(1));
        let ([input], [out]) = io.split::<1, 1>();
        for (i, (out, &x)) in out.iter_mut().zip(input).enumerate() {
            let factor = factor_mod.at_or(i, self.factor.tick());
            let offset = offset_mod.at_or(i, self.offset.tick());
            *out = x * factor + offset;
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            0 => self.factor.set_target(value),
            1 => self.offset.set_target(value),
            _ => {}
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        match index {
            0 => self.factor.gliding(),
            1 => self.offset.gliding(),
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

impl Module for Slew {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let (rise_mod, fall_mod) = (io.param(0), io.param(1));
        let ([input], [out]) = io.split::<1, 1>();
        for (i, (out, &x)) in out.iter_mut().zip(input).enumerate() {
            if x.is_finite() {
                if self.fresh {
                    // A new slew must not sweep up from 0.
                    self.out = f64::from(x);
                    self.fresh = false;
                } else {
                    let rise = rise_mod.at_or(i, self.rise).max(SLEW.params[0].min);
                    let fall = fall_mod.at_or(i, self.fall).max(SLEW.params[1].min);
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
            0 => self.rise = value,
            1 => self.fall = value,
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.out = 0.0;
        self.fresh = true;
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

#[cfg(test)]
mod shaping_tests {
    use super::*;
    use loom_core::testing::run;

    /// Runs a one-input module settled at `params` over `input`.
    fn process(descriptor: &ModuleDescriptor, params: &[(usize, f32)], input: &[f32]) -> Vec<f32> {
        let ModuleRole::Dsp(create) = descriptor.role else {
            unreachable!()
        };
        let mut m = create(&loom_core::ModuleConfig {
            sample_rate: 48_000.0,
            max_block: input.len(),
        });
        for &(index, value) in params {
            m.set_param(index, value);
        }
        m.reset();
        let mut out = vec![0.0; input.len()];
        run(m.as_mut(), 48_000.0, &[input], &[true], &mut [&mut out], &[]);
        out
    }

    /// Constant (level) outputs 1.0 at 0 dB and 0.5012 at -6 dB.
    #[test]
    fn constant_level_in_decibels() {
        for (db, expected) in [(0.0, 1.0), (-6.0, 0.5012), (12.0, 3.981), (-60.0, 0.001)] {
            let out = process(&CONSTANT, &[(0, db)], &[0.0; 4]);
            assert!(
                out.iter().all(|v| (v - expected).abs() < expected * 1e-3),
                "{db} dB: {out:?}"
            );
        }
    }

    /// A NaN limit is ignored, crossed limits swap, and nothing panics.
    #[test]
    fn limit_never_panics_on_nan_or_crossed_limits() {
        assert_eq!(limit(5.0, f32::NAN, 1.0), 1.0);
        assert_eq!(limit(-5.0, -1.0, f32::NAN), -1.0);
        // A NaN limit is open: it does not pin the output to the other one.
        assert_eq!(limit(0.25, f32::NAN, 1.0), 0.25);
        assert_eq!(limit(0.25, -1.0, f32::NAN), 0.25);
        assert_eq!(limit(5.0, f32::NAN, f32::NAN), 5.0);
        assert_eq!(limit(f32::NAN, f32::NAN, 1.0), 0.0);
        assert_eq!(limit(5.0, 1.0, -1.0), 1.0);
        assert_eq!(limit(f32::INFINITY, -1.0, 1.0), 1.0);
    }

    /// Clamp limits the signal, swaps crossed limits, and turns NaN into a
    /// value inside the limits.
    #[test]
    fn clamp_limits_swaps_and_cleans_nan() {
        let input = [-5.0, -0.5, 0.5, 5.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY];
        assert_eq!(
            process(&CLAMP, &[(0, -1.0), (1, 1.0)], &input),
            [-1.0, -0.5, 0.5, 1.0, 0.0, 1.0, -1.0]
        );
        // Crossed limits give the same result as the right way round.
        assert_eq!(
            process(&CLAMP, &[(0, 1.0), (1, -1.0)], &input),
            [-1.0, -0.5, 0.5, 1.0, 0.0, 1.0, -1.0]
        );
        // NaN lands inside limits that do not contain 0.
        assert_eq!(process(&CLAMP, &[(0, 2.0), (1, 3.0)], &[f32::NAN]), [2.0]);
    }

    /// Scale inverts at factor -1 and maps bipolar to unipolar.
    #[test]
    fn scale_inverts_and_offsets() {
        let input = [-1.0, 0.0, 0.5, 1.0];
        assert_eq!(process(&SCALE, &[(0, -1.0)], &input), [1.0, 0.0, -0.5, -1.0]);
        assert_eq!(process(&SCALE, &[(0, 0.5), (1, 0.5)], &input), [0.0, 0.5, 0.75, 1.0]);
    }
}

#[cfg(test)]
mod slew_tests {
    use super::*;
    use loom_core::testing::run;

    /// Runs the slew over `input` with the given rise and fall times.
    fn slew(rise: f32, fall: f32, input: &[f32]) -> Vec<f32> {
        let ModuleRole::Dsp(create) = SLEW.role else {
            unreachable!()
        };
        let mut m = create(&loom_core::ModuleConfig {
            sample_rate: 48_000.0,
            max_block: input.len(),
        });
        m.set_param(0, rise);
        m.set_param(1, fall);
        m.reset();
        let mut out = vec![0.0; input.len()];
        run(m.as_mut(), 48_000.0, &[input], &[true], &mut [&mut out], &[]);
        out
    }

    /// Going up by 1.0 takes `rise` seconds, down takes `fall`, and the
    /// output stops on arrival.
    #[test]
    fn rises_and_falls_at_a_constant_rate() {
        // 10 ms at 48 kHz is 480 samples per 1.0 of change.
        let mut input = vec![0.0];
        input.extend(vec![1.0; 600]);
        let out = slew(0.01, 0.02, &input);
        assert_eq!(out[0], 0.0);
        assert!((out[240] - 0.5).abs() < 1e-3, "{}", out[240]);
        assert!((out[480] - 1.0).abs() < 1e-3, "{}", out[480]);
        assert_eq!(out[600], 1.0, "stops on arrival");
        // Fall by 1.0 in 20 ms, so 0.5 at 480 samples.
        let mut input = vec![1.0];
        input.extend(vec![0.0; 1_000]);
        let out = slew(0.01, 0.02, &input);
        assert_eq!(out[0], 1.0);
        assert!((out[480] - 0.5).abs() < 1e-3, "{}", out[480]);
    }

    /// A long slew of a large value keeps moving: at 100, a step of 2e-6 per
    /// sample is lost in an `f32`, but not in the `f64` the slew keeps.
    #[test]
    fn a_long_slew_of_a_large_value_does_not_freeze() {
        // 10 s per 1.0 is a step of 1 / 480,000 per sample.
        let mut input = vec![100.0];
        input.extend(vec![200.0; 3_000]);
        let out = slew(10.0, 10.0, &input);
        let moved = f64::from(out[3_000]) - 100.0;
        let expected = 3_000.0 / 480_000.0;
        assert!((moved - expected).abs() < expected * 0.1, "{moved} vs {expected}");
    }

    /// A new slew starts at its input instead of sweeping from 0.
    #[test]
    fn starts_at_its_input() {
        let out = slew(1.0, 1.0, &[0.7; 4]);
        assert_eq!(out, [0.7; 4]);
    }
}
