//! Picking between signals (FR-LIB-30). Spec: `docs/design/p2-modules.md`.

use crate::rand::{seed_from, seed_param, stream};
use loom_core::signal::{EdgeDetector, hold_finite};
use loom_core::{
    Channels, Module, ModuleConfig, ModuleDescriptor, ModuleRole, Panel, ParamOptions, ParamSpec, PortDefault, PortIo,
    PortSpec, ProcessContext, SignalType, Widget,
};
use loom_dsp::random::Rng;

/// The most inputs Selector has.
const INPUTS: usize = 8;

/// One signal input of Selector.
const fn signal_input(name: &'static str, label: &'static str) -> PortSpec {
    PortSpec {
        name,
        label,
        signal: SignalType::Audio,
        description: "A signal to pick.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }
}

/// `loom.route.selector` (MT-ROUTE-01): picks one of up to eight inputs.
pub static SELECTOR: ModuleDescriptor = ModuleDescriptor {
    id: "loom.route.selector",
    version: 1,
    name: "Selector",
    category: "Routing",
    family: loom_core::Family::LevelAndRouting,
    description: "Picks one of up to eight inputs: the next on a trigger, the one a value points at, or one at random.",
    inputs: &[
        signal_input("in1", "In 1"),
        signal_input("in2", "In 2"),
        signal_input("in3", "In 3"),
        signal_input("in4", "In 4"),
        signal_input("in5", "In 5"),
        signal_input("in6", "In 6"),
        signal_input("in7", "In 7"),
        signal_input("in8", "In 8"),
        // These three downmix, so both sides of a stereo signal make the same
        // picks (ADR-0036).
        PortSpec {
            name: "step",
            label: "Step",
            signal: SignalType::Gate,
            description: "A rising edge moves to the next input, in Step mode, or picks one, in Random mode.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::Downmix,
        },
        PortSpec {
            name: "select",
            label: "Select",
            signal: SignalType::Modulation,
            description: "Which input to pick, in Select mode: 0 is the first, and 1 is the last in use.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::Downmix,
        },
        PortSpec {
            name: "reset",
            label: "Reset",
            signal: SignalType::Gate,
            description: "A rising edge goes to input 1 and restarts the random picks from the seed.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::Downmix,
        },
    ],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "The picked input, faded from the last one.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[
        ParamSpec {
            name: "mode",
            label: "Mode",
            description: "Step: each trigger moves to the next input. Select: a value picks it. Random: each trigger picks one.",
            min: 0.0,
            max: 2.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer()
        .choices(&["Step", "Select", "Random"]),
        ParamSpec {
            name: "count",
            label: "Count",
            description: "How many inputs are in use.",
            min: 2.0,
            max: 8.0,
            default: 4.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer(),
        // Not logarithmic: 0 must be reachable, and it means an instant switch.
        ParamSpec {
            name: "fade",
            label: "Fade",
            description: "The time a switch takes, in a straight line. At 0 it is instant, which clicks on audio.",
            min: 0.0,
            max: 1.0,
            default: 0.005,
            unit: "s",
            options: ParamOptions::DEFAULT,
        },
        ParamSpec {
            name: "repeats",
            label: "Repeats",
            description: "Avoid: a random pick is never the current input. Allow: it can be.",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer()
        .choices(&["Avoid", "Allow"]),
        seed_param("Picks the sequence of random picks. The same seed gives the same picks."),
    ],
    role: ModuleRole::Dsp(|config| Box::new(Selector::new(config))),
    panel: Panel::new(
        12,
        5,
        &[
            Widget::segmented(0, 0, 12, "mode"),
            Widget::knob(0, 2, "count"),
            Widget::knob(3, 2, "fade"),
        ],
    ),
    displays: &[],
    roles: &[],
};

/// How Selector picks.
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Step,
    Select,
    Random,
}

impl Mode {
    /// The mode a value of the `mode` parameter names.
    fn from_value(value: f32) -> Self {
        match value.round() as i32 {
            1 => Self::Select,
            2 => Self::Random,
            _ => Self::Step,
        }
    }
}

/// State for `loom.route.selector`.
#[derive(Clone)]
struct Selector {
    mode: Mode,
    count: usize,
    fade: f32,
    allow_repeats: bool,
    /// The knob's base seed. Only `set_param` writes it.
    seed: u32,
    /// The seed the stream last started from: the modulated seed while an
    /// assignment is active, otherwise the base.
    active: u32,
    rng: Rng,
    /// Voice identity; stereo sides deliberately share a stream.
    voice: usize,
    /// The input chosen now, from 0.
    current: usize,
    /// True after a start or a reset, until the next `step`, which stays on
    /// input 1 in Step mode, so a clock that arrives after a reset plays it.
    waiting: bool,
    gains: [f32; INPUTS],
    step: EdgeDetector,
    reset: EdgeDetector,
    sample_rate: f32,
}

impl Selector {
    const MODE: usize = SELECTOR.param_index("mode").unwrap();
    const COUNT: usize = SELECTOR.param_index("count").unwrap();
    const FADE: usize = SELECTOR.param_index("fade").unwrap();
    const REPEATS: usize = SELECTOR.param_index("repeats").unwrap();
    const SEED: usize = SELECTOR.param_index("seed").unwrap();

    /// A module with every parameter at its default, on input 1.
    fn new(config: &ModuleConfig) -> Self {
        Self {
            mode: Mode::Step,
            count: 4,
            fade: 0.005,
            allow_repeats: false,
            seed: 1,
            active: 1,
            rng: Self::stream_from(1, 0),
            voice: 0,
            current: 0,
            waiting: true,
            gains: Self::on_input_one(),
            step: EdgeDetector::new(),
            reset: EdgeDetector::new(),
            sample_rate: config.sample_rate,
        }
    }

    /// Gains with only input 1 sounding.
    fn on_input_one() -> [f32; INPUTS] {
        let mut gains = [0.0; INPUTS];
        gains[0] = 1.0;
        gains
    }

    /// The random stream for `seed`. The stereo channel stays out of it, so both
    /// sides of a stereo signal make the same picks (ADR-0036).
    fn stream_from(seed: u32, voice: usize) -> Rng {
        stream(seed, voice * 2, 0)
    }

    /// A count from a value of the `count` parameter.
    fn count_from(value: f32) -> usize {
        value.round().clamp(2.0, INPUTS as f32) as usize
    }

    /// A random input of the first `count`. Avoid takes one number from one
    /// fewer choices and steps over the current input, so it uses one draw per
    /// pick and can never repeat.
    fn pick(&mut self, count: usize, allow_repeats: bool) -> usize {
        let draw = self.rng.draw();
        if allow_repeats {
            (draw * count as f32) as usize
        } else {
            let pick = (draw * (count - 1) as f32) as usize;
            if pick >= self.current { pick + 1 } else { pick }
        }
    }
}

impl Module for Selector {
    /// Takes the running state of another instance, with its random stream, so a
    /// new right side carries on making the left side's picks (ADR-0036).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        if self.voice != ctx.voice() - 1 {
            self.voice = ctx.voice() - 1;
            self.rng = Self::stream_from(self.active, self.voice);
        }
        let (mode_param, count_param) = (io.param(Self::MODE), io.param(Self::COUNT));
        let (fade_param, repeats_param) = (io.param(Self::FADE), io.param(Self::REPEATS));
        let seed_param = io.param(Self::SEED);
        let ([in1, in2, in3, in4, in5, in6, in7, in8, step, select, reset], [out]) = io.split::<11, 1>();
        let inputs = [in1, in2, in3, in4, in5, in6, in7, in8];
        for i in 0..out.len() {
            let next = seed_param.map_or(i, self.seed, seed_from);
            if next != self.active {
                self.active = next;
                self.rng = Self::stream_from(next, self.voice);
            }
            let mode = mode_param.map_or(i, self.mode, Mode::from_value);
            let count = count_param.map_or(i, self.count, Self::count_from);
            // A reset comes before a step on the same frame, so the step is the
            // one that finds the wait, or takes the first random pick.
            if self.reset.rising(reset[i]) {
                self.rng = Self::stream_from(self.active, self.voice);
                self.current = 0;
                self.waiting = true;
            }
            // Before the step, so a step on the frame `count` drops goes from the
            // last input now in use.
            self.current = self.current.min(count - 1);
            if self.step.rising(step[i]) {
                match mode {
                    Mode::Step if self.waiting => self.waiting = false,
                    Mode::Step => self.current = (self.current + 1) % count,
                    Mode::Random => {
                        self.waiting = false;
                        let allow = repeats_param.map_or(i, self.allow_repeats, |v| v >= 0.5);
                        self.current = self.pick(count, allow);
                    }
                    Mode::Select => {}
                }
            }
            // `clamp` passes NaN, and a bad value holds the current input.
            if mode == Mode::Select && select[i].is_finite() {
                self.current = (select[i].clamp(0.0, 1.0) * count as f32) as usize;
            }
            self.current = self.current.min(count - 1);

            let frames = fade_param.at_or(i, self.fade) * self.sample_rate;
            let most = if frames > 1.0 { 1.0 / frames } else { 1.0 };
            let mut total = 0.0;
            for (input, gain) in self.gains.iter_mut().enumerate() {
                let target = if input == self.current { 1.0 } else { 0.0 };
                *gain += (target - *gain).clamp(-most, most);
                total += *gain;
            }
            // Sharing out each gain first keeps large inputs from overflowing
            // on the way, and an input at gain 0 is skipped, so an infinite one
            // not in use cannot make NaN. The shares can round to a sum just above
            // 1, so a sum of finite inputs is held at the largest finite value.
            let (mut sum, mut finite) = (0.0f32, true);
            for (gain, input) in self.gains.iter().zip(&inputs) {
                if *gain > 0.0 {
                    sum += input[i] * (gain / total);
                    finite &= input[i].is_finite();
                }
            }
            out[i] = if finite { hold_finite(sum) } else { sum };
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            Self::MODE => self.mode = Mode::from_value(value),
            Self::COUNT => self.count = Self::count_from(value),
            Self::FADE => self.fade = value,
            Self::REPEATS => self.allow_repeats = value >= 0.5,
            Self::SEED => {
                self.seed = seed_from(value);
                self.active = self.seed;
                self.rng = Self::stream_from(self.seed, self.voice);
            }
            _ => {}
        }
    }

    /// Back to input 1 and the start of the picks.
    fn reset(&mut self) {
        self.step.reset();
        self.reset.reset();
        self.current = 0;
        self.waiting = true;
        self.gains = Self::on_input_one();
        self.rng = Self::stream_from(self.active, self.voice);
    }
}

#[cfg(test)]
mod tests;
