//! The own phase: what a phase input reads while no cable is connected
//! ([ADR-0042](../../../docs/adr/0042-unpatched-phase-inputs-are-wired-for-the-player.md),
//! `docs/design/oscillators.md`).
//!
//! Everything a module with an own phase needs is here, so the next one
//! copies nothing: the `pitch` and `phase` ports, the `tune` and `octave`
//! knobs, the phase itself ([`PitchedPhase`], shared with the Phase module
//! so the two are bit-identical), and [`split`], which reads one call's
//! inputs. The ADR has the checklist, and `tests/own_phase.rs` checks every
//! module that declares [`PortDefault::OwnPhase`].

use loom_core::signal::pitch;
use loom_core::{
    Channels, ModuleDescriptor, Param, ParamOptions, ParamSpec, PortDefault, PortIo, PortSpec, SignalType, Widget,
};
use loom_dsp::phase::Phasor;

/// The `tune` parameter of Phase.
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

/// The `octave` parameter of Phase.
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

/// The `pitch` input of a module with an own phase. It must be input 0.
pub(crate) const PITCH_PORT: PortSpec = PortSpec {
    name: "pitch",
    label: "Pitch",
    signal: SignalType::Pitch,
    description: "Sets the frequency. 0 is middle C, and +0.1 is one octave up. Ignored while a phase cable is connected.",
    default: PortDefault::Value(0.0),
    range: loom_core::PortRange::Signal,
    channels: Channels::Downmix,
};

/// The `phase` input of a module with an own phase. It must be input 1.
/// With no cable it reads the module's own phase (rule 1).
pub(crate) const PHASE_PORT: PortSpec = PortSpec {
    name: "phase",
    label: "Phase",
    signal: SignalType::Phase,
    description: "Position in the wave, from 0 to 1. With no cable, the oscillator runs its own phase at the pitch. A cable replaces it.",
    default: PortDefault::OwnPhase,
    range: loom_core::PortRange::Signal,
    channels: Channels::Downmix,
};

/// The `tune` knob of a module with an own phase: Phase's, with what a
/// phase cable does to it.
pub(crate) const OWN_TUNE: ParamSpec = ParamSpec {
    description: "Shifts the pitch in semitones. Ignored while a phase cable is connected.",
    ..TUNE_PARAM
};

/// The `octave` knob of a module with an own phase: Phase's, with what a
/// phase cable does to it.
pub(crate) const OWN_OCTAVE: ParamSpec = ParamSpec {
    description: "Shifts the pitch in octaves. Ignored while a phase cable is connected.",
    ..OCTAVE_PARAM
};

/// The panel knobs for the tuning, left to right, on the top row.
pub(crate) const TUNING_KNOBS: [Widget; 2] = [Widget::knob(0, 0, "tune"), Widget::knob(3, 0, "octave")];

/// A phase that runs at a pitch, shifted by a tune in semitones and an
/// octave. Phase runs one, and so does every module with an own phase
/// while its phase input has no cable (oscillators.md, rule 1). Sharing it
/// means an own phase is the same, bit for bit, as a Phase module in front.
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

/// Where a module with an own phase finds its phase input and tuning knobs,
/// read from its descriptor when the crate builds.
#[derive(Clone, Copy)]
pub(crate) struct OwnPhasePorts {
    phase_in: usize,
    tune: usize,
    octave: usize,
}

impl OwnPhasePorts {
    /// The indices in `descriptor`. Fails the build unless `pitch` is input
    /// 0 and `phase` input 1, which [`split`] needs, and the module has
    /// `tune` and `octave`.
    pub(crate) const fn of(descriptor: &ModuleDescriptor) -> Self {
        assert!(
            matches!(descriptor.input_index("pitch"), Some(0)),
            "pitch must be input 0"
        );
        assert!(
            matches!(descriptor.input_index("phase"), Some(1)),
            "phase must be input 1"
        );
        Self {
            phase_in: 1,
            tune: descriptor.param_index("tune").unwrap(),
            octave: descriptor.param_index("octave").unwrap(),
        }
    }

    /// Sets a tuning knob on `phase`, if `index` is `tune` or `octave`.
    /// Returns false for any other parameter, for the module to handle.
    pub(crate) fn set_param(self, phase: &mut PitchedPhase, index: usize, value: f32) -> bool {
        if index == self.tune {
            phase.set_tune(value);
        } else if index == self.octave {
            phase.set_octave(value);
        } else {
            return false;
        }
        true
    }
}

/// Where a module reads its phase for one call: its own, run from the
/// pitch and the knobs, or the cable's (oscillators.md, rules 1 and 2).
pub(crate) struct PhaseIn<'a> {
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
    pub(crate) fn at(&self, i: usize, own: &mut PitchedPhase) -> f32 {
        if self.own {
            own.tick(i, self.pitch[i], self.tune, self.octave)
        } else {
            self.cable[i]
        }
    }
}

/// One call's phase inputs and the first output, for a module with an own
/// phase. Read any other parameter from `io` before calling this.
#[inline]
pub(crate) fn split<'a>(io: &'a mut PortIo<'_>, ports: OwnPhasePorts) -> (PhaseIn<'a>, &'a mut [f32]) {
    let own = !io.is_connected(ports.phase_in);
    let (tune, octave) = (io.param(ports.tune), io.param(ports.octave));
    let ([pitch, cable], [out]) = io.split::<2, 1>();
    (
        PhaseIn {
            own,
            pitch,
            cable,
            tune,
            octave,
        },
        out,
    )
}
