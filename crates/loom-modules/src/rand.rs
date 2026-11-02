//! Random and stepped sources (FR-LIB-01).

use loom_core::signal::{EdgeDetector, gate_is_high};
use loom_core::{
    Channels, Module, ModuleDescriptor, ModuleRole, Panel, ParamOptions, ParamSpec, PortDefault, PortIo, PortSpec,
    ProcessContext, SignalType, Widget,
};

/// A seeded random stream: xorshift32 (G. Marsaglia, "Xorshift RNGs", Journal
/// of Statistical Software, 2003), started from a mixed seed.
///
/// Xorshift is poor just after a small seed: seeds 1 and 2 begin with small,
/// similar values. So the start state goes through a finaliser (the mixing
/// step in G. Steele, D. Lea, and C. Flood, "Fast Splittable Pseudorandom
/// Number Generators", 2014), which spreads similar seeds apart.
#[derive(Clone, Copy)]
struct Rng(u32);

impl Rng {
    /// A stream from `seed` (1 to 65535), for this stereo `channel` and
    /// `stream` number. The three fit side by side in one word, so different
    /// streams never start from the same state.
    fn new(seed: u32, channel: usize, stream: u32) -> Self {
        let mut x = seed | (channel as u32) << 16 | stream << 17;
        x ^= x >> 16;
        x = x.wrapping_mul(0x85EB_CA6B);
        x ^= x >> 13;
        x = x.wrapping_mul(0xC2B2_AE35);
        x ^= x >> 16;
        Self(x.max(1))
    }

    /// The next value, from 0 up to but not including 1. Only the top 24 bits
    /// are used: an `f32` holds them exactly, so the result cannot round up
    /// to 1, and a chance of 1 always passes.
    fn draw(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }
}

/// The `seed` parameter of every random module (M2 modules, rule 1).
const fn seed_param(description: &'static str) -> ParamSpec {
    ParamSpec {
        name: "seed",
        label: "Seed",
        description,
        min: 1.0,
        max: 65535.0,
        default: 1.0,
        unit: "",
        options: ParamOptions::DEFAULT,
    }
    .integer()
}

/// A seed value as the whole number the streams start from.
fn seed_from(value: f32) -> u32 {
    value.round().clamp(1.0, 65535.0) as u32
}

/// One trigger input of Chance.
const fn chance_trigger(name: &'static str, label: &'static str, description: &'static str) -> PortSpec {
    PortSpec {
        name,
        label,
        signal: SignalType::Gate,
        description,
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::Downmix,
    }
}

/// One output of Chance.
const fn chance_output(name: &'static str, label: &'static str, description: &'static str) -> PortSpec {
    PortSpec {
        name,
        label,
        signal: SignalType::Gate,
        description,
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }
}

/// One lane's chance parameter.
const fn chance_param(name: &'static str, label: &'static str, description: &'static str) -> ParamSpec {
    ParamSpec {
        name,
        label,
        description,
        min: 0.0,
        max: 1.0,
        default: 0.5,
        unit: "",
        options: ParamOptions::DEFAULT,
    }
}

