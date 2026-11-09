//! Oscillators and noise (FR-LIB-02).
//!
//! An oscillator plays at its pitch, like a VCO: it runs its own phase from
//! the pitch input and its tune and octave knobs. A cable into its phase
//! input replaces that phase, so any phase can drive any shape: one Phase
//! module for several shapes in lock, or a bent phase for a new wave
//! (`docs/design/oscillators.md`).

use crate::phase::{OCTAVE_PARAM, PitchedPhase, TUNE_PARAM};
use loom_core::{
    Channels, Module, ModuleConfig, ModuleDescriptor, ModuleRole, Panel, Param, ParamOptions, ParamSpec, PortDefault,
    PortIo, PortSpec, ProcessContext, SignalType, Widget,
};
use loom_core::{FillsRole, roles};
use loom_dsp::noise::Colour;
use loom_dsp::{noise, oscillator};
use loom_rt::Smoothed;

/// The `pitch` input of every oscillator.
const PITCH_PORT: PortSpec = PortSpec {
    name: "pitch",
    label: "Pitch",
    signal: SignalType::Pitch,
    description: "Sets the frequency. 0 is middle C, and +0.1 is one octave up. Ignored while a phase cable is connected.",
    default: PortDefault::Value(0.0),
    range: loom_core::PortRange::Signal,
    channels: Channels::Downmix,
};

/// The `phase` input of every oscillator. With no cable it reads the
/// oscillator's own phase (oscillators.md, rule 1).
const PHASE_PORT: PortSpec = PortSpec {
    name: "phase",
    label: "Phase",
    signal: SignalType::Phase,
    description: "Position in the wave, from 0 to 1. With no cable, the oscillator runs its own phase at the pitch. A cable replaces it.",
    default: PortDefault::OwnPhase,
    range: loom_core::PortRange::Signal,
    channels: Channels::Downmix,
};

/// The oscillator's `tune` knob: Phase's, with what a phase cable does to it.
const OSC_TUNE: ParamSpec = ParamSpec {
    description: "Shifts the pitch in semitones. Ignored while a phase cable is connected.",
    ..TUNE_PARAM
};

/// The oscillator's `octave` knob: Phase's, with what a phase cable does to it.
const OSC_OCTAVE: ParamSpec = ParamSpec {
    description: "Shifts the pitch in octaves. Ignored while a phase cable is connected.",
    ..OCTAVE_PARAM
};

/// The panel knobs every oscillator starts with, left to right.
const TUNING_KNOBS: [Widget; 2] = [Widget::knob(0, 0, "tune"), Widget::knob(3, 0, "octave")];

/// Where an oscillator reads its phase for one call: its own, run from the
/// pitch and the knobs, or the cable's (oscillators.md, rules 1 and 2).
struct PhaseIn<'a> {
    /// True while the phase input has no cable.
    own: bool,
    pitch: &'a [f32],
    cable: &'a [f32],
    tune: Param<'a>,
    octave: Param<'a>,
}

impl PhaseIn<'_> {
    /// The phase for frame `i`. The own phase only moves while it is used,
    /// so it holds while a cable is connected (rule 3).
    #[inline]
    fn at(&self, i: usize, own: &mut PitchedPhase) -> f32 {
        if self.own {
            own.tick(i, self.pitch[i], self.tune, self.octave)
        } else {
            self.cable[i]
        }
    }
}

/// Sets a tuning knob on an oscillator's own phase, given the oscillator's
/// `tune` and `octave` indices. Any other parameter is left alone.
fn set_tuning(phase: &mut PitchedPhase, [tune, octave]: [usize; 2], index: usize, value: f32) {
    if index == tune {
        phase.set_tune(value);
    } else if index == octave {
        phase.set_octave(value);
    }
}

/// `loom.osc.sine`: sine of the phase input.
pub static SINE: ModuleDescriptor = ModuleDescriptor {
    id: "loom.osc.sine",
    version: 1,
    name: "Sine",
    category: "Oscillator",
    family: loom_core::Family::Sources,
    description: "A sine wave at the pitch, or shaped from a phase cable.",
    inputs: &[PITCH_PORT, PHASE_PORT],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "Sine wave from -1 to 1.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[OSC_TUNE, OSC_OCTAVE],
    role: ModuleRole::Dsp(|config| {
        Box::new(Sine {
            phase: PitchedPhase::new(config.sample_rate),
        })
    }),
    panel: Panel::new(6, 3, &TUNING_KNOBS),
    displays: &[],
    roles: &[FillsRole::of(&roles::SOURCE)],
};

