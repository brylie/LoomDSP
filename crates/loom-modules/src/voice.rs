//! Bundle routing. Spec: `docs/design/polyphony.md`.

use crate::rand::{VOICE_SPREAD_STREAM, seed_from, seed_param, stream};
use loom_core::signal::hold_finite;
use loom_core::{
    Channels, Module, ModuleConfig, ModuleDescriptor, ModuleRole, Panel, ParamOptions, ParamRange, ParamSpec,
    PortDefault, PortIo, PortRange, PortSpec, ProcessContext, SignalType, Widget,
};
use loom_rt::Smoothed;

/// The `voices` parameter that Note input and Voice assigner share: how many
/// output voices the module has. It shapes the graph, so it is not modulated.
pub(crate) const fn voices_param() -> ParamSpec {
    ParamSpec {
        name: "voices",
        label: "Voices",
        description: "Number of simultaneous voices.",
        min: 1.0,
        max: 16.0,
        default: 1.0,
        unit: "",
        options: ParamOptions::DEFAULT,
    }
    .integer()
    .shapes_graph()
}

/// The `mode` parameter that Note input and Voice assigner share (polyphony
/// spec, rule 20). Its choices are filtered by the voice count (ADR-0030).
pub(crate) const fn mode_param() -> ParamSpec {
    ParamSpec {
        name: "mode",
        label: "Mode",
        description: "How notes steal voices, or how a single voice articulates notes.",
        min: 0.0,
        max: 3.0,
        default: 2.0,
        unit: "",
        options: ParamOptions::DEFAULT,
    }
    .integer()
    .choices(&["Oldest", "Quietest", "Legato", "Retrigger"])
    .choices_by_count("voices", 2)
}

/// Maps a stored `mode` choice to the allocator's mode. Stored values stay
/// stable; the allocator falls back when a mode does not fit the voice count.
pub(crate) fn allocator_mode(value: usize) -> loom_dsp::voice::Mode {
    use loom_dsp::voice::Mode;
    match value {
        0 => Mode::Oldest,
        1 => Mode::Quietest,
        3 => Mode::Retrigger,
        _ => Mode::Legato,
    }
}

/// One voice of an input read as one value: the mean of a stereo voice's two
/// sides, or its only side. Kept as exactly `left * 0.5 + right * 0.5`: it
/// cannot overflow, so it is finite exactly when both sides are.
pub(crate) fn stereo_mean(io: &PortIo<'_>, port: usize, voice: usize, frame: usize) -> f32 {
    let left = io.voice_input(port, voice, 0)[frame];
    if io.input_width(port).sides == 2 {
        left * 0.5 + io.voice_input(port, voice, 1)[frame] * 0.5
    } else {
        left
    }
}

/// `loom.voice.merge`: sums, averages, or selects a voice from a bundle.
pub static MERGE: ModuleDescriptor = ModuleDescriptor {
    id: "loom.voice.merge",
    version: 1,
    name: "Voice Merge",
    category: "Voice",
    family: loom_core::Family::LevelAndRouting,
    description: "Sums or averages voices, or follows one voice selected by its gate.",
    inputs: &[
        PortSpec {
            name: "in",
            label: "In",
            description: "The voices to merge.",
            signal: SignalType::Audio,
            default: PortDefault::Value(0.0),
            range: PortRange::Signal,
            channels: Channels::Voices,
        },
        PortSpec {
            name: "gate",
            label: "Gate",
            description: "Which voices are playing. A gate edge updates the selected voice.",
            signal: SignalType::Gate,
            default: PortDefault::Value(1.0),
            range: PortRange::Signal,
            channels: Channels::Voices,
        },
    ],
    outputs: &[
        PortSpec {
            name: "out",
            label: "Out",
            description: "The merged signal, with the input's stereo sides.",
            signal: SignalType::Audio,
            default: PortDefault::Value(0.0),
            range: PortRange::Signal,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "gate",
            label: "Gate",
            description: "High while the voice Out follows is playing. In Sum and Average, while any voice plays.",
            signal: SignalType::Gate,
            default: PortDefault::Value(0.0),
            range: PortRange::Signal,
            channels: Channels::PerChannel,
        },
    ],
    params: &[ParamSpec {
        name: "mode",
        label: "Mode",
        description: "Sum or average all voices, or select the last, lowest, or highest playing voice.",
        min: 0.0,
        max: 4.0,
        default: 0.0,
        unit: "",
        options: ParamOptions::DEFAULT,
    }
    .integer()
    .choices(&["Sum", "Average", "Last", "Lowest", "Highest"])],
    role: ModuleRole::Dsp(|_| Box::new(Merge::default())),
    panel: Panel::new(20, 2, &[Widget::segmented(0, 0, 20, "mode")]),
    displays: &[],
    roles: &[],
};

