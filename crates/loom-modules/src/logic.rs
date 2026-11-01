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
mod tests {
    use super::*;
    use loom_core::testing::run;

    /// One frame per step: a trigger frame, then a low frame.
    fn pulses(count: usize) -> Vec<f32> {
        (0..count * 2).map(|i| if i % 2 == 0 { 1.0 } else { 0.0 }).collect()
    }

    /// The numbers (from 1) of the input triggers that fired the output.
    fn fired(n: f32, offset: f32, trigger_in: &[f32], reset: &[f32]) -> Vec<usize> {
        let mut module = Divide::new();
        module.set_param(0, DIVIDE.params[0].clamp(n));
        module.set_param(1, DIVIDE.params[1].clamp(offset));
        let mut out = vec![0.0; trigger_in.len()];
        run(
            &mut module,
            48_000.0,
            &[trigger_in, reset],
            &[true, true],
            &mut [&mut out],
            &[],
        );
        let mut number = 0;
        let mut result = Vec::new();
        for (frame, &t) in trigger_in.iter().enumerate() {
            if t > 0.5 && (frame == 0 || trigger_in[frame - 1] <= 0.5) {
                number += 1;
            }
            if out[frame] == 1.0 {
                result.push(number);
            } else {
                assert_eq!(out[frame], 0.0);
            }
        }
        result
    }

    #[test]
    fn n_2_offset_0_fires_on_2_4_6() {
        let input = pulses(6);
        assert_eq!(fired(2.0, 0.0, &input, &vec![0.0; input.len()]), [2, 4, 6]);
    }

    #[test]
    fn n_2_offset_1_fires_on_1_3_5() {
        let input = pulses(6);
        assert_eq!(fired(2.0, 1.0, &input, &vec![0.0; input.len()]), [1, 3, 5]);
    }

    #[test]
    fn offset_n_minus_1_fires_on_the_first_trigger() {
        let input = pulses(9);
        assert_eq!(fired(4.0, 3.0, &input, &vec![0.0; input.len()]), [1, 5, 9]);
    }

    #[test]
    fn n_1_fires_on_every_trigger() {
        let input = pulses(4);
        assert_eq!(fired(1.0, 0.0, &input, &vec![0.0; input.len()]), [1, 2, 3, 4]);
    }

    #[test]
    fn offset_of_n_or_more_counts_as_n_minus_1() {
        let input = pulses(7);
        let silent = vec![0.0; input.len()];
        assert_eq!(fired(3.0, 10.0, &input, &silent), fired(3.0, 2.0, &input, &silent));
        assert_eq!(fired(3.0, 10.0, &input, &silent), [1, 4, 7]);
    }

    #[test]
    fn reset_starts_the_count_again() {
        // Triggers 1 to 3, a reset, then triggers 4 to 7.
        let input = pulses(7);
        let mut reset = vec![0.0; input.len()];
        reset[5] = 1.0;
        // Without the reset: 2, 4, 6. After it, counting starts again at
        // trigger 4, so the second and fourth after it fire: 5 and 7.
        assert_eq!(fired(2.0, 0.0, &input, &reset), [2, 5, 7]);
    }

    #[test]
    fn trigger_on_the_reset_frame_counts_as_the_first() {
        let input = pulses(5);
        let mut reset = vec![0.0; input.len()];
        // Reset on the same frame as trigger 2.
        reset[2] = 1.0;
        assert_eq!(fired(2.0, 0.0, &input, &reset), [3, 5]);
    }

    #[test]
    fn a_held_gate_counts_once() {
        let mut input = vec![1.0; 10];
        input.extend([0.0, 1.0, 1.0, 0.0]);
        assert_eq!(fired(2.0, 0.0, &input, &vec![0.0; input.len()]), [2]);
    }