/// State for `loom.osc.sine`.
#[derive(Clone)]
struct Sine {
    phase: PitchedPhase,
}

impl Sine {
    const PHASE_IN: usize = SINE.input_index("phase").unwrap();
    const TUNE: usize = SINE.param_index("tune").unwrap();
    const OCTAVE: usize = SINE.param_index("octave").unwrap();
}

impl Module for Sine {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let (own, tune, octave) = (
            !io.is_connected(Self::PHASE_IN),
            io.param(Self::TUNE),
            io.param(Self::OCTAVE),
        );
        let ([pitch, cable], [out]) = io.split::<2, 1>();
        let phase_in = PhaseIn {
            own,
            pitch,
            cable,
            tune,
            octave,
        };
        for (i, out) in out.iter_mut().enumerate() {
            *out = oscillator::sine(phase_in.at(i, &mut self.phase));
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        set_tuning(&mut self.phase, [Self::TUNE, Self::OCTAVE], index, value);
    }

    fn reset(&mut self) {
        self.phase.reset();
    }
}

/// `loom.osc.saw`: band-limited saw shaped from a phase input.
pub static SAW: ModuleDescriptor = ModuleDescriptor {
    id: "loom.osc.saw",
    version: 1,
    name: "Saw",
    category: "Oscillator",
    family: loom_core::Family::Sources,
    description: "A sawtooth wave, with every harmonic, at the pitch or shaped from a phase cable. Its sharp edges are smoothed so high notes stay clean.",
    inputs: &[PITCH_PORT, PHASE_PORT],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "Saw wave from -1 to 1.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[OSC_TUNE, OSC_OCTAVE],
    role: ModuleRole::Dsp(|config| {
        Box::new(Saw {
            osc: oscillator::Saw::new(),
            phase: PitchedPhase::new(config.sample_rate),
        })
    }),
    panel: Panel::new(6, 3, &TUNING_KNOBS),
    displays: &[],
    roles: &[FillsRole::of(&roles::SOURCE)],
};

/// State for `loom.osc.saw`.
#[derive(Clone)]
struct Saw {
    osc: oscillator::Saw,
    phase: PitchedPhase,
}

impl Saw {
    const PHASE_IN: usize = SAW.input_index("phase").unwrap();
    const TUNE: usize = SAW.param_index("tune").unwrap();
    const OCTAVE: usize = SAW.param_index("octave").unwrap();
}

impl Module for Saw {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let (own, tune, octave) = (
            !io.is_connected(Self::PHASE_IN),
            io.param(Self::TUNE),
            io.param(Self::OCTAVE),
        );
        let ([pitch, cable], [out]) = io.split::<2, 1>();
        let phase_in = PhaseIn {
            own,
            pitch,
            cable,
            tune,
            octave,
        };
        for (i, out) in out.iter_mut().enumerate() {
            *out = self.osc.tick(phase_in.at(i, &mut self.phase));
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        set_tuning(&mut self.phase, [Self::TUNE, Self::OCTAVE], index, value);
    }

    fn reset(&mut self) {
        self.osc.reset();
        self.phase.reset();
    }
}

/// `loom.osc.pulse`: band-limited pulse shaped from a phase input.
pub static PULSE: ModuleDescriptor = ModuleDescriptor {
    id: "loom.osc.pulse",
    version: 1,
    name: "Pulse",
    category: "Oscillator",
    family: loom_core::Family::Sources,
    description: "A pulse wave with a set width, at the pitch or shaped from a phase cable. Its sharp edges are smoothed so high notes stay clean.",
    inputs: &[PITCH_PORT, PHASE_PORT],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "Pulse wave from -1 to 1.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[
        OSC_TUNE,
        OSC_OCTAVE,
        ParamSpec {
            name: "width",
            label: "Width",
            description: "Share of the cycle spent high, from 0.01 to 0.99. 0.5 is a square wave.",
            min: 0.01,
            max: 0.99,
            default: 0.5,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .smooth(0.005),
    ],
    role: ModuleRole::Dsp(|config| {
        Box::new(Pulse {
            osc: oscillator::Pulse::new(),
            phase: PitchedPhase::new(config.sample_rate),
            width: Smoothed::new(
                0.5,
                config.sample_rate,
                PULSE.params[Pulse::WIDTH].options.smooth_seconds,
            ),
        })
    }),
    panel: Panel::new(9, 3, &[TUNING_KNOBS[0], TUNING_KNOBS[1], Widget::knob(6, 0, "width")]),
    displays: &[],
    roles: &[FillsRole::of(&roles::SOURCE)],
};