/// Gate history and the selection held between edges.
#[derive(Clone, Default)]
struct Merge {
    high: [bool; 16],
    order: [u64; 16],
    clock: u64,
    selected: usize,
    mode: usize,
    previous_mode: Option<usize>,
}

impl Merge {
    const IN: usize = MERGE.input_index("in").unwrap();
    const GATE_IN: usize = MERGE.input_index("gate").unwrap();
    const OUT: usize = MERGE.output_index("out").unwrap();
    const GATE_OUT: usize = MERGE.output_index("gate").unwrap();
    const MODE: usize = MERGE.param_index("mode").unwrap();

    /// True when `candidate` should replace `previous` in Lowest (mode 3) or
    /// Highest (mode 4). A non-finite value never wins over a finite one
    /// (#794); between finite values the strict comparison keeps the lower
    /// voice index on a tie.
    fn better(mode: usize, candidate: f32, previous: f32) -> bool {
        match (candidate.is_finite(), previous.is_finite()) {
            (false, _) => false,
            (true, false) => true,
            (true, true) if mode == 3 => candidate < previous,
            (true, true) => candidate > previous,
        }
    }

    /// One voice's gate, its two stereo sides combined by maximum. `f32::max`
    /// keeps the other side when one is NaN.
    fn gate(io: &PortIo<'_>, voice: usize, frame: usize) -> f32 {
        io.voice_input(Self::GATE_IN, voice, 0)[frame].max(io.voice_input(Self::GATE_IN, voice, 1)[frame])
    }

    /// Compares stereo voices by their mean while retaining their two sides.
    fn value(io: &PortIo<'_>, voice: usize, frame: usize) -> f32 {
        stereo_mean(io, Self::IN, voice, frame)
    }
}

impl Module for Merge {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let voices = io.input_width(Self::IN).voices;
        let sides = io.output_width(Self::OUT).sides;
        let mode = io.param(Self::MODE);
        let selected_removed = self.selected >= voices;
        self.selected = self.selected.min(voices - 1);
        // Removed voices have no gate history if the bundle later grows again.
        self.high[voices..].fill(false);
        self.order[voices..].fill(0);
        for frame in 0..io.frames() {
            let mode = mode.map_or(frame, self.mode, |value| value.round().clamp(0.0, 4.0) as usize);
            let mut edge = (frame == 0 && selected_removed) || self.previous_mode != Some(mode);
            self.previous_mode = Some(mode);
            // Simultaneous rises share an age, so ties retain the lowest index.
            self.clock = self.clock.wrapping_add(1);
            for voice in 0..voices {
                let high = Self::gate(io, voice, frame) > 0.5;
                if high != self.high[voice] {
                    edge = true;
                    self.high[voice] = high;
                    if high {
                        self.order[voice] = self.clock;
                    }
                }
            }
            if edge && mode >= 2 {
                let mut picked = None;
                for voice in 0..voices {
                    if !self.high[voice] {
                        continue;
                    }
                    let wins = picked.is_none_or(|previous| match mode {
                        2 => self.order[voice] > self.order[previous],
                        _ => Self::better(mode, Self::value(io, voice, frame), Self::value(io, previous, frame)),
                    });
                    if wins {
                        picked = Some(voice);
                    }
                }
                if let Some(picked) = picked {
                    self.selected = picked;
                }
            }
            for side in 0..sides {
                let value = if mode <= 1 {
                    let mut sum = 0.0;
                    for voice in 0..voices {
                        sum += io.voice_input(Self::IN, voice, side)[frame];
                    }
                    if mode == 1 { sum / voices as f32 } else { sum }
                } else {
                    io.voice_input(Self::IN, self.selected, side)[frame]
                };
                io.voice_output(Self::OUT, 0, side)[frame] = value;
            }
            // The selection above is already made for this frame, so a hand-off
            // between held voices has no low sample.
            let gate = if mode <= 1 {
                (1..voices).fold(Self::gate(io, 0, frame), |high, voice| {
                    high.max(Self::gate(io, voice, frame))
                })
            } else {
                Self::gate(io, self.selected, frame)
            };
            for side in 0..io.output_width(Self::GATE_OUT).sides {
                io.voice_output(Self::GATE_OUT, 0, side)[frame] = gate;
            }
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if index == Self::MODE {
            self.mode = value.round().clamp(0.0, 4.0) as usize;
        }
    }

