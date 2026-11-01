//! Filters (FR-LIB-03).

use loom_core::signal::pitch;
use loom_core::{
    Channels, Module, ModuleConfig, ModuleDescriptor, ModuleRole, Panel, ParamOptions, ParamSpec, PortDefault, PortIo,
    PortSpec, ProcessContext, SignalType, Widget,
};
use loom_rt::Smoothed;
use std::f32::consts::PI;

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
};

#[derive(Clone)]
struct Svf {
    sample_rate: f32,
    cutoff: Smoothed,
    resonance: Smoothed,
    ic1: f32,
    ic2: f32,
}

impl Svf {
    fn new(config: &ModuleConfig) -> Self {
        Self {
            sample_rate: config.sample_rate,
            // Smooth cutoff in the pitch domain, so sweeps sound even.
            // The time matches `ParamSpec::smooth_seconds`, which the engine
            // uses once an assignment is modulating this knob.
            cutoff: Smoothed::new(
                pitch::from_hz(1_000.0),
                config.sample_rate,
                SVF.params[0].options.smooth_seconds,
            ),
            resonance: Smoothed::new(0.2, config.sample_rate, SVF.params[1].options.smooth_seconds),
            ic1: 0.0,
            ic2: 0.0,
        }
    }
}

impl Module for Svf {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let cutoff_param = io.param(0);
        let resonance_param = io.param(1);
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
            let k = 2.0 - 1.98 * resonance;
            let g = (PI * cutoff_hz / self.sample_rate).tan();
            let a1 = 1.0 / (1.0 + g * (g + k));
            let a2 = g * a1;
            let a3 = g * a2;

            let x = input[i];
            let x = if x.is_finite() { x } else { 0.0 };
            let v3 = x - self.ic2;
            let v1 = a1 * self.ic1 + a2 * v3;
            let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
            self.ic1 = 2.0 * v1 - self.ic1;
            self.ic2 = 2.0 * v2 - self.ic2;

            lp[i] = v2;
            bp[i] = v1;
            hp[i] = x - k * v1 - v2;
            // The notch is the lowpass plus the highpass: the bandpass terms
            // cancel and `x - k * v1` is left. One addition, always in step.
            notch[i] = lp[i] + hp[i];
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            0 => self.cutoff.set_target(pitch::from_hz(value)),
            1 => self.resonance.set_target(value),
            _ => {}
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        match index {
            // The cutoff glides in pitch. The engine glides in the knob's
            // position, which is also logarithmic, so the paths match.
            0 => self.cutoff.gliding().map(pitch::to_hz),
            1 => self.resonance.gliding(),
            _ => None,
        }
    }

    /// Clears the filter memory and snaps the smoothing to its targets
    /// (live patching spec, rule 7).
    fn reset(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
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
};

/// State for `loom.filter.onepole`: the smoothed cutoff and the integrator.
#[derive(Clone)]
struct OnePole {
    sample_rate: f32,
    /// The cutoff in pitch units, smoothed over 10 ms.
    cutoff: Smoothed,
    /// The trapezoidal integrator state: the lowpass output so far.
    state: f32,
}

impl OnePole {
    /// A one-pole at 1 kHz with a cleared state.
    fn new(config: &ModuleConfig) -> Self {
        Self {
            sample_rate: config.sample_rate,
            // Smoothed in the pitch domain, like the SVF.
            cutoff: Smoothed::new(
                pitch::from_hz(1_000.0),
                config.sample_rate,
                ONEPOLE.params[0].options.smooth_seconds,
            ),
            state: 0.0,
        }
    }
}

impl Module for OnePole {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let cutoff_param = io.param(0);
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
            // Trapezoidal one-pole. Warping the cutoff with `tan` puts the
            // -3 dB point exactly on the cutoff at any rate.
            let g = (PI * cutoff_hz / self.sample_rate).tan();
            let x = input[i];
            let x = if x.is_finite() { x } else { 0.0 };
            let v = (x - self.state) * g / (1.0 + g);
            let low = v + self.state;
            self.state = low + v;
            lp[i] = low;
            hp[i] = x - low;
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if index == 0 {
            self.cutoff.set_target(pitch::from_hz(value));
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        if index == 0 {
            self.cutoff.gliding().map(pitch::to_hz)
        } else {
            None
        }
    }