/// State for `loom.osc.pulse`.
#[derive(Clone)]
struct Pulse {
    osc: oscillator::Pulse,
    phase: PitchedPhase,
    width: Smoothed,
}

impl Pulse {
    const PHASE_IN: usize = PULSE.input_index("phase").unwrap();
    const TUNE: usize = PULSE.param_index("tune").unwrap();
    const OCTAVE: usize = PULSE.param_index("octave").unwrap();
    const WIDTH: usize = PULSE.param_index("width").unwrap();
}

impl Module for Pulse {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let (own, tune, octave, modulated) = (
            !io.is_connected(Self::PHASE_IN),
            io.param(Self::TUNE),
            io.param(Self::OCTAVE),
            io.param(Self::WIDTH),
        );
        let ([pitch, cable], [out]) = io.split::<2, 1>();
        let phase_in = PhaseIn {
            own,
            pitch,
            cable,
            tune,
            octave,
        };
        for (i, out) in out.iter_mut().enumerate() {
            let width = modulated.at_or(i, self.width.tick()).clamp(0.01, 0.99);
            *out = self.osc.tick(phase_in.at(i, &mut self.phase), width);
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if index == Self::WIDTH {
            self.width.set_target(value);
        } else {
            set_tuning(&mut self.phase, [Self::TUNE, Self::OCTAVE], index, value);
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        if index == Self::WIDTH {
            self.width.gliding()
        } else {
            None
        }
    }

    fn reset(&mut self) {
        let target = self.width.target();
        self.width.reset(target);
        self.osc.reset(target.clamp(0.01, 0.99));
        self.phase.reset();
    }
}

/// `loom.osc.triangle`: band-limited triangle shaped from a phase input.
pub static TRIANGLE: ModuleDescriptor = ModuleDescriptor {
    id: "loom.osc.triangle",
    version: 1,
    name: "Triangle",
    category: "Oscillator",
    family: loom_core::Family::Sources,
    description: "A triangle wave with few harmonics, at the pitch or shaped from a phase cable. Its corners are smoothed so high notes stay clean.",
    inputs: &[PITCH_PORT, PHASE_PORT],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "Triangle wave from -1 to 1.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[OSC_TUNE, OSC_OCTAVE],
    role: ModuleRole::Dsp(|config| {
        Box::new(Triangle {
            osc: oscillator::Triangle::new(),
            phase: PitchedPhase::new(config.sample_rate),
        })
    }),
    panel: Panel::new(6, 3, &TUNING_KNOBS),
    displays: &[],
    roles: &[FillsRole::of(&roles::SOURCE)],
};

/// State for `loom.osc.triangle`.
#[derive(Clone)]
struct Triangle {
    osc: oscillator::Triangle,
    phase: PitchedPhase,
}

impl Triangle {
    const PHASE_IN: usize = TRIANGLE.input_index("phase").unwrap();
    const TUNE: usize = TRIANGLE.param_index("tune").unwrap();
    const OCTAVE: usize = TRIANGLE.param_index("octave").unwrap();
}

impl Module for Triangle {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let (own, tune, octave) = (
            !io.is_connected(Self::PHASE_IN),
            io.param(Self::TUNE),
            io.param(Self::OCTAVE),
        );
        let ([pitch, cable], [out]) = io.split::<2, 1>();
        let phase_in = PhaseIn {
            own,
            pitch,
            cable,
            tune,
            octave,
        };
        for (i, out) in out.iter_mut().enumerate() {
            *out = self.osc.tick(phase_in.at(i, &mut self.phase));
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        set_tuning(&mut self.phase, [Self::TUNE, Self::OCTAVE], index, value);
    }