    fn reset(&mut self) {
        *self = Self {
            mode: self.mode,
            ..Self::default()
        };
    }

    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }
}

/// One of Voice split's outputs: one voice, with the input's stereo sides.
const fn split_out(name: &'static str, label: &'static str, description: &'static str) -> PortSpec {
    PortSpec {
        name,
        label,
        description,
        signal: SignalType::Audio,
        default: PortDefault::Value(0.0),
        range: PortRange::Signal,
        channels: Channels::PerChannel,
    }
}

/// `loom.voice.split` (MT-VOICE-02): the voices of a bundle as separate
/// outputs.
pub static SPLIT: ModuleDescriptor = ModuleDescriptor {
    id: "loom.voice.split",
    version: 1,
    name: "Voice Split",
    category: "Voice",
    family: loom_core::Family::LevelAndRouting,
    description: "Takes the voices of a bundle out on separate outputs, from voice First on. For the two sides of a \
                  stereo cable, use Split.",
    inputs: &[PortSpec {
        name: "in",
        label: "In",
        description: "The voices to take apart.",
        signal: SignalType::Audio,
        default: PortDefault::Value(0.0),
        range: PortRange::Signal,
        channels: Channels::Voices,
    }],
    outputs: &[
        split_out(
            "out1",
            "Out 1",
            "The voice First names. Past the input's last voice it is 0, the input's unpatched value.",
        ),
        split_out(
            "out2",
            "Out 2",
            "The voice after Out 1's. Past the input's last voice it is 0, the input's unpatched value.",
        ),
        split_out(
            "out3",
            "Out 3",
            "The voice after Out 2's. Past the input's last voice it is 0, the input's unpatched value.",
        ),
        split_out(
            "out4",
            "Out 4",
            "The voice after Out 3's. Past the input's last voice it is 0, the input's unpatched value.",
        ),
        split_out(
            "out5",
            "Out 5",
            "The voice after Out 4's. Past the input's last voice it is 0, the input's unpatched value.",
        ),
        split_out(
            "out6",
            "Out 6",
            "The voice after Out 5's. Past the input's last voice it is 0, the input's unpatched value.",
        ),
        split_out(
            "out7",
            "Out 7",
            "The voice after Out 6's. Past the input's last voice it is 0, the input's unpatched value.",
        ),
        split_out(
            "out8",
            "Out 8",
            "The voice after Out 7's. Past the input's last voice it is 0, the input's unpatched value.",
        ),
    ],
    params: &[ParamSpec {
        name: "first",
        label: "First",
        description: "The voice on Out 1. Each output after it carries the next voice.",
        min: 1.0,
        max: 16.0,
        default: 1.0,
        unit: "",
        options: ParamOptions::DEFAULT,
    }
    .integer()],
    role: ModuleRole::Dsp(|_| Box::new(Split { first: 1 })),
    panel: Panel::new(5, 2, &[Widget::number(0, 0, "first")]),
    displays: &[],
    roles: &[],
};

/// State for `loom.voice.split`: only the knob's `first`.
#[derive(Clone)]
struct Split {
    /// The voice on `out1`, from 1.
    first: usize,
}

impl Split {
    const IN: usize = SPLIT.input_index("in").unwrap();
    const FIRST: usize = SPLIT.param_index("first").unwrap();
    const OUTS: [usize; 8] = [
        SPLIT.output_index("out1").unwrap(),
        SPLIT.output_index("out2").unwrap(),
        SPLIT.output_index("out3").unwrap(),
        SPLIT.output_index("out4").unwrap(),
        SPLIT.output_index("out5").unwrap(),
        SPLIT.output_index("out6").unwrap(),
        SPLIT.output_index("out7").unwrap(),
        SPLIT.output_index("out8").unwrap(),
    ];
    /// What an output past the input's last voice holds: `in`'s declared
    /// default (ADR-0043).
    const DEFAULT: f32 = match SPLIT.inputs[Self::IN].default {
        PortDefault::Value(value) => value,
        _ => panic!("Voice split's `in` has a fixed-value default"),
    };

