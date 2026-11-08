//! Filters (FR-LIB-03).

use crate::cached::Cached;
use crate::mix::stereo_port;
use loom_core::signal::pitch;
use loom_core::{
    Channels, Module, ModuleConfig, ModuleDescriptor, ModuleRole, Panel, ParamOptions, ParamSpec, PortDefault, PortIo,
    PortSpec, ProcessContext, SignalType, Widget,
};
use loom_core::{FillsRole, roles};
use loom_dsp::filter::{crossover, ladder, one_pole, svf};
use loom_rt::Smoothed;

/// `loom.filter.svf`: state variable filter with lowpass, bandpass,
/// highpass, and notch outputs.
pub static SVF: ModuleDescriptor = ModuleDescriptor {
    id: "loom.filter.svf",
    version: 1,
    name: "SVF",
    category: "Filter",
    family: loom_core::Family::FiltersAndEffects,
    description: "A resonant filter with low-pass, band-pass, high-pass, and notch outputs. The cutoff input adds pitch: +0.1 doubles the cutoff.",
    inputs: &[
        PortSpec {
            name: "in",
            label: "In",
            signal: SignalType::Audio,
            description: "The audio to filter.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "cutoff",
            label: "Cutoff",
            signal: SignalType::Pitch,
            description: "Moves the cutoff as pitch: +0.1 doubles it.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
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
            range: loom_core::PortRange::Signal,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "bandpass",
            label: "Bandpass",
            signal: SignalType::Audio,
            description: "Keeps the frequencies near the cutoff.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "highpass",
            label: "Highpass",
            signal: SignalType::Audio,
            description: "Keeps the frequencies above the cutoff.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "notch",
            label: "Notch",
            signal: SignalType::Audio,
            description: "Cuts a narrow band around the cutoff and passes the rest.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
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
    // Eight units wide with the knobs centred: the gutters give the
    // Resonance caption and the Bandpass and Highpass labels room (#455).
    panel: Panel::new(8, 3, &[Widget::knob(1, 0, "cutoff"), Widget::knob(4, 0, "resonance")]),
    displays: &[],
    roles: &[FillsRole {
        outputs: &[("out", "lowpass")],
        ..FillsRole::of(&roles::FILTER)
    }],
};

#[derive(Clone)]
struct Svf {
    sample_rate: f32,
    cutoff: Smoothed,
    resonance: Smoothed,
    filter: svf::Svf,
}

impl Svf {
    const CUTOFF: usize = SVF.param_index("cutoff").unwrap();
    const RESONANCE: usize = SVF.param_index("resonance").unwrap();
    fn new(config: &ModuleConfig) -> Self {
        Self {
            sample_rate: config.sample_rate,
            // Smooth cutoff in the pitch domain, so sweeps sound even.
            // The time matches `ParamSpec::smooth_seconds`, which the engine
            // uses once an assignment is modulating this knob.
            cutoff: Smoothed::new(
                pitch::from_hz(1_000.0),
                config.sample_rate,
                SVF.params[Self::CUTOFF].options.smooth_seconds,
            ),
            resonance: Smoothed::new(
                0.2,
                config.sample_rate,
                SVF.params[Self::RESONANCE].options.smooth_seconds,
            ),
            filter: svf::Svf::new(),
        }
    }
}

impl Module for Svf {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let cutoff_param = io.param(Self::CUTOFF);
        let resonance_param = io.param(Self::RESONANCE);
        let ([input, cutoff_mod], [lp, bp, hp, notch]) = io.split::<2, 4>();
        let nyquist_limit = self.sample_rate * 0.49;
        for i in 0..input.len() {
            // Tick even when modulated, so removing the assignment continues
            // the glide from here. The heard value is the engine buffer.
            let cutoff_pitch = self.cutoff.tick();
            let cutoff_pitch = cutoff_param.map_or(i, cutoff_pitch, pitch::from_hz);
            let cutoff_hz = pitch::to_hz(cutoff_pitch + cutoff_mod[i]);
            // `clamp` passes NaN through, and it would reach the state and
            // stay there (#127). Use a safe cutoff for that frame instead.
            // Infinity is clamped as usual.
            let cutoff_hz = if cutoff_hz.is_nan() { 1_000.0 } else { cutoff_hz }.clamp(5.0, nyquist_limit);
            let resonance = resonance_param.at_or(i, self.resonance.tick());
            // Resonance 0..1 maps to damping k = 2..0.02 (Q from 0.5 to 50).
            let coefficients = svf::Coefficients::new(cutoff_hz, 2.0 - 1.98 * resonance, self.sample_rate);

            let x = input[i];
            let x = loom_dsp::guard(x);
            let outputs = self.filter.tick(x, coefficients);
            lp[i] = outputs.low;
            bp[i] = outputs.band;
            hp[i] = outputs.high;
            notch[i] = outputs.notch;
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            Self::CUTOFF => self.cutoff.set_target(pitch::from_hz(value)),
            Self::RESONANCE => self.resonance.set_target(value),
            _ => {}
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        match index {
            // The cutoff glides in pitch. The engine glides in the knob's
            // position, which is also logarithmic, so the paths match.
            Self::CUTOFF => self.cutoff.gliding().map(pitch::to_hz),
            Self::RESONANCE => self.resonance.gliding(),
            _ => None,
        }
    }

    /// Clears the filter memory and snaps the smoothing to its targets
    /// (live patching spec, rule 7).
    fn reset(&mut self) {
        self.filter.reset();
        let (cutoff, resonance) = (self.cutoff.target(), self.resonance.target());
        self.cutoff.reset(cutoff);
        self.resonance.reset(resonance);
    }
}

/// `loom.filter.onepole`: a first-order filter with lowpass and highpass
/// outputs, for smoothing and gentle tone control (MT-FILTER-03).
pub static ONEPOLE: ModuleDescriptor = ModuleDescriptor {
    id: "loom.filter.onepole",
    version: 1,
    name: "One-Pole",
    category: "Filter",
    family: loom_core::Family::FiltersAndEffects,
    description: "A gentle 6 dB per octave filter with low-pass and high-pass outputs; low cutoffs smooth control signals. The cutoff input adds pitch: +0.1 doubles the cutoff.",
    inputs: &[
        PortSpec {
            name: "in",
            label: "In",
            signal: SignalType::Audio,
            description: "The signal to filter.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "cutoff",
            label: "Cutoff",
            signal: SignalType::Pitch,
            description: "Moves the cutoff as pitch: +0.1 doubles it.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
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
            range: loom_core::PortRange::Signal,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "highpass",
            label: "Highpass",
            signal: SignalType::Audio,
            description: "Keeps the frequencies above the cutoff.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
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
    roles: &[FillsRole {
        outputs: &[("out", "lowpass")],
        ..FillsRole::of(&roles::FILTER)
    }],
};

/// State for `loom.filter.onepole`: the smoothed cutoff and the filter.
#[derive(Clone)]
struct OnePole {
    sample_rate: f32,
    /// The cutoff in pitch units, smoothed over 10 ms.
    cutoff: Smoothed,
    filter: one_pole::OnePole,
}

impl OnePole {
    const CUTOFF: usize = ONEPOLE.param_index("cutoff").unwrap();
    /// A one-pole at 1 kHz with a cleared state.
    fn new(config: &ModuleConfig) -> Self {
        Self {
            sample_rate: config.sample_rate,
            // Smoothed in the pitch domain, like the SVF.
            cutoff: Smoothed::new(
                pitch::from_hz(1_000.0),
                config.sample_rate,
                ONEPOLE.params[Self::CUTOFF].options.smooth_seconds,
            ),
            filter: one_pole::OnePole::new(),
        }
    }
}

impl Module for OnePole {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let cutoff_param = io.param(Self::CUTOFF);
        let ([input, cutoff_mod], [lp, hp]) = io.split::<2, 2>();
        let nyquist_limit = self.sample_rate * 0.49;
        for i in 0..input.len() {
            let cutoff_pitch = self.cutoff.tick();
            let cutoff_pitch = cutoff_param.map_or(i, cutoff_pitch, pitch::from_hz);
            let cutoff_hz = pitch::to_hz(cutoff_pitch + cutoff_mod[i]);
            // Cables are never clipped, so a NaN can arrive on the cutoff
            // (after a divide upstream, say). `clamp` passes NaN through,
            // and it would reach the state and stay there. Use a safe
            // cutoff for that frame instead. Infinity is clamped as usual.
            let cutoff_hz = if cutoff_hz.is_nan() { 1_000.0 } else { cutoff_hz }.clamp(0.5, nyquist_limit);
            let coefficients = one_pole::Coefficients::new(cutoff_hz, self.sample_rate);
            let x = input[i];
            let x = loom_dsp::guard(x);
            let outputs = self.filter.tick(x, coefficients);
            lp[i] = outputs.low;
            hp[i] = outputs.high;
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if index == Self::CUTOFF {
            self.cutoff.set_target(pitch::from_hz(value));
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        if index == Self::CUTOFF {
            self.cutoff.gliding().map(pitch::to_hz)
        } else {
            None
        }
    }

    /// Clears the integrator and snaps the cutoff to its target (live
    /// patching spec, rule 7).
    fn reset(&mut self) {
        self.filter.reset();
        let cutoff = self.cutoff.target();
        self.cutoff.reset(cutoff);
    }
}

/// `loom.filter.ladder`: a resonant low-pass made of two, three, or four
/// one-pole stages in a loop, with saturation in the feedback (MT-FILTER-02).
///
/// The algorithm and its sources are in [`ladder`].
///
/// Spec: `docs/design/p1-modules.md`, Ladder low-pass. Explainer:
/// `docs/learn/ladder-filter.md`.
pub static LADDER: ModuleDescriptor = ModuleDescriptor {
    id: "loom.filter.ladder",
    version: 1,
    name: "Ladder",
    category: "Filter",
    family: loom_core::Family::FiltersAndEffects,
    description: "A resonant low-pass with a 12, 18, or 24 dB slope; high resonance thins the bass and, at 18 and 24 dB, sings on its own. The cutoff input adds pitch: +0.1 doubles the cutoff.",
    inputs: &[
        PortSpec {
            name: "in",
            label: "In",
            signal: SignalType::Audio,
            description: "The audio to filter.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "cutoff",
            label: "Cutoff",
            signal: SignalType::Pitch,
            description: "Moves the cutoff as pitch: +0.1 doubles it.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::Downmix,
        },
    ],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "Keeps the frequencies below the cutoff.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
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
            description: "Feeds the output back to boost the cutoff. At 1, the 18 and 24 dB slopes oscillate on their own.",
            min: 0.0,
            max: 1.0,
            default: 0.2,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .smooth(0.005),
        ParamSpec {
            name: "drive",
            label: "Drive",
            description: "Raises the input level, up to 8 times (+18 dB), so the feedback saturates sooner.",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .smooth(0.005),
        ParamSpec {
            name: "slope",
            label: "Slope",
            description: "How steeply the filter cuts above the cutoff: two, three, or four stages in the loop.",
            min: 0.0,
            max: 2.0,
            default: 2.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer()
        .choices(&["12 dB", "18 dB", "24 dB"]),
    ],
    role: ModuleRole::Dsp(|config| Box::new(Ladder::new(config))),
    panel: Panel::new(
        11,
        10,
        &[
            Widget::shape(0, 0, 11, "out", "Response", |params, x, _| ladder_preview(params, x)),
            Widget::segmented(0, 5, 11, "slope"),
            // One unit apart, so "Resonance" fits between its neighbours.
            Widget::knob(0, 7, "cutoff"),
            Widget::knob(4, 7, "resonance"),
            Widget::knob(8, 7, "drive"),
        ],
    ),
    displays: &[],
    roles: &[FillsRole::of(&roles::FILTER)],
};

/// The ladder's small-signal response for the tile. The horizontal axis is
/// logarithmic frequency from 20 Hz to 20 kHz. The closed-loop transfer is
/// `1 / ((1 + j f/fc)^N + k)` (docs/learn/ladder-filter.md). Drive and
/// nonlinear saturation depend on the input, so this remains schematic.
fn ladder_preview(params: &[f32], x: f32) -> f32 {
    if params.len() != LADDER.params.len() {
        return 0.0;
    }
    let (cutoff, resonance, slope) = (params[Ladder::CUTOFF], params[Ladder::RESONANCE], params[Ladder::SLOPE]);
    let frequency = 20.0 * 1000.0_f32.powf(x.clamp(0.0, 1.0));
    let ratio = frequency / cutoff.max(20.0);
    let slope = slope_from(slope);
    let mut real = 1.0;
    let mut imaginary = 0.0;
    for _ in 0..slope.stages() {
        (real, imaginary) = (real - imaginary * ratio, imaginary + real * ratio);
    }
    let feedback = resonance.clamp(0.0, 1.0) * slope.feedback_limit();
    let magnitude = 1.0 / ((real + feedback).powi(2) + imaginary.powi(2)).sqrt();
    ((20.0 * magnitude.log10() + 60.0) / 72.0).clamp(0.0, 1.0)
}

#[cfg(test)]
mod preview_tests;

/// The slope a knob or a modulated value selects. Rounds to the nearest of 0,
/// 1, and 2, and NaN gives 0.
fn slope_from(value: f32) -> ladder::Slope {
    match value.round().clamp(0.0, 2.0) as usize {
        0 => ladder::Slope::Db12,
        1 => ladder::Slope::Db18,
        _ => ladder::Slope::Db24,
    }
}

/// The input gain at drive 1: 8 times, +18 dB.
const LADDER_MAX_DRIVE: f32 = 8.0;

/// State for `loom.filter.ladder`.
#[derive(Clone)]
struct Ladder {
    sample_rate: f32,
    /// The cutoff in pitch units, smoothed over 10 ms.
    cutoff: Smoothed,
    resonance: Smoothed,
    drive: Smoothed,
    /// The knob's slope. Modulation counts for one frame only.
    slope: ladder::Slope,
    filter: ladder::Ladder,
}

impl Ladder {
    const CUTOFF: usize = LADDER.param_index("cutoff").unwrap();
    const RESONANCE: usize = LADDER.param_index("resonance").unwrap();
    const DRIVE: usize = LADDER.param_index("drive").unwrap();
    const SLOPE: usize = LADDER.param_index("slope").unwrap();
    /// A 24 dB ladder at 1 kHz with a cleared state.
    fn new(config: &ModuleConfig) -> Self {
        let params = LADDER.params;
        Self {
            sample_rate: config.sample_rate,
            // Smoothed in the pitch domain, like the SVF.
            cutoff: Smoothed::new(
                pitch::from_hz(1_000.0),
                config.sample_rate,
                params[Self::CUTOFF].options.smooth_seconds,
            ),
            resonance: Smoothed::new(0.2, config.sample_rate, params[Self::RESONANCE].options.smooth_seconds),
            drive: Smoothed::new(0.0, config.sample_rate, params[Self::DRIVE].options.smooth_seconds),
            slope: ladder::Slope::Db24,
            filter: ladder::Ladder::new(config.sample_rate),
        }
    }
}

impl Module for Ladder {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let cutoff_param = io.param(Self::CUTOFF);
        let resonance_param = io.param(Self::RESONANCE);
        let drive_param = io.param(Self::DRIVE);
        let slope_param = io.param(Self::SLOPE);
        let ([input, cutoff_mod], [out]) = io.split::<2, 1>();
        let nyquist_limit = self.sample_rate * 0.45;
        for i in 0..input.len() {
            // Tick even when modulated, so removing the assignment continues
            // the glide from here.
            let cutoff_pitch = self.cutoff.tick();
            let cutoff_pitch = cutoff_param.map_or(i, cutoff_pitch, pitch::from_hz);
            let cutoff_hz = pitch::to_hz(cutoff_pitch + cutoff_mod[i]);
            // `clamp` passes NaN through. Use a safe cutoff for that frame.
            let cutoff_hz = if cutoff_hz.is_nan() { 1_000.0 } else { cutoff_hz }.clamp(5.0, nyquist_limit);
            let resonance = resonance_param.at_or(i, self.resonance.tick()).clamp(0.0, 1.0);
            let drive = drive_param.at_or(i, self.drive.tick()).clamp(0.0, 1.0);
            let slope = slope_param.map_or(i, self.slope, slope_from);
            if slope != self.filter.target() {
                self.filter.change_slope(slope);
            }
            let limit = self.filter.step_slope();

            let x = input[i];
            if !x.is_finite() {
                // A bad input clears the loop, so it cannot stay in it.
                self.filter.clear_loop();
                out[i] = 0.0;
                continue;
            }
            let x = loom_dsp::guard(x);
            let coefficients = ladder::Coefficients::new(ladder::Settings {
                cutoff_hz,
                feedback: resonance * limit,
                // Drive 0 to 1 is a gain of 1 to 8, even in dB.
                input_gain: LADDER_MAX_DRIVE.powf(drive),
                sample_rate: self.sample_rate,
            });
            out[i] = self.filter.tick(x, coefficients);
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            Self::CUTOFF => self.cutoff.set_target(pitch::from_hz(value)),
            Self::RESONANCE => self.resonance.set_target(value),
            Self::DRIVE => self.drive.set_target(value),
            Self::SLOPE => self.slope = slope_from(value),
            _ => {}
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        match index {
            Self::CUTOFF => self.cutoff.gliding().map(pitch::to_hz),
            Self::RESONANCE => self.resonance.gliding(),
            Self::DRIVE => self.drive.gliding(),
            _ => None,
        }
    }

    fn reset(&mut self) {
        // Snap smoothing and any slope change to the knobs (live patching,
        // rule 7), and clear the loop.
        self.cutoff.reset(self.cutoff.target());
        self.resonance.reset(self.resonance.target());
        self.drive.reset(self.drive.target());
        self.filter.reset(self.slope);
    }
}

/// `loom.filter.crossover`: two or three bands that sum to a flat level
/// (MT-FILTER-07). The algorithm is in [`crossover`]. Spec:
/// `docs/design/dynamics-eq-crossover.md`.
pub static CROSSOVER: ModuleDescriptor = ModuleDescriptor {
    id: "loom.filter.crossover",
    version: 1,
    name: "Crossover",
    category: "Filter",
    family: loom_core::Family::FiltersAndEffects,
    description: "Splits a signal into two or three frequency bands that sum to a flat level.",
    inputs: &[stereo_port("in", "In", "The signal to split.", Channels::PerChannel)],
    outputs: &[
        stereo_port(
            "low",
            "Low",
            "The frequencies below the lower split.",
            Channels::PerChannel,
        ),
        stereo_port(
            "mid",
            "Mid",
            "The frequencies between the splits. Silent with two bands.",
            Channels::PerChannel,
        ),
        stereo_port(
            "high",
            "High",
            "The frequencies above the upper split.",
            Channels::PerChannel,
        ),
    ],
    params: &[
        ParamSpec {
            name: "bands",
            label: "Bands",
            description: "How many bands it makes.",
            min: 0.0,
            max: 1.0,
            default: 1.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer()
        .choices(&["Two", "Three"]),
        ParamSpec {
            name: "low_split",
            label: "Low split",
            description: "Where the low band ends.",
            min: 20.0,
            max: 16_000.0,
            default: 200.0,
            unit: "Hz",
            options: ParamOptions::DEFAULT,
        }
        .logarithmic()
        .smooth(0.005),
        ParamSpec {
            name: "high_split",
            label: "High split",
            description: "Where the high band starts. It stays at or above the low split.",
            min: 40.0,
            max: 20_000.0,
            default: 2_000.0,
            unit: "Hz",
            options: ParamOptions::DEFAULT,
        }
        .logarithmic()
        .smooth(0.005)
        .shown_when("bands", &[1]),
    ],
    role: ModuleRole::Dsp(|config| Box::new(Crossover::new(config))),
    panel: Panel::new(
        6,
        5,
        &[
            Widget::segmented(0, 0, 6, "bands"),
            Widget::knob(0, 2, "low_split"),
            Widget::knob(3, 2, "high_split"),
        ],
    ),
    displays: &[],
    roles: &[],
};

/// State for `loom.filter.crossover`.
#[derive(Clone)]
struct Crossover {
    sample_rate: f32,
    filter: crossover::Crossover,
    low_split: Smoothed,
    high_split: Smoothed,
    /// The knob's band count, as 0 for two and 1 for three.
    bands: f32,
    /// 0 is two bands and 1 is three, so a change does not click.
    three: Smoothed,
    coefficients: Cached<(f32, f32), crossover::Coefficients>,
}

impl Crossover {
    const BANDS: usize = CROSSOVER.param_index("bands").unwrap();
    const LOW_SPLIT: usize = CROSSOVER.param_index("low_split").unwrap();
    const HIGH_SPLIT: usize = CROSSOVER.param_index("high_split").unwrap();

    fn new(config: &ModuleConfig) -> Self {
        let rate = config.sample_rate;
        let params = CROSSOVER.params;
        let (low, high) = (params[Self::LOW_SPLIT].default, params[Self::HIGH_SPLIT].default);
        let pitch_of = |index: usize| {
            let param = &params[index];
            Smoothed::new(pitch::from_hz(param.default), rate, param.options.smooth_seconds)
        };
        Self {
            sample_rate: rate,
            filter: crossover::Crossover::new(),
            low_split: pitch_of(Self::LOW_SPLIT),
            high_split: pitch_of(Self::HIGH_SPLIT),
            bands: params[Self::BANDS].default,
            three: Smoothed::new(params[Self::BANDS].default, rate, 0.005),
            coefficients: Cached::new((low, high), move |(low, high)| {
                crossover::Coefficients::new(low, high, rate)
            }),
        }
    }
}

impl Module for Crossover {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let bands = io.param(Self::BANDS);
        let low_default = CROSSOVER.params[Self::LOW_SPLIT].default;
        let high_default = CROSSOVER.params[Self::HIGH_SPLIT].default;
        let low_param = io.param(Self::LOW_SPLIT);
        let high_param = io.param(Self::HIGH_SPLIT);
        let top = self.sample_rate * 0.49;
        let hz_of = |value: f32, fallback: f32| {
            let hz = pitch::to_hz(value);
            if hz.is_nan() { fallback } else { hz }.clamp(1.0, top)
        };
        let ([input], [low_out, mid_out, high_out]) = io.split::<1, 3>();
        for i in 0..input.len() {
            let x = loom_dsp::guard(input[i]);
            let low = hz_of(low_param.map_or(i, self.low_split.tick(), pitch::from_hz), low_default);
            let high = hz_of(
                high_param.map_or(i, self.high_split.tick(), pitch::from_hz),
                high_default,
            );
            let rate = self.sample_rate;
            let coefficients = self
                .coefficients
                .get((low, high), |(low, high)| crossover::Coefficients::new(low, high, rate));
            self.three
                .set_target(if bands.at_or(i, self.bands) >= 0.5 { 1.0 } else { 0.0 });
            let three = self.three.tick();
            let out = self.filter.tick(x, coefficients);
            low_out[i] = out.lower + three * (out.low - out.lower);
            mid_out[i] = three * out.mid;
            high_out[i] = out.upper + three * (out.high - out.upper);
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            Self::BANDS => self.bands = value,
            Self::LOW_SPLIT => self.low_split.set_target(pitch::from_hz(value)),
            Self::HIGH_SPLIT => self.high_split.set_target(pitch::from_hz(value)),
            _ => {}
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        match index {
            // The splits glide in pitch. The engine glides in the knob's
            // position, which is also logarithmic, so the paths match.
            Self::LOW_SPLIT => self.low_split.gliding().map(pitch::to_hz),
            Self::HIGH_SPLIT => self.high_split.gliding().map(pitch::to_hz),
            _ => None,
        }
    }

    fn reset(&mut self) {
        self.filter.reset();
        self.low_split.reset(self.low_split.target());
        self.high_split.reset(self.high_split.target());
        self.three.reset(if self.bands >= 0.5 { 1.0 } else { 0.0 });
    }
}

#[cfg(test)]
mod tests;