    /// Clears the integrator and snaps the cutoff to its target (live
    /// patching spec, rule 7).
    fn reset(&mut self) {
        self.state = 0.0;
        let cutoff = self.cutoff.target();
        self.cutoff.reset(cutoff);
    }
}

/// `loom.filter.ladder`: a resonant low-pass made of two, three, or four
/// one-pole stages in a loop, with saturation in the feedback (MT-FILTER-02).
///
/// Designed from published theory only (ADR-0018):
///
/// - T. Stilson and J. Smith, "Analyzing the Moog VCF with Considerations
///   for Digital Implementation", Proc. ICMC 1996: the loop of identical
///   one-pole stages with inverting feedback, and the gain where it starts
///   to oscillate.
/// - V. Zavalishin, "The Art of VA Filter Design": the trapezoidal (TPT)
///   one-pole stage, the prewarped cutoff, and solving the feedback loop
///   with no unit delay in it (zero-delay feedback).
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
        12,
        11,
        &[
            Widget::shape(0, 0, 12, "out", "Response", ladder_preview),
            Widget::radio(0, 5, "slope"),
            Widget::knob(6, 5, "cutoff"),
            Widget::knob(9, 5, "resonance"),
            Widget::knob(6, 8, "drive"),
        ],
    ),
    displays: &[],
};

/// The ladder's small-signal response for the tile. The horizontal axis is
/// logarithmic frequency from 20 Hz to 20 kHz. The closed-loop transfer is
/// `1 / ((1 + j f/fc)^N + k)` (docs/learn/ladder-filter.md). Drive and
/// nonlinear saturation depend on the input, so this remains schematic.
fn ladder_preview(params: &[f32], x: f32) -> f32 {
    let &[cutoff, resonance, _, slope] = params else {
        return 0.0;
    };
    let frequency = 20.0 * 1000.0_f32.powf(x.clamp(0.0, 1.0));
    let ratio = frequency / cutoff.max(20.0);
    let stage = slope.round().clamp(0.0, 2.0) as usize;
    let mut real = 1.0;
    let mut imaginary = 0.0;
    for _ in 0..stage + 2 {
        (real, imaginary) = (real - imaginary * ratio, imaginary + real * ratio);
    }
    let feedback = resonance.clamp(0.0, 1.0) * LADDER_K_MAX[stage];
    let magnitude = 1.0 / ((real + feedback).powi(2) + imaginary.powi(2)).sqrt();
    ((20.0 * magnitude.log10() + 60.0) / 72.0).clamp(0.0, 1.0)
}

#[cfg(test)]
mod preview_tests {
    use super::ladder_preview;

    /// Cutoff moves the schematic edge and slope changes the high end.
    #[test]
    fn cutoff_and_slope_change_shape() {
        let low = [200.0, 0.0, 0.0, 0.0];
        let high = [2_000.0, 0.0, 0.0, 0.0];
        assert!(ladder_preview(&high, 0.6) > ladder_preview(&low, 0.6));
        assert!(ladder_preview(&low, 0.5) > ladder_preview(&[200.0, 0.0, 0.0, 2.0], 0.5));
    }
}

/// The number of one-pole stages the ladder always runs.
const LADDER_STAGES: usize = 4;

/// The feedback gain at resonance 1 for each slope (2, 3, and 4 stages).
///
/// For N identical stages `1 / (1 + s)` with feedback `-k`, the loop rings
/// forever when `(1 + s)^N = -k` has a root on the imaginary axis: `k = 8`
/// for N = 3 (at `√3` times the cutoff) and `k = 4` for N = 4 (at the
/// cutoff), as Stilson and Smith show. Two stages never reach it. The 18 and
/// 24 dB values are 5% above the limit: at the limit itself the loop only
/// keeps an oscillation that is already there, and the tanh in the feedback
/// pulls it below, so resonance 1 would not sing.
const LADDER_K_MAX: [f32; 3] = [8.0, 8.4, 4.2];