    /// A value of `first` as a whole voice number from 1 to 16.
    fn first_from(value: f32) -> usize {
        value.round().clamp(1.0, loom_dsp::voice::MAX_VOICES as f32) as usize
    }
}

impl Module for Split {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        // Checked before any `voice_input` call, which fans a one-voice cable
        // out to every voice: a mono cable is voice 1 only.
        let voices = io.input_width(Self::IN).voices;
        let first = io.param(Self::FIRST);
        for frame in 0..io.frames() {
            let start = first.map_or(frame, self.first, Self::first_from).saturating_sub(1);
            for (k, &port) in Self::OUTS.iter().enumerate() {
                let voice = start.saturating_add(k);
                for side in 0..io.output_width(port).sides {
                    let value = if voice < voices {
                        io.voice_input(Self::IN, voice, side)[frame]
                    } else {
                        Self::DEFAULT
                    };
                    io.voice_output(port, 0, side)[frame] = value;
                }
            }
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if index == Self::FIRST {
            self.first = Self::first_from(value);
        }
    }
}

/// The control ranges of Voice spread's `amount`, from the smallest up
/// (`docs/design/control-ranges.md`). The finest one sets a detune of a few
/// cents: 10 cents on a pitch cable is about 0.00083.
pub const SPREAD_RANGES: &[ParamRange] = &[
    ParamRange::new(-0.01, 0.01),
    ParamRange::new(-0.1, 0.1),
    ParamRange::new(-1.0, 1.0),
    ParamRange::new(-10.0, 10.0),
];

/// The range a new Voice spread starts with: ±1 octave on a pitch cable, so a
/// Spread just added to a pitch path stays safe.
pub const SPREAD_DEFAULT_RANGE: ParamRange = ParamRange::new(-0.1, 0.1);

