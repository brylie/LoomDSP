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

/// `loom.level.constant` (MT-LEVEL-01): a fixed level, set in decibels.
pub static CONSTANT: ModuleDescriptor = ModuleDescriptor {
    id: "loom.level.constant",
    version: 1,
    name: "Constant (level)",
    category: "Level",
    description: "Outputs a fixed level, set in decibels. 0 dB is a factor of 1.",
    inputs: &[],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Modulation,
        description: "The level as a factor: 10 to the power of dB over 20.",
        default: PortDefault::Value(1.0),
        polarity: Polarity::Unipolar,
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

struct ConstantLevel {
    /// The level in dB. Smoothed in dB, so a turn sounds even.
    db: Smoothed,
}

impl Module for ConstantLevel {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let modulated = io.modulated(0);
        let ([], [out]) = io.split::<0, 1>();
        for (i, out) in out.iter_mut().enumerate() {
            let db = if let Some(samples) = modulated {
                let _ = self.db.tick();
                samples[i]
            } else {
                self.db.tick()
            };
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
    description: "Limits a signal to the range between two limits. NaN becomes 0 first, so Clamp makes a signal safe.",
    inputs: &[PortSpec {
        name: "in",
        label: "In",
        signal: SignalType::Modulation,
        description: "The signal to limit.",
        default: PortDefault::Value(0.0),
        polarity: Polarity::Bipolar,
        channels: Channels::PerChannel,
    }],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Modulation,
        description: "The input, held between the two limits.",
        default: PortDefault::Value(0.0),
        polarity: Polarity::Bipolar,
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

struct Clamp {
    low: Smoothed,
    high: Smoothed,
}

impl Module for Clamp {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let (low_mod, high_mod) = (io.modulated(0), io.modulated(1));
        let ([input], [out]) = io.split::<1, 1>();
        for (i, (out, &x)) in out.iter_mut().zip(input).enumerate() {
            let low = match low_mod {
                Some(samples) => {
                    let _ = self.low.tick();
                    samples[i]
                }
                None => self.low.tick(),
            };
            let high = match high_mod {
                Some(samples) => {
                    let _ = self.high.tick();
                    samples[i]
                }
                None => self.high.tick(),
            };
            // A modulated limit can cross the other one. Swapping keeps the
            // output between them instead of stuck on one.
            let (low, high) = if low <= high { (low, high) } else { (high, low) };
            let x = if x.is_nan() { 0.0 } else { x };
            *out = x.clamp(low, high);
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
    description: "Multiplies a signal by a factor and adds an offset. A factor of -1 inverts. Bipolar to unipolar is factor 0.5 with offset 0.5.",
    inputs: &[PortSpec {
        name: "in",
        label: "In",
        signal: SignalType::Modulation,
        description: "The signal to scale.",
        default: PortDefault::Value(0.0),
        polarity: Polarity::Bipolar,
        channels: Channels::PerChannel,
    }],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Modulation,
        description: "The input times the factor, plus the offset.",
        default: PortDefault::Value(0.0),
        polarity: Polarity::Bipolar,
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

struct Scale {
    factor: Smoothed,
    offset: Smoothed,
}

impl Module for Scale {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let (factor_mod, offset_mod) = (io.modulated(0), io.modulated(1));
        let ([input], [out]) = io.split::<1, 1>();
        for (i, (out, &x)) in out.iter_mut().zip(input).enumerate() {
            let factor = match factor_mod {
                Some(samples) => {
                    let _ = self.factor.tick();
                    samples[i]
                }
                None => self.factor.tick(),
            };
            let offset = match offset_mod {
                Some(samples) => {
                    let _ = self.offset.tick();
                    samples[i]
                }
                None => self.offset.tick(),
            };
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
    description: "Limits how fast a signal can rise and fall. Times are per 1.0 of change, so a glide takes the same time on any signal.",
    inputs: &[PortSpec {
        name: "in",
        label: "In",
        signal: SignalType::Modulation,
        description: "The signal to slow down.",
        default: PortDefault::Value(0.0),
        polarity: Polarity::Bipolar,
        channels: Channels::PerChannel,
    }],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Modulation,
        description: "The input, moving toward it at a limited speed.",
        default: PortDefault::Value(0.0),
        polarity: Polarity::Bipolar,
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

struct Slew {
    sample_rate: f32,
    rise: f32,
    fall: f32,
    out: f32,
    /// True until the first frame, which sets the output with no ramp.
    fresh: bool,
}

impl Module for Slew {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let (rise_mod, fall_mod) = (io.modulated(0), io.modulated(1));
        let ([input], [out]) = io.split::<1, 1>();
        for (i, (out, &x)) in out.iter_mut().zip(input).enumerate() {
            if x.is_finite() {
                if self.fresh {
                    // A new slew must not sweep up from 0.
                    self.out = x;
                    self.fresh = false;
                } else {
                    let rise = rise_mod.map_or(self.rise, |s| s[i]).max(SLEW.params[0].min);
                    let fall = fall_mod.map_or(self.fall, |s| s[i]).max(SLEW.params[1].min);
                    // Constant rate: 1.0 of change takes `rise` or `fall` seconds.
                    if x > self.out {
                        self.out = (self.out + 1.0 / (rise * self.sample_rate)).min(x);
                    } else {
                        self.out = (self.out - 1.0 / (fall * self.sample_rate)).max(x);
                    }
                }
            }
            *out = self.out;
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

    /// A new slew starts at its input instead of sweeping from 0.
    #[test]
    fn starts_at_its_input() {
        let out = slew(1.0, 1.0, &[0.7; 4]);
        assert_eq!(out, [0.7; 4]);
    }
}