/// `loom.rand.chance` (MT-RAND-03): sends each trigger or gate to A or B by
/// chance, in four lanes.
pub static CHANCE: ModuleDescriptor = ModuleDescriptor {
    id: "loom.rand.chance",
    version: 1,
    name: "Chance",
    category: "Random",
    family: loom_core::Family::TimeAndRhythm,
    description: "Four lanes that pass each trigger or gate to output A with a set chance, and to output B otherwise.",
    inputs: &[
        chance_trigger(
            "trigger1",
            "Trigger 1",
            "Lane 1's trigger or gate. Each rising edge makes a new choice.",
        ),
        chance_trigger(
            "trigger2",
            "Trigger 2",
            "Lane 2's trigger or gate. With no cable, it follows the lane above.",
        ),
        chance_trigger(
            "trigger3",
            "Trigger 3",
            "Lane 3's trigger or gate. With no cable, it follows the lane above.",
        ),
        chance_trigger(
            "trigger4",
            "Trigger 4",
            "Lane 4's trigger or gate. With no cable, it follows the lane above.",
        ),
        chance_trigger(
            "reset",
            "Reset",
            "A rising edge restarts every lane's choices from the seed.",
        ),
    ],
    outputs: &[
        chance_output("a1", "A 1", "Carries lane 1's input when the choice was A."),
        chance_output("a2", "A 2", "Carries lane 2's input when the choice was A."),
        chance_output("a3", "A 3", "Carries lane 3's input when the choice was A."),
        chance_output("a4", "A 4", "Carries lane 4's input when the choice was A."),
        chance_output("b1", "B 1", "Carries lane 1's input when the choice was B."),
        chance_output("b2", "B 2", "Carries lane 2's input when the choice was B."),
        chance_output("b3", "B 3", "Carries lane 3's input when the choice was B."),
        chance_output("b4", "B 4", "Carries lane 4's input when the choice was B."),
    ],
    params: &[
        chance_param(
            "chance1",
            "Chance 1",
            "How likely lane 1 is to pick A. At 0 it never does; at 1 it always does.",
        ),
        chance_param(
            "chance2",
            "Chance 2",
            "How likely lane 2 is to pick A. At 0 it never does; at 1 it always does.",
        ),
        chance_param(
            "chance3",
            "Chance 3",
            "How likely lane 3 is to pick A. At 0 it never does; at 1 it always does.",
        ),
        chance_param(
            "chance4",
            "Chance 4",
            "How likely lane 4 is to pick A. At 0 it never does; at 1 it always does.",
        ),
        seed_param("Picks the sequence of choices. The same seed gives the same choices."),
    ],
    role: ModuleRole::Dsp(|_| Box::new(Chance::new())),
    panel: Panel::new(
        12,
        3,
        &[
            Widget::knob(0, 0, "chance1"),
            Widget::knob(3, 0, "chance2"),
            Widget::knob(6, 0, "chance3"),
            Widget::knob(9, 0, "chance4"),
        ],
    ),
    displays: &[],
};

/// Where a lane's input is going.
#[derive(Clone, Copy, PartialEq)]
enum Route {
    /// The input is low, so both outputs are 0.
    Off,
    /// The last rising edge picked A.
    A,
    /// The last rising edge picked B.
    B,
}

/// One lane of Chance. Its stream moves only on its own rising edges, so
/// patching another lane never changes its choices.
#[derive(Clone)]
struct Lane {
    edge: EdgeDetector,
    rng: Rng,
    route: Route,
}

/// State for `loom.rand.chance`.
#[derive(Clone)]
struct Chance {
    chances: [f32; CHANCE_LANES],
    lanes: [Lane; CHANCE_LANES],
    reset: EdgeDetector,
    /// The knob's base seed. Only `set_param` writes it.
    seed: u32,
    /// The seed the streams last started from: the modulated seed while an
    /// assignment is active, otherwise the base.
    active: u32,
    /// The stereo channel the streams last started for.
    channel: usize,
}

/// The number of lanes in Chance.
const CHANCE_LANES: usize = 4;

// The lanes read their parameters and inputs by offset, so a reorder that
// breaks a run must fail the build.
const _: () = {
    assert!(CHANCE.param_index("chance4").unwrap() == Chance::CHANCE1 + 3);
    assert!(CHANCE.input_index("trigger4").unwrap() == Chance::TRIGGER1_IN + 3);
};

impl Chance {
    const CHANCE1: usize = CHANCE.param_index("chance1").unwrap();
    const SEED: usize = CHANCE.param_index("seed").unwrap();
    const TRIGGER1_IN: usize = CHANCE.input_index("trigger1").unwrap();

    fn new() -> Self {
        let mut chance = Self {
            chances: [0.5; CHANCE_LANES],
            lanes: std::array::from_fn(|_| Lane {
                edge: EdgeDetector::new(),
                rng: Rng(1),
                route: Route::Off,
            }),
            reset: EdgeDetector::new(),
            seed: 1,
            active: 1,
            channel: 0,
        };
        chance.restart(1);
        chance
    }

    /// Starts every lane's stream again from `seed`.
    fn restart(&mut self, seed: u32) {
        for (number, lane) in self.lanes.iter_mut().enumerate() {
            lane.rng = Rng::new(seed, self.channel, number as u32);
        }
    }
}

