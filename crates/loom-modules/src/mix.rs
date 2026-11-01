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
    fn new(config: &ModuleConfig) -> Self {
        let level = |i: usize| Smoothed::new(1.0, config.sample_rate, MIXER.params[i].options.smooth_seconds);
        let open = |i: usize| {
            Smoothed::new(
                1.0,
                config.sample_rate,
                MIXER.params[MIXER_INPUTS + i].options.smooth_seconds,
            )
        };
        Self {
            levels: std::array::from_fn(level),
            open: std::array::from_fn(open),
            open_base: [1.0; MIXER_INPUTS],
        }
    }
}

impl Module for Mixer {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let modulated: [Param<'_>; MIXER_INPUTS] = std::array::from_fn(|n| io.param(n));
        let mute_mod: [Param<'_>; MIXER_INPUTS] = std::array::from_fn(|n| io.param(MIXER_INPUTS + n));
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
        match index {
            0..=3 => self.levels[index].set_target(value),
            4..=7 => {
                self.open_base[index - MIXER_INPUTS] = 1.0 - value;
                self.open[index - MIXER_INPUTS].set_target(1.0 - value);
            }
            _ => {}
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        match index {
            0..=3 => self.levels[index].gliding(),
            _ => None,
        }
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
            mix: Smoothed::new(0.5, config.sample_rate, CROSSFADE.params[0].options.smooth_seconds),
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

impl Module for Crossfade {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let (modulated, law_mod) = (io.param(0), io.param(1));
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
            0 => self.mix.set_target(value),
            _ => self.linear = value >= 0.5,
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        if index == 0 { self.mix.gliding() } else { None }
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

impl Module for Join {
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let [left, _] = io.stereo_input(0);
        let [right, _] = io.stereo_input(1);
        let [out_left, out_right] = io.stereo_output(0);
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

impl Module for Split {
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let [left, right] = io.stereo_input(0);
        io.output(0).copy_from_slice(left);
        io.output(1).copy_from_slice(right);
    }
}

#[cfg(test)]
mod tests;
