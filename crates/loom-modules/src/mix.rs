//! Mixing (MT-MIX-01, MT-MIX-02, FR-LIB-05).
//!
//! Spec: `docs/design/p1-modules.md`, Mixing and shaping.

use loom_core::{
    Channels, Module, ModuleConfig, ModuleDescriptor, ModuleRole, Panel, Param, ParamOptions, ParamSpec, PortDefault,
    PortIo, PortSpec, ProcessContext, SignalType, Widget,
};
use loom_rt::Smoothed;
use std::f32::consts::FRAC_PI_2;

/// One audio input of the mixer.
const fn mixer_input(name: &'static str, label: &'static str) -> PortSpec {
    PortSpec {
        name,
        label,
        signal: SignalType::Audio,
        description: "A signal to mix. Each side of a stereo cable is mixed on its own.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }
}

/// One level knob of the mixer.
const fn mixer_level(name: &'static str, label: &'static str) -> ParamSpec {
    ParamSpec {
        name,
        label,
        description: "How loud this input is in the mix.",
        min: 0.0,
        max: 2.0,
        default: 1.0,
        unit: "",
        options: ParamOptions::DEFAULT,
    }
    .smooth(0.005)
}

/// One mute toggle of the mixer.
const fn mixer_mute(name: &'static str, label: &'static str) -> ParamSpec {
    ParamSpec {
        name,
        label,
        description: "Silences this input. The input fades out over 5 ms.",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
        options: ParamOptions::DEFAULT,
    }
    .integer()
    .smooth(0.005)
}

/// Number of inputs on the mixer.
const MIXER_INPUTS: usize = 4;

/// The open state (1 open, 0 muted) for a modulated mute value.
///
/// From 0.5 up is muted, as a whole-number parameter rounds. NaN counts as
/// open. *Why:* a NaN handed to the smoother would stay in its target for
/// good, and the mixer would stay NaN until it was reset.
fn open_state(mute: f32) -> f32 {
    if mute >= 0.5 { 0.0 } else { 1.0 }
}

/// `loom.mix.mixer` (MT-MIX-01): four inputs, each with a level and a mute.
pub static MIXER: ModuleDescriptor = ModuleDescriptor {
    id: "loom.mix.mixer",
    version: 1,
    name: "Mixer",
    category: "Mix",
    family: loom_core::Family::LevelAndRouting,
    description: "Adds four signals, each with its own level and mute. Two mixers in a row make a bigger mix.",
    inputs: &[
        mixer_input("in1", "In 1"),
        mixer_input("in2", "In 2"),
        mixer_input("in3", "In 3"),
        mixer_input("in4", "In 4"),
    ],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "The sum of the inputs, each times its level, muted inputs silent.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[
        mixer_level("level1", "Level 1"),
        mixer_level("level2", "Level 2"),
        mixer_level("level3", "Level 3"),
        mixer_level("level4", "Level 4"),
        mixer_mute("mute1", "Mute 1"),
        mixer_mute("mute2", "Mute 2"),
        mixer_mute("mute3", "Mute 3"),
        mixer_mute("mute4", "Mute 4"),
    ],
    role: ModuleRole::Dsp(|config| Box::new(Mixer::new(config))),
    // Four columns, each a knob over a toggle.
    panel: Panel::new(
        12,
        5,
        &[
            // Each strip is labelled once, by its number (principle 10).
            Widget::knob(0, 0, "level1").captioned("1"),
            Widget::knob(3, 0, "level2").captioned("2"),
            Widget::knob(6, 0, "level3").captioned("3"),
            Widget::knob(9, 0, "level4").captioned("4"),
            Widget::toggle(0, 3, "mute1").captioned(""),
            Widget::toggle(3, 3, "mute2").captioned(""),
            Widget::toggle(6, 3, "mute3").captioned(""),
            Widget::toggle(9, 3, "mute4").captioned(""),
        ],
    ),
    displays: &[],
};

#[derive(Clone)]
struct Mixer {
    levels: [Smoothed; MIXER_INPUTS],
    /// 1 while the input is open and 0 while it is muted, so a mute fades.
    open: [Smoothed; MIXER_INPUTS],
    /// The open state the mute toggles are set to, 1 or 0. A modulated mute
    /// replaces it for as long as the assignment is there, and removing the
    /// assignment returns to it.
    open_base: [f32; MIXER_INPUTS],
}

impl Mixer {
    /// Index of `level1`. `level2` to `level4` follow it (checked below).
    const LEVEL1: usize = MIXER.param_index("level1").unwrap();
    /// Index of `mute1`. `mute2` to `mute4` follow it (checked below).
    const MUTE1: usize = MIXER.param_index("mute1").unwrap();

