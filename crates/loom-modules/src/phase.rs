//! Phase generator: the timing source for oscillators (FR-LIB-01).
//!
//! Why a separate phase module: separating timing from wave shape lets one
//! ramp drive any shaper, and lets users bend time (sync, phase distortion,
//! step indexing) before it becomes sound.

use loom_core::signal::{EdgeDetector, WrapDetector, pitch, trigger};
use loom_core::{
    Channels, DisplayKind, DisplayRange, DisplaySpec, Module, ModuleConfig, ModuleDescriptor, ModuleRole, Panel, Param,
    ParamOptions, ParamSpec, PortDefault, PortIo, PortSpec, ProcessContext, SignalType, TriggerFlag, Widget,
};
use loom_dsp::phase::{Phasor, wrap};
use loom_rt::Smoothed;

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
    params: &[TUNE_PARAM, OCTAVE_PARAM],
    role: ModuleRole::Dsp(|config| Box::new(Phase::new(config))),
    panel: Panel::new(6, 3, &[Widget::knob(0, 0, "tune"), Widget::knob(3, 0, "octave")]),
    displays: &[],
    roles: &[],
};

/// The `tune` parameter of Phase and of every oscillator.
pub(crate) const TUNE_PARAM: ParamSpec = ParamSpec {
    name: "tune",
    label: "Tune",
    description: "Shifts the pitch in semitones.",
    min: -48.0,
    max: 48.0,
    default: 0.0,
    unit: "st",
    options: ParamOptions::DEFAULT,
};

/// The `octave` parameter of Phase and of every oscillator.
pub(crate) const OCTAVE_PARAM: ParamSpec = ParamSpec {
    name: "octave",
    label: "Octave",
    description: "Shifts the pitch in octaves.",
    min: -10.0,
    max: 10.0,
    default: 0.0,
    unit: "oct",
    options: ParamOptions::DEFAULT,
};

/// A phase that runs at a pitch, shifted by a tune in semitones and an
/// octave. Phase runs one, and so does every oscillator while its phase
/// input has no cable (oscillators.md, rule 1). Sharing it means an
/// oscillator's own phase is the same, bit for bit, as a Phase module in
/// front of it.
#[derive(Clone)]
pub(crate) struct PitchedPhase {
    phasor: Phasor,
    sample_rate: f32,
    /// The `tune` knob, in semitones.
    tune: f32,
    /// The `octave` knob, a whole number.
    octave: f32,
}

impl PitchedPhase {
    /// A phase at 0, with both knobs at 0.
    pub(crate) fn new(sample_rate: f32) -> Self {
        Self {
            phasor: Phasor::new(),
            sample_rate,
            tune: 0.0,
            octave: 0.0,
        }
    }

    /// The phase for frame `i`, then one step on at `pitch` plus the knobs.
    /// A modulated knob counts for this frame only, and a modulated octave
    /// rounds to a whole octave as the knob does.
    #[inline]
    pub(crate) fn tick(&mut self, i: usize, pitch_in: f32, tune: Param<'_>, octave: Param<'_>) -> f32 {
        let octave = octave.map_or(i, self.octave, f32::round);
        let offset = tune.at_or(i, self.tune) * pitch::PER_SEMITONE + octave * pitch::PER_OCTAVE;
        // A high pitch overflows `to_hz` to infinity, and `step_for` limits
        // that, so a patch recovers when the pitch comes back (#121).
        self.phasor
            .tick(Phasor::step_for(pitch::to_hz(pitch_in + offset), self.sample_rate))
    }

    /// Sets the tune knob, in semitones.
    pub(crate) fn set_tune(&mut self, semitones: f32) {
        self.tune = semitones;
    }

    /// Sets the octave knob. It rounds to a whole octave.
    pub(crate) fn set_octave(&mut self, octaves: f32) {
        self.octave = octaves.round();
    }

    /// Sets the phase back to 0.
    pub(crate) fn reset(&mut self) {
        self.phasor.reset();
    }
}

#[derive(Clone)]
struct Phase {
    phase: PitchedPhase,
    reset_in: EdgeDetector,
}

impl Phase {
    const TUNE: usize = PHASE.param_index("tune").unwrap();
    const OCTAVE: usize = PHASE.param_index("octave").unwrap();

    fn new(config: &ModuleConfig) -> Self {
        Self {
            phase: PitchedPhase::new(config.sample_rate),
            reset_in: EdgeDetector::new(),
        }
    }
}