    fn reset(&mut self) {
        self.osc.reset();
        self.phase.reset();
    }
}

/// `loom.osc.noise`: white noise from a seeded generator.
pub static NOISE: ModuleDescriptor = ModuleDescriptor {
    id: "loom.osc.noise",
    version: 1,
    name: "Noise",
    category: "Oscillator",
    family: loom_core::Family::Sources,
    description: "White, pink, or brown noise. The seed makes every render the same.",
    inputs: &[],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "Noise with the same loudness in every colour. White stays within -1 to 1; pink and brown can peak a little above.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[ParamSpec {
        name: "seed",
        label: "Seed",
        description: "Picks the noise pattern. The same seed gives the same noise.",
        min: 1.0,
        max: 65_535.0,
        default: 1.0,
        unit: "",
        options: ParamOptions::DEFAULT,
    }
    .integer(),
    ParamSpec {
        name: "colour",
        label: "Colour",
        description: "White falls flat across the spectrum, pink falls 3 dB per octave, and brown falls 6 dB per octave.",
        min: 0.0,
        max: 2.0,
        default: 0.0,
        unit: "",
        options: ParamOptions::DEFAULT,
    }
.integer()
    .choices(&["White", "Pink", "Brown"])],
    role: ModuleRole::Dsp(|config: &ModuleConfig| Box::new(Noise::new(config.sample_rate))),
    panel: Panel::new(9, 2, &[Widget::segmented(0, 0, 9, "colour")]),
    displays: &[],
    roles: &[FillsRole::of(&roles::SOURCE)],
};

/// State for `loom.osc.noise`. The generator and its sources are in
/// [`noise`]. The module adds the channel's seed and the knobs.
struct Noise {
    generator: noise::Noise,
    /// Channel whose seed is used for the current random streams.
    channel: usize,
    /// The knob's base seed. Only `set_param` writes it.
    seed: u32,
    /// The seed the streams were last started from: the modulated seed while
    /// an assignment is active, otherwise the base.
    active: u32,
    colour: u8,
}

impl Noise {
    const SEED: usize = NOISE.param_index("seed").unwrap();
    const COLOUR: usize = NOISE.param_index("colour").unwrap();

    fn new(sample_rate: f32) -> Self {
        Self {
            generator: noise::Noise::new(sample_rate),
            channel: 0,
            seed: 1,
            active: 1,
            colour: 0,
        }
    }

    /// `seed` with the channel mixed in, so stereo sides differ.
    fn channel_seed(&self, seed: u32) -> u32 {
        if self.channel == 0 { seed } else { seed ^ 0x85EB_CA6B }
    }

    /// Starts every stream again from `seed`.
    fn restart(&mut self, seed: u32) {
        self.generator.restart(self.channel_seed(seed));
    }

    /// Starts the random streams again from `seed`, and keeps the brown memory.
    fn restart_keeping_brown(&mut self, seed: u32) {
        self.generator.restart_keeping_brown(self.channel_seed(seed));
    }
}

/// The colour for a panel value.
#[inline]
fn colour_of(value: u8) -> Colour {
    match value {
        0 => Colour::White,
        1 => Colour::Pink,
        _ => Colour::Brown,
    }
}

impl Module for Noise {
    /// Takes the settings and the brown filter memory of another instance,
    /// then starts the random streams again from the seed, as `reset` does
    /// (stereo spec, rule 27). A copied generator would play the other
    /// side's numbers. The first call on the right mixes in its channel.
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        let Some(other) = other.as_any().downcast_ref::<Self>() else {
            return false;
        };
        self.seed = other.seed;
        self.channel = other.channel;
        self.active = other.active;
        self.colour = other.colour;
        self.generator.copy_brown_from(&other.generator);
        self.restart_keeping_brown(self.active);
        true
    }

    fn process(&mut self, ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        if self.channel != ctx.channel() {
            self.channel = ctx.channel();
            self.restart_keeping_brown(self.active);
        }
        let (seed, colour_mod) = (io.param(Self::SEED), io.param(Self::COLOUR));
        let ([], [out]) = io.split::<0, 1>();
        for (i, out) in out.iter_mut().enumerate() {
            // The colour counts for this frame only; the base stays in the
            // field, so removing an assignment returns to it.
            let colour = colour_mod.map_or(i, self.colour, |v| v.round().max(0.0) as u8);
            // Restart the pattern when the seed in use changes, including
            // back to the base when an assignment is removed.
            let next = seed.map_or(i, self.seed, |v| v.round().max(1.0) as u32);
            if next != self.active {
                self.active = next;
                self.restart(next);
            }
            *out = self.generator.tick(colour_of(colour));
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            Self::SEED => {
                self.seed = value.round().max(1.0) as u32;
                self.active = self.seed;
                self.restart(self.seed);
            }
            Self::COLOUR => self.colour = value as u8,
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.restart(self.active);
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod own_phase_tests;

#[cfg(test)]
mod noise_colour_tests;