    /// Which input a parameter index belongs to, in the run of
    /// `MIXER_INPUTS` parameters that starts at `first`.
    fn input_of(first: usize, index: usize) -> Option<usize> {
        index.checked_sub(first).filter(|&n| n < MIXER_INPUTS)
    }

    fn new(config: &ModuleConfig) -> Self {
        let level = |i: usize| {
            Smoothed::new(
                1.0,
                config.sample_rate,
                MIXER.params[Self::LEVEL1 + i].options.smooth_seconds,
            )
        };
        let open = |i: usize| {
            Smoothed::new(
                1.0,
                config.sample_rate,
                MIXER.params[Self::MUTE1 + i].options.smooth_seconds,
            )
        };
        Self {
            levels: std::array::from_fn(level),
            open: std::array::from_fn(open),
            open_base: [1.0; MIXER_INPUTS],
        }
    }
}

// The Mixer reads its levels and mutes as two runs of `MIXER_INPUTS`
// parameters, so each run must stay in order in the descriptor. A reorder
// that breaks a run is a build error.
const _: () = {
    let names = [
        ["level1", "level2", "level3", "level4"],
        ["mute1", "mute2", "mute3", "mute4"],
    ];
    let firsts = [Mixer::LEVEL1, Mixer::MUTE1];
    let mut run = 0;
    while run < names.len() {
        let mut n = 0;
        while n < MIXER_INPUTS {
            assert!(
                matches!(MIXER.param_index(names[run][n]), Some(i) if i == firsts[run] + n),
                "Mixer level and mute parameters must each stay in order"
            );
            n += 1;
        }
        run += 1;
    }
};