impl Module for Phase {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let tune = io.param(Self::TUNE);
        let octave = io.param(Self::OCTAVE);
        let ([pitch_in, reset], [out]) = io.split::<2, 1>();
        for (i, ((out, &p), &r)) in out.iter_mut().zip(pitch_in).zip(reset).enumerate() {
            if self.reset_in.rising(r) {
                self.phase.reset();
            }
            *out = self.phase.tick(i, p, tune, octave);
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            Self::TUNE => self.phase.set_tune(value),
            Self::OCTAVE => self.phase.set_octave(value),
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.phase.reset();
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
        range: DisplayRange::UNIT,
    }],
    roles: &[],
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
    roles: &[],
};

struct Wrap {
    fold: bool,
}

impl Wrap {
    const MODE: usize = WRAP.param_index("mode").unwrap();

    /// A triangle fold into `[0, 1]`: 1 stays 1, 1.25 comes back to 0.75.
    fn fold(x: f32) -> f32 {
        let t = x - 2.0 * (x / 2.0).floor();
        let folded = if t <= 1.0 { t } else { 2.0 - t };
        folded.clamp(0.0, 1.0)
    }
}

impl Module for Wrap {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let mode_mod = io.param(Self::MODE);
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
                wrap(x)
            };
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if index == Self::MODE {
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
    panel: Panel::new(
        10,
        2,
        &[Widget::number(0, 0, "multiply"), Widget::number(5, 0, "divide")],
    ),
    displays: &[],
    roles: &[],
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
    const MULTIPLY: usize = SCALE.param_index("multiply").unwrap();
    const DIVIDE: usize = SCALE.param_index("divide").unwrap();

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
        let (multiply_mod, divide_mod) = (io.param(Self::MULTIPLY), io.param(Self::DIVIDE));
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
            Self::MULTIPLY => self.multiply = value.round().max(1.0) as i64,
            Self::DIVIDE => self.divide = value.round().max(1.0) as i64,
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.previous = None;
        self.wraps = 0;
        self.reset.reset();
    }
}