    #[test]
    fn changing_n_keeps_the_count_since_reset() {
        // Five triggers at N = 2, then three more at N = 3.
        let mut module = Divide::new();
        let first = pulses(5);
        let mut out = vec![0.0; first.len()];
        let silent = vec![0.0; first.len()];
        run(
            &mut module,
            48_000.0,
            &[&first, &silent],
            &[true, true],
            &mut [&mut out],
            &[],
        );
        assert_eq!(out.iter().filter(|&&v| v == 1.0).count(), 2, "triggers 2 and 4");

        module.set_param(0, 3.0);
        let second = pulses(3);
        let mut out = vec![0.0; second.len()];
        let silent = vec![0.0; second.len()];
        run(
            &mut module,
            48_000.0,
            &[&second, &silent],
            &[true, true],
            &mut [&mut out],
            &[],
        );
        // The count goes on from 5, so only trigger 6 fires, as if N had been
        // 3 since the reset. A count kept modulo 2 would be at 1, and fire
        // on trigger 7 instead.
        let fired: Vec<usize> = (0..out.len()).filter(|&i| out[i] == 1.0).map(|i| i / 2 + 6).collect();
        assert_eq!(fired, [6]);
    }

    #[test]
    fn same_output_for_any_block_size() {
        let input: Vec<f32> = (0..500).map(|i| if i % 7 < 2 { 1.0 } else { 0.0 }).collect();
        let reset: Vec<f32> = (0..500).map(|i| if i % 97 == 50 { 1.0 } else { 0.0 }).collect();
        let render = |block: usize| {
            let mut module = Divide::new();
            module.set_param(0, 3.0);
            module.set_param(1, 1.0);
            let mut out = vec![0.0; input.len()];
            for ((out, t), r) in out.chunks_mut(block).zip(input.chunks(block)).zip(reset.chunks(block)) {
                run(&mut module, 48_000.0, &[t, r], &[true, true], &mut [out], &[]);
            }
            out
        };
        let whole = render(500);
        assert!(whole.contains(&1.0));
        for block in [1, 3, 64] {
            assert_eq!(render(block), whole, "block size {block}");
        }
    }
}

#[cfg(test)]
mod gate_tests {
    use super::*;
    use loom_core::ModuleConfig;
    use loom_core::testing::run;

    /// Runs `descriptor` once over `inputs` (one slice per input port) with
    /// the given parameters, in blocks of `block` frames, and returns output 0.
    fn process(
        descriptor: &ModuleDescriptor,
        params: &[(usize, f32)],
        inputs: &[&[f32]],
        connected: &[bool],
        block: usize,
    ) -> Vec<f32> {
        let ModuleRole::Dsp(create) = descriptor.role else {
            unreachable!()
        };
        let frames = inputs[0].len();
        let mut m = create(&ModuleConfig {
            sample_rate: 48_000.0,
            max_block: block,
        });
        for &(index, value) in params {
            m.set_param(index, value);
        }
        m.reset();
        let mut out = vec![0.0; frames];
        for start in (0..frames).step_by(block) {
            let end = (start + block).min(frames);
            let slices: Vec<&[f32]> = inputs.iter().map(|s| &s[start..end]).collect();
            run(
                m.as_mut(),
                48_000.0,
                &slices,
                connected,
                &mut [&mut out[start..end]],
                &[],
            );
        }
        out
    }

    /// Without hysteresis, equal values are low in both modes.
    #[test]
    fn compare_modes_and_equality() {
        let a = [0.0, 0.5, 1.0];
        let b = [0.5; 3];
        assert_eq!(process(&COMPARE, &[], &[&a, &b], &[true, true], 3), [0.0, 0.0, 1.0]);
        assert_eq!(
            process(&COMPARE, &[(0, 1.0)], &[&a, &b], &[true, true], 3),
            [1.0, 0.0, 0.0]
        );
    }

    /// Noise smaller than the band does not make the gate chatter.
    #[test]
    fn compare_hysteresis_does_not_chatter() {
        // Rises past B = 0.5, then wobbles by +-0.05 around B.
        let mut a = vec![0.0, 0.8];
        a.extend((0..100).map(|i| 0.5 + if i % 2 == 0 { 0.05 } else { -0.05 }));
        let b = vec![0.5; a.len()];
        let gate = process(&COMPARE, &[(1, 0.2)], &[&a, &b], &[true, true], 64);
        let changes = gate.windows(2).filter(|w| w[0] != w[1]).count();
        assert_eq!(changes, 1, "only the first rise: {gate:?}");
        // With no hysteresis the same signal chatters.
        let gate = process(&COMPARE, &[], &[&a, &b], &[true, true], 64);
        assert!(gate.windows(2).filter(|w| w[0] != w[1]).count() > 10);
    }