/// `loom.voice.spread` (MT-VOICE-03): a different offset for each voice from
/// one control.
pub static SPREAD: ModuleDescriptor = ModuleDescriptor {
    id: "loom.voice.spread",
    version: 1,
    name: "Voice Spread",
    category: "Voice",
    family: loom_core::Family::LevelAndRouting,
    description: "Gives each voice its own offset from one control. On a one-voice cable it does nothing.",
    inputs: &[PortSpec {
        name: "in",
        label: "In",
        description: "The signal to offset. Each voice gets its own offset.",
        signal: SignalType::Modulation,
        default: PortDefault::Value(0.0),
        range: PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        description: "The input plus this voice's offset times the amount.",
        signal: SignalType::Modulation,
        default: PortDefault::Value(0.0),
        range: PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[
        ParamSpec {
            name: "amount",
            label: "Amount",
            description: "How far apart the voices are. Each voice adds its offset times this amount.",
            min: -10.0,
            max: 10.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .smooth(0.005)
        .ranges(SPREAD_RANGES, SPREAD_DEFAULT_RANGE),
        ParamSpec {
            name: "mode",
            label: "Mode",
            description: "Even spreads the voices from minus to plus the amount, Series steps them from 0 up to the \
                          amount, and Random gives each voice a fixed random offset, from minus to plus the amount.",
            min: 0.0,
            max: 2.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer()
        .choices(&["Even", "Series", "Random"]),
        seed_param("Picks the offsets in Random mode. The same seed gives the same offsets.").shown_when("mode", &[2]),
    ],
    role: ModuleRole::Dsp(|config| Box::new(Spread::new(config))),
    panel: Panel::new(
        12,
        5,
        &[Widget::segmented(0, 0, 12, "mode"), Widget::knob(0, 2, "amount")],
    ),
    displays: &[],
    roles: &[],
};

/// How Voice spread places the voices.
#[derive(Clone, Copy, PartialEq, Debug)]
enum SpreadMode {
    /// Evenly from -1 to 1, as the stack's spread.
    Even,
    /// From 0 up to 1.
    Series,
    /// A seeded draw from -1 to 1 for each voice.
    Random,
}

impl SpreadMode {
    /// The mode a value of the `mode` parameter names. A value out of range is
    /// Even, so it cannot panic.
    fn from_value(value: f32) -> Self {
        match value.round() as i32 {
            1 => Self::Series,
            2 => Self::Random,
            _ => Self::Even,
        }
    }
}

/// The offsets of one voice in each mode, and what they were computed from.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Offsets {
    /// The voice, from 1. 0 until the first call fills the cache.
    voice: usize,
    /// The voice count.
    count: usize,
    /// The rounded seed the Random offset came from.
    seed: u32,
    even: f32,
    series: f32,
    random: f32,
}

impl Offsets {
    /// The offsets of voice `voice` of `count` at `seed` (polyphony spec,
    /// Voice spread). Every mode gives 0 at one voice.
    fn new(voice: usize, count: usize, seed: u32) -> Self {
        let index = voice.saturating_sub(1);
        let (even, series, random) = if count <= 1 {
            (0.0, 0.0, 0.0)
        } else {
            (
                loom_dsp::blend::spread_offset(index as u32, count as u32),
                index as f32 / (count - 1) as f32,
                Self::random(index, seed),
            )
        };
        Self {
            voice,
            count,
            seed,
            even,
            series,
            random,
        }
    }

    /// The Random offset of the zero-based voice `index`: the first draw of its
    /// own stream, from -1 up to 1. The side bit stays 0, so both sides of a
    /// voice draw the same offset (ADR-0045).
    fn random(index: usize, seed: u32) -> f32 {
        2.0 * stream(seed, index.min(15) * 2, VOICE_SPREAD_STREAM).draw() - 1.0
    }

    /// The offset `mode` uses.
    fn get(&self, mode: SpreadMode) -> f32 {
        match mode {
            SpreadMode::Even => self.even,
            SpreadMode::Series => self.series,
            SpreadMode::Random => self.random,
        }
    }
}

/// State for `loom.voice.spread`.
#[derive(Clone)]
struct Spread {
    amount: Smoothed,
    mode: SpreadMode,
    /// The knob's base seed. Only `set_param` writes it.
    seed: u32,
    /// This voice's offsets, filled again when the voice, the count, or the
    /// rounded seed changes.
    offsets: Offsets,
}

impl Spread {
    const AMOUNT: usize = SPREAD.param_index("amount").unwrap();
    const MODE: usize = SPREAD.param_index("mode").unwrap();
    const SEED: usize = SPREAD.param_index("seed").unwrap();

    /// A module with every parameter at its default. The cache starts at voice
    /// 0, which no copy has, so the first call fills it.
    fn new(config: &ModuleConfig) -> Self {
        let amount = &SPREAD.params[Self::AMOUNT];
        Self {
            amount: Smoothed::new(amount.default, config.sample_rate, amount.options.smooth_seconds),
            mode: SpreadMode::Even,
            seed: 1,
            offsets: Offsets {
                voice: 0,
                ..Offsets::new(1, 1, 1)
            },
        }
    }
}

impl Module for Spread {
    /// Takes the running state of another instance as it is. Its offsets come
    /// from the seed and the voice, not the side, so a new right side already
    /// agrees with its left side (ADR-0045).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let (voice, count) = (ctx.voice(), ctx.voice_count());
        if voice != self.offsets.voice || count != self.offsets.count {
            self.offsets = Offsets::new(voice, count, self.offsets.seed);
        }
        let amount_param = io.param(Self::AMOUNT);
        let mode_param = io.param(Self::MODE);
        let seed_param = io.param(Self::SEED);
        let ([input], [out]) = io.split::<1, 1>();
        for (i, (out, &x)) in out.iter_mut().zip(input.iter()).enumerate() {
            let seed = seed_param.map_or(i, self.seed, seed_from);
            if seed != self.offsets.seed {
                self.offsets = Offsets::new(voice, count, seed);
            }
            // The smoother ticks on every frame in every mode, so a glide takes
            // the same time at any block size.
            let amount = amount_param.at_or(i, self.amount.tick());
            let offset = self
                .offsets
                .get(mode_param.map_or(i, self.mode, SpreadMode::from_value));
            *out = if offset == 0.0 {
                x
            } else if x.is_finite() {
                hold_finite(x + amount * offset)
            } else {
                x + amount * offset
            };
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            Self::AMOUNT => self.amount.set_target(value),
            Self::MODE => self.mode = SpreadMode::from_value(value),
            Self::SEED => self.seed = seed_from(value),
            _ => {}
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        if index == Self::AMOUNT {
            self.amount.gliding()
        } else {
            None
        }
    }

    /// Snaps the amount to its target (live patching spec, rule 7).
    fn reset(&mut self) {
        let target = self.amount.target();
        self.amount.reset(target);
    }
}
