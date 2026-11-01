//! Phase generator: the timing source for oscillators (FR-LIB-01).
//!
//! Why a separate phase module: separating timing from wave shape lets one
//! ramp drive any shaper, and lets users bend time (sync, phase distortion,
//! step indexing) before it becomes sound.

use loom_core::signal::{EdgeDetector, WrapDetector, pitch, trigger};
use loom_core::{
    Channels, DisplayKind, DisplaySpec, Module, ModuleConfig, ModuleDescriptor, ModuleRole, Panel, ParamOptions,
    ParamSpec, PortDefault, PortIo, PortSpec, ProcessContext, SignalType, TriggerFlag, Widget,
};

/// `loom.phase.generator`: a ramp from 0 to 1 at the frequency set by pitch.
pub static PHASE: ModuleDescriptor = ModuleDescriptor {
    id: "loom.phase.generator",
    version: 1,
    name: "Phase",
    category: "Phase",
    family: loom_core::Family::TimeAndRhythm,
    description: "Ramp from 0 to 1 at the pitch frequency. Drives oscillators and LFOs.",
    inputs: &[
        PortSpec {
            name: "pitch",
            label: "Pitch",
            signal: SignalType::Pitch,
            description: "Sets the frequency. 0 is middle C, and +0.1 is one octave up.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::Downmix,
        },
        PortSpec {
            name: "reset",
            label: "Reset",
            signal: SignalType::Gate,
            description: "A rising edge sets the phase back to 0.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::Downmix,
        },
    ],
    outputs: &[PortSpec {
        name: "phase",
        label: "Phase",
        signal: SignalType::Phase,
        description: "A ramp from 0 up to 1, once per cycle.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[
        ParamSpec {
            name: "tune",
            label: "Tune",
            description: "Shifts the pitch in semitones.",
            min: -48.0,
            max: 48.0,
            default: 0.0,
            unit: "st",
            options: ParamOptions::DEFAULT,
        },
        ParamSpec {
            name: "octave",
            label: "Octave",
            description: "Shifts the pitch in octaves.",
            min: -10.0,
            max: 10.0,
            default: 0.0,
            unit: "oct",
            options: ParamOptions::DEFAULT,
        },
    ],
    role: ModuleRole::Dsp(|config| Box::new(Phase::new(config))),
    panel: Panel::new(6, 3, &[Widget::knob(0, 0, "tune"), Widget::knob(3, 0, "octave")]),
    displays: &[],
};

#[derive(Clone)]
struct Phase {
    phase: f64,
    sample_rate: f64,
    tune: f32,
    octave: f32,
    reset_in: EdgeDetector,
}

impl Phase {
    fn new(config: &ModuleConfig) -> Self {
        Self {
            phase: 0.0,
            sample_rate: f64::from(config.sample_rate),
            tune: 0.0,
            octave: 0.0,
            reset_in: EdgeDetector::new(),
        }
    }
}

/// Largest phase step per sample: just under half a cycle (Nyquist).
///
/// Not exactly 0.5: then the phase would read 0, 0.5, 0, 0.5, and no step
/// would be more than half a cycle, so `WrapDetector` would see no wraps
/// and a Phase Trigger would stop firing. At 0.49 every wrap is a jump of
/// 0.51, clear of `f32` rounding.
const MAX_STEP: f64 = 0.49;

/// Phase step per sample for a frequency, at most [`MAX_STEP`].
///
/// A high pitch overflows `to_hz` to infinity, and `inf - floor(inf)` is
/// NaN. Without this limit the phase would stay NaN for good, so a patch
/// stays silent even after the pitch comes back (#121). Frequencies near
/// Nyquist and above cannot be heard as themselves anyway. NaN pitch stops
/// the phase.
fn increment(hz: f32, sample_rate: f64) -> f64 {
    let step = f64::from(hz) / sample_rate;
    if step.is_nan() { 0.0 } else { step.min(MAX_STEP) }
}

impl Module for Phase {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let tune = io.param(0);
        let octave = io.param(1);
        let ([pitch_in, reset], [out]) = io.split::<2, 1>();
        for (i, ((out, &p), &r)) in out.iter_mut().zip(pitch_in).zip(reset).enumerate() {
            let offset =
                tune.at_or(i, self.tune) * pitch::PER_SEMITONE + octave.at_or(i, self.octave) * pitch::PER_OCTAVE;
            if self.reset_in.rising(r) {
                self.phase = 0.0;
            }

            *out = self.phase as f32;
            self.phase += increment(pitch::to_hz(p + offset), self.sample_rate);
            self.phase -= self.phase.floor();
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            0 => self.tune = value,
            1 => self.octave = value.round(),
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.phase = 0.0;
        self.reset_in.reset();
    }
}

/// `loom.phase.trigger`: fires a trigger each time its input phase wraps.
///
/// It turns any phase into triggers. With Phase Scale in front, it is a
/// clock multiplier that stays locked to its input. The wrap rule is
/// [`WrapDetector`]'s, shared with every end-of-cycle trigger.
pub static PHASE_TRIGGER: ModuleDescriptor = ModuleDescriptor {
    id: "loom.phase.trigger",
    version: 1,
    name: "Phase Trigger",
    category: "Phase",
    family: loom_core::Family::TimeAndRhythm,
    description: "Fires a trigger each time the phase wraps, forward or backward.",
    inputs: &[PortSpec {
        name: "phase",
        label: "Phase",
        signal: SignalType::Phase,
        description: "The phase to watch. Each wrap fires a trigger.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::Downmix,
    }],
    outputs: &[PortSpec {
        name: "trigger",
        label: "Trigger",
        signal: SignalType::Gate,
        description: "Fires once each time the phase wraps.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[],
    role: ModuleRole::Dsp(|_| {
        Box::new(PhaseTrigger {
            wrap: WrapDetector::new(),
            fired: TriggerFlag::default(),
        })
    }),
    panel: Panel::new(3, 2, &[Widget::light(0, 0, "fired")]),
    displays: &[DisplaySpec {
        name: "fired",
        label: "Fired",
        description: "Flashes each time the phase wraps.",
        kind: DisplayKind::Trigger,
    }],
};

#[derive(Clone)]
struct PhaseTrigger {
    wrap: WrapDetector,
    /// Whether the output fired since the light last read it.
    fired: TriggerFlag,
}

impl Module for PhaseTrigger {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let ([phase], [out]) = io.split::<1, 1>();
        for (out, &p) in out.iter_mut().zip(phase) {
            let fired = self.wrap.wrapped(p);
            self.fired.record(fired);
            *out = trigger(fired);
        }
    }

    fn displays(&self, values: &mut [f32]) {
        values[0] = self.fired.take();
    }

    fn reset(&mut self) {
        self.wrap.reset();
        self.fired.clear();
    }
}

/// `loom.phase.wrap` (MT-PHASE-08): brings any number into the phase range.
pub static WRAP: ModuleDescriptor = ModuleDescriptor {
    id: "loom.phase.wrap",
    version: 1,
    name: "Phase Wrap and Fold",
    category: "Phase",
    family: loom_core::Family::TimeAndRhythm,
    description: "Brings any signal into the phase range, 0 to 1. Wrap goes round the edge; fold turns back at the edges.",
    inputs: &[PortSpec {
        name: "in",
        label: "In",
        signal: SignalType::Modulation,
        description: "The signal to bring into the phase range.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Phase,
        description: "A phase from 0 to 1.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[ParamSpec {
        name: "mode",
        label: "Mode",
        description: "Wrap goes round the edge. Fold turns back at the edges.",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
        options: ParamOptions::DEFAULT,
    }
    .integer()
    .choices(&["Wrap", "Fold"])],
    role: ModuleRole::Dsp(|_| Box::new(Wrap { fold: false })),
    panel: Panel::new(5, 2, &[Widget::segmented(0, 0, 5, "mode")]),
    displays: &[],
};

struct Wrap {
    fold: bool,
}

impl Wrap {
    /// `x - floor(x)`, always in `[0, 1)`.
    fn wrap(x: f32) -> f32 {
        let w = x - x.floor();
        // A tiny negative `x` can round the result up to exactly 1.
        if w >= 1.0 { 0.0 } else { w }
    }

    /// A triangle fold into `[0, 1]`: 1 stays 1, 1.25 comes back to 0.75.
    fn fold(x: f32) -> f32 {
        let t = x - 2.0 * (x / 2.0).floor();
        let folded = if t <= 1.0 { t } else { 2.0 - t };
        folded.clamp(0.0, 1.0)
    }
}

impl Module for Wrap {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let mode_mod = io.param(0);
        let ([input], [out]) = io.split::<1, 1>();
        for (i, (out, &x)) in out.iter_mut().zip(input).enumerate() {
            // A modulated mode counts for this frame only; the base stays
            // in the field.
            let fold = mode_mod.map_or(i, self.fold, |v| v >= 0.5);
            // A phase module must not pass NaN on to oscillators.
            *out = if !x.is_finite() {
                0.0
            } else if fold {
                Self::fold(x)
            } else {
                Self::wrap(x)
            };
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if index == 0 {
            self.fold = value >= 0.5;
        }
    }
}

/// `loom.phase.scale` (MT-PHASE-02): runs a phase faster or slower by a
/// whole-number ratio, and stays locked to it.
pub static SCALE: ModuleDescriptor = ModuleDescriptor {
    id: "loom.phase.scale",
    version: 1,
    name: "Phase Scale",
    category: "Phase",
    family: loom_core::Family::TimeAndRhythm,
    description: "Runs a phase at multiply / divide times its speed, locked to the input. Divide counts the input's wraps from the first cycle or the last reset.",
    inputs: &[
        PortSpec {
            name: "phase",
            label: "Phase",
            signal: SignalType::Phase,
            description: "The phase to scale.",
            // An unpatched Phase Scale follows the bar, so it works as a
            // clock multiplier with no cables. A flat 0 would never wrap.
            default: PortDefault::BarPhase,
            range: loom_core::PortRange::Signal,
            channels: Channels::Downmix,
        },
        PortSpec {
            name: "reset",
            label: "Reset",
            signal: SignalType::Gate,
            description: "A rising edge starts counting the input's wraps again from zero.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::Downmix,
        },
    ],
    outputs: &[PortSpec {
        name: "phase",
        label: "Phase",
        signal: SignalType::Phase,
        description: "The scaled phase, from 0 to 1.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[
        ParamSpec {
            name: "multiply",
            label: "Multiply",
            description: "How many times faster the output runs.",
            min: 1.0,
            max: 16.0,
            default: 1.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer(),
        ParamSpec {
            name: "divide",
            label: "Divide",
            description: "How many times slower the output runs: it wraps once every this many input wraps.",
            min: 1.0,
            max: 16.0,
            default: 1.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer(),
    ],
    role: ModuleRole::Dsp(|_| Box::new(PhaseScale::new())),
    panel: Panel::new(6, 3, &[Widget::knob(0, 0, "multiply"), Widget::knob(3, 0, "divide")]),
    displays: &[],
};

#[derive(Clone)]
struct PhaseScale {
    multiply: i64,
    divide: i64,
    /// The previous input phase. `None` before the first frame.
    previous: Option<f32>,
    /// Wraps of the input since the first cycle or the last reset. Taken
    /// modulo `divide`, so a changed divide still lines up.
    wraps: i64,
    reset: EdgeDetector,
}

impl PhaseScale {
    fn new() -> Self {
        Self {
            multiply: 1,
            divide: 1,
            previous: None,
            wraps: 0,
            reset: EdgeDetector::new(),
        }
    }
}

impl Module for PhaseScale {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let (multiply_mod, divide_mod) = (io.param(0), io.param(1));
        let ([phase, reset], [out]) = io.split::<2, 1>();
        for (i, out) in out.iter_mut().enumerate() {
            // A modulated value counts for this frame only. The base stays
            // in the field, so removing the assignment returns to it.
            let multiply = multiply_mod.map_or(i, self.multiply, |v| v.round().max(1.0) as i64);
            let divide = divide_mod.map_or(i, self.divide, |v| v.round().max(1.0) as i64);
            let p = if phase[i].is_finite() { phase[i] } else { 0.0 };
            // A jump of more than half a cycle is a wrap, as in Phase
            // Trigger. Going forward adds a cycle and going backward takes
            // one away. The first frame has nothing to compare with.
            if let Some(previous) = self.previous {
                let step = p - previous;
                if step < -0.5 {
                    self.wraps += 1;
                } else if step > 0.5 {
                    self.wraps -= 1;
                }
            }
            self.previous = Some(p);
            // Reset on the same frame as a wrap: the reset comes first, so
            // this frame is the first one of the new count.
            if self.reset.rising(reset[i]) {
                self.wraps = 0;
            }
            // The cycle we are in, among `divide` of them, plus how far
            // through the input cycle we are. Done in f64, so a long run
            // does not lose the fraction.
            let cycle = self.wraps.rem_euclid(divide) as f64;
            let scaled = (cycle + f64::from(p)) * multiply as f64 / divide as f64;
            let wrapped = (scaled - scaled.floor()) as f32;
            // An f32 can round up to exactly 1.
            *out = if wrapped >= 1.0 { 0.0 } else { wrapped };
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            0 => self.multiply = value.round().max(1.0) as i64,
            1 => self.divide = value.round().max(1.0) as i64,
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.previous = None;
        self.wraps = 0;
        self.reset.reset();
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod wrap_tests;

#[cfg(test)]
mod scale_tests;