/// How fast the feedback limit moves during a slope change, per second:
/// from 8.4 to 4.2 in 10 ms.
const LADDER_LIMIT_RAMP: f32 = (8.4 - 4.2) / 0.01;

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
    /// The knob's slope, 0 to 2. Modulation counts for one frame only.
    slope: usize,
    /// The slope the output is at or fading to.
    target: usize,
    /// How much each tap (after stage 2, 3, and 4) adds to the output and
    /// to the feedback. One of them is 1, except during a slope change.
    weights: [f32; 3],
    /// The weights when the current slope change began.
    from: [f32; 3],
    /// Progress of the slope crossfade, from 0 to 1. 1 means done.
    fade: f32,
    /// Progress per frame of the 10 ms crossfade.
    fade_step: f32,
    /// The feedback gain at resonance 1 now. During a slope change it
    /// moves toward the limit of the taps in use at [`LADDER_LIMIT_RAMP`],
    /// so the gain never jumps.
    limit: f32,
    /// How far `limit` may move in one frame.
    limit_step: f32,
    /// The trapezoidal integrator state of each stage. All four always
    /// run, so a change of slope finds the stages already moving.
    state: [f32; LADDER_STAGES],
    /// `tanh(y) / y` for the last output `y`: the slope of the line from
    /// the origin to the point on the tanh curve. See [`Ladder::tick`].
    saturation: f32,
}

impl Ladder {
    /// A 24 dB ladder at 1 kHz with a cleared state.
    fn new(config: &ModuleConfig) -> Self {
        let params = LADDER.params;
        Self {
            sample_rate: config.sample_rate,
            // Smoothed in the pitch domain, like the SVF.
            cutoff: Smoothed::new(
                pitch::from_hz(1_000.0),
                config.sample_rate,
                params[0].options.smooth_seconds,
            ),
            resonance: Smoothed::new(0.2, config.sample_rate, params[1].options.smooth_seconds),
            drive: Smoothed::new(0.0, config.sample_rate, params[2].options.smooth_seconds),
            slope: 2,
            target: 2,
            weights: [0.0, 0.0, 1.0],
            from: [0.0, 0.0, 1.0],
            fade: 1.0,
            fade_step: 1.0 / (0.01 * config.sample_rate).max(1.0),
            limit: LADDER_K_MAX[2],
            limit_step: LADDER_LIMIT_RAMP / config.sample_rate,
            state: [0.0; LADDER_STAGES],
            saturation: 1.0,
        }
    }

    /// Starts a 10 ms crossfade from the current tap weights to `slope`.
    /// A change during a crossfade starts from where that one had got to.
    fn change_slope(&mut self, slope: usize) {
        self.target = slope;
        self.from = self.weights;
        self.fade = 0.0;
    }

    /// Moves a slope change on by one frame and returns the feedback gain
    /// at resonance 1 for this frame.
    ///
    /// The gain may never pass the limit of any tap that is heard or about
    /// to be heard. So when the new slope's limit is lower, the gain first
    /// moves down to it, with only the old taps heard, and the crossfade
    /// waits. When it is higher, the crossfade runs first, at the old gain,
    /// and the gain then moves up. The gain never jumps, so the output
    /// never clicks.
    fn step_slope(&mut self) -> f32 {
        let wanted = self
            .weights
            .iter()
            .zip(LADDER_K_MAX)
            .enumerate()
            .filter(|&(tap, (&w, _))| tap == self.target || w > 0.0)
            .map(|(_, (_, k))| k)
            .fold(f32::INFINITY, f32::min);
        self.limit = if self.limit > wanted {
            (self.limit - self.limit_step).max(wanted)
        } else {
            (self.limit + self.limit_step).min(wanted)
        };
        if self.fade < 1.0 && self.limit <= LADDER_K_MAX[self.target] {
            self.fade = (self.fade + self.fade_step).min(1.0);
            for (tap, weight) in self.weights.iter_mut().enumerate() {
                let to = if tap == self.target { 1.0 } else { 0.0 };
                *weight = self.from[tap] + (to - self.from[tap]) * self.fade;
            }
            if self.fade >= 1.0 {
                self.weights = [0.0; 3];
                self.weights[self.target] = 1.0;
            }
        }
        self.limit
    }

