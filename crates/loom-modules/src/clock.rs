//! Clock: a phase, trigger, and gate at a musical rate (MT-CLOCK-02,
//! FR-LIB-27).
//!
//! The phase is computed from the transport's absolute position, never by
//! adding a step per frame. So it stays aligned with bar 1 after a stop, a
//! tempo change, or a reset, and it is the same at any block size.
//! Spec: `docs/design/clock.md`, the Clock row of Modules and Clock details.
//! Background: `docs/learn/phase-and-cycles.md`.

use loom_core::signal::{WrapDetector, trigger};
use loom_core::{
    Channels, DisplayKind, DisplaySpec, Module, ModuleDescriptor, ModuleRole, Panel, ParamOptions, ParamSpec,
    PortDefault, PortIo, PortSpec, ProcessContext, SignalType, TriggerFlag, Widget,
};

/// The rate names, in order from 0. Each length comes straight, dotted, and
/// triplet, from 4 bars down to 1/32.
const RATE_NAMES: &[&str] = &[
    "4 bars",
    "4 bars dotted",
    "4 bars triplet",
    "2 bars",
    "2 bars dotted",
    "2 bars triplet",
    "1 bar",
    "1 bar dotted",
    "1 bar triplet",
    "1/2",
    "1/2 dotted",
    "1/2 triplet",
    "1/4",
    "1/4 dotted",
    "1/4 triplet",
    "1/8",
    "1/8 dotted",
    "1/8 triplet",
    "1/16",
    "1/16 dotted",
    "1/16 triplet",
    "1/32",
    "1/32 dotted",
    "1/32 triplet",
];

/// Index of the default rate, 1/4.
const QUARTER: usize = 12;

/// `loom.clock.clock`: a phase at a musical rate, read from the transport,
/// with a trigger on each wrap and a gate for the first part of each cycle.
pub static CLOCK: ModuleDescriptor = ModuleDescriptor {
    id: "loom.clock.clock",
    version: 1,
    name: "Clock",
    category: "Clock",
    family: loom_core::Family::TimeAndRhythm,
    description: "A phase, trigger, and gate at a musical rate, locked to the transport's bar 1.",
    inputs: &[],
    outputs: &[
        PortSpec {
            name: "phase",
            label: "Phase",
            signal: SignalType::Phase,
            description: "Rises from 0 to 1 once per cycle of the rate, counted from bar 1.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "trigger",
            label: "Trigger",
            signal: SignalType::Gate,
            description: "Fires once at the start of each cycle while the transport runs.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "gate",
            label: "Gate",
            signal: SignalType::Gate,
            description: "High for the first part of each cycle, as long as the gate width says.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::PerChannel,
        },
    ],
    params: &[
        ParamSpec {
            name: "rate",
            label: "Rate",
            description: "The length of one cycle: 4 bars to 1/32, straight, dotted, or triplet.",
            min: 0.0,
            max: 23.0,
            default: QUARTER as f32,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer()
        .choices(RATE_NAMES),
        ParamSpec {
            name: "width",
            label: "Width",
            description: "How much of each cycle the gate is high.",
            min: 1.0,
            max: 99.0,
            default: 50.0,
            unit: "%",
            options: ParamOptions::DEFAULT,
        },
    ],
    role: ModuleRole::Dsp(|_| Box::new(Clock::new())),
    panel: Panel::new(
        9,
        3,
        &[
            Widget::knob(0, 0, "rate"),
            Widget::knob(3, 0, "width"),
            Widget::light(6, 0, "fired"),
        ],
    ),
    displays: &[DisplaySpec {
        name: "fired",
        label: "Fired",
        description: "Flashes on each trigger.",
        kind: DisplayKind::Trigger,
    }],
};

/// Index of `rate`.
const RATE: usize = CLOCK.param_index("rate").unwrap();
/// Index of `width`.
const WIDTH: usize = CLOCK.param_index("width").unwrap();

/// The length of one cycle of rate `index`, in quarter-note beats.
///
/// Bar rates follow the time signature; note rates do not, so 1/4 is one
/// beat in any metre. Dotted is 3/2 of straight, triplet 2/3.
fn rate_beats(index: usize, beats_per_bar: f64) -> f64 {
    let straight = match index / 3 {
        0 => 4.0 * beats_per_bar,
        1 => 2.0 * beats_per_bar,
        2 => beats_per_bar,
        3 => 2.0,
        4 => 1.0,
        5 => 0.5,
        6 => 0.25,
        _ => 0.125,
    };
    match index % 3 {
        0 => straight,
        1 => straight * 1.5,
        _ => straight * 2.0 / 3.0,
    }
}

/// A rate parameter value as an index into [`RATE_NAMES`]. NaN gives 0.
fn rate_index(value: f32) -> usize {
    (value.round().max(0.0) as usize).min(RATE_NAMES.len() - 1)
}

/// State for `loom.clock.clock`.
///
/// The phase needs no state: it comes from the transport. The state is for
/// the trigger, which must know the previous frame.
#[derive(Clone)]
struct Clock {
    rate: usize,
    width: f32,
    /// The rate the last frame used. A change re-arms the wrap detector.
    active_rate: usize,
    wrap: WrapDetector,
    /// Whether the trigger fired since the light last read it.
    fired: TriggerFlag,
}

impl Clock {
    fn new() -> Self {
        Self {
            rate: QUARTER,
            width: 50.0,
            active_rate: QUARTER,
            wrap: WrapDetector::new(),
            fired: TriggerFlag::default(),
        }
    }
}

impl Module for Clock {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let transport = ctx.transport();
        let playing = transport.playing();
        let beats_per_bar = transport.beats_per_bar();
        // A jump, or a stopped transport, starts the wrap rule afresh: the
        // next playing frame fires only when it is exactly on a cycle start.
        // So a reset fires once on bar 1 (rule 8), and starting again after
        // a stop does not fire a trigger halfway through a cycle.
        if transport.jumped() || !playing {
            self.wrap.reset();
        }
        let (rate_mod, width_mod) = (io.param(RATE), io.param(WIDTH));
        let ([], [phase_out, trigger_out, gate_out]) = io.split::<0, 3>();
        for i in 0..phase_out.len() {
            // A modulated value counts for this frame only.
            let rate = rate_mod.map_or(i, self.rate, rate_index);
            if rate != self.active_rate {
                // A new rate moves the phase at once. Re-arm, so the move
                // itself never fires a trigger.
                self.active_rate = rate;
                self.wrap.reset();
            }
            let phase = transport.phase_at(i, rate_beats(rate, beats_per_bar));
            let fired = playing && self.wrap.wrapped(phase);
            self.fired.record(fired);
            let width = width_mod.at_or(i, self.width).clamp(1.0, 99.0) * 0.01;
            phase_out[i] = phase;
            trigger_out[i] = trigger(fired);
            gate_out[i] = if phase < width { 1.0 } else { 0.0 };
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            RATE => self.rate = rate_index(value),
            WIDTH => self.width = value,
            _ => {}
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

#[cfg(test)]
mod tests;
