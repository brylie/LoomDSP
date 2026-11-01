//! Logic modules: count and route triggers for arrangement (FR-LIB-22,
//! FR-LIB-27, ADR-0012).
//!
//! Inputs count rising edges, and trigger outputs are high for one sample
//! (ADR-0007), so a logic module never misses or merges a trigger.

use loom_core::signal::{EdgeDetector, gate_is_high, trigger};
use loom_core::{
    Channels, DisplayKind, DisplaySpec, Module, ModuleDescriptor, ModuleRole, Panel, ParamOptions, ParamSpec,
    PortDefault, PortIo, PortSpec, ProcessContext, SignalType, TriggerFlag, Widget,
};

/// `loom.logic.divide`: fires on every Nth incoming trigger.
///
/// Counting starts again at each reset. It fires on the triggers numbered
/// N, 2N, 3N after a reset, and `offset` moves each firing that many
/// triggers earlier. Offset N-1 fires on the first trigger. An offset of N
/// or more counts as N-1.
pub static DIVIDE: ModuleDescriptor = ModuleDescriptor {
    id: "loom.logic.divide",
    version: 1,
    name: "Clock Divide",
    category: "Logic",
    family: loom_core::Family::TimeAndRhythm,
    description: "Fires on every Nth trigger, so a fast clock makes slower rhythms. Offset fires earlier in the count, and Reset starts the count again.",
    inputs: &[
        PortSpec {
            name: "trigger",
            label: "Trigger",
            signal: SignalType::Gate,
            description: "The triggers to count.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::Downmix,
        },
        PortSpec {
            name: "reset",
            label: "Reset",
            signal: SignalType::Gate,
            description: "Starts the count again.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::Downmix,
        },
    ],
    outputs: &[PortSpec {
        name: "trigger",
        label: "Trigger",
        signal: SignalType::Gate,
        description: "Fires on every Nth trigger.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[
        ParamSpec {
            name: "n",
            label: "Division",
            description: "Fires on every Nth trigger.",
            min: 1.0,
            max: 64.0,
            default: 2.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer(),
        ParamSpec {
            name: "offset",
            label: "Offset",
            description: "Fires this many triggers earlier in the count.",
            min: 0.0,
            max: 63.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer(),
    ],
    role: ModuleRole::Dsp(|_| Box::new(Divide::new())),
    panel: Panel::new(
        9,
        3,
        &[
            Widget::knob(0, 0, "n"),
            Widget::knob(3, 0, "offset"),
            Widget::light(6, 0, "fired"),
        ],
    ),
    displays: &[DisplaySpec {
        name: "fired",
        label: "Fired",
        description: "Flashes each time the output fires.",
        kind: DisplayKind::Trigger,
    }],
};

#[derive(Clone)]
struct Divide {
    n: u64,
    offset: u64,
    /// Triggers counted since the last reset. Kept whole, not modulo N, so
    /// changing N gives the same result as if N had been set at the reset.
    count: u64,
    trigger_in: EdgeDetector,
    reset_in: EdgeDetector,
    /// Whether the output fired since the light last read it.
    fired: TriggerFlag,
}

impl Divide {
    fn new() -> Self {
        Self {
            n: 2,
            offset: 0,
            count: 0,
            trigger_in: EdgeDetector::new(),
            reset_in: EdgeDetector::new(),
            fired: TriggerFlag::default(),
        }
    }

    /// Counts one trigger. True when this trigger fires the output.
    fn count_trigger(&mut self, n: u64, offset: u64) -> bool {
        self.count = self.count.wrapping_add(1);
        let offset = offset.min(n - 1);
        (self.count + offset).is_multiple_of(n)
    }
}

impl Module for Divide {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let n = io.param(0);
        let offset = io.param(1);
        let ([trigger_in, reset], [out]) = io.split::<2, 1>();
        for (i, ((out, &t), &r)) in out.iter_mut().zip(trigger_in).zip(reset).enumerate() {
            // Modulated values live only for this frame. The fields keep the
            // knob's base, so removing an assignment returns to it.
            let n = n.map_or(i, self.n, |v| (v as u64).max(1));
            let offset = offset.map_or(i, self.offset, |v| v as u64);
            // Reset first, so a trigger on the same frame as a reset counts
            // as the first trigger after it.
            if self.reset_in.rising(r) {
                self.count = 0;
            }
            let fired = self.trigger_in.rising(t) && self.count_trigger(n, offset);
            self.fired.record(fired);
            *out = trigger(fired);
        }
    }

    fn displays(&self, values: &mut [f32]) {
        values[0] = self.fired.take();
    }

    fn set_param(&mut self, index: usize, value: f32) {
        // The engine clamps and rounds whole-number parameters, so the
        // casts below are exact. `max(1)` guards the division all the same.
        match index {
            0 => self.n = (value as u64).max(1),
            1 => self.offset = value as u64,
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.count = 0;
        self.trigger_in.reset();
        self.reset_in.reset();
        self.fired.clear();
    }
}

/// `loom.logic.compare` (MT-LOGIC-01): a gate from comparing two signals.
pub static COMPARE: ModuleDescriptor = ModuleDescriptor {
    id: "loom.logic.compare",
    version: 1,
    name: "Compare",
    category: "Logic",
    family: loom_core::Family::LogicAndMath,
    description: "Makes a gate from two signals: high while A is above B, or while A is below B. Hysteresis keeps a noisy signal from chattering.",
    inputs: &[
        PortSpec {
            name: "a",
            label: "A",
            signal: SignalType::Modulation,
            description: "The signal to test.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::Downmix,
        },
        PortSpec {
            name: "b",
            label: "B",
            signal: SignalType::Modulation,
            description: "The threshold A is compared with.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::Downmix,
        },
    ],
    outputs: &[PortSpec {
        name: "gate",
        label: "Gate",
        signal: SignalType::Gate,
        description: "High while the comparison holds. Equal values count as low.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[
        ParamSpec {
            name: "mode",
            label: "Mode",
            description: "Above: high while A is above B. Below: high while A is below B.",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer()
        .choices(&["Above", "Below"]),
        ParamSpec {
            name: "hysteresis",
            label: "Hysteresis",
            description: "A dead band around B. Once the gate is high, A must go this far past B the other way to end it.",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        },
    ],
    role: ModuleRole::Dsp(|_| {
        Box::new(Compare {
            below: false,
            hysteresis: 0.0,
            high: false,
        })
    }),
    panel: Panel::new(
        7,
        5,
        &[Widget::segmented(0, 0, 7, "mode"), Widget::knob(2, 2, "hysteresis")],
    ),
    displays: &[],
};

/// State for `loom.logic.compare`: the mode, the dead band, and whether the
/// gate is high now (the hysteresis needs it).
#[derive(Clone)]
struct Compare {
    below: bool,
    hysteresis: f32,
    high: bool,
}

impl Module for Compare {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let (mode_mod, hysteresis_mod) = (io.param(0), io.param(1));
        let ([a, b], [gate]) = io.split::<2, 1>();
        for (i, gate) in gate.iter_mut().enumerate() {
            let below = mode_mod.map_or(i, self.below, |v| v >= 0.5);
            let hysteresis = hysteresis_mod.at_or(i, self.hysteresis).max(0.0);
            // Flip the sign for Below, and it is the Above test.
            let (a, b) = if below { (-a[i], -b[i]) } else { (a[i], b[i]) };
            // Once high, the gate stays high until A falls `hysteresis`
            // below B. A noisy signal near B cannot chatter inside the band.
            let threshold = if self.high { b - hysteresis } else { b };
            self.high = a > threshold;
            *gate = trigger(self.high);
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            0 => self.below = value >= 0.5,
            1 => self.hysteresis = value,
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.high = false;
    }
}

/// One gate input of the logic module.
const fn logic_input(name: &'static str, label: &'static str) -> PortSpec {
    PortSpec {
        name,
        label,
        signal: SignalType::Gate,
        description: "A gate to combine. An input with no cable is ignored, not counted as low.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::Downmix,
    }
}

/// `loom.logic.logic` (MT-LOGIC-02): And, Or, and Xor of up to four gates.
pub static LOGIC: ModuleDescriptor = ModuleDescriptor {
    id: "loom.logic.logic",
    version: 1,
    name: "Logic",
    category: "Logic",
    family: loom_core::Family::LogicAndMath,
    description: "Combines up to four gates with And, Or, or Xor; only inputs with a cable count. Invert gives Nand, Nor, Xnor, and Not.",
    inputs: &[
        logic_input("in1", "In 1"),
        logic_input("in2", "In 2"),
        logic_input("in3", "In 3"),
        logic_input("in4", "In 4"),
    ],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Gate,
        description: "The result: exactly 0 or 1.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[
        ParamSpec {
            name: "op",
            label: "Operation",
            description: "And: all connected inputs are high. Or: any is high. Xor: an odd number are high.",
            min: 0.0,
            max: 2.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer()
        .choices(&["And", "Or", "Xor"]),
        ParamSpec {
            name: "invert",
            label: "Invert",
            description: "Flips the result.",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer(),
    ],
    role: ModuleRole::Dsp(|_| Box::new(Logic { op: 0, invert: false })),
    panel: Panel::new(
        9,
        2,
        &[Widget::segmented(0, 0, 6, "op"), Widget::toggle(6, 0, "invert")],
    ),
    displays: &[],
};

/// State for `loom.logic.logic`: the operation and the invert flag. The
/// result is computed fresh each frame, so there is nothing else to keep.
struct Logic {
    op: u8,
    invert: bool,
}

impl Module for Logic {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let connected: [bool; 4] = std::array::from_fn(|n| io.is_connected(n));
        let (op_mod, invert_mod) = (io.param(0), io.param(1));
        let (inputs, [out]) = io.split::<4, 1>();
        for (i, out) in out.iter_mut().enumerate() {
            // Modulated values count for this frame only; the base stays in
            // the fields.
            let op = op_mod.map_or(i, self.op, |v| v.round().clamp(0.0, 2.0) as u8);
            let invert = invert_mod.map_or(i, self.invert, |v| v >= 0.5);
            let mut count = 0;
            let mut high = 0;
            for n in 0..4 {
                if connected[n] {
                    count += 1;
                    high += usize::from(gate_is_high(inputs[n][i]));
                }
            }
            // With no cable at all the result is low before `invert`.
            let result = match op {
                _ if count == 0 => false,
                0 => high == count,
                1 => high > 0,
                _ => high % 2 == 1,
            };
            *out = trigger(result != invert);
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            0 => self.op = value as u8,
            1 => self.invert = value >= 0.5,
            _ => {}
        }
    }
}

/// `loom.logic.memory` (MT-LOGIC-03): a latch that keeps a gate until told.
pub static MEMORY: ModuleDescriptor = ModuleDescriptor {
    id: "loom.logic.memory",
    version: 1,
    name: "Memory",
    category: "Logic",
    family: loom_core::Family::LogicAndMath,
    description: "Remembers a gate, in set and reset, toggle, or data (a D latch) mode. In set and reset or toggle mode, reset wins a tie.",
    inputs: &[
        PortSpec {
            name: "a",
            label: "A",
            signal: SignalType::Gate,
            description: "In Set/Reset, a rising edge turns the output on. In Toggle, a rising edge flips it. In Data, this is the level to remember.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::Downmix,
        },
        PortSpec {
            name: "b",
            label: "B",
            signal: SignalType::Gate,
            description: "A rising edge turns the output off (Set/Reset, Toggle), or is the clock that remembers A (Data).",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::Downmix,
        },
    ],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Gate,
        description: "The remembered gate. It starts low.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[ParamSpec {
        name: "mode",
        label: "Mode",
        description: "Set/Reset, Toggle, or Data. Changing it keeps the state.",
        min: 0.0,
        max: 2.0,
        default: 0.0,
        unit: "",
        options: ParamOptions::DEFAULT,
    }
    .integer()
    .choices(&["Set/Reset", "Toggle", "Data"])],
    role: ModuleRole::Dsp(|_| Box::new(Memory::default())),
    panel: Panel::new(9, 2, &[Widget::select(0, 0, "mode"), Widget::light(6, 0, "state")]),
    displays: &[DisplaySpec {
        name: "state",
        label: "State",
        description: "Lit while the remembered gate is high.",
        kind: DisplayKind::Level,
    }],
};

/// State for `loom.logic.memory`: the mode, the remembered gate, and one edge
/// detector for each input.
#[derive(Default, Clone)]
struct Memory {
    mode: u8,
    out: bool,
    edge_a: EdgeDetector,
    edge_b: EdgeDetector,
}

impl Module for Memory {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let mode_mod = io.param(0);
        let ([a, b], [out]) = io.split::<2, 1>();
        for (i, out) in out.iter_mut().enumerate() {
            let mode = mode_mod.map_or(i, self.mode, |v| v.round().clamp(0.0, 2.0) as u8);
            // Both detectors see every frame in every mode, so changing the
            // mode never looks like an edge.
            let rise_a = self.edge_a.rising(a[i]);
            let rise_b = self.edge_b.rising(b[i]);
            match mode {
                0 | 1 if rise_b => self.out = false, // reset wins a tie
                0 if rise_a => self.out = true,
                1 if rise_a => self.out = !self.out,
                2 if rise_b => self.out = gate_is_high(a[i]),
                _ => {}
            }
            *out = trigger(self.out);
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if index == 0 {
            self.mode = value as u8;
        }
    }

    fn reset(&mut self) {
        self.out = false;
        self.edge_a.reset();
        self.edge_b.reset();
    }

    fn displays(&self, values: &mut [f32]) {
        values[0] = trigger(self.out);
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod gate_tests;