    /// One frame of the filter: input `x`, prewarped gain `g`, feedback
    /// gain `k`, input gain `gain`. Returns the output.
    ///
    /// Each stage is Zavalishin's trapezoidal one-pole: with
    /// `G = g / (1 + g)`, its output is `y = G·x + s / (1 + g)`, where `s`
    /// is its state. So every tap is a straight line in the loop input `u`:
    /// `y = a·u + b`. The loop is `u = gain·x - k·tanh(y)`. The tanh is
    /// replaced by its secant through the origin at the last output,
    /// `tanh(y) ≈ y · tanh(y_last) / y_last`, so `u` comes from one
    /// division, with no unit delay in the loop (zero-delay feedback). The
    /// secant slope is at most 1, so it only ever lowers the loop gain.
    fn tick(&mut self, x: f32, g: f32, k: f32, gain: f32) -> f32 {
        let big_g = g / (1.0 + g);
        let carry = 1.0 / (1.0 + g);
        // The taps after stages 2, 3, and 4 as `a·u + b`.
        let (mut a, mut b) = (1.0, 0.0);
        let mut taps = [(0.0, 0.0); 3];
        for (stage, &s) in self.state.iter().enumerate() {
            a *= big_g;
            b = big_g * b + carry * s;
            if stage >= 1 {
                taps[stage - 1] = (a, b);
            }
        }
        let (mut fa, mut fb) = (0.0, 0.0);
        for (&(a, b), &w) in taps.iter().zip(&self.weights) {
            fa += w * a;
            fb += w * b;
        }
        let loop_gain = k * self.saturation;
        let u = (gain * x - loop_gain * fb) / (1.0 + loop_gain * fa);

        // Run the four stages on `u`.
        let mut input = u;
        let mut outs = [0.0; LADDER_STAGES];
        for (s, out) in self.state.iter_mut().zip(&mut outs) {
            let v = (input - *s) * big_g;
            let y = v + *s;
            *s = y + v;
            *out = y;
            input = y;
        }
        let y: f32 = outs[1..].iter().zip(&self.weights).map(|(y, w)| y * w).sum();
        self.saturation = if y.abs() < 1e-4 { 1.0 } else { y.tanh() / y };
        y
    }
}

impl Module for Ladder {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let cutoff_param = io.param(0);
        let resonance_param = io.param(1);
        let drive_param = io.param(2);
        let slope_param = io.param(3);
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
            let slope = slope_param.map_or(i, self.slope, |v| v.round().clamp(0.0, 2.0) as usize);
            if slope != self.target {
                self.change_slope(slope);
            }
            let limit = self.step_slope();

