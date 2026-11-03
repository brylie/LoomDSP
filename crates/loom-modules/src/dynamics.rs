//! Dynamics (MT-DYN-01): compress, limit, expand, gate, or duck, with a
//! sidechain. Spec: `docs/design/dynamics-eq-crossover.md`. Explainer:
//! `docs/learn/dynamics-detection.md`. The algorithms are in
//! [`loom_dsp::dynamics`] and [`loom_dsp::loudness`].

use crate::cached::Cached;
use crate::mix::stereo_port;
use loom_core::{
    Channels, DisplayKind, DisplayRange, DisplaySpec, Module, ModuleConfig, ModuleDescriptor, ModuleRole, Panel,
    ParamOptions, ParamSpec, PortDefault, PortIo, PortSpec, ProcessContext, SignalType, Widget,
};
use loom_dsp::dynamics::{Coefficients, Curve, Detection, Dynamics as Algorithm, Mode, Settings};
use loom_dsp::loudness::LevelMatch;
use loom_rt::Smoothed;

/// The modes that use the ratio and the knee: compress and expand.
const SLOPED: &[u32] = &[0, 2];
/// The modes with an attack and a choice of detection: all but limit.
const NOT_LIMIT: &[u32] = &[0, 2, 3, 4];
/// The modes that use the range: expand, gate, and duck.
const RANGED: &[u32] = &[2, 3, 4];
/// The modes that hold: Gate and Duck.
const HELD: &[u32] = &[3, 4];