impl Module for Chance {
    /// Takes the running state of another instance, then starts the streams
    /// again from the seed (stereo spec, rule 27).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        let Some(other) = other.as_any().downcast_ref::<Self>() else {
            return false;
        };
        *self = other.clone();
        self.restart(self.active);
        true
    }

    fn process(&mut self, ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        if self.channel != ctx.channel() {
            self.channel = ctx.channel();
            self.restart(self.active);
        }
        let seed = io.param(Self::SEED);
        let chances: [_; CHANCE_LANES] = std::array::from_fn(|n| io.param(Self::CHANCE1 + n));
        let connected: [bool; CHANCE_LANES] = std::array::from_fn(|n| io.is_connected(Self::TRIGGER1_IN + n));
        let ([trigger1, trigger2, trigger3, trigger4, reset], [a1, a2, a3, a4, b1, b2, b3, b4]) = io.split::<5, 8>();
        // A lane with no cable reads the lane above.
        let mut inputs = [trigger1, trigger2, trigger3, trigger4];
        for n in 1..CHANCE_LANES {
            if !connected[n] {
                inputs[n] = inputs[n - 1];
            }
        }
        let (a, b) = ([a1, a2, a3, a4], [b1, b2, b3, b4]);
        for i in 0..reset.len() {
            let next = seed.map_or(i, self.seed, seed_from);
            let restart = self.reset.rising(reset[i]);
            if next != self.active || restart {
                self.active = next;
                self.restart(next);
            }
            for n in 0..CHANCE_LANES {
                let lane = &mut self.lanes[n];
                let input = inputs[n][i];
                if lane.edge.rising(input) {
                    let chance = chances[n].at_or(i, self.chances[n]);
                    lane.route = if lane.rng.draw() < chance { Route::A } else { Route::B };
                } else if !gate_is_high(input) {
                    lane.route = Route::Off;
                }
                a[n][i] = if lane.route == Route::A { input } else { 0.0 };
                b[n][i] = if lane.route == Route::B { input } else { 0.0 };
            }
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if index == Self::SEED {
            self.seed = seed_from(value);
            self.active = self.seed;
            self.restart(self.seed);
        } else if let Some(n) = index.checked_sub(Self::CHANCE1).filter(|&n| n < CHANCE_LANES) {
            self.chances[n] = value;
        }
    }

    fn reset(&mut self) {
        self.reset.reset();
        for lane in &mut self.lanes {
            lane.edge.reset();
            lane.route = Route::Off;
        }
        self.restart(self.active);
    }
}

/// `loom.rand.sample-hold` (MT-RAND-02): takes a value and holds it.
pub static SAMPLE_HOLD: ModuleDescriptor = ModuleDescriptor {
    id: "loom.rand.sample-hold",
    version: 1,
    name: "Sample and Hold",
    category: "Random",
    family: loom_core::Family::Modulation,
    description: "Takes the input's value on a trigger and holds it. With noise on the input, it makes stepped random values.",
    inputs: &[
        PortSpec {
            name: "in",
            label: "In",
            signal: SignalType::Modulation,
            description: "The signal to sample.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "trigger",
            label: "Trigger",
            signal: SignalType::Gate,
            description: "A rising edge takes a new value (Sample), or lets the input through while high (Track).",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::Downmix,
        },
    ],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Modulation,
        description: "The held value. It starts at 0.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[ParamSpec {
        name: "mode",
        label: "Mode",
        description: "Sample: take the value on the rising edge. Track: follow the input while the trigger is high.",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
        options: ParamOptions::DEFAULT,
    }
    .integer()
    .choices(&["Sample", "Track"])],
    role: ModuleRole::Dsp(|_| {
        Box::new(SampleHold {
            track: false,
            edge: EdgeDetector::new(),
            held: 0.0,
        })
    }),
    panel: Panel::new(7, 2, &[Widget::segmented(0, 0, 7, "mode")]),
    displays: &[],
};

/// State for `loom.rand.sample-hold`: `track` is the mode, `edge` finds the
/// trigger edges, and `held` is the value on the output.
#[derive(Clone)]
struct SampleHold {
    track: bool,
    edge: EdgeDetector,
    held: f32,
}

impl SampleHold {
    const MODE: usize = SAMPLE_HOLD.param_index("mode").unwrap();
}

impl Module for SampleHold {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let mode_mod = io.param(Self::MODE);
        let ([input, trigger], [out]) = io.split::<2, 1>();
        for (i, out) in out.iter_mut().enumerate() {
            let track = mode_mod.map_or(i, self.track, |v| v >= 0.5);
            // The detector sees every frame, so a mode change is no event.
            let rising = self.edge.rising(trigger[i]);
            let take = if track { gate_is_high(trigger[i]) } else { rising };
            if take && input[i].is_finite() {
                self.held = input[i];
            }
            *out = self.held;
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if index == Self::MODE {
            self.track = value >= 0.5;
        }
    }

    fn reset(&mut self) {
        self.held = 0.0;
        self.edge.reset();
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod chance_tests;