            let x = input[i];
            if !x.is_finite() {
                // A bad input clears the loop, so it cannot stay in it.
                self.state = [0.0; LADDER_STAGES];
                self.saturation = 1.0;
                out[i] = 0.0;
                continue;
            }
            let g = (PI * cutoff_hz / self.sample_rate).tan();
            // Drive 0 to 1 is a gain of 1 to 8, even in dB.
            let gain = LADDER_MAX_DRIVE.powf(drive);
            out[i] = self.tick(x, g, resonance * limit, gain);
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            0 => self.cutoff.set_target(pitch::from_hz(value)),
            1 => self.resonance.set_target(value),
            2 => self.drive.set_target(value),
            3 => self.slope = value.round().clamp(0.0, 2.0) as usize,
            _ => {}
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        match index {
            0 => self.cutoff.gliding().map(pitch::to_hz),
            1 => self.resonance.gliding(),
            2 => self.drive.gliding(),
            _ => None,
        }
    }

    fn reset(&mut self) {
        // Snap smoothing and any slope change to the knobs (live patching,
        // rule 7), and clear the loop.
        self.cutoff.reset(self.cutoff.target());
        self.resonance.reset(self.resonance.target());
        self.drive.reset(self.drive.target());
        self.target = self.slope;
        self.weights = [0.0; 3];
        self.weights[self.slope] = 1.0;
        self.from = self.weights;
        self.fade = 1.0;
        self.limit = LADDER_K_MAX[self.slope];
        self.state = [0.0; LADDER_STAGES];
        self.saturation = 1.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::testing::run;
    use std::f32::consts::TAU;

    const RATE: f32 = 48_000.0;

    /// Builds a module from a descriptor, settled at the given parameters.
    fn make(descriptor: &ModuleDescriptor, params: &[(usize, f32)]) -> Box<dyn Module> {
        let ModuleRole::Dsp(create) = descriptor.role else {
            unreachable!()
        };
        let mut m = create(&ModuleConfig {
            sample_rate: RATE,
            max_block: 48_000,
        });
        for &(index, value) in params {
            m.set_param(index, value);
        }
        m.reset();
        m
    }

    /// RMS of the second half of a buffer, after the filter has settled.
    fn settled_rms(samples: &[f32]) -> f32 {
        let tail = &samples[samples.len() / 2..];
        (tail.iter().map(|v| v * v).sum::<f32>() / tail.len() as f32).sqrt()
    }

    /// A sine of `hz` lasting one second.
    fn sine(hz: f32) -> Vec<f32> {
        (0..RATE as usize).map(|i| (TAU * hz * i as f32 / RATE).sin()).collect()
    }

    /// Gain in dB of one SVF output (0 low, 1 band, 2 high, 3 notch) for a sine.
    fn svf_gain_db(output: usize, cutoff: f32, hz: f32) -> f32 {
        let mut svf = make(&SVF, &[(0, cutoff), (1, 0.2)]);
        // Let the 10 ms smoother settle, so the cutoff is the parameter.
        let input = sine(hz);
        let cutoff_mod = vec![0.0; input.len()];
        let mut outs = vec![vec![0.0; input.len()]; 4];
        let [a, b, c, d] = &mut outs[..] else { unreachable!() };
        run(
            svf.as_mut(),
            RATE,
            &[&input, &cutoff_mod],
            &[true, false],
            &mut [a, b, c, d],
            &[],
        );
        20.0 * (settled_rms(&outs[output]) / settled_rms(&input)).log10()
    }

    /// The notch cuts a sine at the cutoff by 30 dB or more, and passes a
    /// sine two octaves away within 1 dB (MT-FILTER-01).
    #[test]
    fn svf_notch_cuts_the_cutoff_and_passes_the_rest() {
        assert!(svf_gain_db(3, 1_000.0, 1_000.0) < -30.0);
        assert!(svf_gain_db(3, 1_000.0, 250.0).abs() < 1.0);
        assert!(svf_gain_db(3, 1_000.0, 4_000.0).abs() < 1.0);
    }

    /// Both one-pole outputs are 3 dB down at their cutoff.
    #[test]
    fn onepole_is_three_db_down_at_the_cutoff() {
        for cutoff in [100.0, 1_000.0, 10_000.0] {
            let mut filter = make(&ONEPOLE, &[(0, cutoff)]);
            let input = sine(cutoff);
            let cutoff_mod = vec![0.0; input.len()];
            let (mut lp, mut hp) = (vec![0.0; input.len()], vec![0.0; input.len()]);
            run(
                filter.as_mut(),
                RATE,
                &[&input, &cutoff_mod],
                &[true, false],
                &mut [&mut lp, &mut hp],
                &[],
            );
            for out in [&lp, &hp] {
                let db = 20.0 * (settled_rms(out) / settled_rms(&input)).log10();
                assert!((db + 3.01).abs() < 0.1, "cutoff {cutoff}: {db} dB");
            }
        }
    }

    /// A NaN on the cutoff cable does not poison the filter state: the
    /// output is finite during the NaN and the filter works afterwards.
    #[test]
    fn onepole_recovers_from_nan_on_the_cutoff_cable() {
        let mut filter = make(&ONEPOLE, &[(0, 1_000.0)]);
        let input = vec![0.5; 256];
        let mut cutoff_mod = vec![0.0; 256];
        cutoff_mod[10..14].fill(f32::NAN);
        let (mut lp, mut hp) = (vec![0.0; 256], vec![0.0; 256]);
        run(
            filter.as_mut(),
            RATE,
            &[&input, &cutoff_mod],
            &[true, true],
            &mut [&mut lp, &mut hp],
            &[],
        );
        assert!(lp.iter().chain(&hp).all(|v| v.is_finite()), "{lp:?}");
        // A constant input of 0.5 settles near 0.5 at the lowpass.
        assert!((lp[255] - 0.5).abs() < 0.05, "{}", lp[255]);
    }

    /// A NaN in the input does not poison the filter state.
    #[test]
    fn onepole_recovers_from_nan() {
        let mut filter = make(&ONEPOLE, &[(0, 1_000.0)]);
        let mut input = vec![0.5; 64];
        input[3] = f32::NAN;
        let cutoff_mod = vec![0.0; 64];
        let (mut lp, mut hp) = (vec![0.0; 64], vec![0.0; 64]);
        run(
            filter.as_mut(),
            RATE,
            &[&input, &cutoff_mod],
            &[true, false],
            &mut [&mut lp, &mut hp],
            &[],
        );
        assert!(lp.iter().chain(&hp).all(|v| v.is_finite()));
    }

    /// Indices of the ladder's parameters.
    const CUTOFF: usize = 0;
    const RESONANCE: usize = 1;
    const DRIVE: usize = 2;
    const SLOPE: usize = 3;

    /// Runs a new ladder over `input` with nothing on its cutoff input.
    fn ladder(params: &[(usize, f32)], input: &[f32]) -> Vec<f32> {
        let mut filter = make(&LADDER, params);
        ladder_run(filter.as_mut(), input)
    }

    /// Runs an existing ladder over `input`.
    fn ladder_run(filter: &mut dyn Module, input: &[f32]) -> Vec<f32> {
        let cutoff_mod = vec![0.0; input.len()];
        let mut out = vec![0.0; input.len()];
        run(
            filter,
            RATE,
            &[input, &cutoff_mod],
            &[true, false],
            &mut [&mut out],
            &[],
        );
        out
    }

    /// Gain in dB of the ladder for a quiet sine, so the tanh stays linear.
    /// The sine is computed in `f64`: an `f32` phase jitters enough to
    /// leave noise below the cutoff louder than a tone far above it.
    fn ladder_gain_db(params: &[(usize, f32)], hz: f32) -> f32 {
        let input: Vec<f32> = (0..RATE as usize)
            .map(|i| (0.01 * (std::f64::consts::TAU * f64::from(hz) * i as f64 / f64::from(RATE)).sin()) as f32)
            .collect();
        let out = ladder(params, &input);
        20.0 * (settled_rms(&out) / settled_rms(&input)).log10()
    }

    /// The frequency of a settled oscillation, from its rising zero
    /// crossings in the second half of `samples`.
    fn frequency(samples: &[f32]) -> f32 {
        let tail = &samples[samples.len() / 2..];
        let crossings: Vec<usize> = (1..tail.len())
            .filter(|&i| tail[i - 1] < 0.0 && tail[i] >= 0.0)
            .collect();
        let (first, last) = (crossings[0], crossings[crossings.len() - 1]);
        (crossings.len() - 1) as f32 * RATE / (last - first) as f32
    }

    /// One click at frame 0, then a second of silence: a start for
    /// self-oscillation.
    fn click() -> Vec<f32> {
        let mut input = vec![0.0; RATE as usize];
        input[0] = 0.1;
        input
    }

    /// The largest jump between neighbouring samples.
    fn max_step(samples: &[f32]) -> f32 {
        samples.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max)
    }

    /// An octave well above the cutoff, each slope cuts about 12, 18, or
    /// 24 dB more (p1-modules.md, Acceptance criteria).
    #[test]
    fn ladder_slopes_are_12_18_and_24_db() {
        for (slope, db) in [(0.0, 12.0), (1.0, 18.0), (2.0, 24.0)] {
            let params = [(CUTOFF, 100.0), (RESONANCE, 0.0), (SLOPE, slope)];
            let drop = ladder_gain_db(&params, 1_600.0) - ladder_gain_db(&params, 3_200.0);
            assert!((drop - db).abs() < 1.0, "slope {slope}: {drop} dB per octave");
        }
    }

    /// At resonance 1 the 24 dB slope sings at the cutoff and the 18 dB
    /// slope at about 1.7 times it, after one click. The 18 dB check stays
    /// below 0.29 times the sample rate. The tanh keeps the level bounded.
    #[test]
    fn ladder_self_oscillates_at_resonance_one() {
        for cutoff in [200.0, 1_000.0, 5_000.0] {
            let out = ladder(&[(CUTOFF, cutoff), (RESONANCE, 1.0), (SLOPE, 2.0)], &click());
            let rms = settled_rms(&out);
            assert!(rms > 0.1, "24 dB at {cutoff} Hz: rms {rms}");
            assert!(out.iter().all(|v| v.abs() < 2.0), "24 dB at {cutoff} Hz stays bounded");
            let hz = frequency(&out);
            assert!(
                (hz / cutoff - 1.0).abs() < 0.02,
                "24 dB at {cutoff} Hz sings at {hz} Hz"
            );

            let out = ladder(&[(CUTOFF, cutoff), (RESONANCE, 1.0), (SLOPE, 1.0)], &click());
            let rms = settled_rms(&out);
            assert!(rms > 0.1, "18 dB at {cutoff} Hz: rms {rms}");
            // √3 times the cutoff, through the same prewarping as the cutoff.
            let warped = (3f32.sqrt() * (PI * cutoff / RATE).tan()).atan() * RATE / PI;
            let hz = frequency(&out);
            assert!(
                (hz / warped - 1.0).abs() < 0.02,
                "18 dB at {cutoff} Hz sings at {hz} Hz, not {warped}"
            );
            assert!(hz / cutoff > 1.6, "18 dB at {cutoff} Hz: {hz} Hz");
        }
    }

    /// The 12 dB slope never sings. At resonance 1 its peak, at √7 times
    /// the cutoff, is about 4 dB above its own gain at DC.
    #[test]
    fn ladder_12_db_has_a_peak_but_does_not_sing() {
        let params = [(CUTOFF, 500.0), (RESONANCE, 1.0), (SLOPE, 0.0)];
        let out = ladder(&params, &click());
        assert!(settled_rms(&out) < 1e-6, "rings out: {}", settled_rms(&out));
        let dc = ladder(&params, &vec![0.01; RATE as usize]);
        let dc_db = 20.0 * (dc[dc.len() - 1] / 0.01).log10();
        // Without gain compensation, DC is 1 / (1 + 8): about -19 dB.
        assert!((dc_db + 19.08).abs() < 0.1, "{dc_db}");
        let peak = ladder_gain_db(&params, 500.0 * 7f32.sqrt()) - dc_db;
        assert!((peak - 4.0).abs() < 0.3, "peak {peak} dB");
    }

    /// Resonance thins the low end, with no gain compensation.
    #[test]
    fn ladder_resonance_thins_the_bass() {
        let dc_gain = |resonance| {
            let out = ladder(&[(CUTOFF, 1_000.0), (RESONANCE, resonance)], &vec![0.01; 4_800]);
            out[out.len() - 1] / 0.01
        };
        assert!((dc_gain(0.0) - 1.0).abs() < 1e-3);
        // k = 0.5 × 4.2 = 2.1, so the DC gain is 1 / 3.1.
        assert!((dc_gain(0.5) - 1.0 / 3.1).abs() < 1e-3, "{}", dc_gain(0.5));
    }

    /// Drive raises the input level from 1 to 8 times, even in dB.
    #[test]
    fn ladder_drive_raises_the_level() {
        let gain = |drive| {
            let out = ladder(&[(RESONANCE, 0.0), (DRIVE, drive)], &vec![0.01; 4_800]);
            out[out.len() - 1] / 0.01
        };
        assert!((gain(0.0) - 1.0).abs() < 1e-3);
        assert!((gain(1.0) - 8.0).abs() < 1e-2, "{}", gain(1.0));
        assert!((gain(1.0 / 3.0) - 2.0).abs() < 1e-2, "{}", gain(1.0 / 3.0));
    }

    /// The output stays finite for any input: huge values, NaN, and
    /// infinity on the input, at any setting.
    #[test]
    fn ladder_stays_finite() {
        let mut input = sine(300.0);
        for (i, v) in input.iter_mut().enumerate() {
            *v *= match i % 4_000 {
                0..1_000 => 1e30,
                1_000..1_010 => f32::NAN,
                1_010..1_020 => f32::INFINITY,
                2_000..2_010 => f32::NEG_INFINITY,
                _ => 50.0,
            };
        }
        for slope in [0.0, 1.0, 2.0] {
            for drive in [0.0, 1.0] {
                for cutoff in [20.0, 20_000.0] {
                    let out = ladder(
                        &[(CUTOFF, cutoff), (RESONANCE, 1.0), (DRIVE, drive), (SLOPE, slope)],
                        &input,
                    );
                    assert!(
                        out.iter().all(|v| v.is_finite()),
                        "slope {slope}, drive {drive}, {cutoff} Hz"
                    );
                }
            }
        }
    }

    /// During a slope change the feedback gain never passes the limit of a
    /// tap that is heard, never jumps, and ends at the new slope's limit.
    #[test]
    fn ladder_slope_change_keeps_the_gain_under_every_heard_limit() {
        for (from, to) in [(0, 2), (2, 0), (1, 2), (2, 1), (0, 1), (1, 0)] {
            let mut ladder = Ladder::new(&ModuleConfig {
                sample_rate: RATE,
                max_block: 64,
            });
            ladder.set_param(SLOPE, from as f32);
            ladder.reset();
            ladder.change_slope(to);
            let mut last = ladder.limit;
            for _ in 0..2_000 {
                let k = ladder.step_slope();
                for (tap, &w) in ladder.weights.iter().enumerate() {
                    assert!(w == 0.0 || k <= LADDER_K_MAX[tap], "{from} to {to}: k {k} on tap {tap}");
                }
                assert!((k - last).abs() <= ladder.limit_step * 1.001, "{from} to {to}: jump");
                last = k;
            }
            assert_eq!(ladder.weights[to], 1.0, "{from} to {to}");
            assert_eq!(last, LADDER_K_MAX[to], "{from} to {to}");
        }
    }

    /// Changing the slope while a note plays is click-free and does not
    /// burst into oscillation: no step in the output is bigger than the
    /// biggest step of a steady slope, and the level stays bounded.
    #[test]
    fn ladder_slope_change_is_click_free() {
        let input: Vec<f32> = sine(110.0).iter().map(|v| v * 0.5).collect();
        let block = 4_800;
        for resonance in [0.5, 0.95, 1.0] {
            let params = [(CUTOFF, 800.0), (RESONANCE, resonance), (SLOPE, 1.0)];
            let mut filter = make(&LADDER, &params);
            let mut out = Vec::new();
            // 18, 24, 12, 24, 18 dB, then 24 dB for only 1 ms, then 12 dB.
            for (n, slope) in [1.0, 2.0, 0.0, 2.0, 1.0, 2.0, 0.0].into_iter().enumerate() {
                filter.set_param(SLOPE, slope);
                let len = if n == 5 { 48 } else { block };
                let start = out.len();
                out.extend(ladder_run(filter.as_mut(), &input[start..start + len]));
            }
            assert!(out.iter().all(|v| v.is_finite()));
            let steady = [0.0, 1.0, 2.0]
                .map(|slope| {
                    let out = ladder(
                        &[(CUTOFF, 800.0), (RESONANCE, resonance), (SLOPE, slope)],
                        &input[..block * 2],
                    );
                    max_step(&out[block / 2..])
                })
                .into_iter()
                .fold(0.0, f32::max);
            let step = max_step(&out[block / 2..]);
            assert!(
                step <= steady * 1.1,
                "resonance {resonance}: step {step}, steady {steady}"
            );
            let peak = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(peak < 1.5, "resonance {resonance}: peak {peak}");
        }
    }
}