impl Module for Mixer {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let modulated: [Param<'_>; MIXER_INPUTS] = std::array::from_fn(|n| io.param(Self::LEVEL1 + n));
        let mute_mod: [Param<'_>; MIXER_INPUTS] = std::array::from_fn(|n| io.param(Self::MUTE1 + n));
        let (inputs, [out]) = io.split::<MIXER_INPUTS, 1>();
        for (i, out) in out.iter_mut().enumerate() {
            let mut sum = 0.0;
            for n in 0..MIXER_INPUTS {
                // The smoother glides to the wanted state, so a modulated
                // mute fades like a knob mute does.
                let want = mute_mod[n].map_or(i, self.open_base[n], open_state);
                if want != self.open[n].target() {
                    self.open[n].set_target(want);
                }
                let level = modulated[n].at_or(i, self.levels[n].tick());
                sum += inputs[n][i] * level * self.open[n].tick();
            }
            *out = sum;
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(n) = Self::input_of(Self::LEVEL1, index) {
            self.levels[n].set_target(value);
        } else if let Some(n) = Self::input_of(Self::MUTE1, index) {
            self.open_base[n] = 1.0 - value;
            self.open[n].set_target(1.0 - value);
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        Self::input_of(Self::LEVEL1, index).and_then(|n| self.levels[n].gliding())
    }

    fn reset(&mut self) {
        for smoothed in self.levels.iter_mut().chain(self.open.iter_mut()) {
            let target = smoothed.target();
            smoothed.reset(target);
        }
    }
}

/// `loom.mix.crossfade` (MT-MIX-02): a blend between two signals.
pub static CROSSFADE: ModuleDescriptor = ModuleDescriptor {
    id: "loom.mix.crossfade",
    version: 1,
    name: "Crossfade",
    category: "Mix",
    family: loom_core::Family::LevelAndRouting,
    description: "Blends two signals. Equal power keeps the loudness of audio steady; linear is exact for control signals.",
    inputs: &[
        PortSpec {
            name: "a",
            label: "A",
            signal: SignalType::Audio,
            description: "The signal heard when the mix is 0.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "b",
            label: "B",
            signal: SignalType::Audio,
            description: "The signal heard when the mix is 1.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::PerChannel,
        },
    ],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "A blend of A and B.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[
        ParamSpec {
            name: "mix",
            label: "Mix",
            description: "0 is all A and 1 is all B.",
            min: 0.0,
            max: 1.0,
            default: 0.5,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .smooth(0.005),
        ParamSpec {
            name: "law",
            label: "Law",
            description: "How the two signals share the mix: equal power, or linear.",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer()
        .choices(&["Equal power", "Linear"]),
    ],
    role: ModuleRole::Dsp(|config| {
        Box::new(Crossfade {
            mix: Smoothed::new(
                0.5,
                config.sample_rate,
                CROSSFADE.params[Crossfade::MIX].options.smooth_seconds,
            ),
            linear: false,
        })
    }),
    panel: Panel::new(11, 5, &[Widget::segmented(0, 0, 11, "law"), Widget::knob(4, 2, "mix")]),
    displays: &[],
};

#[derive(Clone)]
struct Crossfade {
    mix: Smoothed,
    linear: bool,
}

impl Crossfade {
    const MIX: usize = CROSSFADE.param_index("mix").unwrap();
    const LAW: usize = CROSSFADE.param_index("law").unwrap();
}

impl Module for Crossfade {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let (modulated, law_mod) = (io.param(Self::MIX), io.param(Self::LAW));
        let ([a, b], [out]) = io.split::<2, 1>();
        for (i, out) in out.iter_mut().enumerate() {
            let linear = law_mod.map_or(i, self.linear, |v| v >= 0.5);
            let mix = modulated.at_or(i, self.mix.tick()).clamp(0.0, 1.0);
            // `clamp` passes NaN through. Use the middle for that frame.
            let mix = if mix.is_nan() { 0.5 } else { mix };
            let (gain_a, gain_b) = if linear {
                (1.0 - mix, mix)
            } else {
                // Audio from two sources adds in power, so the gains follow
                // a quarter circle and the sum of their squares stays 1.
                let (sin, cos) = (mix * FRAC_PI_2).sin_cos();
                (cos, sin)
            };
            *out = a[i] * gain_a + b[i] * gain_b;
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            Self::MIX => self.mix.set_target(value),
            Self::LAW => self.linear = value >= 0.5,
            _ => {}
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        if index == Self::MIX { self.mix.gliding() } else { None }
    }

    fn reset(&mut self) {
        let target = self.mix.target();
        self.mix.reset(target);
    }
}

/// A port on the stereo channel tools.
const fn stereo_port(
    name: &'static str,
    label: &'static str,
    description: &'static str,
    channels: Channels,
) -> PortSpec {
    PortSpec {
        name,
        label,
        description,
        signal: SignalType::Audio,
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels,
    }
}

/// `loom.mix.join`: combines two mono signals into one stereo cable.
pub static JOIN: ModuleDescriptor = ModuleDescriptor {
    id: "loom.mix.join",
    version: 1,
    name: "Join",
    category: "Mix",
    family: loom_core::Family::LevelAndRouting,
    description: "Combines two signals into a stereo cable.",
    inputs: &[
        stereo_port("left", "Left", "The signal for the left side.", Channels::Downmix),
        stereo_port("right", "Right", "The signal for the right side.", Channels::Downmix),
    ],
    outputs: &[stereo_port(
        "out",
        "Out",
        "The combined stereo signal.",
        Channels::Stereo,
    )],
    params: &[],
    role: ModuleRole::Dsp(|_| Box::new(Join)),
    panel: Panel::EMPTY,
    displays: &[],
};

/// A stateless stereo joiner.
#[derive(Clone)]
struct Join;

impl Join {
    const LEFT_IN: usize = JOIN.input_index("left").unwrap();
    const RIGHT_IN: usize = JOIN.input_index("right").unwrap();
    const OUT: usize = JOIN.output_index("out").unwrap();
}

impl Module for Join {
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let [left, _] = io.stereo_input(Self::LEFT_IN);
        let [right, _] = io.stereo_input(Self::RIGHT_IN);
        let [out_left, out_right] = io.stereo_output(Self::OUT);
        out_left.copy_from_slice(left);
        out_right.copy_from_slice(right);
    }
}

/// `loom.mix.split`: separates the sides of a stereo cable.
pub static SPLIT: ModuleDescriptor = ModuleDescriptor {
    id: "loom.mix.split",
    version: 1,
    name: "Split",
    category: "Mix",
    family: loom_core::Family::LevelAndRouting,
    description: "Separates a stereo cable into its two sides.",
    inputs: &[stereo_port("in", "In", "The stereo signal to split.", Channels::Stereo)],
    outputs: &[
        stereo_port("left", "Left", "The left side of the signal.", Channels::PerChannel),
        stereo_port("right", "Right", "The right side of the signal.", Channels::PerChannel),
    ],
    params: &[],
    role: ModuleRole::Dsp(|_| Box::new(Split)),
    panel: Panel::EMPTY,
    displays: &[],
};

/// A stateless stereo splitter.
#[derive(Clone)]
struct Split;

impl Split {
    const IN: usize = SPLIT.input_index("in").unwrap();
    const LEFT_OUT: usize = SPLIT.output_index("left").unwrap();
    const RIGHT_OUT: usize = SPLIT.output_index("right").unwrap();
}

impl Module for Split {
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let [left, right] = io.stereo_input(Self::IN);
        io.output(Self::LEFT_OUT).copy_from_slice(left);
        io.output(Self::RIGHT_OUT).copy_from_slice(right);
    }
}

#[cfg(test)]
mod tests;