/// `loom.dyn.dynamics`: one gain on both sides, from a level and a curve.
pub static DYNAMICS: ModuleDescriptor = ModuleDescriptor {
    id: "loom.dyn.dynamics",
    version: 1,
    name: "Dynamics",
    category: "Dynamics",
    family: loom_core::Family::FiltersAndEffects,
    description: "Compresses, limits, expands, gates, or ducks a signal, with a sidechain that can come from another signal.",
    inputs: &[
        stereo_port("in", "In", "The signal to process.", Channels::Stereo),
        stereo_port(
            "sidechain",
            "Side",
            "When connected, sets the level instead of the input.",
            Channels::Downmix,
        ),
    ],
    outputs: &[
        stereo_port("out", "Out", "The processed signal.", Channels::Stereo),
        PortSpec {
            name: "gain",
            label: "Gain",
            signal: SignalType::Modulation,
            description: "The gain from the threshold and ratio, from 0 to 1, to drive anything else. It leaves out match, output, and mix.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Span(0.0, 1.0),
            channels: Channels::PerChannel,
        },
    ],
    params: &[
        ParamSpec {
            name: "mode",
            label: "Mode",
            description: "What it does about the level.",
            min: 0.0,
            max: 4.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer()
        .choices(&["Compress", "Limit", "Expand", "Gate", "Duck"]),
        ParamSpec {
            name: "threshold",
            label: "Threshold",
            description: "The level where it starts to act.",
            min: -60.0,
            max: 0.0,
            default: -18.0,
            unit: "dB",
            options: ParamOptions::DEFAULT,
        }
        .smooth(0.005),
        ParamSpec {
            name: "ratio",
            label: "Ratio",
            description: "How strongly it acts past the threshold.",
            min: 1.0,
            max: 20.0,
            default: 4.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .logarithmic()
        .shown_when("mode", SLOPED),
        ParamSpec {
            name: "knee",
            label: "Knee",
            description: "How gently it starts, as a width around the threshold.",
            min: 0.0,
            max: 24.0,
            default: 6.0,
            unit: "dB",
            options: ParamOptions::DEFAULT,
        }
        .shown_when("mode", SLOPED),
        ParamSpec {
            name: "range",
            label: "Range",
            description: "The most the gain can fall.",
            min: 0.0,
            max: 96.0,
            default: 40.0,
            unit: "dB",
            options: ParamOptions::DEFAULT,
        }
        .shown_when("mode", RANGED),
        ParamSpec {
            name: "attack",
            label: "Attack",
            description: "How fast the gain moves when the signal rises.",
            min: 0.000_1,
            max: 0.2,
            default: 0.01,
            unit: "s",
            options: ParamOptions::DEFAULT,
        }
        .logarithmic()
        .shown_when("mode", NOT_LIMIT),
        ParamSpec {
            name: "hold",
            label: "Hold",
            description: "How long it stays as it is after the signal falls, before the release begins.",
            min: 0.001,
            max: 2.0,
            default: 0.02,
            unit: "s",
            options: ParamOptions::DEFAULT,
        }
        .logarithmic()
        .shown_when("mode", HELD),
        ParamSpec {
            name: "release",
            label: "Release",
            description: "How fast the gain moves when the signal falls.",
            min: 0.01,
            max: 2.0,
            default: 0.15,
            unit: "s",
            options: ParamOptions::DEFAULT,
        }
        .logarithmic(),
        ParamSpec {
            name: "detect",
            label: "Detect",
            description: "How it reads the level: each peak, or the average power.",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer()
        .choices(&["Peak", "RMS"])
        .shown_when("mode", NOT_LIMIT),
        ParamSpec {
            name: "match",
            label: "Match",
            description: "Turns the result to the loudness of the input, so you compare the sound and not the level.",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer()
        .choices(&["Off", "On"]),
        ParamSpec {
            name: "output",
            label: "Output",
            description: "The level after everything else.",
            min: -24.0,
            max: 24.0,
            default: 0.0,
            unit: "dB",
            options: ParamOptions::DEFAULT,
        }
        .smooth(0.005),
        ParamSpec {
            name: "mix",
            label: "Mix",
            description: "0 is the input alone, and 1 is the processed signal alone.",
            min: 0.0,
            max: 1.0,
            default: 1.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .smooth(0.005),
    ],
    role: ModuleRole::Dsp(|config| Box::new(Dynamics::new(config))),
    panel: Panel::new(
        15,
        14,
        &[
            Widget::shape(0, 0, 12, "out", "Curve", |params, x, _| curve_preview(params, x)),
            Widget::meter(12, 0, "reduction"),
            Widget::select(0, 6, "mode"),
            Widget::segmented(6, 6, 6, "detect"),
            Widget::toggle(12, 6, "match"),
            Widget::knob(0, 8, "threshold").captioned("Thresh"),
            Widget::knob(3, 8, "ratio"),
            Widget::knob(6, 8, "knee"),
            Widget::knob(9, 8, "range"),
            Widget::knob(0, 11, "attack"),
            Widget::knob(3, 11, "hold"),
            Widget::knob(6, 11, "release"),
            Widget::knob(9, 11, "output"),
            Widget::knob(12, 11, "mix"),
        ],
    ),
    displays: &[DisplaySpec {
        name: "reduction",
        label: "Reduction",
        description: "How far the gain has fallen.",
        kind: DisplayKind::Level,
        range: DisplayRange {
            format: loom_core::DisplayFormat::Number { decimals: 1 },
            min: -36.0,
            max: 0.0,
            unit: "dB",
        },
    }],
};

/// The mode for a parameter value.
#[inline]
fn mode_of(value: f32) -> Mode {
    match value as u32 {
        0 => Mode::Compress,
        1 => Mode::Limit,
        2 => Mode::Expand,
        3 => Mode::Gate,
        _ => Mode::Duck,
    }
}

/// The curve on the panel: input level across and output level up, both from
/// -60 to 0 dB. It calls the same function as the audio.
fn curve_preview(params: &[f32], x: f32) -> f32 {
    let curve = Curve {
        mode: mode_of(params[Dynamics::MODE]),
        threshold_db: params[Dynamics::THRESHOLD],
        ratio: params[Dynamics::RATIO],
        knee_db: params[Dynamics::KNEE],
        range_db: params[Dynamics::RANGE],
    };
    let level_db = -60.0 + 60.0 * x.clamp(0.0, 1.0);
    ((level_db + curve.gain_db(level_db) + 60.0) / 60.0).clamp(0.0, 1.0)
}

/// A decibel value as a factor.
#[inline]
fn factor(db: f32) -> f32 {
    (db * std::f32::consts::LN_10 / 20.0).exp()
}

/// State for `loom.dyn.dynamics`.
#[derive(Clone)]
struct Dynamics {
    sample_rate: f32,
    algorithm: Algorithm,
    level_match: LevelMatch,
    coefficients: Cached<Settings, Coefficients>,
    threshold: Smoothed,
    output: Smoothed,
    mix: Smoothed,
    /// 0 is off and 1 is on, so `match` does not click.
    match_amount: Smoothed,
    mode: f32,
    ratio: f32,
    knee: f32,
    range: f32,
    attack: f32,
    hold: f32,
    release: f32,
    detect: f32,
    match_on: bool,
    /// Whether the matcher has run since its last reset.
    match_used: bool,
    /// The lowest gain in dB in the last call.
    reduction: f32,
}

impl Dynamics {
    const IN: usize = DYNAMICS.input_index("in").unwrap();
    const SIDECHAIN_IN: usize = DYNAMICS.input_index("sidechain").unwrap();
    const OUT: usize = DYNAMICS.output_index("out").unwrap();
    const GAIN_OUT: usize = DYNAMICS.output_index("gain").unwrap();
    const MODE: usize = DYNAMICS.param_index("mode").unwrap();
    const THRESHOLD: usize = DYNAMICS.param_index("threshold").unwrap();
    const RATIO: usize = DYNAMICS.param_index("ratio").unwrap();
    const KNEE: usize = DYNAMICS.param_index("knee").unwrap();
    const RANGE: usize = DYNAMICS.param_index("range").unwrap();
    const ATTACK: usize = DYNAMICS.param_index("attack").unwrap();
    const HOLD: usize = DYNAMICS.param_index("hold").unwrap();
    const RELEASE: usize = DYNAMICS.param_index("release").unwrap();
    const DETECT: usize = DYNAMICS.param_index("detect").unwrap();
    const MATCH: usize = DYNAMICS.param_index("match").unwrap();
    const OUTPUT: usize = DYNAMICS.param_index("output").unwrap();
    const MIX: usize = DYNAMICS.param_index("mix").unwrap();

    fn new(config: &ModuleConfig) -> Self {
        let params = DYNAMICS.params;
        let rate = config.sample_rate;
        let smoothed = |index: usize| Smoothed::new(params[index].default, rate, params[index].options.smooth_seconds);
        let settings = Settings {
            curve: Curve {
                mode: mode_of(params[Self::MODE].default),
                threshold_db: params[Self::THRESHOLD].default,
                ratio: params[Self::RATIO].default,
                knee_db: params[Self::KNEE].default,
                range_db: params[Self::RANGE].default,
            },
            detection: Detection::Peak,
            attack_seconds: params[Self::ATTACK].default,
            hold_seconds: params[Self::HOLD].default,
            release_seconds: params[Self::RELEASE].default,
            sample_rate: rate,
        };
        Self {
            sample_rate: rate,
            algorithm: Algorithm::new(rate),
            level_match: LevelMatch::new(rate),
            coefficients: Cached::new(settings, Coefficients::new),
            threshold: smoothed(Self::THRESHOLD),
            output: smoothed(Self::OUTPUT),
            mix: smoothed(Self::MIX),
            match_amount: Smoothed::new(0.0, rate, 0.005),
            mode: params[Self::MODE].default,
            ratio: params[Self::RATIO].default,
            knee: params[Self::KNEE].default,
            range: params[Self::RANGE].default,
            attack: params[Self::ATTACK].default,
            hold: params[Self::HOLD].default,
            release: params[Self::RELEASE].default,
            detect: params[Self::DETECT].default,
            match_on: false,
            match_used: false,
            reduction: 0.0,
        }
    }
}

impl Module for Dynamics {
    /// Takes the running state of another instance (ADR-0022). The engine does
    /// not copy a module with `Stereo` ports, but a replacement can still take over.
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let (threshold, ratio, knee, range) = (
            io.param(Self::THRESHOLD),
            io.param(Self::RATIO),
            io.param(Self::KNEE),
            io.param(Self::RANGE),
        );
        let (mode, attack, hold, release, detect) = (
            io.param(Self::MODE),
            io.param(Self::ATTACK),
            io.param(Self::HOLD),
            io.param(Self::RELEASE),
            io.param(Self::DETECT),
        );
        let (level_match, output, mix) = (io.param(Self::MATCH), io.param(Self::OUTPUT), io.param(Self::MIX));
        let sidechained = io.is_connected(Self::SIDECHAIN_IN);
        let [left_in, right_in] = io.stereo_input(Self::IN);
        let [sidechain, _] = io.stereo_input(Self::SIDECHAIN_IN);
        let frames = left_in.len();
        let mut reduction = 0.0f32;
        for i in 0..frames {
            let settings = Settings {
                curve: Curve {
                    mode: mode_of(mode.at_or(i, self.mode)),
                    threshold_db: threshold.at_or(i, self.threshold.tick()),
                    ratio: ratio.at_or(i, self.ratio),
                    knee_db: knee.at_or(i, self.knee),
                    range_db: range.at_or(i, self.range),
                },
                detection: if detect.at_or(i, self.detect) >= 0.5 {
                    Detection::Rms
                } else {
                    Detection::Peak
                },
                attack_seconds: attack.at_or(i, self.attack),
                hold_seconds: hold.at_or(i, self.hold),
                release_seconds: release.at_or(i, self.release),
                sample_rate: self.sample_rate,
            };
            let coefficients = self.coefficients.get(settings, Coefficients::new);
            // A bad sample cannot reach the state.
            let (left, right) = (loom_dsp::guard(left_in[i]), loom_dsp::guard(right_in[i]));
            let key = if sidechained {
                loom_dsp::guard(sidechain[i]).abs()
            } else {
                left.abs().max(right.abs())
            };
            let gain = self.algorithm.tick(key, coefficients);
            reduction = reduction.min(self.algorithm.gain_db());
            let (wet_left, wet_right) = (left * gain, right * gain);

            let matching = level_match.at_or(i, f32::from(u8::from(self.match_on))) >= 0.5;
            self.match_amount.set_target(if matching { 1.0 } else { 0.0 });
            let amount = self.match_amount.tick();
            // A matcher that was off has nothing to say yet.
            let matched = if amount > 0.0 {
                self.match_used = true;
                1.0 + amount * (self.level_match.tick([left, right], [wet_left, wet_right]) - 1.0)
            } else {
                if self.match_used {
                    self.level_match.reset();
                    self.match_used = false;
                }
                1.0
            };

            let blend = mix.at_or(i, self.mix.tick());
            let out_gain = factor(output.at_or(i, self.output.tick()));
            let blended = |dry: f32, wet: f32| (dry + blend * (wet * matched - dry)) * out_gain;
            let (out_left, out_right) = (blended(left, wet_left), blended(right, wet_right));
            io.output(Self::GAIN_OUT)[i] = gain;
            let [left_out, right_out] = io.stereo_output(Self::OUT);
            left_out[i] = out_left;
            right_out[i] = out_right;
        }
        self.reduction = reduction;
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            Self::MODE => self.mode = value,
            Self::THRESHOLD => self.threshold.set_target(value),
            Self::RATIO => self.ratio = value,
            Self::KNEE => self.knee = value,
            Self::RANGE => self.range = value,
            Self::ATTACK => self.attack = value,
            Self::HOLD => self.hold = value,
            Self::RELEASE => self.release = value,
            Self::DETECT => self.detect = value,
            Self::MATCH => self.match_on = value >= 0.5,
            Self::OUTPUT => self.output.set_target(value),
            Self::MIX => self.mix.set_target(value),
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.algorithm.reset();
        self.level_match.reset();
        self.match_used = false;
        self.threshold.reset(self.threshold.target());
        self.output.reset(self.output.target());
        self.mix.reset(self.mix.target());
        self.match_amount.reset(if self.match_on { 1.0 } else { 0.0 });
        self.reduction = 0.0;
    }

    fn displays(&self, values: &mut [f32]) {
        values[0] = self.reduction;
    }
}