    /// Logic ignores unconnected inputs, and invert flips the result.
    #[test]
    fn logic_counts_only_connected_inputs() {
        let high = [1.0; 2];
        let low = [0.0; 2];
        // And of two wires, with the other two inputs unconnected.
        let and = process(&LOGIC, &[], &[&high, &high, &low, &low], &[true, true, false, false], 2);
        assert_eq!(and, [1.0, 1.0]);
        // Or: one high is enough. Xor of two highs is low.
        let or = process(
            &LOGIC,
            &[(0, 1.0)],
            &[&high, &low, &low, &low],
            &[true, true, false, false],
            2,
        );
        assert_eq!(or, [1.0, 1.0]);
        let xor = process(
            &LOGIC,
            &[(0, 2.0)],
            &[&high, &high, &low, &low],
            &[true, true, false, false],
            2,
        );
        assert_eq!(xor, [0.0, 0.0]);
        // Xor of three highs (odd) is high.
        let xor3 = process(
            &LOGIC,
            &[(0, 2.0)],
            &[&high, &high, &high, &low],
            &[true, true, true, false],
            2,
        );
        assert_eq!(xor3, [1.0, 1.0]);
        // No cable: low, and Not (invert) makes it high.
        assert_eq!(process(&LOGIC, &[], &[&high[..]; 4], &[false; 4], 2), [0.0, 0.0]);
        assert_eq!(
            process(&LOGIC, &[(1, 1.0)], &[&high[..]; 4], &[false; 4], 2),
            [1.0, 1.0]
        );
        // Not: one connected input, Or, invert.
        assert_eq!(
            process(
                &LOGIC,
                &[(0, 1.0), (1, 1.0)],
                &[&high, &low, &low, &low],
                &[true, false, false, false],
                2
            ),
            [0.0, 0.0]
        );
    }

    /// Set/Reset: reset wins a tie, and edges fall on the same frame at
    /// any block size, including the first and last frame of a block.
    #[test]
    fn memory_set_reset_and_block_invariance() {
        // a rises at 2 and 9, b rises at 5 and 9 (a tie).
        let mut a = vec![0.0; 16];
        let mut b = vec![0.0; 16];
        a[2..4].fill(1.0);
        a[9..11].fill(1.0);
        b[5..7].fill(1.0);
        b[9..11].fill(1.0);
        let expected: Vec<f32> = (0..16).map(|i| if (2..5).contains(&i) { 1.0 } else { 0.0 }).collect();
        for block in [1, 3, 4, 16] {
            assert_eq!(
                process(&MEMORY, &[], &[&a, &b], &[true, true], block),
                expected,
                "block {block}"
            );
        }
    }

    /// Toggle flips on each rising edge of A; B resets.
    #[test]
    fn memory_toggle_and_data() {
        let mut a = vec![0.0; 12];
        a[1..3].fill(1.0);
        a[5..7].fill(1.0);
        let mut b = vec![0.0; 12];
        b[8] = 1.0;
        let toggled = process(&MEMORY, &[(0, 1.0)], &[&a, &b], &[true, true], 12);
        let want: Vec<f32> = (0..12).map(|i| if (1..5).contains(&i) { 1.0 } else { 0.0 }).collect();
        assert_eq!(toggled, want);
        // Data: on a clock edge the output takes the level of A.
        let data = vec![0.0, 1.0, 1.0, 1.0, 0.0, 0.0];
        let clock = vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0];
        assert_eq!(
            process(&MEMORY, &[(0, 2.0)], &[&data, &clock], &[true, true], 6),
            [0.0, 0.0, 1.0, 1.0, 1.0, 0.0]
        );
    }
}