/// `loom.phase.reset` (MT-PHASE-04): restarts a phase at zero on a trigger.
pub static RESET: ModuleDescriptor = ModuleDescriptor {
    id: "loom.phase.reset",
    version: 1,
    name: "Phase Reset",
    category: "Phase",
    family: loom_core::Family::TimeAndRhythm,
    description: "Restarts a phase at zero on a trigger. After that, the phase moves at the input's speed.",
    inputs: &[
        PortSpec {
            name: "phase",
            label: "Phase",
            signal: SignalType::Phase,
            description: "The phase to restart. With no cable, it follows the bar.",
            // An unpatched Phase Reset restarts the bar phase from a
            // trigger, and holds when the transport stops.
            default: PortDefault::BarPhase,
            range: loom_core::PortRange::Signal,
            channels: Channels::Downmix,
        },
        PortSpec {
            name: "reset",
            label: "Reset",
            signal: SignalType::Gate,
            description: "A rising edge makes the output 0 on that frame.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::Downmix,
        },
    ],
    outputs: &[PortSpec {
        name: "phase",
        label: "Phase",
        signal: SignalType::Phase,
        description: "The input phase, counted from the last reset.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[],
    role: ModuleRole::Dsp(|_| {
        Box::new(PhaseReset {
            offset: 0.0,
            edge: EdgeDetector::new(),
        })
    }),
    panel: Panel::new(2, 2, &[]),
    displays: &[],
    roles: &[],
};

/// State for `loom.phase.reset`: `offset` is the input's wrapped value at the
/// last reset. The module keeps a distance from the input, not a phase of its
/// own, so it can never run at another speed or drift away from the input.
#[derive(Clone)]
struct PhaseReset {
    offset: f32,
    edge: EdgeDetector,
}

impl Module for PhaseReset {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let ([phase, reset], [out]) = io.split::<2, 1>();
        for ((out, &phase), &reset) in out.iter_mut().zip(phase).zip(reset) {
            let restart = self.edge.rising(reset);
            // A phase module must not pass NaN on to oscillators, and a bad
            // value must not stay in the offset.
            if !phase.is_finite() {
                *out = 0.0;
                continue;
            }
            // Cables are not limited to 0 to 1. Wrapping first keeps both
            // values small, so a huge input can neither overflow here nor
            // leave an offset so large that later fractions are lost.
            let phase = wrap(phase);
            if restart {
                self.offset = phase;
            }
            *out = wrap(phase - self.offset);
        }
    }

    fn reset(&mut self) {
        self.offset = 0.0;
        self.edge.reset();
    }
}

/// `loom.phase.shift` (MT-PHASE-05, offset): moves a phase along its cycle, so
/// a Phase Trigger after it fires at a chosen point of the cycle.
pub static SHIFT: ModuleDescriptor = ModuleDescriptor {
    id: "loom.phase.shift",
    version: 1,
    name: "Phase Shift",
    category: "Phase",
    family: loom_core::Family::TimeAndRhythm,
    description: "Moves a phase along its cycle, at the same speed. Put it before a Phase Trigger to fire at a chosen point.",
    inputs: &[
        PortSpec {
            name: "phase",
            label: "Phase",
            signal: SignalType::Phase,
            description: "The phase to move. With no cable, it follows the bar.",
            default: PortDefault::BarPhase,
            range: loom_core::PortRange::Signal,
            channels: Channels::Downmix,
        },
        PortSpec {
            name: "offset",
            label: "Offset",
            signal: SignalType::Modulation,
            description: "Added to the Offset knob, in cycles.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::Downmix,
        },
    ],
    outputs: &[PortSpec {
        name: "phase",
        label: "Phase",
        signal: SignalType::Phase,
        description: "The input phase, moved by the offset and wrapped to 0 to 1.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[ParamSpec {
        name: "offset",
        label: "Offset",
        description: "How far to move the phase, in cycles.",
        min: -1.0,
        max: 1.0,
        default: 0.0,
        unit: "",
        options: ParamOptions::DEFAULT,
    }
    .smooth(0.005)],
    role: ModuleRole::Dsp(|config| {
        Box::new(PhaseShift {
            offset: Smoothed::new(
                0.0,
                config.sample_rate,
                SHIFT.params[PhaseShift::OFFSET].options.smooth_seconds,
            ),
        })
    }),
    panel: Panel::new(3, 3, &[Widget::knob(0, 0, "offset")]),
    displays: &[],
    roles: &[],
};

/// State for `loom.phase.shift`: only the knob's glide.
#[derive(Clone)]
struct PhaseShift {
    offset: Smoothed,
}

impl PhaseShift {
    const OFFSET: usize = SHIFT.param_index("offset").unwrap();
}

impl Module for PhaseShift {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let offset_param = io.param(Self::OFFSET);
        let ([phase, offset_in], [out]) = io.split::<2, 1>();
        for (i, out) in out.iter_mut().enumerate() {
            // Tick even when modulated, so removing the assignment continues
            // the glide from here.
            let offset = offset_param.at_or(i, self.offset.tick());
            // A bad offset is no offset. Each term is wrapped first, so huge
            // values cannot overflow the sum or lose its fraction.
            let moved = if offset_in[i].is_finite() {
                wrap(offset_in[i])
            } else {
                0.0
            };
            *out = if phase[i].is_finite() {
                wrap(wrap(phase[i]) + offset + moved)
            } else {
                0.0
            };
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if index == Self::OFFSET {
            self.offset.set_target(value);
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        if index == Self::OFFSET {
            self.offset.gliding()
        } else {
            None
        }
    }

    fn reset(&mut self) {
        self.offset.reset(self.offset.target());
    }
}

/// `loom.phase.reverse` (MT-PHASE-05, reverse): runs a phase backward.
pub static REVERSE: ModuleDescriptor = ModuleDescriptor {
    id: "loom.phase.reverse",
    version: 1,
    name: "Phase Reverse",
    category: "Phase",
    family: loom_core::Family::TimeAndRhythm,
    description: "Runs a phase backward, at the same speed. An input of 0 stays 0, so the cycles still line up.",
    inputs: &[PortSpec {
        name: "phase",
        label: "Phase",
        signal: SignalType::Phase,
        description: "The phase to reverse. With no cable, it follows the bar.",
        default: PortDefault::BarPhase,
        range: loom_core::PortRange::Signal,
        channels: Channels::Downmix,
    }],
    outputs: &[PortSpec {
        name: "phase",
        label: "Phase",
        signal: SignalType::Phase,
        description: "The input phase, running from 1 down to 0.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[],
    role: ModuleRole::Dsp(|_| Box::new(PhaseReverse)),
    panel: Panel::new(2, 2, &[]),
    displays: &[],
    roles: &[],
};

/// `loom.phase.reverse` has no state.
struct PhaseReverse;

impl Module for PhaseReverse {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let ([phase], [out]) = io.split::<1, 1>();
        for (out, &phase) in out.iter_mut().zip(phase) {
            // An input of 0 gives 0, so Phase Trigger's first-frame rule keeps
            // working on a reversed phase.
            *out = if phase.is_finite() {
                wrap(1.0 - wrap(phase))
            } else {
                0.0
            };
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod wrap_tests;

#[cfg(test)]
mod scale_tests;

#[cfg(test)]
mod reset_tests;

#[cfg(test)]
mod shift_tests;
